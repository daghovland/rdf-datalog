/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the dagalog library pipeline.
//!
//! These tests exercise the public API in `src/lib.rs`:
//! - `load_file`
//! - `apply_ontologies`
//! - `run_sparql_query`
//! - `format_results`
//!
//! All test data lives in `tests/testdata/`.

use dag_rdf::Datastore;
use dagalog::{
    OutputFormat, apply_ontologies, format_results, graph_element_display, load_file,
    run_sparql_query,
};
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

// ── Load helpers ──────────────────────────────────────────────────────────────

#[test]
fn load_turtle_file() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("should load family.ttl");
    // family.ttl has at least the type/name triples for Alice, Bob, Charlie
    assert!(
        ds.named_graphs.quad_count >= 6,
        "expected at least 6 triples, got {}",
        ds.named_graphs.quad_count
    );
}

#[test]
fn load_trig_file_default_graph() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("named_graphs.trig")).expect("should load named_graphs.trig");
    // named_graphs.trig has 2 triples in default graph + 2 in named graphs
    assert!(
        ds.named_graphs.quad_count >= 4,
        "expected at least 4 triples (across all graphs), got {}",
        ds.named_graphs.quad_count
    );
}

#[test]
fn load_nonexistent_file_returns_error() {
    let mut ds = Datastore::new(1000);
    let result = load_file(&mut ds, Path::new("/nonexistent/path/file.ttl"));
    assert!(result.is_err(), "should fail to open nonexistent file");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("cannot open"),
        "error should mention 'cannot open': {}",
        msg
    );
}

// ── Basic SPARQL ──────────────────────────────────────────────────────────────

#[test]
fn sparql_select_all_persons() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("should load");

    let sparql = "PREFIX ex: <http://example.org/family#> SELECT ?p WHERE { ?p a ex:Person . }";
    let result = run_sparql_query(&ds, sparql).expect("query should succeed");
    let persons: Vec<_> = result
        .rows
        .iter()
        .filter_map(|r| r.get("p"))
        .map(graph_element_display)
        .collect();

    assert!(
        persons.contains(&"<http://example.org/family#Alice>".to_string()),
        "Alice should be a Person; got: {:?}",
        persons
    );
    assert!(
        persons.contains(&"<http://example.org/family#Charlie>".to_string()),
        "Charlie should be a Person; got: {:?}",
        persons
    );
    // Without reasoning Bob (an Employee) is NOT a Person
    assert!(
        !persons.contains(&"<http://example.org/family#Bob>".to_string()),
        "Bob should NOT be a Person without reasoning"
    );
}

#[test]
fn sparql_filter_by_name() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("should load");

    let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?p WHERE {
    ?p ex:name ?n .
    FILTER(?n = "Alice")
}"#;
    let result = run_sparql_query(&ds, sparql).expect("query should succeed");
    assert_eq!(
        result.rows.len(),
        1,
        "expected exactly one result for name=Alice"
    );
    let val = graph_element_display(result.rows[0].get("p").unwrap());
    assert_eq!(val, "<http://example.org/family#Alice>");
}

#[test]
fn sparql_empty_result() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("should load");

    let sparql = "PREFIX ex: <http://example.org/family#> SELECT ?x WHERE { ?x a ex:Unicorn . }";
    let result = run_sparql_query(&ds, sparql).expect("query should succeed");
    assert!(result.rows.is_empty(), "expected no results for ex:Unicorn");
}

#[test]
fn sparql_invalid_query_returns_error() {
    let ds = Datastore::new(1000);
    let result = run_sparql_query(&ds, "THIS IS NOT SPARQL");
    assert!(result.is_err(), "invalid SPARQL should return an error");
}

// ── OWL-RL reasoning ──────────────────────────────────────────────────────────

#[test]
fn owl_rl_subclass_reasoning() {
    let mut ds = Datastore::new(10_000);
    // family.ttl has both the schema (Employee subClassOf Person) and data
    let stats = apply_ontologies(&mut ds, &[testdata("family.ttl")])
        .expect("apply_ontologies should succeed");

    assert!(
        stats.axiom_count > 0,
        "expected OWL axioms to be extracted from family.ttl, got 0"
    );
    assert!(
        stats.rule_count > 0,
        "expected Datalog rules to be generated, got 0"
    );

    let sparql = "PREFIX ex: <http://example.org/family#> SELECT ?p WHERE { ?p a ex:Person . }";
    let result = run_sparql_query(&ds, sparql).expect("query should succeed");
    let persons: Vec<_> = result
        .rows
        .iter()
        .filter_map(|r| r.get("p"))
        .map(graph_element_display)
        .collect();

    assert!(
        persons.contains(&"<http://example.org/family#Alice>".to_string()),
        "Alice should be a Person; got: {:?}",
        persons
    );
    assert!(
        persons.contains(&"<http://example.org/family#Bob>".to_string()),
        "Bob should be inferred as a Person (Employee subClassOf Person); got: {:?}",
        persons
    );
}

#[test]
fn owl_rl_data_separate_from_ontology() {
    // Load data into datastore, then apply ontology separately.
    // This tests the typical use case: --data file.ttl --ontology schema.ttl
    let mut ds = Datastore::new(10_000);

    // Load data (only the ABox triples)
    let data_ttl = r#"
@prefix ex: <http://example.org/family#> .
ex:Bob a ex:Employee ;
    ex:name "Bob" .
ex:Alice a ex:Person ;
    ex:name "Alice" .
"#;
    turtle_parser::parse_turtle(&mut ds, data_ttl.as_bytes()).expect("data parse");

    let data_triples = ds.named_graphs.quad_count;

    // Apply ontology (the TBox)
    let schema_ttl = r#"
@prefix ex: <http://example.org/family#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
<http://example.org/schema> a owl:Ontology .
ex:Employee rdfs:subClassOf ex:Person .
"#;
    // Write schema to a temp file so apply_ontologies can load it
    let schema_path = {
        let dir = std::env::temp_dir();
        let p = dir.join("dagalog_test_schema.ttl");
        std::fs::write(&p, schema_ttl).expect("write temp schema");
        p
    };

    let stats = apply_ontologies(&mut ds, &[schema_path]).expect("apply_ontologies");
    assert!(stats.rule_count > 0, "expected rules from SubClassOf");
    assert!(
        ds.named_graphs.quad_count > data_triples,
        "reasoning should have added triples"
    );

    let sparql = "PREFIX ex: <http://example.org/family#> SELECT ?p WHERE { ?p a ex:Person . }";
    let result = run_sparql_query(&ds, sparql).expect("query");
    let persons: Vec<_> = result
        .rows
        .iter()
        .filter_map(|r| r.get("p"))
        .map(graph_element_display)
        .collect();

    assert!(
        persons.contains(&"<http://example.org/family#Bob>".to_string()),
        "Bob should be inferred as Person; got: {:?}",
        persons
    );
}

// ── Output formats ────────────────────────────────────────────────────────────

#[test]
fn output_format_table() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("load");

    let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person ?name WHERE {
    ?person a ex:Person .
    ?person ex:name ?name .
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query");
    let output = format_results(&result, &OutputFormat::Table);

    assert!(output.contains("?person"), "header should have ?person");
    assert!(output.contains("?name"), "header should have ?name");
    assert!(output.contains("Alice"), "should contain value Alice");
    assert!(output.contains("---"), "should have separator line");
}

#[test]
fn output_format_csv_header() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("load");

    let sparql = "PREFIX ex: <http://example.org/family#> SELECT ?p ?n WHERE { ?p ex:name ?n . }";
    let result = run_sparql_query(&ds, sparql).expect("query");
    let output = format_results(&result, &OutputFormat::Csv);

    let first_line = output.lines().next().unwrap_or("");
    assert_eq!(first_line, "p,n", "CSV header should be variable names");
    // Raw values: IRI without <>, literal without RDF quoting
    assert!(
        output.contains("Alice") || output.contains("Bob"),
        "CSV should contain raw literal values"
    );
    assert!(
        !output.contains("\"\"\""),
        "CSV should not double-escape literals"
    );
}

#[test]
fn output_format_json_structure() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("load");

    let sparql = "PREFIX ex: <http://example.org/family#> SELECT ?p WHERE { ?p a ex:Person . }";
    let result = run_sparql_query(&ds, sparql).expect("query");
    let output = format_results(&result, &OutputFormat::Json);

    assert!(
        output.starts_with("{\"head\":{\"vars\":"),
        "should start with SPARQL JSON head"
    );
    assert!(
        output.contains("\"results\":{\"bindings\":"),
        "should have results bindings"
    );
    assert!(
        output.contains("\"type\":\"uri\""),
        "IRIs should have type:uri"
    );
    assert!(
        output.contains("http://example.org/family#Alice"),
        "should contain Alice IRI"
    );
}

#[test]
fn output_format_json_literals() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("load");

    let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?name WHERE { ex:Alice ex:name ?name . }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query");
    let output = format_results(&result, &OutputFormat::Json);

    assert!(
        output.contains("\"type\":\"literal\""),
        "literals should have type:literal"
    );
    assert!(
        output.contains("Alice"),
        "should contain literal value Alice"
    );
}

// ── SPARQL query file ─────────────────────────────────────────────────────────

#[test]
fn sparql_query_from_file() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("family.ttl")).expect("load");

    let query_str = std::fs::read_to_string(testdata("family.sparql")).expect("read family.sparql");
    let result = run_sparql_query(&ds, &query_str).expect("query from file");
    assert!(
        !result.rows.is_empty(),
        "family.sparql should return results"
    );
    assert_eq!(result.variables, vec!["person", "name"]);
}

// ── TriG named graphs ─────────────────────────────────────────────────────────

#[test]
fn trig_load_and_query() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("named_graphs.trig")).expect("load trig");

    // Top-level BGP patterns query the default graph only.
    let sparql = "PREFIX ex: <http://example.org/ng#> SELECT ?x WHERE { ?x ex:name ?n . }";
    let result = run_sparql_query(&ds, sparql).expect("query");
    let names: Vec<_> = result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();

    assert!(
        names.contains(&"<http://example.org/ng#Alice>".to_string()),
        "should find Alice from default graph"
    );
    assert!(
        !names.contains(&"<http://example.org/ng#Bob>".to_string()),
        "should not find Bob without a GRAPH clause"
    );
    assert!(
        !names.contains(&"<http://example.org/ng#Carol>".to_string()),
        "should not find Carol without a GRAPH clause"
    );

    let graph_sparql = r#"
PREFIX ex: <http://example.org/ng#>
SELECT ?g ?x WHERE {
    GRAPH ?g { ?x ex:name ?n . }
}
"#;
    let graph_result = run_sparql_query(&ds, graph_sparql).expect("graph query");
    let graph_subjects: Vec<_> = graph_result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();

    assert!(
        graph_subjects.contains(&"<http://example.org/ng#Bob>".to_string()),
        "GRAPH ?g should find Bob in named graph"
    );
    assert!(
        graph_subjects.contains(&"<http://example.org/ng#Carol>".to_string()),
        "GRAPH ?g should find Carol in named graph"
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Executable versions of every code example shown in README.md.
//!
//! If a test here breaks, the corresponding README section is wrong — update
//! both together.  The test names match the README headings they cover.

use dag_rdf::Datastore;
use dagalog::{apply_rules, graph_element_display, run_sparql_query};
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

// ── JSON-LD parsing ───────────────────────────────────────────────────────────

/// README §JSON-LD parsing — inline document.
#[test]
fn readme_jsonld_parse_inline() {
    let jsonld = r#"{
      "@context": { "foaf": "http://xmlns.com/foaf/0.1/" },
      "@id": "http://example.org/alice",
      "@type": "foaf:Person",
      "foaf:name": "Alice"
    }"#;

    let mut ds = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes()).expect("parse must succeed");

    let result = run_sparql_query(
        &ds,
        "SELECT ?name WHERE { \
            <http://example.org/alice> \
            <http://xmlns.com/foaf/0.1/name> ?name }",
    )
    .expect("query must succeed");
    assert_eq!(result.rows.len(), 1);
    let name = graph_element_display(result.rows[0].get("name").unwrap());
    assert!(name.contains("Alice"), "got: {name}");
}

/// README §JSON-LD parsing — @type becomes rdf:type triple.
#[test]
fn readme_jsonld_type_triple() {
    let jsonld = r#"{
      "@context": { "schema": "http://schema.org/" },
      "@id": "http://example.org/book/1",
      "@type": "schema:Book",
      "schema:name": "Learning RDF"
    }"#;

    let mut ds = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes()).expect("parse must succeed");

    let types = run_sparql_query(
        &ds,
        "SELECT ?t WHERE { \
            <http://example.org/book/1> \
            <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ?t }",
    )
    .expect("query must succeed");
    assert_eq!(types.rows.len(), 1);
    let t = graph_element_display(types.rows[0].get("t").unwrap());
    assert_eq!(t, "<http://schema.org/Book>", "got: {t}");
}

/// README §JSON-LD parsing — language-tagged and typed literals.
#[test]
fn readme_jsonld_literals() {
    let jsonld = r#"{
      "@context": {
        "dc": "http://purl.org/dc/elements/1.1/",
        "xsd": "http://www.w3.org/2001/XMLSchema#",
        "published": { "@id": "dc:date", "@type": "xsd:date" }
      },
      "@id": "http://example.org/article/1",
      "dc:title": [
        { "@value": "Hello RDF", "@language": "en" },
        { "@value": "Hallo RDF", "@language": "de" }
      ],
      "published": "2025-01-15"
    }"#;

    let mut ds = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes()).expect("parse must succeed");

    // Both language variants of dc:title are stored
    let titles = run_sparql_query(
        &ds,
        "SELECT ?t WHERE { \
            <http://example.org/article/1> \
            <http://purl.org/dc/elements/1.1/title> ?t }",
    )
    .expect("query must succeed");
    assert_eq!(titles.rows.len(), 2, "expected en and de titles");

    // The publication date is a typed literal
    let dates = run_sparql_query(
        &ds,
        "SELECT ?d WHERE { \
            <http://example.org/article/1> \
            <http://purl.org/dc/elements/1.1/date> ?d }",
    )
    .expect("query must succeed");
    assert_eq!(dates.rows.len(), 1);
    let d = graph_element_display(dates.rows[0].get("d").unwrap());
    assert!(
        d.contains("2025-01-15") && d.contains("XMLSchema#date"),
        "expected typed date literal, got: {d}"
    );
}

/// README §JSON-LD parsing — named graphs via @graph.
#[test]
fn readme_jsonld_named_graph() {
    let jsonld = r#"{
      "@context": { "ex": "http://example.org/" },
      "@id": "http://example.org/myGraph",
      "@graph": [
        { "@id": "http://example.org/alice", "ex:knows": { "@id": "http://example.org/bob" } },
        { "@id": "http://example.org/bob",   "ex:name":  "Bob" }
      ]
    }"#;

    let mut ds = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes()).expect("parse must succeed");

    // Triples live in the named graph, not the default graph
    let in_named = run_sparql_query(
        &ds,
        "SELECT ?s ?p ?o WHERE { \
            GRAPH <http://example.org/myGraph> { ?s ?p ?o } }",
    )
    .expect("query must succeed");
    assert!(in_named.rows.len() >= 2, "expected triples in named graph");

    let in_default = run_sparql_query(
        &ds,
        "SELECT ?o WHERE { \
            <http://example.org/alice> <http://example.org/knows> ?o }",
    )
    .expect("query must succeed");
    assert_eq!(
        in_default.rows.len(),
        0,
        "triples must not leak to default graph"
    );
}

// ── JSON-LD serialisation ─────────────────────────────────────────────────────

/// README §JSON-LD serialisation — Turtle → JSON-LD round-trip.
#[test]
fn readme_jsonld_serialize_roundtrip() {
    let ttl = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        <http://example.org/alice>
            a foaf:Person ;
            foaf:name "Alice" ;
            foaf:knows <http://example.org/bob> .
        <http://example.org/bob>
            a foaf:Person ;
            foaf:name "Bob" .
    "#;

    let mut ds1 = Datastore::new(10_000);
    turtle_parser::parse_turtle(&mut ds1, ttl.as_bytes()).expect("Turtle parse must succeed");

    // Serialise to JSON-LD
    let jsonld = jsonld_parser::serialize_jsonld(&ds1);
    assert!(jsonld.contains("@context"), "output must have @context");
    assert!(jsonld.contains("@id"), "output must have @id entries");
    assert!(
        jsonld.contains("http://example.org/alice"),
        "alice must appear in output"
    );

    // Re-parse: triple count must be preserved
    let mut ds2 = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds2, jsonld.as_bytes()).expect("re-parse must succeed");

    let count1 = run_sparql_query(&ds1, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }")
        .unwrap()
        .rows
        .len();
    let count2 = run_sparql_query(&ds2, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }")
        .unwrap()
        .rows
        .len();
    assert_eq!(count1, count2, "round-trip must preserve triple count");
}

/// README §JSON-LD serialisation — expanded form has no @context.
#[test]
fn readme_jsonld_serialize_expanded() {
    let jsonld_in = r#"{
      "@context": { "foaf": "http://xmlns.com/foaf/0.1/" },
      "@id": "http://example.org/alice",
      "foaf:name": "Alice"
    }"#;

    let mut ds = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds, jsonld_in.as_bytes()).expect("parse must succeed");

    let expanded = jsonld_parser::serialize_jsonld_expanded(&ds);

    // Must be a JSON array
    let v: serde_json::Value = serde_json::from_str(&expanded).expect("must be valid JSON");
    assert!(v.is_array(), "expanded form must be a JSON array");

    // No @context at the top level
    for node in v.as_array().unwrap() {
        assert!(
            !node
                .as_object()
                .map(|o| o.contains_key("@context"))
                .unwrap_or(false),
            "expanded form must not contain @context"
        );
    }

    // Full IRI for foaf:name must appear
    assert!(
        expanded.contains("http://xmlns.com/foaf/0.1/name"),
        "expanded form must use full IRI"
    );
}

// ── Turtle / TriG parsing ─────────────────────────────────────────────────────

/// README §Turtle parsing — basic triple loading.
#[test]
fn readme_turtle_parse_basic() {
    let ttl = r#"
        PREFIX dc: <http://purl.org/dc/elements/1.1/>
        <http://example.org/book/1> dc:title "SPARQL Tutorial" .
    "#;

    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse must succeed");

    let result = run_sparql_query(
        &ds,
        r#"SELECT ?title WHERE {
            <http://example.org/book/1>
            <http://purl.org/dc/elements/1.1/title>
            ?title }"#,
    )
    .expect("query must succeed");
    assert_eq!(result.rows.len(), 1);
    let title = graph_element_display(result.rows[0].get("title").unwrap());
    assert!(title.contains("SPARQL Tutorial"), "got: {title}");
}

/// README §TriG parsing — named graph in TriG format.
#[test]
fn readme_trig_named_graph() {
    let trig = r#"
        PREFIX ex: <http://example.org/>
        ex:graph1 {
            ex:subject ex:predicate ex:object .
        }
    "#;

    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_trig(&mut ds, trig.as_bytes()).expect("TriG parse must succeed");

    let result = run_sparql_query(
        &ds,
        "SELECT ?s ?p ?o WHERE { \
            GRAPH <http://example.org/graph1> { ?s ?p ?o } }",
    )
    .expect("query must succeed");
    assert_eq!(result.rows.len(), 1);
}

// ── SPARQL queries ────────────────────────────────────────────────────────────

/// README §SPARQL — basic SELECT with FILTER.
#[test]
fn readme_sparql_filter() {
    let ttl = r#"
        PREFIX ns: <http://example.org/ns#>
        <http://example.org/cheap>     ns:price  9 ; ns:title "Budget Book" .
        <http://example.org/expensive> ns:price 49 ; ns:title "Deluxe Book" .
    "#;

    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse");

    let result = run_sparql_query(
        &ds,
        "PREFIX ns: <http://example.org/ns#>
         SELECT ?title ?price WHERE {
             ?x ns:price ?price ;
                ns:title ?title .
             FILTER (?price < 20)
         }",
    )
    .expect("query must succeed");

    assert_eq!(result.rows.len(), 1);
    let title = graph_element_display(result.rows[0].get("title").unwrap());
    assert!(title.contains("Budget Book"), "got: {title}");
}

/// README §SPARQL — OPTIONAL pattern.
#[test]
fn readme_sparql_optional() {
    let ttl = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        <http://example.org/alice> foaf:name "Alice" ; foaf:mbox <mailto:alice@example.org> .
        <http://example.org/bob>   foaf:name "Bob" .
    "#;

    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse");

    let result = run_sparql_query(
        &ds,
        "PREFIX foaf: <http://xmlns.com/foaf/0.1/>
         SELECT ?name ?mbox WHERE {
             ?x foaf:name ?name .
             OPTIONAL { ?x foaf:mbox ?mbox }
         }",
    )
    .expect("query must succeed");

    // Alice: 1 row (name + mbox). Bob: 1 row (name only, mbox unbound).
    assert_eq!(result.rows.len(), 2);
}

/// README §SPARQL — GRAPH clause for named-graph queries.
#[test]
fn readme_sparql_graph_clause() {
    let trig = r#"
        PREFIX ex: <http://example.org/>
        ex:scientists {
            ex:curie   ex:field "Radioactivity" .
            ex:turing  ex:field "Computing" .
        }
    "#;

    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_trig(&mut ds, trig.as_bytes()).expect("TriG parse");

    let result = run_sparql_query(
        &ds,
        "SELECT ?person ?field WHERE {
             GRAPH <http://example.org/scientists> {
                 ?person <http://example.org/field> ?field
             }
         }",
    )
    .expect("query must succeed");
    assert_eq!(result.rows.len(), 2);
}

/// README §SPARQL — DISTINCT and LIMIT.
#[test]
fn readme_sparql_distinct_limit() {
    let ttl = r#"
        PREFIX ex: <http://example.org/>
        ex:a ex:tag ex:x , ex:y , ex:z .
        ex:b ex:tag ex:x , ex:w .
    "#;

    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse");

    let result = run_sparql_query(
        &ds,
        "SELECT DISTINCT ?tag WHERE { ?s <http://example.org/tag> ?tag }
         LIMIT 3",
    )
    .expect("query must succeed");
    assert_eq!(result.rows.len(), 3, "LIMIT 3 should return exactly 3 rows");
}

// ── OWL-RL reasoning ─────────────────────────────────────────────────────────

/// README §OWL reasoning — equality inference via owl:sameAs.
#[test]
fn readme_owl_same_as() {
    use dagalog::load_file;
    use datalog::evaluate_rules;
    use owl2rl2datalog::owl2datalog;
    use rdf_owl_translator::rdf2owl;

    let mut ds = Datastore::new(100_000);
    load_file(&mut ds, &testdata("equality.owl")).expect("equality.owl must load");

    let ontology = rdf2owl(&mut ds).ontology;
    let rules = owl2datalog(&mut ds.resources, &ontology);
    assert!(!rules.is_empty(), "OWL-RL should produce Datalog rules");
    evaluate_rules(rules, &mut ds);

    // ind2 gets the same type as ind1 via owl:sameAs
    let typed = run_sparql_query(
        &ds,
        "SELECT ?t WHERE { \
            <https://example.com/vocab#ind2> \
            <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ?t }",
    )
    .expect("query must succeed");
    assert!(
        !typed.rows.is_empty(),
        "ind2 must have at least one rdf:type after reasoning"
    );
}

// ── Custom Datalog rules ──────────────────────────────────────────────────────

/// README §Datalog rules — simple forward-chaining rule.
#[test]
fn readme_datalog_rule_forward_chain() {
    let mut ds = Datastore::new(10_000);
    dagalog::load_file(&mut ds, &testdata("data.ttl")).expect("data.ttl must load");

    const PRED: &str = "https://example.com/data#predicate";
    const OBJ2: &str = "https://example.com/data#object2";

    // Before rules: object2 does not exist
    let before =
        run_sparql_query(&ds, &format!("SELECT ?s WHERE {{ ?s <{PRED}> <{OBJ2}> }}")).unwrap();
    assert_eq!(
        before.rows.len(),
        0,
        "object2 should not exist before rules"
    );

    // Apply rules from data.ttl companion file
    apply_rules(&mut ds, &[testdata("rules.datalog")]).expect("rules must apply");

    // After rules: object2 is derived
    let after =
        run_sparql_query(&ds, &format!("SELECT ?s WHERE {{ ?s <{PRED}> <{OBJ2}> }}")).unwrap();
    assert_eq!(after.rows.len(), 1, "rule should have derived object2");
}

/// README §Datalog rules — stratified negation.
#[test]
fn readme_datalog_stratified_negation() {
    let mut ds = Datastore::new(10_000);
    dagalog::load_file(&mut ds, &testdata("test_stratified.ttl")).expect("test data must load");

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const TYPE3: &str = "http://example.com/data#Type3";

    apply_rules(&mut ds, &[testdata("test_stratified.datalog")]).expect("rules must apply");

    let result = run_sparql_query(
        &ds,
        &format!("SELECT ?x WHERE {{ ?x <{RDF_TYPE}> <{TYPE3}> }}"),
    )
    .unwrap();
    assert_eq!(
        result.rows.len(),
        1,
        "stratified negation should derive Type3"
    );
}

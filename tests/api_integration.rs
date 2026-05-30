/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests translated from DagSemTools `Api.Tests/TestApi.cs`.
//!
//! Each test corresponds to a `[Fact]` in the original C# suite.

use dag_rdf::{Datastore, IriReference, RdfResource};
use dagalog::{apply_rules, graph_element_display, load_file, run_sparql_query};
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn load(name: &str) -> Datastore {
    let mut ds = Datastore::new(100_000);
    load_file(&mut ds, &testdata(name)).expect("test data must load");
    ds
}

/// Convenience: count quads whose predicate+object match the given IRIs.
fn count_with_predicate_object(ds: &Datastore, pred: &str, obj: &str) -> usize {
    let pred_id = ds
        .resources
        .resource_map
        .get(&dag_rdf::GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(pred.to_string()))))
        .copied();
    let obj_id = ds
        .resources
        .resource_map
        .get(&dag_rdf::GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(obj.to_string()))))
        .copied();
    match (pred_id, obj_id) {
        (Some(p), Some(o)) => ds.quads_matching(None, None, Some(p), Some(o)).len(),
        _ => 0,
    }
}

// ── TestApi.Test1 ─────────────────────────────────────────────────────────────

/// Translated from `TestApi.Test1`: loads example1.ttl and checks the label.
#[test]
fn test1_load_example1_label() {
    let ds = load("example1.ttl");
    // example1.ttl has exactly one rdfs:label on FuelEfficiency
    let sparql = r#"
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?label
WHERE { <http://dbpedia.org/datatype/FuelEfficiency> rdfs:label ?label . }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query must succeed");
    assert_eq!(result.rows.len(), 1, "expected exactly one label on FuelEfficiency");
}

// ── TestApi.TestAbbreviatedBlankNode ─────────────────────────────────────────

/// Translated from `TestApi.TestAbbreviatedBlankNode`.
#[test]
fn test_abbreviated_blank_node() {
    let ds = load("abbreviated_blank_nodes.ttl");

    // foaf:knows: 2 triples
    let knows_count = {
        let sparql = "PREFIX foaf: <http://xmlns.com/foaf/0.1/> SELECT ?s ?o WHERE { ?s foaf:knows ?o }";
        run_sparql_query(&ds, sparql).unwrap().rows.len()
    };
    assert_eq!(knows_count, 2, "expected 2 foaf:knows triples");

    // foaf:name: 3 triples
    let name_count = {
        let sparql = "PREFIX foaf: <http://xmlns.com/foaf/0.1/> SELECT ?s ?n WHERE { ?s foaf:name ?n }";
        run_sparql_query(&ds, sparql).unwrap().rows.len()
    };
    assert_eq!(name_count, 3, "expected 3 foaf:name triples");

    // foaf:mbox: 1 triple
    let mbox_count = {
        let sparql = "PREFIX foaf: <http://xmlns.com/foaf/0.1/> SELECT ?s ?m WHERE { ?s foaf:mbox ?m }";
        run_sparql_query(&ds, sparql).unwrap().rows.len()
    };
    assert_eq!(mbox_count, 1, "expected 1 foaf:mbox triple");

    // Eve has exactly one foaf:name
    let eve_count = {
        let sparql = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/> SELECT ?s WHERE { ?s foaf:name "Eve" }"#;
        run_sparql_query(&ds, sparql).unwrap().rows.len()
    };
    assert_eq!(eve_count, 1, "expected Eve to have exactly one foaf:name");
}

// ── TestApi.TestDatalogReasoning ─────────────────────────────────────────────

/// Translated from `TestApi.TestDatalogReasoning`.
///
/// data.ttl: `ex:subject ex:predicate ex:object .`
/// rules.datalog: `[?s, ex:predicate, ex:object2] :- ex:predicate[?s, ex:object].`
/// Expected: after applying rules, `ex:subject ex:predicate ex:object2` exists.
#[test]
fn test_datalog_reasoning() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("data.ttl")).unwrap();

    const PRED: &str = "https://example.com/data#predicate";
    const OBJ: &str = "https://example.com/data#object";
    const OBJ2: &str = "https://example.com/data#object2";

    // Before rules: predicate→object exists, predicate→object2 does not
    assert_eq!(count_with_predicate_object(&ds, PRED, OBJ), 1);
    assert_eq!(count_with_predicate_object(&ds, PRED, OBJ2), 0, "object2 should not exist before rules");

    apply_rules(&mut ds, &[testdata("rules.datalog")]).unwrap();

    // After rules: both exist
    assert_eq!(count_with_predicate_object(&ds, PRED, OBJ), 1);
    assert_eq!(
        count_with_predicate_object(&ds, PRED, OBJ2),
        1,
        "Datalog reasoning should have derived ex:subject ex:predicate ex:object2"
    );
}

// ── TestApi.TestNamedGraphDatalogReasoning ────────────────────────────────────

/// Translated from `TestApi.TestNamedGraphDatalogReasoning`.
///
/// namedgraph.trig: `ex:graph { ex:subject ex:predicate ex:object. }`
/// namedgraph.datalog: `[?s, ex:predicate, ex:object2] ?graph :- ex:predicate[?s, ex:object] ?graph .`
#[test]
fn test_named_graph_datalog_reasoning() {
    // Use inline data matching the DagSemTools namedgraph.trig exactly
    let trig = r#"
prefix ex: <https://example.com/data#>
ex:graph { ex:subject ex:predicate ex:object. }
"#;
    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_trig(&mut ds, trig.as_bytes()).expect("TriG parse must succeed");

    const PRED: &str = "https://example.com/data#predicate";
    const OBJ: &str = "https://example.com/data#object";
    const OBJ2: &str = "https://example.com/data#object2";

    assert_eq!(count_with_predicate_object(&ds, PRED, OBJ), 1);
    assert_eq!(count_with_predicate_object(&ds, PRED, OBJ2), 0);

    apply_rules(&mut ds, &[testdata("namedgraph.datalog")]).unwrap();

    assert_eq!(
        count_with_predicate_object(&ds, PRED, OBJ2),
        1,
        "named-graph Datalog reasoning should derive object2 in the named graph"
    );
}

// ── TestApi.TestA ─────────────────────────────────────────────────────────────

/// Translated from `TestApi.TestA`.
///
/// test2.ttl contains `asset:Point-1 a data:property` (among others).
/// Verifies that querying by object `data:property` finds exactly one rdf:type triple.
#[test]
fn test_a_rdf_type_query() {
    let ds = load("test2.ttl");
    const DATA_PROPERTY: &str = "http://example.com/data#property";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    assert_eq!(
        count_with_predicate_object(&ds, RDF_TYPE, DATA_PROPERTY),
        1,
        "expected exactly one rdf:type data:property triple in test2.ttl"
    );
}

// ── TestApi.TestDatalog2 ──────────────────────────────────────────────────────

/// Translated from `TestApi.TestDatalog2`.
///
/// test2.datalog infers new `data:property` type assertions via terminal propagation.
/// Before: 1 object with rdf:type data:property.
/// After:  3 objects have rdf:type data:property.
#[test]
fn test_datalog2_propagation() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("test2.ttl")).unwrap();

    const DATA_PROPERTY: &str = "http://example.com/data#property";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

    assert_eq!(count_with_predicate_object(&ds, RDF_TYPE, DATA_PROPERTY), 1, "before rules");

    apply_rules(&mut ds, &[testdata("test2.datalog")]).unwrap();

    assert_eq!(
        count_with_predicate_object(&ds, RDF_TYPE, DATA_PROPERTY),
        3,
        "after rules: data:property should be inferred for 3 nodes via hasTerminal"
    );
}

// ── TestApi.TestDatalogStratified ─────────────────────────────────────────────

/// Translated from `TestApi.TestDatalogStratified`.
///
/// test_stratified.datalog uses stratified negation:
///   Type3[?x] :- Type[?x], NOT Type2[?x].
///   Type2[?x] :- NOT Type[?x], NOT Type4[?x].
///
/// Starting with data:Node rdf:type data:Type, after reasoning
/// data:Node should become rdf:type data:Type3 (not Type2, because NOT Type is false).
#[test]
fn test_datalog_stratified_negation() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("test_stratified.ttl")).unwrap();

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const DATA_TYPE: &str = "http://example.com/data#Type";
    const DATA_TYPE3: &str = "http://example.com/data#Type3";

    // Before: data:Node rdf:type data:Type (1), no data:Type3
    assert_eq!(count_with_predicate_object(&ds, RDF_TYPE, DATA_TYPE), 1, "before rules");
    assert_eq!(count_with_predicate_object(&ds, RDF_TYPE, DATA_TYPE3), 0, "no Type3 before rules");

    apply_rules(&mut ds, &[testdata("test_stratified.datalog")]).unwrap();

    // After: data:Node should also be data:Type3
    assert_eq!(
        count_with_predicate_object(&ds, RDF_TYPE, DATA_TYPE3),
        1,
        "stratified negation should derive data:Node rdf:type data:Type3"
    );
}

// ── SPARQL tests (TestApi.TestSparql1-6) ─────────────────────────────────────

fn parse_inline_ttl(ttl: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    turtle_parser::parse_turtle(&mut ds, ttl.as_bytes()).expect("inline Turtle must parse");
    ds
}

/// TestSparql1 — simple title retrieval (SPARQL 1.1 spec §2.1).
#[test]
fn sparql1_simple_title_retrieval() {
    let ds = parse_inline_ttl(
        r#"<http://example.org/book/book1> <http://purl.org/dc/elements/1.1/title> "SPARQL Tutorial" ."#,
    );
    let result = run_sparql_query(
        &ds,
        r#"SELECT ?title WHERE { <http://example.org/book/book1> <http://purl.org/dc/elements/1.1/title> ?title . }"#,
    )
    .unwrap();
    assert_eq!(result.rows.len(), 1);
    let title = graph_element_display(result.rows[0].get("title").unwrap());
    assert_eq!(title, "\"SPARQL Tutorial\"");
}

/// TestSparql2 — name+mbox join (SPARQL 1.1 spec §2.2).
#[test]
fn sparql2_name_mbox_join() {
    let ds = parse_inline_ttl(r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
_:a foaf:name "Johnny Lee Outlaw" .
_:a foaf:mbox <mailto:jlow@example.com> .
_:b foaf:name "Peter Goodguy" .
_:b foaf:mbox <mailto:peter@example.org> .
_:c foaf:mbox <mailto:carol@example.org> .
"#);
    let result = run_sparql_query(&ds, r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name ?mbox WHERE { ?x foaf:name ?name . ?x foaf:mbox ?mbox }
"#).unwrap();
    assert_eq!(result.rows.len(), 2, "two people have both name and mbox");
    for row in &result.rows {
        assert!(row.contains_key("name"));
        assert!(row.contains_key("mbox"));
    }
}

/// TestSparql3 — language-tagged literal matching (SPARQL 1.1 spec §2.3.1).
#[test]
fn sparql3_language_tagged_literal() {
    let ds = parse_inline_ttl(r#"
PREFIX ns: <http://example.org/ns#>
PREFIX :   <http://example.org/ns#>
:x ns:p "cat"@en .
:y ns:p "42"^^<http://www.w3.org/2001/XMLSchema#integer> .
"#);
    // Plain "cat" (no lang tag) matches nothing
    let r = run_sparql_query(&ds, r#"SELECT ?v WHERE { ?v ?p "cat" }"#).unwrap();
    assert_eq!(r.rows.len(), 0, "plain 'cat' should not match 'cat'@en");

    // "cat"@en matches :x
    let r = run_sparql_query(&ds, r#"SELECT ?v WHERE { ?v ?p "cat"@en }"#).unwrap();
    assert_eq!(r.rows.len(), 1);
    let v = graph_element_display(r.rows[0].get("v").unwrap());
    assert_eq!(v, "<http://example.org/ns#x>");
}

/// TestSparql4 — typed integer literal matching (SPARQL 1.1 spec §2.3.2).
#[test]
fn sparql4_typed_integer_literal() {
    let ds = parse_inline_ttl(r#"
PREFIX ns: <http://example.org/ns#>
PREFIX :   <http://example.org/ns#>
:x ns:p "cat"@en .
:y ns:p "42"^^<http://www.w3.org/2001/XMLSchema#integer> .
"#);
    let r = run_sparql_query(&ds, r#"SELECT ?v WHERE { ?v ?p 42 }"#).unwrap();
    assert_eq!(r.rows.len(), 1, "integer 42 should match the xsd:integer literal");
    let v = graph_element_display(r.rows[0].get("v").unwrap());
    assert_eq!(v, "<http://example.org/ns#y>");
}

/// TestSparql5 — custom datatype matching (SPARQL 1.1 spec §2.3.3).
#[test]
fn sparql5_custom_datatype_literal() {
    let ds = parse_inline_ttl(r#"
PREFIX dt: <http://example.org/datatype#>
PREFIX ns: <http://example.org/ns#>
PREFIX :   <http://example.org/ns#>
:z ns:p "abc"^^dt:specialDatatype .
"#);
    let r = run_sparql_query(
        &ds,
        r#"SELECT ?v WHERE { ?v ?p "abc"^^<http://example.org/datatype#specialDatatype> }"#,
    )
    .unwrap();
    assert_eq!(r.rows.len(), 1);
    let v = graph_element_display(r.rows[0].get("v").unwrap());
    assert_eq!(v, "<http://example.org/ns#z>");
}

/// TestSparql6 — blank node subjects (SPARQL 1.1 spec §2.4).
#[test]
fn sparql6_blank_node_subjects() {
    let ds = parse_inline_ttl(r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
_:a foaf:name "Alice" .
_:b foaf:name "Bob" .
"#);
    let result = run_sparql_query(
        &ds,
        r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/> SELECT ?x ?name WHERE { ?x foaf:name ?name }"#,
    )
    .unwrap();
    assert_eq!(result.rows.len(), 2, "Alice and Bob");
    let names: Vec<_> = result
        .rows
        .iter()
        .map(|r| graph_element_display(r.get("name").unwrap()))
        .collect();
    assert!(names.contains(&"\"Alice\"".to_string()));
    assert!(names.contains(&"\"Bob\"".to_string()));
    // Both x values should be blank nodes
    for row in &result.rows {
        let x = graph_element_display(row.get("x").unwrap());
        assert!(x.starts_with("_:"), "subject should be a blank node, got {}", x);
    }
}

// ── TestApi.TestSparqlOptionalPatterns ────────────────────────────────────────

/// Translated from `TestApi.TestSparqlOptionalPatterns`.
#[test]
fn sparql_optional_patterns() {
    let ds = parse_inline_ttl(r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
_:a rdf:type foaf:Person ; foaf:name "Alice" ; foaf:mbox <mailto:alice@example.com> .
_:a foaf:mbox <mailto:alice@work.example> .
_:b rdf:type foaf:Person ; foaf:name "Bob" .
"#);
    let result = run_sparql_query(&ds, r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name ?mbox WHERE {
    ?x foaf:name ?name .
    OPTIONAL { ?x foaf:mbox ?mbox }
}
"#).unwrap();
    // Alice has 2 mboxes (2 rows), Bob has 0 (1 row with unbound mbox) → 3 rows total
    assert_eq!(result.rows.len(), 3, "Alice×2 mboxes + Bob×0 mboxes = 3 rows");
    let names: Vec<_> = result
        .rows
        .iter()
        .map(|r| graph_element_display(r.get("name").unwrap()))
        .collect();
    assert!(names.iter().any(|n| n.contains("Alice")));
    assert!(names.iter().any(|n| n.contains("Bob")));
}

// ── TestApi.TestSparqlBasicJoin ───────────────────────────────────────────────

/// Translated from `TestApi.TestSparqlBasicJoin`.
#[test]
fn sparql_basic_join() {
    let ds = parse_inline_ttl(r#"
PREFIX ns: <http://example.org/ns#>
_:a ns:price 20 .
_:a ns:title "Cheap Book" .
"#);
    let result = run_sparql_query(&ds, r#"
PREFIX ns: <http://example.org/ns#>
SELECT ?title ?price WHERE { ?x ns:price ?price . ?x ns:title ?title . }
"#).unwrap();
    assert_eq!(result.rows.len(), 1, "one book with both price and title");
}

// ── TestApi.TestSparqlBind ────────────────────────────────────────────────────

/// Translated from `TestApi.TestSparqlBind`.
#[test]
fn sparql_bind_variable() {
    let ds = parse_inline_ttl(r#"
PREFIX ns: <http://example.org/ns#>
_:a ns:price 20 .
"#);
    let result = run_sparql_query(&ds, r#"
PREFIX ns: <http://example.org/ns#>
SELECT ?price ?double WHERE { ?x ns:price ?price . BIND(?price AS ?double) }
"#).unwrap();
    assert_eq!(result.rows.len(), 1);
    let price = graph_element_display(result.rows[0].get("price").unwrap());
    let double = graph_element_display(result.rows[0].get("double").unwrap());
    assert_eq!(price, double, "BIND(?price AS ?double) should give same value");
}

// ── TestApi.TestSparqlFilter ──────────────────────────────────────────────────

/// Translated from `TestApi.TestSparqlFilter`.
#[test]
fn sparql_filter_numeric() {
    let ds = parse_inline_ttl(r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
PREFIX ns: <http://example.org/ns#>
_:a ns:price 20 ; dc:title "Cheap Book" .
_:b ns:price 40 ; dc:title "Expensive Book" .
"#);
    // Without FILTER: both books
    let r = run_sparql_query(&ds, r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
PREFIX ns: <http://example.org/ns#>
SELECT ?title ?price WHERE { ?x ns:price ?price . ?x dc:title ?title . }
"#).unwrap();
    assert_eq!(r.rows.len(), 2, "both books without filter");

    // With FILTER price < 30: only cheap book
    let r = run_sparql_query(&ds, r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
PREFIX ns: <http://example.org/ns#>
SELECT ?title ?price WHERE { ?x ns:price ?price . ?x dc:title ?title . FILTER (?price < 30) }
"#).unwrap();
    assert_eq!(r.rows.len(), 1, "only the cheap book passes FILTER price < 30");
}

// ── TestApi.TestSparqlAggregate ───────────────────────────────────────────────

/// Translated from `TestApi.TestSparqlAggregate`.
/// Marked ignore because aggregate functions (SUM + GROUP BY) are not yet implemented.
#[test]
#[ignore = "aggregate functions (SUM / GROUP BY) not yet implemented in the SPARQL engine"]
fn sparql_aggregate_sum_group_by() {
    let ds = parse_inline_ttl(r#"
PREFIX : <http://books.example/>
:org1 :hasBook :book1 . :book1 :price 10 .
:org1 :hasBook :book2 . :book2 :price 20 .
:org2 :hasBook :book3 . :book3 :price 30 .
"#);
    let result = run_sparql_query(&ds, r#"
PREFIX : <http://books.example/>
SELECT ?org (SUM(?lprice) AS ?totalPrice)
WHERE { ?org :hasBook ?book . ?book :price ?lprice . }
GROUP BY ?org
"#).unwrap();
    assert_eq!(result.rows.len(), 2, "two organisations");
}

//! Integration tests for XML sources in the RML pipeline.
//!
//! These tests verify that XML data mapped via RML can be queried with SPARQL,
//! combined with Turtle ontologies, and reasoned over with OWL-RL — mirroring
//! the JSON integration tests in `rml_json_integration.rs` but using XML sources.
//!
//! Run just this file: `cargo test --test rml_xml_integration`

use dag_rdf::Datastore;
use dagalog::{apply_ontologies, graph_element_display, load_file, run_sparql_query};
use rml::apply_rml_mapping;
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn testdata_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
}

// ── XML + SPARQL SELECT ───────────────────────────────────────────────────────

#[test]
fn xml_mapped_data_is_queryable_with_sparql() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_xml_persons_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    let result = run_sparql_query(
        &ds,
        "PREFIX ex: <http://example.com/>
         SELECT ?name WHERE { ?p a ex:Person ; ex:name ?name . }",
    )
    .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|r| graph_element_display(r.get("name").unwrap()))
        .collect();

    assert_eq!(result.rows.len(), 2);
    assert!(names.contains(&"\"Alice\"".to_string()));
    assert!(names.contains(&"\"Bob\"".to_string()));
}

#[test]
fn xml_subject_iris_follow_template() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_xml_persons_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    let result = run_sparql_query(
        &ds,
        "PREFIX ex: <http://example.com/>
         SELECT ?name WHERE { <http://example.com/Person/1> ex:name ?name . }",
    )
    .unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        graph_element_display(result.rows[0].get("name").unwrap()),
        "\"Alice\""
    );
}

// ── XML + Turtle ontology ─────────────────────────────────────────────────────

#[test]
fn xml_combined_with_turtle_ontology() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("rml_hierarchy.ttl")).unwrap();
    apply_rml_mapping(
        &testdata("rml_xml_students_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    let result = run_sparql_query(
        &ds,
        "PREFIX ex: <http://example.com/>
         SELECT ?name WHERE { ?s a ex:Student ; ex:name ?name . }",
    )
    .unwrap();

    assert_eq!(result.rows.len(), 2, "both students from XML");
}

// ── XML + OWL-RL reasoning ────────────────────────────────────────────────────

#[test]
fn xml_plus_owlrl_reasoning_infers_superclass_membership() {
    let mut ds = Datastore::new(10_000);
    // Ontology: Student ⊆ Person ⊆ Agent
    load_file(&mut ds, &testdata("rml_hierarchy.ttl")).unwrap();
    // Map students from XML — generates rdf:type ex:Student via rml:class
    apply_rml_mapping(
        &testdata("rml_xml_students_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();
    apply_ontologies(&mut ds, &[]).unwrap();

    let person_count = run_sparql_query(
        &ds,
        "SELECT ?s WHERE { ?s a <http://example.com/Person> . }",
    )
    .unwrap()
    .rows
    .len();

    let agent_count =
        run_sparql_query(&ds, "SELECT ?s WHERE { ?s a <http://example.com/Agent> . }")
            .unwrap()
            .rows
            .len();

    assert_eq!(
        person_count, 2,
        "students inferred as Person via rdfs:subClassOf"
    );
    assert_eq!(
        agent_count, 2,
        "students inferred as Agent via transitive subClassOf"
    );
}

// ── XML nested iterator ───────────────────────────────────────────────────────

#[test]
fn xml_deep_xpath_iterator() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_xml_iterator_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    let result = run_sparql_query(
        &ds,
        "PREFIX ex: <http://example.com/>
         SELECT ?name WHERE { ?s ex:name ?name . }",
    )
    .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|r| graph_element_display(r.get("name").unwrap()))
        .collect();

    assert_eq!(result.rows.len(), 2);
    assert!(names.contains(&"\"Eve\"".to_string()));
    assert!(names.contains(&"\"Frank\"".to_string()));
}

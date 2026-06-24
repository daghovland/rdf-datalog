//! Integration tests for the `rml` crate wired into the dagalog pipeline.
//!
//! These tests verify that CSV data mapped via RML can be queried with SPARQL,
//! combined with existing Turtle ontologies, and reasoned over with OWL-RL.
//! They live here (root-crate tests) rather than inside `rml/tests/` so they
//! can cross crate boundaries freely.

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

// ── RML + SPARQL SELECT ───────────────────────────────────────────────────────

#[test]
//#[ignore]
fn rml_mapped_data_is_queryable_with_sparql_select() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_persons_mapping.ttl"),
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
    // graph_element_display wraps literals in SPARQL notation: "Alice"
    assert!(names.contains(&"\"Alice\"".to_string()));
    assert!(names.contains(&"\"Bob\"".to_string()));
}

#[test]
//#[ignore]
fn rml_sparql_subject_iris_follow_template() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_persons_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    let result = run_sparql_query(
        &ds,
        "PREFIX ex: <http://example.com/>
         SELECT ?p WHERE { <http://example.com/Person/1> ex:name ?name . BIND(<http://example.com/Person/1> AS ?p) }",
    )
    .unwrap();

    assert_eq!(
        result.rows.len(),
        1,
        "Person/1 (Alice) must exist with that exact IRI"
    );
}

#[test]
//#[ignore]
fn rml_class_shorthand_triples_visible_in_sparql() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_persons_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    // rml:class ex:Person should have generated rdf:type triples
    let result = run_sparql_query(
        &ds,
        "SELECT ?p WHERE { ?p a <http://example.com/Person> . }",
    )
    .unwrap();

    assert_eq!(
        result.rows.len(),
        2,
        "both CSV rows should yield rdf:type Person"
    );
}

#[test]
//#[ignore]
fn rml_sparql_filter_on_mapped_literal() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_persons_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    // age column maps to plain strings; filter by string equality
    let result = run_sparql_query(
        &ds,
        r#"PREFIX ex: <http://example.com/>
           SELECT ?name WHERE {
               ?p ex:name ?name ; ex:age ?age .
               FILTER(?age = "30")
           }"#,
    )
    .unwrap();

    assert_eq!(result.rows.len(), 1);
    // graph_element_display wraps literals: "Alice" → "\"Alice\""
    assert_eq!(
        graph_element_display(result.rows[0].get("name").unwrap()),
        "\"Alice\""
    );
}

// ── RML + Turtle ontology ─────────────────────────────────────────────────────

#[test]
//#[ignore]
fn rml_combined_with_turtle_ontology_in_same_datastore() {
    let mut ds = Datastore::new(10_000);
    // Load an ontology from Turtle (rdfs:subClassOf hierarchy)
    load_file(&mut ds, &testdata("rml_hierarchy.ttl")).unwrap();
    // Map CSV data using predicates from that ontology
    apply_rml_mapping(
        &testdata("rml_students_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    // Both ontology triples and mapped instance triples should be present
    let result = run_sparql_query(
        &ds,
        "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
         PREFIX ex: <http://example.com/>
         SELECT ?name WHERE { ?s a ex:Student ; ex:name ?name . }",
    )
    .unwrap();

    assert_eq!(result.rows.len(), 2, "both students from CSV");
}

// ── RML + OWL-RL reasoning ────────────────────────────────────────────────────

#[test]
//#[ignore]
fn rml_plus_owlrl_reasoning_infers_superclass_membership() {
    let mut ds = Datastore::new(10_000);
    // Ontology: Student ⊆ Person ⊆ Agent
    load_file(&mut ds, &testdata("rml_hierarchy.ttl")).unwrap();
    // Map students from CSV — generates rdf:type ex:Student triples via rml:class
    apply_rml_mapping(
        &testdata("rml_students_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();
    // Run OWL-RL reasoning: extracts axioms from the loaded ontology and applies them
    apply_ontologies(&mut ds, &[]).unwrap();

    // After reasoning, Students should be inferred as Person and Agent too
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
        "both students inferred as Person via rdfs:subClassOf"
    );
    assert_eq!(
        agent_count, 2,
        "both students inferred as Agent via transitive subClassOf"
    );
}

#[test]
//#[ignore]
fn rml_owlrl_does_not_infer_subclass_without_ontology() {
    let mut ds = Datastore::new(10_000);
    // Map students without loading the hierarchy ontology
    apply_rml_mapping(
        &testdata("rml_students_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();
    apply_ontologies(&mut ds, &[]).unwrap();

    // Without the rdfs:subClassOf axiom, Student instances must not be inferred as Person
    let result = run_sparql_query(
        &ds,
        "SELECT ?s WHERE { ?s a <http://example.com/Person> . }",
    )
    .unwrap();

    assert_eq!(
        result.rows.len(),
        0,
        "no ontology loaded — Person membership must not be inferred"
    );
}

// ── RML two-source mapping ────────────────────────────────────────────────────

#[test]
//#[ignore]
fn rml_mapping_with_two_triples_maps_populates_both() {
    let mut ds = Datastore::new(10_000);
    // mapping file has two TriplesMap blocks: one for persons.csv, one for students.csv
    apply_rml_mapping(
        &testdata("rml_two_sources_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    let name_pred = "PREFIX ex: <http://example.com/> SELECT ?name WHERE { ?s ex:name ?name . }";
    let result = run_sparql_query(&ds, name_pred).unwrap();

    // persons.csv has 2 rows, students.csv has 2 rows — 4 name triples total
    assert_eq!(result.rows.len(), 4);
}

#[test]
//#[ignore]
fn rml_two_maps_subjects_have_distinct_iris() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_two_sources_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    // Persons use /Person/{id}, Students use /Student/{id} — no IRI collision.
    // Verify by checking the specific IRIs from each source (persons.csv has id 1,2;
    // students.csv has id 101,102).
    let person1 = run_sparql_query(
        &ds,
        "SELECT ?n WHERE { <http://example.com/Person/1> <http://example.com/name> ?n . }",
    )
    .unwrap();
    let person2 = run_sparql_query(
        &ds,
        "SELECT ?n WHERE { <http://example.com/Person/2> <http://example.com/name> ?n . }",
    )
    .unwrap();
    let student101 = run_sparql_query(
        &ds,
        "SELECT ?n WHERE { <http://example.com/Student/101> <http://example.com/name> ?n . }",
    )
    .unwrap();
    let student102 = run_sparql_query(
        &ds,
        "SELECT ?n WHERE { <http://example.com/Student/102> <http://example.com/name> ?n . }",
    )
    .unwrap();

    assert_eq!(person1.rows.len(), 1);
    assert_eq!(person2.rows.len(), 1);
    assert_eq!(student101.rows.len(), 1);
    assert_eq!(student102.rows.len(), 1);
}

// ── Idempotency ───────────────────────────────────────────────────────────────

#[test]
//#[ignore]
fn applying_same_mapping_twice_does_not_duplicate_triples() {
    let mut ds = Datastore::new(10_000);
    apply_rml_mapping(
        &testdata("rml_persons_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();
    apply_rml_mapping(
        &testdata("rml_persons_mapping.ttl"),
        &testdata_dir(),
        &mut ds,
    )
    .unwrap();

    // The quad tables deduplicate — triple count must equal single-application count
    let result = run_sparql_query(
        &ds,
        "PREFIX ex: <http://example.com/>
         SELECT ?name WHERE { ?p ex:name ?name . }",
    )
    .unwrap();

    assert_eq!(
        result.rows.len(),
        2,
        "duplicate triples must be deduped by the quad table"
    );
}

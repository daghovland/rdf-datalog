/// End-to-end tests for `rml:joinCondition` (cross-source-map joins).
/// Red phase: fixtures and test bodies are in place, but `translate.rs`/
/// `engine.rs` do not construct or execute `LogicalPlan::Join` yet, so all
/// tests here are `#[ignore]`d until the join is implemented.
/// See `docs/plans/RML_JOIN_PLAN.md` for the design.
use dag_rdf::ingress::{Quad, Triple};
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use rml::apply_rml_mapping;
use std::path::Path;

fn fixture(case: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(case)
}

fn iri(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

macro_rules! intern {
    ($ds:expr, $elem:expr) => {
        $ds.add_resource($elem)
    };
}

// ── rmltc0009a_join: child/parent join via rml:joinCondition ────────────────

#[test]
#[ignore]
fn rmltc0009a_join_matched_row_produces_object_iri_from_parent_subject() {
    let dir = fixture("rmltc0009a_join");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/student/10"));
    let p = intern!(ds, iri("http://example.com/practises"));
    let o = intern!(ds, iri("http://example.com/sport/100"));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
#[ignore]
fn rmltc0009a_join_unmatched_row_produces_no_join_triple() {
    let dir = fixture("rmltc0009a_join");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    // student 20 has an empty Sport column, so it has no matching parent row
    // and must not get an ex:practises triple at all.
    let s = intern!(ds, iri("http://example.com/student/20"));
    let p = intern!(ds, iri("http://example.com/practises"));
    assert_eq!(
        ds.get_triples_with_subject(s)
            .filter(|t| t.predicate == p)
            .count(),
        0
    );
}

#[test]
#[ignore]
fn rmltc0009a_join_unmatched_row_still_gets_non_join_triples() {
    let dir = fixture("rmltc0009a_join");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/student/20"));
    let p = intern!(ds, iri("http://xmlns.com/foaf/0.1/name"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Demi Moore".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
#[ignore]
fn rmltc0009a_join_parent_triples_map_also_produces_its_own_triples() {
    let dir = fixture("rmltc0009a_join");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/sport/100"));
    let p = intern!(ds, iri("http://www.w3.org/2000/01/rdf-schema#label"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Tennis".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
#[ignore]
fn rmltc0009a_join_produces_exactly_one_join_triple_for_matched_row() {
    let dir = fixture("rmltc0009a_join");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/student/10"));
    let p = intern!(ds, iri("http://example.com/practises"));
    assert_eq!(
        ds.get_triples_with_subject(s)
            .filter(|t| t.predicate == p)
            .count(),
        1
    );
}

// ── rmltc0009b_join: same join, with the join triple placed in a named graph ─

#[test]
#[ignore]
fn rmltc0009b_join_with_named_graphs_matched_row() {
    let dir = fixture("rmltc0009b_join");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let g = intern!(ds, iri("http://example.com/PractisesGraph"));
    let s = intern!(ds, iri("http://example.com/student/10"));
    let p = intern!(ds, iri("http://example.com/practises"));
    let o = intern!(ds, iri("http://example.com/sport/100"));
    assert!(ds.contains_quad(&Quad {
        triple_id: g,
        subject: s,
        predicate: p,
        obj: o
    }));
}

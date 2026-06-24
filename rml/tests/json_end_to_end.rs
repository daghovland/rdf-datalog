/// End-to-end tests against JSON/JSONL RML fixtures.
/// Each test loads a mapping.ttl and applies it to the JSON source in the same
/// directory, then asserts the expected RDF triples are present.
use dag_rdf::ingress::{Quad, Triple};
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use ingress::RDF_TYPE;
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

// ── rmltc0001b: simple JSON template IRI + reference literal ─────────────────

#[test]
fn rmltc0001b_basic_json_template_and_reference_literal() {
    let dir = fixture("rmltc0001b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    let p = intern!(ds, iri("http://example.com/name"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Venus Williams".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
fn rmltc0001b_produces_exactly_one_triple() {
    let dir = fixture("rmltc0001b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    assert_eq!(ds.get_triples_with_subject(s).count(), 1);
}

// ── rmltc0002b: multiple predicates from JSON fields ─────────────────────────

#[test]
fn rmltc0002b_all_four_triples_present() {
    let dir = fixture("rmltc0002b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Person/1"));
    assert_eq!(ds.get_triples_with_subject(s).count(), 2);

    let p_name = intern!(ds, iri("http://example.com/name"));
    let o_name = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p_name,
        obj: o_name
    }));

    let p_age = intern!(ds, iri("http://example.com/age"));
    let o_age = ds.add_literal_resource(RdfLiteral::LiteralString("30".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p_age,
        obj: o_age
    }));
}

// ── rmltc0007c: language-tagged literal ──────────────────────────────────────

#[test]
fn rmltc0007c_language_tagged_literal() {
    let dir = fixture("rmltc0007c");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    let p = intern!(ds, iri("http://example.com/sport"));
    let o = ds.add_literal_resource(RdfLiteral::LangLiteral {
        lang: "en".to_string(),
        literal: "Tennis".to_string(),
    });
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── rmltc0007d: datatype literal ─────────────────────────────────────────────

#[test]
fn rmltc0007d_datatype_literal() {
    let dir = fixture("rmltc0007d");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    let p = intern!(ds, iri("http://example.com/age"));
    let o = ds.add_literal_resource(RdfLiteral::TypedLiteral {
        type_iri: IriReference("http://www.w3.org/2001/XMLSchema#integer".to_string()),
        literal: "32".to_string(),
    });
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── rmltc0009b: triple in a named graph ──────────────────────────────────────

#[test]
fn rmltc0009b_triple_placed_in_named_graph() {
    let dir = fixture("rmltc0009b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let g = intern!(ds, iri("http://example.com/CityGraph"));
    let s = intern!(ds, iri("http://example.com/City/Paris"));
    let p = intern!(ds, iri("http://example.com/country"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("France".to_string()));
    assert!(ds.contains_quad(&Quad {
        triple_id: g,
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
fn rmltc0009b_triple_not_in_default_graph() {
    let dir = fixture("rmltc0009b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/City/Paris"));
    let p = intern!(ds, iri("http://example.com/country"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("France".to_string()));
    assert!(!ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── rmltc0010b: rml:class shorthand ──────────────────────────────────────────

#[test]
fn rmltc0010b_class_shorthand_emits_rdf_type_triple() {
    let dir = fixture("rmltc0010b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    let p = intern!(ds, iri(RDF_TYPE));
    let o = intern!(ds, iri("http://example.com/Student"));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
fn rmltc0010b_produces_exactly_two_triples() {
    let dir = fixture("rmltc0010b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    assert_eq!(ds.get_triples_with_subject(s).count(), 2);
}

// ── rmltc0014a: nested JSONPath reference ─────────────────────────────────────

#[test]
fn rmltc0014a_nested_jsonpath_extracts_deep_field() {
    let dir = fixture("rmltc0014a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    let p = intern!(ds, iri("http://example.com/name"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Venus Williams".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── rmltc0015a: rml:iterator selects nested array ────────────────────────────

#[test]
fn rmltc0015a_iterator_yields_two_rows() {
    let dir = fixture("rmltc0015a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s10 = intern!(ds, iri("http://example.com/Student/10"));
    let s11 = intern!(ds, iri("http://example.com/Student/11"));
    let p = intern!(ds, iri("http://example.com/name"));
    let o10 = ds.add_literal_resource(RdfLiteral::LiteralString("Venus Williams".to_string()));
    let o11 = ds.add_literal_resource(RdfLiteral::LiteralString("Tom Johnson".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s10,
        predicate: p,
        obj: o10
    }));
    assert!(ds.contains_triple(&Triple {
        subject: s11,
        predicate: p,
        obj: o11
    }));
}

#[test]
fn rmltc0015a_each_subject_has_exactly_one_triple() {
    let dir = fixture("rmltc0015a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s10 = intern!(ds, iri("http://example.com/Student/10"));
    let s11 = intern!(ds, iri("http://example.com/Student/11"));
    assert_eq!(ds.get_triples_with_subject(s10).count(), 1);
    assert_eq!(ds.get_triples_with_subject(s11).count(), 1);
}

// ── jsonl_basic: JSONL source ─────────────────────────────────────────────────

#[test]
fn jsonl_basic_two_rows_mapped_to_two_subjects() {
    let dir = fixture("jsonl_basic");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s10 = intern!(ds, iri("http://example.com/Person/10"));
    let s11 = intern!(ds, iri("http://example.com/Person/11"));
    let p = intern!(ds, iri("http://example.com/name"));
    let o10 = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let o11 = ds.add_literal_resource(RdfLiteral::LiteralString("Bob".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s10,
        predicate: p,
        obj: o10
    }));
    assert!(ds.contains_triple(&Triple {
        subject: s11,
        predicate: p,
        obj: o11
    }));
}

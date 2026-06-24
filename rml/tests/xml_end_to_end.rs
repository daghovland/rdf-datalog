/// End-to-end tests against XML RML fixtures.
/// Each test loads a mapping.ttl and applies it to the XML source in the same
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

// ── rmltc0001c: simple XML template IRI + reference literal ──────────────────

#[test]
fn rmltc0001c_basic_xml_template_and_reference_literal() {
    let dir = fixture("rmltc0001c");
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
fn rmltc0001c_produces_exactly_one_triple() {
    let dir = fixture("rmltc0001c");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    assert_eq!(ds.get_triples_with_subject(s).count(), 1);
}

// ── rmltc0002c: multiple predicates from XML elements ────────────────────────

#[test]
fn rmltc0002c_all_four_triples_present() {
    let dir = fixture("rmltc0002c");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let name_p = intern!(ds, iri("http://example.com/name"));
    let age_p = intern!(ds, iri("http://example.com/age"));
    let alice_s = intern!(ds, iri("http://example.com/Person/1"));
    let bob_s = intern!(ds, iri("http://example.com/Person/2"));
    let alice_name = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let bob_name = ds.add_literal_resource(RdfLiteral::LiteralString("Bob".to_string()));
    let alice_age = ds.add_literal_resource(RdfLiteral::LiteralString("30".to_string()));
    let bob_age = ds.add_literal_resource(RdfLiteral::LiteralString("25".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: alice_s,
        predicate: name_p,
        obj: alice_name
    }));
    assert!(ds.contains_triple(&Triple {
        subject: alice_s,
        predicate: age_p,
        obj: alice_age
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob_s,
        predicate: name_p,
        obj: bob_name
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob_s,
        predicate: age_p,
        obj: bob_age
    }));
}

// ── rmltc0007e: language-tagged literal from XML element ─────────────────────

#[test]
fn rmltc0007e_language_tagged_literal() {
    let dir = fixture("rmltc0007e");
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

// ── rmltc0007f: datatype literal from XML element ────────────────────────────

#[test]
fn rmltc0007f_datatype_literal() {
    let dir = fixture("rmltc0007f");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    let p = intern!(ds, iri("http://example.com/age"));
    let o = ds.add_literal_resource(RdfLiteral::TypedLiteral {
        type_iri: IriReference("http://www.w3.org/2001/XMLSchema#integer".to_string()),
        literal: "30".to_string(),
    });
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── rmltc0009c: named graph from XML mapping ─────────────────────────────────

#[test]
fn rmltc0009c_named_graph() {
    let dir = fixture("rmltc0009c");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let g = intern!(ds, iri("http://example.com/CityGraph"));
    let s = intern!(ds, iri("http://example.com/City/Amsterdam"));
    let p = intern!(ds, iri("http://example.com/country"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Netherlands".to_string()));
    assert!(ds.contains_quad(&Quad {
        triple_id: g,
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── rmltc0010c: rml:class shorthand injects rdf:type ─────────────────────────

#[test]
fn rmltc0010c_rml_class_injects_rdf_type() {
    let dir = fixture("rmltc0010c");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri("http://example.com/Student/10"));
    let rdf_type = intern!(ds, iri(RDF_TYPE));
    let student_class = intern!(ds, iri("http://example.com/Student"));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: rdf_type,
        obj: student_class,
    }));
}

// ── rmltc0014b: nested XML element via multi-step XPath ──────────────────────

#[test]
fn rmltc0014b_nested_element_xpath() {
    let dir = fixture("rmltc0014b");
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

// ── rmltc0015b: multiple XML elements via iterator ───────────────────────────

#[test]
fn rmltc0015b_iterator_yields_two_students() {
    let dir = fixture("rmltc0015b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let p = intern!(ds, iri("http://example.com/name"));
    let s1 = intern!(ds, iri("http://example.com/Student/1"));
    let s2 = intern!(ds, iri("http://example.com/Student/2"));
    let alice = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let bob = ds.add_literal_resource(RdfLiteral::LiteralString("Bob".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: s1,
        predicate: p,
        obj: alice
    }));
    assert!(ds.contains_triple(&Triple {
        subject: s2,
        predicate: p,
        obj: bob
    }));
}

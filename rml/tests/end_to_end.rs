/// End-to-end tests against W3C RML test case fixtures (CSV subset).
/// Each test loads a mapping.ttl from tests/fixtures/<case>/ and applies it to
/// the CSV source in the same directory, then asserts the expected RDF triples
/// are present in the resulting Datastore.
use dag_rdf::ingress::{Quad, Triple};
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use ingress::RDF_TYPE;
use rml::{RmlError, apply_rml_mapping};
use std::path::Path;

fn fixture(case: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(case)
}

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

/// Returns the GraphElementId of `elem`, interning it if necessary.
/// Safe to call after `apply_rml_mapping` because the element was already
/// interned during mapping execution; this call just looks it up.
macro_rules! intern {
    ($ds:expr, $elem:expr) => {
        $ds.add_resource($elem)
    };
}

// ── RMLTC0001a: simple template IRI subject + reference literal object ─────────

#[test]
//#[ignore]
fn rmltc0001a_basic_template_iri_and_reference_literal() {
    let dir = fixture("rmltc0001a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri_element("http://example.com/Student/10"));
    let p = intern!(ds, iri_element("http://example.com/name"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Venus Williams".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
//#[ignore]
fn rmltc0001a_produces_exactly_one_triple() {
    let dir = fixture("rmltc0001a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = ds.add_resource(iri_element("http://example.com/Student/10"));
    let count = ds.get_triples_with_subject(s).count();
    assert_eq!(count, 1);
}

// ── RMLTC0002a: multiple predicates, two rows ──────────────────────────────────

#[test]
//#[ignore]
fn rmltc0002a_all_four_triples_present() {
    let dir = fixture("rmltc0002a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let name_pred = intern!(ds, iri_element("http://example.com/name"));
    let age_pred = intern!(ds, iri_element("http://example.com/age"));

    let alice_s = intern!(ds, iri_element("http://example.com/Person/1"));
    let bob_s = intern!(ds, iri_element("http://example.com/Person/2"));

    let alice_name = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let bob_name = ds.add_literal_resource(RdfLiteral::LiteralString("Bob".to_string()));
    let alice_age = ds.add_literal_resource(RdfLiteral::LiteralString("30".to_string()));
    let bob_age = ds.add_literal_resource(RdfLiteral::LiteralString("25".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: alice_s,
        predicate: name_pred,
        obj: alice_name
    }));
    assert!(ds.contains_triple(&Triple {
        subject: alice_s,
        predicate: age_pred,
        obj: alice_age
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob_s,
        predicate: name_pred,
        obj: bob_name
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob_s,
        predicate: age_pred,
        obj: bob_age
    }));
}

// ── RMLTC0003a: blank node subject ────────────────────────────────────────────

#[test]
//#[ignore]
fn rmltc0003a_blank_node_subject_has_correct_object() {
    let dir = fixture("rmltc0003a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let country_pred = intern!(ds, iri_element("http://example.com/country"));
    let usa_obj = ds.add_literal_resource(RdfLiteral::LiteralString("United States".to_string()));

    // The subject is a blank node — we verify via object+predicate lookup
    let triples: Vec<_> = ds
        .get_triples_with_object_predicate(usa_obj, country_pred)
        .collect();
    assert_eq!(
        triples.len(),
        1,
        "expected exactly one triple with object 'United States'"
    );
}

#[test]
//#[ignore]
fn rmltc0003a_subject_is_blank_node() {
    let dir = fixture("rmltc0003a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let country_pred = intern!(ds, iri_element("http://example.com/country"));
    let usa_obj = ds.add_literal_resource(RdfLiteral::LiteralString("United States".to_string()));

    let triples: Vec<_> = ds
        .get_triples_with_object_predicate(usa_obj, country_pred)
        .collect();
    let subject_id = triples[0].subject;
    // Blank nodes are stored as AnonymousBlankNode variants
    assert!(
        matches!(
            ds.resources.get_graph_element(subject_id),
            GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(_))
        ),
        "subject should be a blank node"
    );
}

// ── RMLTC0007a: language-tagged literal ───────────────────────────────────────

#[test]
//#[ignore]
fn rmltc0007a_language_tagged_literal() {
    let dir = fixture("rmltc0007a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri_element("http://example.com/Person/1"));
    let p = intern!(ds, iri_element("http://example.com/name"));
    let o = ds.add_literal_resource(RdfLiteral::LangLiteral {
        lang: "en".to_string(),
        literal: "Alice".to_string(),
    });

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── RMLTC0007b: datatype literal ──────────────────────────────────────────────

#[test]
//#[ignore]
fn rmltc0007b_datatype_literal_stored_as_typed_literal() {
    let dir = fixture("rmltc0007b");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri_element("http://example.com/Person/Alice"));
    let p = intern!(ds, iri_element("http://example.com/score"));
    let o = ds.add_literal_resource(RdfLiteral::TypedLiteral {
        type_iri: IriReference("http://www.w3.org/2001/XMLSchema#integer".to_string()),
        literal: "42".to_string(),
    });

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── RMLTC0009a: named graph ────────────────────────────────────────────────────

#[test]
//#[ignore]
fn rmltc0009a_triple_placed_in_named_graph() {
    let dir = fixture("rmltc0009a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let graph_id = intern!(ds, iri_element("http://example.com/CityGraph"));
    let s = intern!(ds, iri_element("http://example.com/City/Paris"));
    let p = intern!(ds, iri_element("http://example.com/country"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("France".to_string()));

    assert!(ds.contains_quad(&Quad {
        triple_id: graph_id,
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
//#[ignore]
fn rmltc0009a_triple_not_in_default_graph() {
    let dir = fixture("rmltc0009a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri_element("http://example.com/City/Paris"));
    let p = intern!(ds, iri_element("http://example.com/country"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("France".to_string()));

    // Must NOT be in the default graph
    assert!(!ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

// ── RMLTC0010a: rml:class shorthand ───────────────────────────────────────────

#[test]
//#[ignore]
fn rmltc0010a_class_shorthand_emits_rdf_type_triple() {
    let dir = fixture("rmltc0010a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri_element("http://example.com/Student/10"));
    let rdf_type = intern!(ds, iri_element(RDF_TYPE));
    let student_class = intern!(ds, iri_element("http://example.com/Student"));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: rdf_type,
        obj: student_class
    }));
}

#[test]
//#[ignore]
fn rmltc0010a_data_triple_also_present() {
    let dir = fixture("rmltc0010a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri_element("http://example.com/Student/10"));
    let p = intern!(ds, iri_element("http://example.com/name"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
//#[ignore]
fn rmltc0010a_produces_exactly_two_triples() {
    let dir = fixture("rmltc0010a");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = intern!(ds, iri_element("http://example.com/Student/10"));
    let count = ds.get_triples_with_subject(s).count();
    assert_eq!(count, 2, "expected rdf:type triple + name triple");
}

// ── IRI percent-encoding in templates ─────────────────────────────────────────

#[test]
//#[ignore]
fn spaces_in_csv_values_are_percent_encoded_in_iri_subjects() {
    let dir = fixture("encoding");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    // "Venus Williams" in IRI position → Venus%20Williams
    let s = intern!(
        ds,
        iri_element("http://example.com/Person/Venus%20Williams")
    );
    let p = intern!(ds, iri_element("http://example.com/name"));
    let triples: Vec<_> = ds.get_triples_with_subject_predicate(s, p).collect();
    assert_eq!(triples.len(), 1);
}

#[test]
//#[ignore]
fn spaces_in_csv_values_are_not_encoded_in_literal_objects() {
    let dir = fixture("encoding");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    // "Venus Williams" as a literal object must stay "Venus Williams", not encoded
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Venus Williams".to_string()));
    let p = intern!(ds, iri_element("http://example.com/label"));
    let triples: Vec<_> = ds.get_triples_with_object_predicate(o, p).collect();
    assert_eq!(triples.len(), 1);
}

// ── Security: path traversal via absolute rml:source ──────────────────────────

#[test]
fn absolute_rml_source_path_is_rejected() {
    // A mapping that uses an absolute rml:source path must be rejected even if
    // base_dir.join(absolute) would silently resolve to the absolute path.
    let tmp = tempfile::tempdir().unwrap();
    let mapping_ttl = tmp.path().join("mapping.ttl");
    std::fs::write(
        &mapping_ttl,
        r#"@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

ex:TM a rml:TriplesMap ;
  rml:logicalSource [
    rml:source "/etc/hostname" ;
    rml:referenceFormulation rml:CSV
  ] ;
  rml:subjectMap  [ rml:template "http://example.com/{col1}" ] ;
  rml:predicateObjectMap [
    rml:predicate ex:name ;
    rml:objectMap [ rml:reference "col1" ]
  ] .
"#,
    )
    .unwrap();
    let mut ds = Datastore::new(64);
    let err = apply_rml_mapping(&mapping_ttl, tmp.path(), &mut ds)
        .expect_err("absolute rml:source path must be rejected");
    assert!(
        matches!(err, RmlError::PathTraversal { .. }),
        "expected PathTraversal, got: {err}"
    );
}

#[test]
fn dotdot_rml_source_path_is_rejected() {
    // A mapping that uses a relative path that escapes base_dir via ".." must
    // also be rejected.
    let tmp = tempfile::tempdir().unwrap();
    let subdir = tmp.path().join("sub");
    std::fs::create_dir(&subdir).unwrap();
    let mapping_ttl = subdir.join("mapping.ttl");
    std::fs::write(
        &mapping_ttl,
        r#"@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

ex:TM a rml:TriplesMap ;
  rml:logicalSource [
    rml:source "../../../etc/hostname" ;
    rml:referenceFormulation rml:CSV
  ] ;
  rml:subjectMap  [ rml:template "http://example.com/{col1}" ] ;
  rml:predicateObjectMap [
    rml:predicate ex:name ;
    rml:objectMap [ rml:reference "col1" ]
  ] .
"#,
    )
    .unwrap();
    let mut ds = Datastore::new(64);
    let err = apply_rml_mapping(&mapping_ttl, &subdir, &mut ds)
        .expect_err("dot-dot rml:source path must be rejected");
    assert!(
        matches!(err, RmlError::PathTraversal { .. }),
        "expected PathTraversal, got: {err}"
    );
}

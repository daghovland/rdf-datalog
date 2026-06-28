/// Phase 8: load stOTTR templates and instances from fixture files on disk.
/// https://github.com/daghovland/rdf-datalog/issues/21
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use ottr::{expand_documents, load_stottr_file};
use std::path::Path;

fn literal_element(s: &str) -> dag_rdf::RdfLiteral {
    RdfLiteral::LiteralString(s.to_string())
}

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Load a single file that contains both template definitions and instances.
#[test]
fn load_combined_file_expands_to_correct_triples() {
    let doc = load_stottr_file(&fixtures_dir().join("combined.stottr")).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let rdf_type = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let rdfs_label = ds.add_resource(iri_element("http://www.w3.org/2000/01/rdf-schema#label"));
    let ex_thing = ds.add_resource(iri_element("http://example.com/Thing"));
    let widget = ds.add_resource(iri_element("http://example.com/Widget"));
    let gadget = ds.add_resource(iri_element("http://example.com/Gadget"));
    let widget_label = ds.add_literal_resource(RdfLiteral::LiteralString("Widget".to_string()));
    let gadget_label = ds.add_literal_resource(RdfLiteral::LiteralString("Gadget".to_string()));

    // Widget → rdf:type ex:Thing, rdfs:label "Widget"
    assert!(ds.contains_triple(&Triple {
        subject: widget,
        predicate: rdf_type,
        obj: ex_thing
    }));
    assert!(ds.contains_triple(&Triple {
        subject: widget,
        predicate: rdfs_label,
        obj: widget_label
    }));

    // Gadget → rdf:type ex:Thing, rdfs:label "Gadget"
    assert!(ds.contains_triple(&Triple {
        subject: gadget,
        predicate: rdf_type,
        obj: ex_thing
    }));
    assert!(ds.contains_triple(&Triple {
        subject: gadget,
        predicate: rdfs_label,
        obj: gadget_label
    }));
}

/// Load templates from one file and instances from another (two-file pattern).
#[test]
fn load_two_files_person_template_and_instances() {
    let fixtures = fixtures_dir();
    let template_doc = load_stottr_file(&fixtures.join("person_template.stottr")).unwrap();
    let instance_doc = load_stottr_file(&fixtures.join("person_instances.stottr")).unwrap();

    let mut ds = Datastore::new(100);
    expand_documents(&[template_doc, instance_doc], &mut ds).unwrap();

    let rdf_type = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let foaf_person = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/Person"));
    let foaf_name = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/name"));
    let foaf_mbox = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/mbox"));

    let alice = ds.add_resource(iri_element("http://example.com/alice"));
    let bob = ds.add_resource(iri_element("http://example.com/bob"));
    let alice_name = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let bob_name = ds.add_literal_resource(RdfLiteral::LiteralString("Bob".to_string()));
    let alice_mail = ds.add_resource(iri_element("mailto:alice@example.com"));
    let bob_mail = ds.add_resource(iri_element("mailto:bob@example.com"));

    // Alice: rdf:type foaf:Person, foaf:name "Alice", foaf:mbox mailto:alice@example.com
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: rdf_type,
        obj: foaf_person
    }));
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: foaf_name,
        obj: alice_name
    }));
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: foaf_mbox,
        obj: alice_mail
    }));

    // Bob: rdf:type foaf:Person, foaf:name "Bob", foaf:mbox mailto:bob@example.com
    assert!(ds.contains_triple(&Triple {
        subject: bob,
        predicate: rdf_type,
        obj: foaf_person
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob,
        predicate: foaf_name,
        obj: bob_name
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob,
        predicate: foaf_mbox,
        obj: bob_mail
    }));
}

/// cross_types.stottr: `ex:Types` delegates to `ex:Type` with `cross` expansion,
/// producing one `rdf:type` triple per (instance, type) combination.
#[test]
fn cross_types_fixture_produces_cartesian_type_triples() {
    let doc = load_stottr_file(&fixtures_dir().join("cross_types.stottr")).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let rdf_type = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let ann = ds.add_resource(iri_element("http://example.com/ann"));
    let carol = ds.add_resource(iri_element("http://example.com/carol"));
    let person = ds.add_resource(iri_element("http://example.com/Person"));
    let employee = ds.add_resource(iri_element("http://example.com/Employee"));
    let manager = ds.add_resource(iri_element("http://example.com/Manager"));

    // ann gets both Person and Employee
    assert!(ds.contains_triple(&Triple {
        subject: ann,
        predicate: rdf_type,
        obj: person
    }));
    assert!(ds.contains_triple(&Triple {
        subject: ann,
        predicate: rdf_type,
        obj: employee
    }));
    // carol gets both Person and Manager
    assert!(ds.contains_triple(&Triple {
        subject: carol,
        predicate: rdf_type,
        obj: person
    }));
    assert!(ds.contains_triple(&Triple {
        subject: carol,
        predicate: rdf_type,
        obj: manager
    }));
    // exactly 4 rdf:type triples total
    assert_eq!(ds.get_triples_with_predicate(rdf_type).count(), 4);
}

/// zipmin_names.stottr: `ex:PairedNames` delegates to `ex:PersonName` with `zipMin`
/// expansion, pairing persons and names by index and truncating to the shorter list.
#[test]
fn zipmin_names_fixture_pairs_by_index_and_drops_excess() {
    let doc = load_stottr_file(&fixtures_dir().join("zipmin_names.stottr")).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let foaf_name = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/name"));
    let alice = ds.add_resource(iri_element("http://example.com/alice"));
    let bob = ds.add_resource(iri_element("http://example.com/bob"));
    let carol = ds.add_resource(iri_element("http://example.com/carol"));
    let alice_name = ds.add_literal_resource(literal_element("Alice"));
    let bob_name = ds.add_literal_resource(literal_element("Bob"));

    // alice and bob are paired
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: foaf_name,
        obj: alice_name
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob,
        predicate: foaf_name,
        obj: bob_name
    }));
    // carol has no name triple (list truncated at min(3,2)=2)
    assert_eq!(ds.get_triples_with_subject(carol).count(), 0);
    // exactly 2 foaf:name triples total
    assert_eq!(ds.get_triples_with_predicate(foaf_name).count(), 2);
}

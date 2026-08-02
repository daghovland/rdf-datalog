/// wOTTR: RDF/Turtle representation of OTTR templates and instances.
/// https://github.com/daghovland/rdf-datalog/issues/246
/// Spec consulted: https://spec.ottr.xyz/wOTTR/0.4.5/
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use ottr::ast::{Argument, Term};
use ottr::types::OttrType;
use ottr::wottr::parse_wottr_str;
use std::path::Path;

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("wottr")
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixtures_dir().join(name)).unwrap()
}

/// Phase 1: template with no parameters — actually one IRI-typed parameter —
/// wired to a single `ottr:Triple` pattern instance, plus a top-level
/// instance call. Simplest possible wOTTR document.
#[test]

fn parses_template_with_single_triple_pattern_and_expands() {
    let text = read_fixture("no_params.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    assert_eq!(doc.templates.len(), 1);
    assert_eq!(doc.instances.len(), 1);

    let mut ds = Datastore::new(100);
    ottr::expand_documents(&[doc], &mut ds).unwrap();

    let rdf_type = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let ex_thing = ds.add_resource(iri_element("http://example.com/Thing"));
    let widget = ds.add_resource(iri_element("http://example.com/Widget"));

    assert!(ds.contains_triple(&Triple {
        subject: widget,
        predicate: rdf_type,
        obj: ex_thing
    }));
}

/// Phase 2: parameters without an explicit `ottr:type` default to `OttrType::Iri`.
#[test]

fn untyped_parameters_default_to_iri_type() {
    let text = read_fixture("untyped_params.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    let template = &doc.templates[0];
    assert_eq!(template.parameters.len(), 2);
    for param in &template.parameters {
        assert_eq!(param.ottr_type, OttrType::Iri);
        assert!(!param.optional);
    }

    let mut ds = Datastore::new(100);
    ottr::expand_documents(&[doc], &mut ds).unwrap();
    let rel = ds.add_resource(iri_element("http://example.com/rel"));
    let a = ds.add_resource(iri_element("http://example.com/A"));
    let b = ds.add_resource(iri_element("http://example.com/B"));
    assert!(ds.contains_triple(&Triple {
        subject: a,
        predicate: rel,
        obj: b
    }));
}

/// Phase 3: explicit `ottr:IRI`/`ottr:BlankNode`/`ottr:Literal`/datatype-IRI
/// parameter types are mapped to the corresponding `OttrType` variants.
#[test]

fn explicit_parameter_types_are_mapped() {
    let text = read_fixture("typed_params.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    let template = &doc.templates[0];
    assert_eq!(template.parameters.len(), 4);
    assert_eq!(template.parameters[0].ottr_type, OttrType::Iri);
    assert_eq!(template.parameters[1].ottr_type, OttrType::BlankNode);
    assert_eq!(template.parameters[2].ottr_type, OttrType::Literal(None));
    assert_eq!(
        template.parameters[3].ottr_type,
        OttrType::Literal(Some(IriReference(
            "http://www.w3.org/2001/XMLSchema#string".to_string()
        )))
    );
}

/// Phase 4: a template instance calling a second user-defined template
/// (not just the built-in `ottr:Triple`), i.e. nested template expansion.
#[test]

fn nested_user_templates_expand_transitively() {
    let text = read_fixture("nested_templates.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    assert_eq!(doc.templates.len(), 2);

    let mut ds = Datastore::new(100);
    ottr::expand_documents(&[doc], &mut ds).unwrap();
    let rdf_type = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let ex_inner = ds.add_resource(iri_element("http://example.com/Inner"));
    let foo = ds.add_resource(iri_element("http://example.com/Foo"));
    assert!(ds.contains_triple(&Triple {
        subject: foo,
        predicate: rdf_type,
        obj: ex_inner
    }));
}

/// Phase 5: `ottr:none` in the compact `ottr:values` list maps to
/// `Argument::None`, and (via the existing expander) silently drops the
/// triple that would have used it.
#[test]

fn ottr_none_argument_drops_the_triple() {
    let text = read_fixture("none_argument.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    let instance = &doc.instances[0];
    assert_eq!(instance.arguments[1], Argument::None);

    let mut ds = Datastore::new(100);
    ottr::expand_documents(&[doc], &mut ds).unwrap();
    let maybe = ds.add_resource(iri_element("http://example.com/maybe"));
    let a = ds.add_resource(iri_element("http://example.com/A"));
    // No triple should have been produced: the `none` argument dropped it.
    assert!(ds.get_triples_with_subject_predicate(a, maybe).count() == 0);
}

/// Phase 6: `ottr:cross` instance modifier combined with a nested-RDF-list
/// argument value expands to one triple per list element.
#[test]

fn cross_modifier_expands_nested_list_argument() {
    let text = read_fixture("cross_list.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    let instance = &doc.instances[0];
    assert!(matches!(instance.arguments[1], Argument::List(_)));

    let mut ds = Datastore::new(100);
    ottr::expand_documents(&[doc], &mut ds).unwrap();
    let has_topping = ds.add_resource(iri_element("http://example.com/hasTopping"));
    let pizza = ds.add_resource(iri_element("http://example.com/Pizza"));
    for topping in ["Cheese", "Ham", "Olives"] {
        let t = ds.add_resource(iri_element(&format!("http://example.com/{topping}")));
        assert!(ds.contains_triple(&Triple {
            subject: pizza,
            predicate: has_topping,
            obj: t
        }));
    }
}

/// Phase 8: `ottr:annotation` instances (metadata on a template, not part of
/// its expansion pattern) must not leak into the top-level document
/// instances — they reference an annotation "template" (e.g.
/// `ex:TemplateAnnotation`) that is typically never defined, so treating them
/// as ordinary top-level instances would fail expansion with
/// `UnknownTemplate`.
#[test]
fn annotation_instances_are_excluded_from_top_level_instances() {
    let text = read_fixture("annotation.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    assert_eq!(doc.instances.len(), 1);

    let mut ds = Datastore::new(100);
    ottr::expand_documents(&[doc], &mut ds).unwrap();
    let rel = ds.add_resource(iri_element("http://example.com/rel"));
    let a = ds.add_resource(iri_element("http://example.com/A"));
    let b = ds.add_resource(iri_element("http://example.com/B"));
    assert!(ds.contains_triple(&Triple {
        subject: a,
        predicate: rel,
        obj: b
    }));
}

/// Phase 7: the canonical `ottr:arguments` (list of `ottr:Argument` blank
/// nodes with `ottr:value`) encoding is equivalent to the compact
/// `ottr:values` encoding used by the other fixtures.
#[test]

fn canonical_arguments_encoding_is_equivalent_to_compact_values() {
    let text = read_fixture("canonical_arguments.ttl");
    let doc = parse_wottr_str(&text).unwrap();
    let instance = &doc.instances[0];
    assert_eq!(
        instance.arguments,
        vec![
            Argument::Term(Term::Iri(IriReference("http://example.com/A".to_string()))),
            Argument::Term(Term::Iri(IriReference("http://example.com/B".to_string()))),
        ]
    );

    let mut ds = Datastore::new(100);
    ottr::expand_documents(&[doc], &mut ds).unwrap();
    let rel = ds.add_resource(iri_element("http://example.com/rel"));
    let a = ds.add_resource(iri_element("http://example.com/A"));
    let b = ds.add_resource(iri_element("http://example.com/B"));
    assert!(ds.contains_triple(&Triple {
        subject: a,
        predicate: rel,
        obj: b
    }));
}

/// Phase 4: base expansion (`ottr:Triple`), no template nesting.
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use ottr::ast::{Argument, Instance, Term};
use ottr::parser::parse_stottr;
use ottr::{expand, expand_documents};
use std::collections::HashMap;

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

const DIRECT_TRIPLE: &str = r#"
@prefix ottr: <http://ns.ottr.xyz/0.4/> .

ottr:Triple(<http://example.com/Alice>, <http://example.com/knows>, <http://example.com/Bob>) .
"#;

#[test]
#[ignore]
fn direct_ottr_triple_instance_inserts_one_quad() {
    let doc = parse_stottr(DIRECT_TRIPLE).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let s = ds.add_resource(iri_element("http://example.com/Alice"));
    let p = ds.add_resource(iri_element("http://example.com/knows"));
    let o = ds.add_resource(iri_element("http://example.com/Bob"));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

const SINGLE_TEMPLATE_CALL: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person ] :: {
  ottr:Triple (?person, rdf:type, foaf:Person)
} .

ex:Person(<http://example.com/Alice>) .
"#;

#[test]
#[ignore]
fn user_template_with_single_triple_body_expands_on_one_call() {
    let doc = parse_stottr(SINGLE_TEMPLATE_CALL).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let s = ds.add_resource(iri_element("http://example.com/Alice"));
    let p = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let o = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/Person"));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

const TWO_TEMPLATE_CALLS: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person ] :: {
  ottr:Triple (?person, rdf:type, foaf:Person)
} .

ex:Person(<http://example.com/Alice>) .
ex:Person(<http://example.com/Bob>) .
"#;

#[test]
#[ignore]
fn same_template_called_twice_produces_two_sets_of_quads() {
    let doc = parse_stottr(TWO_TEMPLATE_CALLS).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let p = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let o = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/Person"));

    let alice = ds.add_resource(iri_element("http://example.com/Alice"));
    let bob = ds.add_resource(iri_element("http://example.com/Bob"));

    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: p,
        obj: o
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob,
        predicate: p,
        obj: o
    }));
}

#[test]
#[ignore]
fn none_argument_in_triple_position_omits_the_triple() {
    let alice = IriReference("http://example.com/Alice".to_string());
    let knows = IriReference("http://example.com/knows".to_string());
    let ottr_triple = IriReference("http://ns.ottr.xyz/0.4/Triple".to_string());

    let instance = Instance {
        template: ottr_triple,
        arguments: vec![
            Argument::Term(Term::Iri(alice.clone())),
            Argument::None,
            Argument::Term(Term::Iri(knows)),
        ],
        expander: None,
    };

    let mut ds = Datastore::new(100);
    expand(&HashMap::new(), &[instance], &mut ds).unwrap();

    let s = ds.add_resource(iri_element(&alice.0));
    assert_eq!(ds.get_triples_with_subject(s).count(), 0);
}

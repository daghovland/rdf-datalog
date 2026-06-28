/// Phase 5: nested template calls + recursion guard.
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use ottr::parser::parse_stottr;
use ottr::{OttrError, expand_documents};

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

const TWO_LEVEL_NESTING: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:NamedThing [ ottr:IRI ?thing, ottr:Literal ?name ] :: {
  ottr:Triple (?thing, foaf:name, ?name)
} .

ex:Person [ ottr:IRI ?person, ottr:Literal ?name ] :: {
  ex:NamedThing (?person, ?name),
  ottr:Triple (?person, rdf:type, foaf:Person)
} .

ex:Person(<http://example.com/Alice>, "Alice") .
"#;

#[test]
fn template_calling_another_template_expands_through_both_levels() {
    let doc = parse_stottr(TWO_LEVEL_NESTING).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let alice = ds.add_resource(iri_element("http://example.com/Alice"));
    let name_pred = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/name"));
    let name_val = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let type_pred = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let person_class = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/Person"));

    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: name_pred,
        obj: name_val
    }));
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: type_pred,
        obj: person_class
    }));
}

const THREE_LEVEL_NESTING: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

ex:Label [ ottr:IRI ?x, ottr:Literal ?l ] :: {
  ottr:Triple (?x, rdfs:label, ?l)
} .

ex:Mid [ ottr:IRI ?x, ottr:Literal ?l ] :: {
  ex:Label (?x, ?l)
} .

ex:Top [ ottr:IRI ?x, ottr:Literal ?l ] :: {
  ex:Mid (?x, ?l)
} .

ex:Top(<http://example.com/Alice>, "Alice") .
"#;

#[test]
fn three_levels_of_template_nesting_reach_the_base_triple() {
    let doc = parse_stottr(THREE_LEVEL_NESTING).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let alice = ds.add_resource(iri_element("http://example.com/Alice"));
    let label_pred = ds.add_resource(iri_element("http://www.w3.org/2000/01/rdf-schema#label"));
    let name_val = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: label_pred,
        obj: name_val
    }));
}

const SELF_RECURSIVE_TEMPLATE: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .

ex:Loop [ ottr:IRI ?x ] :: {
  ex:Loop (?x)
} .

ex:Loop(<http://example.com/Alice>) .
"#;

#[test]
fn self_recursive_template_call_errors_instead_of_overflowing_the_stack() {
    let doc = parse_stottr(SELF_RECURSIVE_TEMPLATE).unwrap();
    let mut ds = Datastore::new(100);
    let result = expand_documents(&[doc], &mut ds);

    assert!(matches!(result, Err(OttrError::RecursiveTemplate(_))));
}

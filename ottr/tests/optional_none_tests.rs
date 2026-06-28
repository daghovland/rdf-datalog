/// Phase 6: `none` keyword parsing and selective triple-dropping.
/// https://github.com/daghovland/rdf-datalog/issues/19
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use ottr::ast::Argument;
use ottr::expand_documents;
use ottr::parser::parse_stottr;

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

const NONE_IN_INSTANCE_CALL: &str = r#"
@prefix ex: <http://example.com/> .

ex:Person(<http://example.com/Alice>, none) .
"#;

#[test]
fn none_keyword_parses_to_argument_none() {
    let doc = parse_stottr(NONE_IN_INSTANCE_CALL).unwrap();
    assert_eq!(doc.instances[0].arguments[1], Argument::None);
}

const SELECTIVE_NONE_DROP: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person, ottr:Literal ?name ] :: {
  ottr:Triple (?person, rdf:type, foaf:Person),
  ottr:Triple (?person, foaf:name, ?name)
} .

ex:Person(<http://example.com/Alice>, none) .
"#;

#[test]
fn none_bound_to_one_param_drops_only_triples_referencing_it() {
    let doc = parse_stottr(SELECTIVE_NONE_DROP).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let alice = ds.add_resource(iri_element("http://example.com/Alice"));
    let type_pred = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let person_class = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/Person"));

    // The rdf:type triple doesn't reference ?name, so it must be present.
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: type_pred,
        obj: person_class
    }));
    // The foaf:name triple references ?name (bound to none) — must be absent.
    assert_eq!(ds.get_triples_with_subject(alice).count(), 1);
}

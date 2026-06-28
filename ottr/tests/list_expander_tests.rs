/// Phase 7: list expanders (`cross`, `zipMin`).
/// https://github.com/daghovland/rdf-datalog/issues/20
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use ottr::expand_documents;
use ottr::parser::parse_stottr;

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

/// Template whose body uses `++?name |cross` over a single list parameter.
const CROSS_ONE_LIST: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:PersonNames [ ottr:IRI ?person, ottr:Literal ?name ] :: {
  cross | ottr:Triple (?person, foaf:name, ++?name)
} .

ex:PersonNames(<http://example.com/Alice>, ("Alice", "Alicia")) .
"#;

#[test]
fn cross_over_one_list_produces_one_triple_per_element() {
    let doc = parse_stottr(CROSS_ONE_LIST).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let alice = ds.add_resource(iri_element("http://example.com/Alice"));
    let name_pred = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/name"));
    let alice_name = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let alicia_name = ds.add_literal_resource(RdfLiteral::LiteralString("Alicia".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: name_pred,
        obj: alice_name,
    }));
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: name_pred,
        obj: alicia_name,
    }));
    assert_eq!(ds.get_triples_with_subject(alice).count(), 2);
}

/// Template whose body uses two `++?var` args and `|cross` → cartesian product.
const CROSS_TWO_LISTS: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

ex:Types [ ottr:IRI ?thing, ottr:IRI ?type ] :: {
  cross | ottr:Triple (++?thing, rdf:type, ++?type)
} .

ex:Types(
  (<http://example.com/Alice>, <http://example.com/Bob>),
  (<http://example.com/Person>, <http://example.com/Agent>)
) .
"#;

#[test]
fn cross_over_two_lists_produces_cartesian_product() {
    let doc = parse_stottr(CROSS_TWO_LISTS).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let type_pred = ds.add_resource(iri_element(
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
    ));
    let alice = ds.add_resource(iri_element("http://example.com/Alice"));
    let bob = ds.add_resource(iri_element("http://example.com/Bob"));
    let person = ds.add_resource(iri_element("http://example.com/Person"));
    let agent = ds.add_resource(iri_element("http://example.com/Agent"));

    // All four combinations of {Alice, Bob} × {Person, Agent}.
    for subject in [alice, bob] {
        for obj in [person, agent] {
            assert!(ds.contains_triple(&Triple {
                subject,
                predicate: type_pred,
                obj,
            }));
        }
    }
}

/// Template using `|zipMin` — pairs lists by index, truncates to the shorter.
const ZIP_MIN: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Names [ ottr:IRI ?person, ottr:Literal ?name ] :: {
  zipMin | ottr:Triple (++?person, foaf:name, ++?name)
} .

ex:Names(
  (<http://example.com/Alice>, <http://example.com/Bob>),
  ("Alice", "Robert", "Bobby")
) .
"#;

#[test]
fn zip_min_pairs_by_index_and_truncates_to_the_shorter_list() {
    let doc = parse_stottr(ZIP_MIN).unwrap();
    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();

    let name_pred = ds.add_resource(iri_element("http://xmlns.com/foaf/0.1/name"));
    let alice = ds.add_resource(iri_element("http://example.com/Alice"));
    let bob = ds.add_resource(iri_element("http://example.com/Bob"));
    let alice_name = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_string()));
    let robert_name = ds.add_literal_resource(RdfLiteral::LiteralString("Robert".to_string()));
    let bobby_name = ds.add_literal_resource(RdfLiteral::LiteralString("Bobby".to_string()));

    // Exactly 2 triples (min(2, 3) = 2).
    assert!(ds.contains_triple(&Triple {
        subject: alice,
        predicate: name_pred,
        obj: alice_name,
    }));
    assert!(ds.contains_triple(&Triple {
        subject: bob,
        predicate: name_pred,
        obj: robert_name,
    }));
    // "Bobby" is the third element — no triple should reference it.
    let bobby_id = ds.add_literal_resource(RdfLiteral::LiteralString("Bobby".to_string()));
    assert_eq!(bobby_id, bobby_name); // same intern
    assert!(!ds.contains_triple(&Triple {
        subject: bob,
        predicate: name_pred,
        obj: bobby_name,
    }));
}

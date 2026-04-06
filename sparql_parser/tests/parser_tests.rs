use sparql_parser::ast::*;
use sparql_parser::{parse_query, ParserContext};
use std::collections::HashMap;

#[test]
fn test_parse_simple_select() {
    let sparql = "SELECT ?title WHERE { <http://example.org/book/book1> <http://purl.org/dc/elements/1.1/title> ?title }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse");

    let Query::Select {
        projection,
        where_clause,
        ..
    } = query;

    assert_eq!(projection.len(), 1);
    assert_eq!(
        projection[0],
        ProjectionElement::Variable("title".to_string())
    );

    assert_eq!(where_clause.len(), 1);
    if let QueryComponent::BGP(triples) = &where_clause[0] {
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].object, Term::Variable("title".to_string()));
    } else {
        panic!("Expected BGP");
    }
}

#[test]
fn test_parse_with_prefix() {
    let sparql = "
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?name
        WHERE {
          ?person foaf:name ?name .
        }
    ";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse");

    let Query::Select {
        projection,
        where_clause,
        ..
    } = query;

    assert_eq!(projection.len(), 1);
    if let ProjectionElement::Variable(v) = &projection[0] {
        assert_eq!(v, "name");
    }

    if let QueryComponent::BGP(triples) = &where_clause[0] {
        assert_eq!(triples.len(), 1);
        if let Term::Constant(dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(iri))) =
            &triples[0].predicate
        {
            assert_eq!(iri.0, "http://xmlns.com/foaf/0.1/name");
        } else {
            panic!("Expected IRI predicate");
        }
    }
}

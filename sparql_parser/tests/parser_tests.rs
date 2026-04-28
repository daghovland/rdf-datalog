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

#[test]
fn test_parse_sparql12_graph_clause_with_iri() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?name WHERE {
          GRAPH <http://example.org/graph/people> {
            ?person foaf:name ?name .
          }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse SPARQL 1.2 GRAPH clause");
    let Query::Select { where_clause, .. } = query;

    assert_eq!(where_clause.len(), 1);
    match &where_clause[0] {
        QueryComponent::Graph(_, inner) => {
            assert_eq!(inner.len(), 1);
            assert!(matches!(inner[0], QueryComponent::BGP(_)));
        }
        other => panic!("Expected Graph component, got: {:?}", other),
    }
}

#[test]
fn test_parse_sparql12_graph_clause_with_variable_graph_name() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?g ?name WHERE {
          GRAPH ?g {
            ?person foaf:name ?name .
          }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx)
        .expect("Should parse SPARQL 1.2 GRAPH clause with variable graph name");
    let Query::Select {
        projection,
        where_clause,
        ..
    } = query;

    assert_eq!(projection.len(), 2);
    assert_eq!(where_clause.len(), 1);
    match &where_clause[0] {
        QueryComponent::Graph(Term::Variable(v), inner) => {
            assert_eq!(v, "g");
            assert_eq!(inner.len(), 1);
            assert!(matches!(inner[0], QueryComponent::BGP(_)));
        }
        other => panic!(
            "Expected Graph component with variable graph name, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_parse_semicolon_comma_and_property_path() {
    let sparql = r#"
        PREFIX ex: <https://example.com/>
        SELECT ?s ?o1 ?o2 WHERE {
          ?s ex:p ?o1, ?o2 ;
             ex:q/ex:r ?z .
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse shorthand and path");
    let Query::Select { where_clause, .. } = query;

    assert_eq!(where_clause.len(), 1);
    match &where_clause[0] {
        QueryComponent::BGP(triples) => {
            // ?s ex:p ?o1
            // ?s ex:p ?o2
            // ?s ex:q ?__path_n
            // ?__path_n ex:r ?z
            assert_eq!(triples.len(), 4);
        }
        other => panic!("Expected BGP, got: {:?}", other),
    }
}

#[test]
fn test_parse_sparql12_optional_with_bound_filter() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?name WHERE {
          ?x foaf:name ?name .
          OPTIONAL { ?x foaf:mbox ?mbox . }
          FILTER(!BOUND(?mbox))
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) =
        parse_query(sparql, &mut ctx).expect("Should parse SPARQL 1.2 OPTIONAL/FILTER example");
    let Query::Select { where_clause, .. } = query;

    assert_eq!(where_clause.len(), 3);
    assert!(matches!(where_clause[0], QueryComponent::BGP(_)));
    assert!(matches!(where_clause[1], QueryComponent::Optional(_)));
    assert!(matches!(where_clause[2], QueryComponent::Filter(_)));
}

#[test]
fn test_parse_sparql12_union_example() {
    let sparql = r#"
        PREFIX ex: <http://example.org/>
        SELECT ?x WHERE {
          { ?x ex:givenName "Alice" . }
          UNION
          { ?x ex:givenName "Bob" . }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse SPARQL 1.2 UNION example");
    let Query::Select { where_clause, .. } = query;

    assert_eq!(where_clause.len(), 1);
    assert!(matches!(where_clause[0], QueryComponent::Union(_, _)));
}

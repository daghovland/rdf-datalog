use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfResource};
use sparql_parser::{execute, parse_query, ParserContext};
use std::collections::HashMap;

fn iri_node(iri: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
}

fn add_quad(ds: &mut Datastore, graph: &str, subject: &str, predicate: &str, object: &str) {
    let g = ds.add_resource(iri_node(graph));
    let s = ds.add_resource(iri_node(subject));
    let p = ds.add_resource(iri_node(predicate));
    let o = ds.add_resource(iri_node(object));
    ds.add_quad(Quad {
        triple_id: g,
        subject: s,
        predicate: p,
        obj: o,
    });
}

fn run_query(ds: &Datastore, query: &str) -> sparql_parser::SelectResult {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, parsed) = parse_query(query, &mut ctx).expect("query should parse");
    execute(&parsed, ds).expect("query should execute")
}

#[test]
fn graph_iri_scope_matches_only_that_named_graph() {
    let mut ds = Datastore::new(1_000);

    add_quad(
        &mut ds,
        "http://example.org/graph/one",
        "http://example.org/alice",
        "http://xmlns.com/foaf/0.1/name",
        "http://example.org/name/alice",
    );
    add_quad(
        &mut ds,
        "http://example.org/graph/two",
        "http://example.org/bob",
        "http://xmlns.com/foaf/0.1/name",
        "http://example.org/name/bob",
    );

    let query = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?s WHERE {
  GRAPH <http://example.org/graph/one> {
    ?s foaf:name ?name .
  }
}
"#;

    let result = run_query(&ds, query);
    assert_eq!(result.rows.len(), 1);

    let row = &result.rows[0];
    let s = row.get("s").expect("?s should be bound");
    assert_eq!(s, &iri_node("http://example.org/alice"));
}

#[test]
fn graph_variable_binds_graph_names() {
    let mut ds = Datastore::new(1_000);

    add_quad(
        &mut ds,
        "http://example.org/graph/one",
        "http://example.org/alice",
        "http://xmlns.com/foaf/0.1/name",
        "http://example.org/name/alice",
    );
    add_quad(
        &mut ds,
        "http://example.org/graph/two",
        "http://example.org/bob",
        "http://xmlns.com/foaf/0.1/name",
        "http://example.org/name/bob",
    );

    let query = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?g ?s WHERE {
  GRAPH ?g {
    ?s foaf:name ?name .
  }
}
"#;

    let result = run_query(&ds, query);
    assert_eq!(result.rows.len(), 2);

    let mut graph_iris: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| row.get("g"))
        .filter_map(|el| match el {
            GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => Some(iri.0.clone()),
            _ => None,
        })
        .collect();
    graph_iris.sort();

    assert_eq!(
        graph_iris,
        vec![
            "http://example.org/graph/one".to_string(),
            "http://example.org/graph/two".to_string(),
        ]
    );
}

#[test]
fn default_graph_query_does_not_implicitly_include_named_graphs() {
    let mut ds = Datastore::new(1_000);

    // Named graph triple only
    add_quad(
        &mut ds,
        "http://example.org/graph/one",
        "http://example.org/alice",
        "http://xmlns.com/foaf/0.1/name",
        "http://example.org/name/alice",
    );

    let query = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?s WHERE {
  ?s foaf:name ?name .
}
"#;

    let result = run_query(&ds, query);
    assert!(result.rows.is_empty());
}


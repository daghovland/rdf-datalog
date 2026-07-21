//! Tests for SPARQL `BASE <iri>` directive parsing and relative-IRI
//! resolution against it, per SPARQL 1.1 §4.1 / RFC 3986.
//!
//! See issue [#217](https://github.com/daghovland/rdf-datalog/issues/217).

use dag_rdf::{GraphElement, IriReference, RdfResource};
use sparql_parser::{ast::*, parse_query, ParserContext};
use std::collections::HashMap;

fn iri_term(iri: &str) -> Term {
    Term::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        iri.to_string(),
    ))))
}

fn first_triple(query: &Query) -> &TriplePattern {
    let Query::Select { where_clause, .. } = query else {
        panic!("expected a SELECT query");
    };
    let QueryComponent::BGP(triples) = &where_clause[0] else {
        panic!("expected a basic graph pattern as the first WHERE component");
    };
    &triples[0]
}

#[test]
fn base_directive_resolves_relative_iri() {
    let sparql = "BASE <http://example.org/> SELECT * WHERE { <foo> ?p ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("BASE <...> SELECT ... should parse");
    let triple = first_triple(&query);
    assert_eq!(triple.subject, iri_term("http://example.org/foo"));
}

#[test]
fn caller_supplied_default_base_resolves_relative_iri() {
    // No BASE directive in the query text itself — the caller supplies a
    // default base (e.g. the query file's own path), matching how
    // `turtle::parse_turtle_with_base` is already given a base by its callers.
    let sparql = "SELECT * WHERE { <foo> ?p ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: Some("http://example.org/".to_string()),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let triple = first_triple(&query);
    assert_eq!(triple.subject, iri_term("http://example.org/foo"));
}

#[test]
fn base_directive_overrides_caller_supplied_default() {
    let sparql = "BASE <http://override.example/> SELECT * WHERE { <foo> ?p ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: Some("http://example.org/".to_string()),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let triple = first_triple(&query);
    assert_eq!(triple.subject, iri_term("http://override.example/foo"));
}

#[test]
fn base_directive_itself_resolved_against_caller_default() {
    // A relative BASE IRI is itself resolved against whatever base was
    // already in effect (here, the caller-supplied default) before being
    // installed as the new effective base — RFC 3986 base-URI composition.
    let sparql = "BASE <sub/> SELECT * WHERE { <foo> ?p ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: Some("http://example.org/".to_string()),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let triple = first_triple(&query);
    assert_eq!(triple.subject, iri_term("http://example.org/sub/foo"));
}

#[test]
fn absolute_iri_queries_are_unaffected_regression() {
    // Regression guard: no BASE directive, no caller-supplied base, and an
    // already-absolute IRI — behavior must be identical to before this
    // feature existed.
    let sparql = "SELECT * WHERE { <http://example.org/book/book1> ?p ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let triple = first_triple(&query);
    assert_eq!(triple.subject, iri_term("http://example.org/book/book1"));
}

#[test]
fn relative_iri_with_no_base_at_all_is_kept_verbatim() {
    // Decided fallback (see issue #217 discussion): when there is genuinely
    // no base available (no BASE directive, no caller-supplied default), a
    // relative IRI reference is kept as-is rather than raising a parse
    // error. This intentionally differs from `turtle::parse_turtle`'s
    // stricter no-base behavior (which rejects non-absolute IRIs outright)
    // because several existing regression tests and W3C SPARQL 1.1 suite
    // `.rq` fixtures (`tests/w3c_sparql11_suite.rs::load_data_into_named_graph`)
    // already rely on bare relative-looking IRIs like `GRAPH <exists02.ttl>`
    // parsing verbatim when no base is supplied.
    let sparql = "SELECT * WHERE { <foo> ?p ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) =
        parse_query(sparql, &mut ctx).expect("relative IRI with no base should still parse");
    let triple = first_triple(&query);
    assert_eq!(triple.subject, iri_term("foo"));
}

#[test]
fn prefix_decl_iri_is_resolved_against_base() {
    // Per the SPARQL grammar, a PREFIX declaration's IRIREF is just another
    // IRI reference and is likewise subject to base resolution.
    let sparql = "
        BASE <http://example.org/ns/>
        PREFIX ex: <sub/>
        SELECT * WHERE { ?s ex:foo ?o }
    ";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let triple = first_triple(&query);
    assert_eq!(triple.predicate, iri_term("http://example.org/ns/sub/foo"));
}

#[test]
fn graph_clause_iri_is_resolved_against_base() {
    let sparql = "BASE <http://example.org/> SELECT * WHERE { GRAPH <g1> { ?s ?p ?o } }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let Query::Select { where_clause, .. } = query else {
        panic!("expected a SELECT query");
    };
    let QueryComponent::Graph(graph_term, _inner) = &where_clause[0] else {
        panic!("expected a GRAPH component");
    };
    assert_eq!(*graph_term, iri_term("http://example.org/g1"));
}

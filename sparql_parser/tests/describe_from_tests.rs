//! Tests for the DESCRIBE query form (#49) and FROM / FROM NAMED dataset
//! clauses (#50). See docs/plans/SPARQL_MISSING_FEATURES_PLAN.md.

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfResource};
use sparql_parser::{ast::*, execute, parse_query, NetworkPolicy, ParserContext, QueryResult};
use std::collections::HashMap;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn iri(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

fn add_default_triple(ds: &mut Datastore, s: &str, p: &str, o: &str) {
    use dag_rdf::DEFAULT_GRAPH_ELEMENT_ID;
    let s = ds.add_resource(iri(s));
    let p = ds.add_resource(iri(p));
    let o = ds.add_resource(iri(o));
    ds.add_quad(Quad {
        triple_id: DEFAULT_GRAPH_ELEMENT_ID,
        subject: s,
        predicate: p,
        obj: o,
    });
}

fn add_named_triple(ds: &mut Datastore, graph: &str, s: &str, p: &str, o: &str) {
    let g = ds.add_resource(iri(graph));
    let s = ds.add_resource(iri(s));
    let p = ds.add_resource(iri(p));
    let o = ds.add_resource(iri(o));
    ds.add_quad(Quad {
        triple_id: g,
        subject: s,
        predicate: p,
        obj: o,
    });
}

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
        base: None,
    }
}

// ── DESCRIBE — parser tests ───────────────────────────────────────────────────

#[test]
fn test_describe_iri_parses() {
    let sparql = "DESCRIBE <http://example.org/Alice>";
    let (_, query) = parse_query(sparql, &mut ctx()).expect("should parse DESCRIBE");
    let Query::Describe {
        resources,
        where_clause,
        ..
    } = query
    else {
        panic!("expected Query::Describe, got something else");
    };
    assert_eq!(resources.len(), 1);
    assert_eq!(
        resources[0],
        Term::Constant(iri("http://example.org/Alice"))
    );
    assert!(where_clause.is_empty());
}

#[test]
fn test_describe_var_with_where_parses() {
    let sparql = "PREFIX ex: <http://example.org/> DESCRIBE ?s WHERE { ?s a ex:Person }";
    let (_, query) = parse_query(sparql, &mut ctx()).expect("should parse DESCRIBE ?s WHERE");
    let Query::Describe {
        resources,
        where_clause,
        ..
    } = query
    else {
        panic!("expected Query::Describe");
    };
    assert_eq!(resources, vec![Term::Variable("s".to_string())]);
    assert!(!where_clause.is_empty());
}

#[test]
fn test_describe_star_parses() {
    let sparql = "PREFIX ex: <http://example.org/> DESCRIBE * WHERE { ?s a ex:Person }";
    let (_, query) = parse_query(sparql, &mut ctx()).expect("should parse DESCRIBE *");
    let Query::Describe { resources, .. } = query else {
        panic!("expected Query::Describe");
    };
    // DESCRIBE * is represented as an empty resources list
    assert!(
        resources.is_empty(),
        "DESCRIBE * should produce empty resource list"
    );
}

// ── DESCRIBE — executor tests ─────────────────────────────────────────────────

#[test]
fn test_describe_iri_returns_subject_triples() {
    let mut ds = Datastore::new(1_000);
    add_default_triple(
        &mut ds,
        "http://example.org/Alice",
        "http://ex.org/name",
        "http://ex.org/Alice_name",
    );
    add_default_triple(
        &mut ds,
        "http://example.org/Alice",
        "http://ex.org/age",
        "http://ex.org/Alice_age",
    );
    add_default_triple(
        &mut ds,
        "http://example.org/Bob",
        "http://ex.org/name",
        "http://ex.org/Bob_name",
    );

    let (_, query) =
        parse_query("DESCRIBE <http://example.org/Alice>", &mut ctx()).expect("should parse");

    match execute(&query, &ds, NetworkPolicy::Deny).expect("should execute") {
        QueryResult::Describe(triples) => {
            assert_eq!(
                triples.len(),
                2,
                "should return exactly the 2 triples where Alice is subject"
            );
            for t in &triples {
                assert_eq!(
                    t.subject,
                    iri("http://example.org/Alice"),
                    "all returned triples should have Alice as subject"
                );
            }
        }
        other => panic!(
            "expected QueryResult::Describe, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[test]
fn test_describe_var_resolves_to_subjects() {
    let mut ds = Datastore::new(1_000);
    let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let ex_person = "http://example.org/Person";
    add_default_triple(&mut ds, "http://example.org/Alice", rdf_type, ex_person);
    add_default_triple(
        &mut ds,
        "http://example.org/Alice",
        "http://ex.org/name",
        "http://ex.org/n",
    );
    add_default_triple(&mut ds, "http://example.org/Bob", rdf_type, ex_person);

    let sparql = "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
                  PREFIX ex:  <http://example.org/>
                  DESCRIBE ?s WHERE { ?s rdf:type ex:Person }";
    let (_, query) = parse_query(sparql, &mut ctx()).expect("should parse");

    match execute(&query, &ds, NetworkPolicy::Deny).expect("should execute") {
        QueryResult::Describe(triples) => {
            // WHERE matches Alice and Bob; describe returns triples where they are subjects
            assert!(
                triples.len() >= 2,
                "should have at least Alice's and Bob's triples"
            );
        }
        _ => panic!("expected QueryResult::Describe"),
    }
}

// ── FROM / FROM NAMED — parser tests ─────────────────────────────────────────

#[test]
fn test_from_clause_parses() {
    let sparql = "SELECT ?s FROM <http://example.org/graph1> WHERE { ?s ?p ?o }";
    let (_, query) = parse_query(sparql, &mut ctx()).expect("should parse FROM");
    let Query::Select { dataset, .. } = query else {
        panic!("expected Query::Select");
    };
    assert_eq!(dataset.len(), 1);
    assert_eq!(
        dataset[0],
        DatasetClause::Default(iri("http://example.org/graph1"))
    );
}

#[test]
fn test_from_named_clause_parses() {
    let sparql = "SELECT ?s FROM NAMED <http://example.org/graph1> WHERE { ?s ?p ?o }";
    let (_, query) = parse_query(sparql, &mut ctx()).expect("should parse FROM NAMED");
    let Query::Select { dataset, .. } = query else {
        panic!("expected Query::Select");
    };
    assert_eq!(dataset.len(), 1);
    assert_eq!(
        dataset[0],
        DatasetClause::Named(iri("http://example.org/graph1"))
    );
}

#[test]
fn test_from_restricts_to_named_graph() {
    let mut ds = Datastore::new(1_000);
    add_named_triple(
        &mut ds,
        "http://example.org/g1",
        "http://ex.org/Alice",
        "http://ex.org/name",
        "http://ex.org/alice_name",
    );
    add_named_triple(
        &mut ds,
        "http://example.org/g2",
        "http://ex.org/Bob",
        "http://ex.org/name",
        "http://ex.org/bob_name",
    );

    // FROM g1 should only see Alice's triple
    let sparql = "SELECT ?s FROM <http://example.org/g1> WHERE { ?s ?p ?o }";
    let (_, query) = parse_query(sparql, &mut ctx()).expect("should parse");
    match execute(&query, &ds, NetworkPolicy::Deny).expect("should execute") {
        QueryResult::Select(r) => {
            assert_eq!(
                r.rows.len(),
                1,
                "FROM g1 should return exactly 1 row (Alice)"
            );
        }
        _ => panic!("expected SELECT result"),
    }
}

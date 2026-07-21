/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression + correctness net for the LIMIT short-circuit optimisation
//! (issue #165). The optimisation stops producing solutions once OFFSET+LIMIT
//! rows exist, but only when the full solution set is not needed. These tests
//! lock in that the *result* is unchanged: for every query shape, `LIMIT k`
//! must return exactly the first `k` rows the unlimited query returns, and
//! `OFFSET o LIMIT k` the `k` rows after skipping `o`. The performance win
//! itself (early termination) is exercised by the unit tests on the budget
//! logic inside `sparql_parser::execute`.

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfResource};
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult, SolutionRow};
use std::collections::HashMap;

fn iri_node(iri: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
}

fn add_quad(ds: &mut Datastore, subject: &str, predicate: &str, object: &str) {
    let s = ds.add_resource(iri_node(subject));
    let p = ds.add_resource(iri_node(predicate));
    let o = ds.add_resource(iri_node(object));
    ds.add_quad(Quad {
        triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
        subject: s,
        predicate: p,
        obj: o,
    });
}

fn run_query(ds: &Datastore, query: &str) -> Vec<SolutionRow> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (rest, parsed) = parse_query(query, &mut ctx).expect("query should parse");
    assert!(
        rest.trim().is_empty(),
        "parser left input unconsumed: {rest:?} (query: {query:?})"
    );
    match execute(&parsed, ds, NetworkPolicy::Deny).expect("query should execute") {
        QueryResult::Select(r) => r.rows,
        _ => panic!("expected SELECT result"),
    }
}

/// A store with `n` `ex:sK ex:p ex:oK` triples inserted in K order.
fn linear_store(n: usize) -> Datastore {
    let mut ds = Datastore::new(4 * n as u32 + 16);
    for k in 0..n {
        add_quad(
            &mut ds,
            &format!("http://example.org/s{k}"),
            "http://example.org/p",
            &format!("http://example.org/o{k}"),
        );
    }
    ds
}

/// Assert that `LIMIT k` (optionally after `OFFSET o`) over `body` returns
/// exactly the `k` rows the unlimited query returns after skipping `o`.
/// This is the invariant the short-circuit must never break.
fn assert_limit_is_prefix(
    ds: &Datastore,
    select_prefix: &str,
    body: &str,
    offset: usize,
    k: usize,
) {
    // This parser accepts LIMIT before OFFSET only (SPARQL's other ordering is
    // a separate gap), so keep LIMIT first.
    let unlimited = run_query(ds, &format!("{select_prefix} WHERE {body}"));
    let limited = run_query(
        ds,
        &format!("{select_prefix} WHERE {body} LIMIT {k} OFFSET {offset}"),
    );
    let expected: Vec<SolutionRow> = unlimited.iter().skip(offset).take(k).cloned().collect();
    assert_eq!(
        limited,
        expected,
        "LIMIT {k} OFFSET {offset} must equal unlimited[{offset}..{}] for `{body}`",
        offset + k
    );
}

#[test]
fn plain_bgp_limit_returns_exactly_n_rows() {
    let ds = linear_store(50);
    let rows = run_query(&ds, "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10");
    assert_eq!(rows.len(), 10, "LIMIT 10 must return exactly 10 rows");
}

#[test]
fn plain_bgp_limit_is_prefix_of_unlimited() {
    let ds = linear_store(50);
    assert_limit_is_prefix(&ds, "SELECT ?s ?p ?o", "{ ?s ?p ?o }", 0, 10);
}

#[test]
fn plain_bgp_offset_limit_is_prefix_of_unlimited() {
    let ds = linear_store(50);
    assert_limit_is_prefix(&ds, "SELECT ?s ?p ?o", "{ ?s ?p ?o }", 7, 10);
}

#[test]
fn limit_larger_than_result_set_returns_all() {
    let ds = linear_store(5);
    let rows = run_query(&ds, "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 100");
    assert_eq!(
        rows.len(),
        5,
        "LIMIT beyond the result size returns all rows"
    );
}

#[test]
fn offset_beyond_result_set_returns_empty() {
    let ds = linear_store(5);
    let rows = run_query(
        &ds,
        "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10 OFFSET 100",
    );
    assert!(rows.is_empty(), "OFFSET past the end returns no rows");
}

#[test]
fn multi_pattern_bgp_join_limit_is_prefix() {
    // ?s ex:p ?o . ?s ex:p ?o2 — a self join so each subject fans out.
    let mut ds = Datastore::new(1_000);
    for k in 0..20 {
        let s = format!("http://example.org/s{k}");
        add_quad(&mut ds, &s, "http://example.org/p", "http://example.org/a");
        add_quad(&mut ds, &s, "http://example.org/p", "http://example.org/b");
    }
    assert_limit_is_prefix(
        &ds,
        "SELECT ?s ?o ?o2",
        "{ ?s <http://example.org/p> ?o . ?s <http://example.org/p> ?o2 }",
        0,
        7,
    );
}

#[test]
fn repeated_variable_pattern_limit_is_prefix() {
    // ?x ?p ?x can drop matched quads (subject != object). The quad-take gate
    // must not under-produce: LIMIT must still equal the unlimited prefix.
    let mut ds = Datastore::new(1_000);
    for k in 0..10 {
        let n = format!("http://example.org/n{k}");
        // A self-loop (matches ?x ?p ?x) plus a non-loop edge (does not).
        add_quad(&mut ds, &n, "http://example.org/loop", &n);
        add_quad(
            &mut ds,
            &n,
            "http://example.org/edge",
            "http://example.org/other",
        );
    }
    assert_limit_is_prefix(&ds, "SELECT ?x ?p", "{ ?x ?p ?x }", 0, 3);
}

#[test]
fn union_limit_is_prefix() {
    let ds = linear_store(30);
    assert_limit_is_prefix(
        &ds,
        "SELECT ?s",
        "{ { ?s <http://example.org/p> ?o } UNION { ?s <http://example.org/p> ?o } }",
        0,
        5,
    );
}

#[test]
fn optional_limit_is_prefix() {
    let mut ds = Datastore::new(1_000);
    for k in 0..15 {
        let s = format!("http://example.org/s{k}");
        add_quad(&mut ds, &s, "http://example.org/p", "http://example.org/o");
        if k % 2 == 0 {
            add_quad(
                &mut ds,
                &s,
                "http://example.org/q",
                "http://example.org/extra",
            );
        }
    }
    assert_limit_is_prefix(
        &ds,
        "SELECT ?s ?x",
        "{ ?s <http://example.org/p> ?o OPTIONAL { ?s <http://example.org/q> ?x } }",
        0,
        6,
    );
}

#[test]
fn filter_limit_is_prefix() {
    let ds = linear_store(30);
    assert_limit_is_prefix(
        &ds,
        "SELECT ?s ?o",
        "{ ?s <http://example.org/p> ?o FILTER(isIRI(?o)) }",
        0,
        5,
    );
}

#[test]
fn distinct_limit_is_prefix() {
    // Two triples per subject collapse to one DISTINCT ?s row.
    let mut ds = Datastore::new(1_000);
    for k in 0..20 {
        let s = format!("http://example.org/s{k}");
        add_quad(&mut ds, &s, "http://example.org/p", "http://example.org/a");
        add_quad(&mut ds, &s, "http://example.org/p", "http://example.org/b");
    }
    assert_limit_is_prefix(
        &ds,
        "SELECT DISTINCT ?s",
        "{ ?s <http://example.org/p> ?o }",
        0,
        5,
    );
}

#[test]
fn aggregate_group_by_limit_is_prefix() {
    let mut ds = Datastore::new(1_000);
    for k in 0..12 {
        let s = format!("http://example.org/s{}", k % 4);
        add_quad(
            &mut ds,
            &s,
            "http://example.org/p",
            &format!("http://example.org/o{k}"),
        );
    }
    assert_limit_is_prefix(
        &ds,
        "SELECT ?s (COUNT(?o) AS ?c)",
        "{ ?s <http://example.org/p> ?o } GROUP BY ?s",
        0,
        2,
    );
}

#[test]
fn subquery_limit_is_prefix() {
    let ds = linear_store(20);
    let body = "{ SELECT ?s WHERE { ?s <http://example.org/p> ?o } LIMIT 8 }";
    let unlimited = run_query(&ds, &format!("SELECT ?s WHERE {body}"));
    assert_eq!(unlimited.len(), 8, "inner LIMIT 8 caps the subquery");
    let limited = run_query(&ds, &format!("SELECT ?s WHERE {body} LIMIT 3"));
    let expected: Vec<SolutionRow> = unlimited.iter().take(3).cloned().collect();
    assert_eq!(
        limited, expected,
        "outer LIMIT 3 is a prefix of the subquery result"
    );
}

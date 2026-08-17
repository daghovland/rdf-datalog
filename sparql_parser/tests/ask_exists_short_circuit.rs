/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression + correctness net for the ASK/EXISTS short-circuit optimisation
//! (issue #536). `Query::Ask` and `FILTER (NOT) EXISTS` only need to know
//! whether at least one solution exists, so evaluation is now budgeted to a
//! single row (reusing the exact same `budget`/quad-limit machinery `LIMIT`
//! already relies on, issue #165 — see `eval_components_budgeted`'s doc
//! comment in `sparql_parser/src/execute/components.rs`).
//!
//! These tests lock in that the *result* is unchanged by the short-circuit:
//! ASK/EXISTS/NOT EXISTS must return exactly the same boolean they returned
//! before, across empty patterns, zero-match patterns, and highly
//! unselective (many-match) patterns — the case the short-circuit actually
//! optimises. The performance win itself (early termination at the
//! `Datastore::quads_matching_limited` index scan) is exercised by the
//! existing budget/quad-limit unit tests in
//! `sparql_parser/src/execute/mod.rs` (`limit_budget_tests`) and
//! `sparql_parser/tests/limit_short_circuit.rs`, which this PR's ASK/EXISTS
//! change deliberately routes through unmodified — see the comment on the
//! `Query::Ask` arm in `execute_inner` for exactly how far the short-circuit
//! reaches (only the last top-level WHERE-clause component; earlier
//! components in a multi-pattern join are still fully materialised).

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfResource};
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult};
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

/// A store with `n` `ex:sK ex:p ex:oK` triples — an unselective pattern
/// against `?s ex:p ?o` matches all `n` of them.
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

fn run_ask(ds: &Datastore, query: &str) -> bool {
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
        QueryResult::Ask(b) => b,
        _ => panic!("expected an ASK result"),
    }
}

fn run_select_count(ds: &Datastore, query: &str) -> usize {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (rest, parsed) = parse_query(query, &mut ctx).expect("query should parse");
    assert!(rest.trim().is_empty(), "unconsumed input: {rest:?}");
    match execute(&parsed, ds, NetworkPolicy::Deny).expect("query should execute") {
        QueryResult::Select(r) => r.rows.len(),
        _ => panic!("expected a SELECT result"),
    }
}

// ── ASK ──────────────────────────────────────────────────────────────────

#[test]
fn ask_true_when_pattern_matches_exactly_one() {
    let mut ds = Datastore::new(10);
    add_quad(
        &mut ds,
        "http://example.org/a",
        "http://example.org/p",
        "http://example.org/b",
    );
    assert!(run_ask(
        &ds,
        "ASK { <http://example.org/a> <http://example.org/p> <http://example.org/b> }"
    ));
}

#[test]
fn ask_false_when_pattern_matches_nothing() {
    let ds = Datastore::new(10);
    assert!(!run_ask(
        &ds,
        "ASK { <http://example.org/a> <http://example.org/p> <http://example.org/b> }"
    ));
}

#[test]
fn ask_empty_where_clause_is_vacuously_true() {
    let ds = Datastore::new(10);
    assert!(
        run_ask(&ds, "ASK {}"),
        "ASK {{}} has a single trivial solution (the empty substitution) regardless of store content"
    );
}

#[test]
fn ask_true_over_unselective_pattern_with_many_solutions() {
    // The exact case the short-circuit optimises: a pattern matching many
    // rows, where only the *existence* of one match is needed. Correctness
    // must be unaffected by stopping at the first match.
    let ds = linear_store(500);
    assert!(run_ask(&ds, "ASK { ?s <http://example.org/p> ?o }"));
}

#[test]
fn ask_false_over_large_store_when_pattern_absent() {
    let ds = linear_store(500);
    assert!(!run_ask(&ds, "ASK { ?s <http://example.org/absent> ?o }"));
}

#[test]
fn ask_with_filter_is_unaffected_by_short_circuit() {
    // The budget only reaches the *last* top-level component (the FILTER
    // here follows the BGP), so this also exercises the "earlier components
    // fully materialised, only last one budgeted" boundary documented on
    // the `Query::Ask` arm.
    let ds = linear_store(10);
    assert!(run_ask(
        &ds,
        "ASK { ?s <http://example.org/p> ?o FILTER(?o = <http://example.org/o3>) }"
    ));
    assert!(!run_ask(
        &ds,
        "ASK { ?s <http://example.org/p> ?o FILTER(?o = <http://example.org/nope>) }"
    ));
}

#[test]
fn ask_repeated_variable_pattern_still_correct() {
    // A pattern with a repeated variable disables the quad-take gate inside
    // the BGP evaluator (see `pattern_repeats_variable`); the budget-of-1
    // wiring at the ASK level must still produce the right boolean via the
    // solutions-level cap.
    let mut ds = Datastore::new(10);
    let n = "http://example.org/n";
    add_quad(&mut ds, n, "http://example.org/loop", n);
    add_quad(
        &mut ds,
        n,
        "http://example.org/edge",
        "http://example.org/other",
    );
    assert!(run_ask(&ds, "ASK { ?x ?p ?x }"));
    assert!(!run_ask(&ds, "ASK { ?x <http://example.org/edge> ?x }"));
}

// ── FILTER EXISTS / NOT EXISTS ──────────────────────────────────────────

#[test]
fn filter_exists_true_selects_matching_rows() {
    let mut ds = Datastore::new(10);
    add_quad(
        &mut ds,
        "http://example.org/a",
        "http://example.org/p",
        "http://example.org/b",
    );
    add_quad(
        &mut ds,
        "http://example.org/a",
        "http://example.org/q",
        "http://example.org/c",
    );
    let count = run_select_count(
        &ds,
        "SELECT ?s WHERE { ?s <http://example.org/q> ?o FILTER EXISTS { ?s <http://example.org/p> ?x } }",
    );
    assert_eq!(count, 1);
}

#[test]
fn filter_exists_false_excludes_nonmatching_rows() {
    let mut ds = Datastore::new(10);
    add_quad(
        &mut ds,
        "http://example.org/a",
        "http://example.org/q",
        "http://example.org/c",
    );
    let count = run_select_count(
        &ds,
        "SELECT ?s WHERE { ?s <http://example.org/q> ?o FILTER EXISTS { ?s <http://example.org/p> ?x } }",
    );
    assert_eq!(count, 0, "no ?s has a <p> edge, so EXISTS excludes the row");
}

#[test]
fn filter_not_exists_inverts_exists() {
    let mut ds = Datastore::new(10);
    add_quad(
        &mut ds,
        "http://example.org/a",
        "http://example.org/p",
        "http://example.org/b",
    );
    add_quad(
        &mut ds,
        "http://example.org/a",
        "http://example.org/q",
        "http://example.org/c",
    );
    add_quad(
        &mut ds,
        "http://example.org/d",
        "http://example.org/q",
        "http://example.org/c",
    );

    let exists_count = run_select_count(
        &ds,
        "SELECT ?s WHERE { ?s <http://example.org/q> ?o FILTER EXISTS { ?s <http://example.org/p> ?x } }",
    );
    let not_exists_count = run_select_count(
        &ds,
        "SELECT ?s WHERE { ?s <http://example.org/q> ?o FILTER NOT EXISTS { ?s <http://example.org/p> ?x } }",
    );
    // Every ?s <q> ?o row falls into exactly one of the two buckets.
    assert_eq!(exists_count, 1, "only ex:a has a <p> edge");
    assert_eq!(not_exists_count, 1, "only ex:d lacks a <p> edge");
}

#[test]
fn filter_exists_over_unselective_inner_pattern_stays_correct() {
    // The inner EXISTS pattern matches many rows for the same outer ?s — the
    // short-circuit must still let every outer row through unchanged.
    let mut ds = linear_store(200);
    add_quad(
        &mut ds,
        "http://example.org/probe",
        "http://example.org/marker",
        "http://example.org/x",
    );
    let count = run_select_count(
        &ds,
        "SELECT ?s WHERE { ?s <http://example.org/marker> ?m FILTER EXISTS { ?x <http://example.org/p> ?y } }",
    );
    assert_eq!(
        count, 1,
        "the outer pattern matches exactly one row (the probe), and the inner \
         EXISTS pattern is unselective (matches all 200 linear_store rows) but \
         only needs to know that at least one match exists"
    );
}

#[test]
fn filter_exists_does_not_leak_inner_bindings_into_outer_scope() {
    // EXISTS { ?s <p> ?inner } must not bind ?inner in the outer projection —
    // the short-circuit must not change this (it never materialises `sols`
    // into the outer solution regardless of how many rows it finds).
    let mut ds = Datastore::new(10);
    add_quad(
        &mut ds,
        "http://example.org/a",
        "http://example.org/p",
        "http://example.org/b",
    );
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let query =
        "SELECT ?s ?inner WHERE { ?s <http://example.org/p> ?o FILTER EXISTS { ?s <http://example.org/p> ?inner } }";
    let (_, parsed) = parse_query(query, &mut ctx).expect("query should parse");
    match execute(&parsed, &ds, NetworkPolicy::Deny).expect("query should execute") {
        QueryResult::Select(r) => {
            assert_eq!(r.rows.len(), 1);
            assert!(
                !r.rows[0].contains_key("inner"),
                "?inner is bound only inside the EXISTS sub-pattern's own scope \
                 and must not leak into the outer solution"
            );
        }
        _ => panic!("expected a SELECT result"),
    }
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.
Contact: hovlanddag@gmail.com
*/

//! Regression guards for Phase B of the join-reordering plan
//! (`docs/plans/JOIN_REORDERING_PLAN.md`, issue
//! [#141](https://github.com/daghovland/rdf-datalog/issues/141)): replacing
//! `PartialSub = HashMap<String, GraphElement>` with
//! `HashMap<String, PartialSubValue>` where bindings from triple-pattern
//! matches become `Interned(GraphElementId)` and bindings from BIND/VALUES
//! become `Computed(GraphElement)`.
//!
//! This is a regression net, not a classic red-green test — there is no
//! "naturally failing" state because Phase B is a behavior-preserving refactor
//! with no new observable functionality: these must pass identically before and
//! after the change.
//!
//! Key correctness risks guarded here:
//! - `compatible()` / `psv_eq()` comparing an `Interned` value (from a triple
//!   pattern) with a `Computed` value (from VALUES or BIND) — must compare by
//!   *resolved value*, not by variant.
//! - `resolve_match_term` resolving a `Computed` variable to a store ID for use
//!   as a pattern constraint.
//! - `eval_expr_as_filter` building a PartialSub from `HashMap<String,
//!   GraphElementId>` without unnecessary clones.

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfLiteral, RdfResource};
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult, SolutionRow};
use std::collections::HashMap;

fn iri_node(iri: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
}

fn str_literal(s: &str) -> GraphElement {
    GraphElement::GraphLiteral(RdfLiteral::LiteralString(s.to_string()))
}

fn add_triple(ds: &mut Datastore, subject: &str, predicate: &str, object: GraphElement) {
    let s = ds.add_resource(iri_node(subject));
    let p = ds.add_resource(iri_node(predicate));
    let o = ds.add_resource(object);
    ds.add_quad(Quad {
        triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
        subject: s,
        predicate: p,
        obj: o,
    });
}

fn run_query(ds: &Datastore, sparql: &str) -> Vec<SolutionRow> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, parsed) = parse_query(sparql, &mut ctx).expect("query should parse");
    match execute(&parsed, ds, NetworkPolicy::Deny).expect("query should execute") {
        QueryResult::Select(r) => r.rows,
        _ => panic!("expected SELECT result"),
    }
}

// ── VALUES + triple-pattern: Inline/Indexed cross-variant unification ─────────

/// VALUES binds `?name` to the string `"Alice"` (after Phase B: `Inline`).
/// The triple pattern also matches `?name` against the same string stored in
/// the graph (after Phase B: `Indexed`). They must unify — the result must
/// contain exactly one row binding `?person` to `:Alice`.
///
/// This exercises `compatible()` / `bind!` with mixed Inline+Indexed variants.
#[test]
fn values_clause_unifies_with_triple_pattern_binding() {
    let mut ds = Datastore::new(1_000);
    add_triple(
        &mut ds,
        "http://example.org/Alice",
        "http://example.org/name",
        str_literal("Alice"),
    );
    add_triple(
        &mut ds,
        "http://example.org/Bob",
        "http://example.org/name",
        str_literal("Bob"),
    );

    // VALUES ?name { "Alice" } filters to a single matching person
    let query = r#"
        SELECT ?person WHERE {
            VALUES ?name { "Alice" }
            ?person <http://example.org/name> ?name .
        }
    "#;

    let rows = run_query(&ds, query);
    assert_eq!(rows.len(), 1, "exactly one person named Alice");
    assert_eq!(
        rows[0].get("person"),
        Some(&iri_node("http://example.org/Alice"))
    );
}

// ── BIND + subsequent triple-pattern: Inline used as pattern constraint ───────

/// BIND copies `?person` into `?p2` (after Phase B: `Inline(person_gel)`).
/// `?p2` is then used as the subject constraint of a second triple pattern,
/// which requires `ast_term_to_dag_term` to resolve an `Inline` value back to
/// a store ID. The join must still produce the correct result.
#[test]
fn bind_alias_used_as_constraint_in_subsequent_pattern() {
    let mut ds = Datastore::new(1_000);
    add_triple(
        &mut ds,
        "http://example.org/Alice",
        "http://example.org/name",
        str_literal("Alice"),
    );
    add_triple(
        &mut ds,
        "http://example.org/Bob",
        "http://example.org/name",
        str_literal("Bob"),
    );
    add_triple(
        &mut ds,
        "http://example.org/Alice",
        "http://example.org/knows",
        iri_node("http://example.org/Bob"),
    );

    // BIND aliases ?person as ?p2, then ?p2 is used as a subject constraint
    let query = r#"
        SELECT ?person ?other WHERE {
            ?person <http://example.org/name> "Alice" .
            BIND(?person AS ?p2)
            ?p2 <http://example.org/knows> ?other .
        }
    "#;

    let rows = run_query(&ds, query);
    assert_eq!(rows.len(), 1, "Alice knows exactly one person");
    assert_eq!(
        rows[0].get("person"),
        Some(&iri_node("http://example.org/Alice"))
    );
    assert_eq!(
        rows[0].get("other"),
        Some(&iri_node("http://example.org/Bob"))
    );
}

// ── Inline+Indexed via UNION branches: both variants reaching compatible() ────

/// When UNION's left branch binds `?x` via a triple pattern (after Phase B:
/// `Indexed`) and the right branch binds `?x` via VALUES (after Phase B:
/// `Inline`), subsequent patterns that join on `?x` must work correctly for
/// both. This exercises `compatible()` when merging UNION rows that have the
/// same variable populated by different paths.
#[test]
fn union_with_values_branch_produces_correct_rows() {
    let mut ds = Datastore::new(1_000);
    add_triple(
        &mut ds,
        "http://example.org/Alice",
        "http://example.org/knows",
        iri_node("http://example.org/Bob"),
    );

    // Left branch: ?x bound by triple pattern (Indexed after Phase B)
    // Right branch: ?x bound by VALUES (Inline after Phase B)
    // Both must yield rows where ?x = :Alice
    let query = r#"
        SELECT ?x WHERE {
            {
                ?x <http://example.org/knows> <http://example.org/Bob> .
            } UNION {
                VALUES ?x { <http://example.org/Alice> }
            }
        }
    "#;

    let rows = run_query(&ds, query);
    // Both UNION branches match :Alice, so we expect two rows (UNION is a bag)
    assert_eq!(rows.len(), 2, "both branches bind ?x to :Alice");
    for row in &rows {
        assert_eq!(
            row.get("x"),
            Some(&iri_node("http://example.org/Alice")),
            "every row must have x = :Alice"
        );
    }
}

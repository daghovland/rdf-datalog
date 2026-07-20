/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! End-to-end (parse + execute) correctness tests for the SPARQL 1.1 §18.3
//! `MINUS` domain-disjointness escape (issue
//! [#187](https://github.com/daghovland/rdf-datalog/issues/187)):
//!
//! ```text
//! minus(Ω1, Ω2) = { μ1 ∈ Ω1 | ∀ μ2 ∈ Ω2, μ1 and μ2 are not compatible, OR
//!                    dom(μ1) ∩ dom(μ2) = ∅ }
//! ```
//!
//! Before this fix, `sparql_parser::execute::eval_component`'s
//! `QueryComponent::Minus` arm threaded the outer row into the inner body's
//! evaluation, so every produced inner solution was a trivial extension of
//! the outer row and therefore always "compatible" with it — the
//! domain-disjointness escape never fired, and `MINUS` behaved exactly like
//! `FILTER NOT EXISTS { inner }`.
//!
//! `full_minuend_survives_only_when_domain_disjoint_or_incompatible` and
//! `part_minuend_partially_bound_domain_check` are hand-ported from the W3C
//! SPARQL 1.1 negation test suite (`full-minuend`/`part-minuend` in
//! `tests/testdata/w3c_sparql11/negation/`), not run from the vendored
//! `.rq`/`.ttl`/`.srx` files directly: the harness in
//! `tests/w3c_sparql11_suite.rs` silently drops every entry in that manifest
//! whose `mf:action` keyword sits on its own line (as all entries in the
//! `negation` manifest do), so `w3c_sparql11_negation` currently runs zero
//! real assertions — a separate, pre-existing gap tracked in
//! [#192](https://github.com/daghovland/rdf-datalog/issues/192) and not
//! fixed here. These two tests are exactly the scenario that a naive
//! "thread `sub` into the body, then check `ms.keys()` for domain overlap"
//! fix would still get wrong: an `OPTIONAL` inside the `MINUS` body can leave
//! a shared variable genuinely unbound for a given inner row, and seeding
//! `sub` would corrupt that row's real domain by carrying the outer value
//! through regardless.

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfLiteral, RdfResource};
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult, SolutionRow};
use std::collections::HashMap;

fn iri_node(iri: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
}

fn literal(s: &str) -> GraphElement {
    GraphElement::GraphLiteral(RdfLiteral::LiteralString(s.to_string()))
}

fn add_quad(ds: &mut Datastore, subject: &str, predicate: &str, object: GraphElement) {
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

fn add_iri_quad(ds: &mut Datastore, subject: &str, predicate: &str, object: &str) {
    add_quad(ds, subject, predicate, iri_node(object));
}

fn run_query(ds: &Datastore, query: &str) -> Vec<SolutionRow> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, parsed) = parse_query(query, &mut ctx).expect("query should parse");
    match execute(&parsed, ds, NetworkPolicy::Deny).expect("query should execute") {
        QueryResult::Select(r) => r.rows,
        QueryResult::Ask(_) | QueryResult::Construct(_) | QueryResult::Describe(_) => {
            panic!("expected SELECT result")
        }
    }
}

/// Order-independent comparison: a row is a set of (var, value) pairs, and
/// the result is a multiset of rows.
fn rows_as_sorted_bag(rows: &[SolutionRow]) -> Vec<Vec<(String, GraphElement)>> {
    let mut bag: Vec<Vec<(String, GraphElement)>> = rows
        .iter()
        .map(|row| {
            let mut entries: Vec<(String, GraphElement)> =
                row.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            entries.sort();
            entries
        })
        .collect();
    bag.sort();
    bag
}

fn sorted_bag_of(rows: Vec<Vec<(&str, GraphElement)>>) -> Vec<Vec<(String, GraphElement)>> {
    let mut bag: Vec<Vec<(String, GraphElement)>> = rows
        .into_iter()
        .map(|row| {
            let mut entries: Vec<(String, GraphElement)> =
                row.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            entries.sort();
            entries
        })
        .collect();
    bag.sort();
    bag
}

/// The issue's own reproduction: the `MINUS` body (`?x ex:age "25"`) shares
/// *no* variable at all with the outer pattern (`?s ex:age ?o`). Per SPARQL
/// 1.1 §18.3, `dom(μ1) ∩ dom(μ2) = ∅` for every possible μ2, so the
/// domain-disjointness escape applies unconditionally and `MINUS` must
/// exclude nothing, regardless of what data the body matches. Before the
/// fix this returned 0 rows (both were wrongly excluded because the body
/// happens to be satisfiable at all, by alice).
#[test]
fn zero_shared_vars_minus_never_excludes() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example.org/";

    add_quad(
        &mut ds,
        &format!("{ex}alice"),
        &format!("{ex}age"),
        literal("25"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}bob"),
        &format!("{ex}age"),
        literal("30"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?s WHERE {{
  ?s ex:age ?o
  MINUS {{ ?x ex:age "25" }}
}}"#
    );

    let rows = run_query(&ds, &query);
    assert_eq!(
        rows.len(),
        2,
        "MINUS's ?x shares no variable with the outer ?s/?o, so neither row \
         may ever be excluded by it"
    );
    let mut subjects: Vec<GraphElement> =
        rows.iter().map(|r| r.get("s").unwrap().clone()).collect();
    subjects.sort();
    let mut expected = vec![
        iri_node(&format!("{ex}alice")),
        iri_node(&format!("{ex}bob")),
    ];
    expected.sort();
    assert_eq!(subjects, expected);
}

/// Regression guard: a `MINUS` body that *does* share a variable with the
/// outer pattern, and genuinely conflicts with it on that variable's value,
/// must still exclude the matching outer rows. This is the plain,
/// non-`OPTIONAL` shared-variable case the fix must not regress.
#[test]
fn shared_var_minus_still_excludes_matching_rows() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example.org/";

    for i in 0..5 {
        add_iri_quad(
            &mut ds,
            &format!("{ex}s{i}"),
            &format!("{ex}age"),
            &format!("{ex}v{i}"),
        );
    }
    // s1 and s3 are additionally marked excluded.
    add_iri_quad(
        &mut ds,
        &format!("{ex}s1"),
        &format!("{ex}excludeMe"),
        &format!("{ex}true"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}s3"),
        &format!("{ex}excludeMe"),
        &format!("{ex}true"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?s WHERE {{
  ?s ex:age ?o
  MINUS {{ ?s ex:excludeMe ?any }}
}}"#
    );

    let rows = run_query(&ds, &query);
    let mut subjects: Vec<GraphElement> =
        rows.iter().map(|r| r.get("s").unwrap().clone()).collect();
    subjects.sort();
    let mut expected = vec![
        iri_node(&format!("{ex}s0")),
        iri_node(&format!("{ex}s2")),
        iri_node(&format!("{ex}s4")),
    ];
    expected.sort();
    assert_eq!(
        subjects, expected,
        "s1 and s3 share ?s with a compatible, domain-overlapping MINUS solution and must be excluded"
    );
}

/// Ported from the W3C SPARQL 1.1 negation test suite's `full-minuend`
/// (`tests/testdata/w3c_sparql11/negation/full-minuend.{rq,ttl,srx}`).
///
/// The outer pattern fully binds `?a ?b ?c`. The `MINUS` body binds `?d`
/// unconditionally and `?b`/`?c` only *if* the corresponding `OPTIONAL`
/// matches — so different `?d` solutions have genuinely different real
/// domains, and a fix that seeds the body with the outer row (rather than
/// evaluating it independently) cannot tell an `OPTIONAL` that truly left a
/// variable unbound from one that merely inherited the outer value.
#[test]
fn full_minuend_survives_only_when_domain_disjoint_or_incompatible() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example/";

    for i in 0..4 {
        add_iri_quad(
            &mut ds,
            &format!("{ex}a{i}"),
            &format!("{ex}p1"),
            &format!("{ex}b{i}"),
        );
        add_iri_quad(
            &mut ds,
            &format!("{ex}a{i}"),
            &format!("{ex}p2"),
            &format!("{ex}c{i}"),
        );
    }
    // d0: bare :Sub, no q1/q2 at all -> its real MINUS-side domain is just
    // {d}, disjoint from {a,b,c} -> can never exclude anything.
    add_iri_quad(
        &mut ds,
        &format!("{ex}d0"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );
    // d1: q1=b1, q2=c1 -> domain {d,b,c}, matches a1's row exactly ->
    // excludes a1.
    add_iri_quad(
        &mut ds,
        &format!("{ex}d1"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d1"),
        &format!("{ex}q1"),
        &format!("{ex}b1"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d1"),
        &format!("{ex}q2"),
        &format!("{ex}c1"),
    );
    // d2: q1=b2 only, no q2 -> real domain {d,b}; overlaps a2's row on `b`
    // alone, compatible on it -> excludes a2 even though `c` was never
    // bound by this MINUS solution.
    add_iri_quad(
        &mut ds,
        &format!("{ex}d2"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d2"),
        &format!("{ex}q1"),
        &format!("{ex}b2"),
    );
    // d3: q1=b3, q2=cx (mismatched) -> domain {d,b,c}, but c=cx conflicts
    // with a3's c3 -> not compatible -> does not exclude a3.
    add_iri_quad(
        &mut ds,
        &format!("{ex}d3"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d3"),
        &format!("{ex}q1"),
        &format!("{ex}b3"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d3"),
        &format!("{ex}q2"),
        &format!("{ex}cx"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?a ?b ?c {{
  ?a ex:p1 ?b .
  ?a ex:p2 ?c .
  MINUS {{
    ?d ex:type ex:Sub
    OPTIONAL {{ ?d ex:q1 ?b }}
    OPTIONAL {{ ?d ex:q2 ?c }}
  }}
}}"#
    );

    let rows = run_query(&ds, &query);
    let expected = sorted_bag_of(vec![
        vec![
            ("a", iri_node(&format!("{ex}a0"))),
            ("b", iri_node(&format!("{ex}b0"))),
            ("c", iri_node(&format!("{ex}c0"))),
        ],
        vec![
            ("a", iri_node(&format!("{ex}a3"))),
            ("b", iri_node(&format!("{ex}b3"))),
            ("c", iri_node(&format!("{ex}c3"))),
        ],
    ]);
    assert_eq!(
        rows_as_sorted_bag(&rows),
        expected,
        "only a0 (no compatible+overlapping MINUS solution) and a3 (d3 conflicts on c) survive"
    );
}

/// Ported from the W3C SPARQL 1.1 negation test suite's `part-minuend`
/// (`tests/testdata/w3c_sparql11/negation/part-minuend.{rq,ttl,srx}`). Here
/// the *outer* pattern also uses `OPTIONAL`, so some outer rows themselves
/// have a partial domain (e.g. `?c` unbound) — exercising the
/// domain-disjointness escape from the Ω1 side as well as the Ω2 side.
#[test]
fn part_minuend_partially_bound_domain_check() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example/";

    add_iri_quad(
        &mut ds,
        &format!("{ex}a1"),
        &format!("{ex}type"),
        &format!("{ex}Min"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}a1"),
        &format!("{ex}p1"),
        &format!("{ex}b1"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}a2"),
        &format!("{ex}type"),
        &format!("{ex}Min"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}a2"),
        &format!("{ex}p1"),
        &format!("{ex}b2"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}a3"),
        &format!("{ex}type"),
        &format!("{ex}Min"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}a3"),
        &format!("{ex}p1"),
        &format!("{ex}b3"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}a4"),
        &format!("{ex}type"),
        &format!("{ex}Min"),
    );

    add_iri_quad(
        &mut ds,
        &format!("{ex}d1"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d1"),
        &format!("{ex}q1"),
        &format!("{ex}b1"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d3"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d3"),
        &format!("{ex}q1"),
        &format!("{ex}b3"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d3"),
        &format!("{ex}q2"),
        &format!("{ex}c3"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d4"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d4"),
        &format!("{ex}q1"),
        &format!("{ex}b4"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d4"),
        &format!("{ex}q2"),
        &format!("{ex}c4"),
    );
    add_iri_quad(
        &mut ds,
        &format!("{ex}d5"),
        &format!("{ex}type"),
        &format!("{ex}Sub"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?a ?b ?c {{
  ?a ex:type ex:Min
  OPTIONAL {{ ?a ex:p1 ?b }}
  OPTIONAL {{ ?a ex:p2 ?c }}
  MINUS {{
    ?d ex:type ex:Sub
    OPTIONAL {{ ?d ex:q1 ?b }}
    OPTIONAL {{ ?d ex:q2 ?c }}
  }}
}}"#
    );

    let rows = run_query(&ds, &query);
    let expected = sorted_bag_of(vec![
        vec![
            ("a", iri_node(&format!("{ex}a2"))),
            ("b", iri_node(&format!("{ex}b2"))),
        ],
        vec![("a", iri_node(&format!("{ex}a4")))],
    ]);
    assert_eq!(
        rows_as_sorted_bag(&rows),
        expected,
        "a1 and a3 are excluded (compatible+overlapping MINUS solutions d1, d3); \
         a2 survives with only ?b bound; a4 survives via the domain-disjointness \
         escape (its domain {{a}} is disjoint from the body's {{d,b,c}})"
    );
}

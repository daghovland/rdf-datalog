/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Cyclic/star BGP join-reordering re-evaluation (issue
//! [#507](https://github.com/daghovland/rdf-datalog/issues/507)), a
//! precursor to [#39](https://github.com/daghovland/rdf-datalog/issues/39)
//! (Phase D of `docs/plans/JOIN_REORDERING_PLAN.md`: worst-case-optimal
//! join for cyclic star BGPs).
//!
//! **Star vs. cyclic, precisely.** A "star" BGP — `?s p1 ?o1 . ?s p2 ?o2 .
//! ?s p3 ?o3` — has an *acyclic* join graph (a tree rooted at `?s`): any
//! binary join order that picks the most selective pattern first does fine,
//! which is exactly what Phase A's `join_ordering::order_patterns` already
//! does. This includes every query in `tests/testdata/dblp.benchmark.tsv`
//! whose name contains "star" (`join-3-star-largest-sum-of-join-sizes`,
//! `join-xlarge-star-on-small-predicates`, etc.) — a scripted scan of all
//! 105 queries' triple-pattern join graphs (counting edges vs. nodes per
//! connected component) found **zero** queries whose join graph actually
//! has a cycle. The pathological shape worst-case-optimal join literature
//! targets is a genuine *cycle*: `?a :e ?b . ?b :e ?c . ?c :e ?a` (a
//! triangle on three pairwise-shared variables, no single variable shared
//! by all three patterns) — provably not optimally orderable by *any*
//! sequence of binary joins, regardless of which pattern goes first.
//!
//! This file adds the genuinely cyclic shape the DBLP benchmark corpus
//! doesn't contain, as a synthetic adversarial construction:
//! - a "hub" vertex with high degree in both directions,
//! - a small number of independent closing edges between non-hub
//!   vertices, each of which turns into a real triangle through the hub,
//!   so the *final* triangle count stays small and fixed while the number
//!   of vertices (and hence the hub's degree) grows.
//!
//! Any binary join order materializes an intermediate join result
//! quadratic in the hub's degree before the final pattern filters it back
//! down to the small, fixed triangle count — see
//! [`synthetic_triangle_graph`]'s doc for the exact construction and why.
//!
//! Ground truth is computed independently (brute-force triangle
//! enumeration over the generated edge list, not a hand-derived formula),
//! so the correctness assertion doesn't depend on getting the combinatorics
//! right by hand.

use dag_rdf::Datastore;
use sparql_parser::{NetworkPolicy, ParserContext, QueryResult, execute, explain_query, parse_query};
use std::collections::HashMap;
use std::io::Cursor;
use std::time::Instant;
use turtle::parse_ntriples;

const EX: &str = "http://example.org/jrb#";

/// Build a synthetic N-Triples graph with a deliberately cyclic/star-
/// adversarial shape for the triangle query
/// `?a :edge ?b . ?b :edge ?c . ?c :edge ?a`.
///
/// Construction, parameterized by `n` (number of non-hub leaves) and `k`
/// (number of independent closing edges, `k <= n / 2`):
/// - `hub :edge leaf_i` for every `i` in `0..n` (hub out-degree `n`).
/// - `leaf_i :edge hub` for every `i` in `0..n` (hub in-degree `n`).
/// - `leaf_(2*j) :edge leaf_(2*j+1)` for every `j` in `0..k` — `k`
///   independent (non-overlapping) closing edges, each completing exactly
///   one directed 3-cycle through the hub: `hub -> leaf_2j -> leaf_2j+1 ->
///   hub`.
///
/// Total edges: `2*n + k` (linear in `n`). Total triangles: a small,
/// `n`-independent constant determined by `k` alone (see
/// [`brute_force_triangle_count`] for the exact count rather than a
/// hand-derived formula).
///
/// Why this is adversarial for *any* binary join order: every pattern uses
/// the same predicate, so Phase A's cardinality tie-break can't distinguish
/// them, and by symmetry every rotation of the triangle pattern picks the
/// hub as the first-bound variable in some row. Once one pattern binds a
/// variable to `hub`, the next pattern's indexed lookup on `hub` returns
/// `n` matches — for the `n` rows where the first pattern's binding
/// happens to land on `hub`, that's `O(n)` work each, i.e. `O(n^2)` total
/// intermediate rows, even though only `O(k)` of them survive the final
/// pattern's filter back down to a real triangle.
fn synthetic_triangle_graph(n: usize, k: usize) -> String {
    assert!(k * 2 <= n, "k closing edges need 2*k distinct leaves <= n");
    let mut nt = String::new();
    let hub = format!("<{EX}hub>");
    let leaf = |i: usize| format!("<{EX}leaf{i}>");
    let edge = format!("<{EX}edge>");
    for i in 0..n {
        nt.push_str(&format!("{hub} {edge} {} .\n", leaf(i)));
        nt.push_str(&format!("{} {edge} {hub} .\n", leaf(i)));
    }
    for j in 0..k {
        nt.push_str(&format!("{} {edge} {} .\n", leaf(2 * j), leaf(2 * j + 1)));
    }
    nt
}

/// Brute-force ground truth for the number of `(a, b, c)` bindings
/// satisfying `edge(a,b) . edge(b,c) . edge(c,a)` over the edge list
/// `synthetic_triangle_graph` produced — independent of the SPARQL
/// executor, so the test's correctness assertion isn't just checking the
/// executor agrees with its own join order.
fn brute_force_triangle_count(n: usize, k: usize) -> usize {
    let mut out_edges: HashMap<usize, Vec<usize>> = HashMap::new();
    let hub = usize::MAX; // sentinel distinct from any leaf index
    let mut add = |from: usize, to: usize| out_edges.entry(from).or_default().push(to);
    for i in 0..n {
        add(hub, i);
        add(i, hub);
    }
    for j in 0..k {
        add(2 * j, 2 * j + 1);
    }
    let mut count = 0usize;
    for (&a, bs) in &out_edges {
        for &b in bs {
            if let Some(cs) = out_edges.get(&b) {
                for &c in cs {
                    if let Some(a_edges) = out_edges.get(&c) {
                        if a_edges.contains(&a) {
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    count
}

const TRIANGLE_QUERY: &str = concat!(
    "PREFIX : <http://example.org/jrb#> ",
    "SELECT ?a ?b ?c WHERE { ?a :edge ?b . ?b :edge ?c . ?c :edge ?a . }"
);

fn load_graph(nt: &str) -> Datastore {
    let mut datastore = Datastore::new(nt.len());
    parse_ntriples(&mut datastore, Cursor::new(nt.as_bytes().to_vec()))
        .expect("synthetic N-Triples must parse");
    datastore
}

fn run_triangle_query(datastore: &Datastore) -> usize {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(TRIANGLE_QUERY, &mut ctx).expect("triangle query must parse");
    match execute(&query, datastore, NetworkPolicy::Deny).expect("triangle query must execute") {
        QueryResult::Select(r) => r.rows.len(),
        other => panic!("expected Select result, got {other:?}"),
    }
}

/// Correctness check at a small, fast-in-debug-mode scale: the SPARQL
/// executor's result count for the genuinely cyclic triangle query matches
/// an independently computed ground truth. This is the "test" half of this
/// diagnostic file — not ignored, runs in the normal `cargo test` suite.
#[test]
fn synthetic_triangle_query_matches_brute_force_count() {
    let (n, k) = (40, 5);
    let nt = synthetic_triangle_graph(n, k);
    let datastore = load_graph(&nt);
    let expected = brute_force_triangle_count(n, k);
    let actual = run_triangle_query(&datastore);
    assert_eq!(
        actual, expected,
        "SPARQL triangle query result count must match brute-force ground truth"
    );
    // Sanity: every closing edge must actually produce a triangle, and no
    // accidental triangle can arise from the hub edges alone, otherwise
    // this isn't testing what the doc comment claims.
    assert_eq!(expected, 3 * k, "expected exactly 3 rows (one per rotation) per closing edge");
}

/// Diagnostic sweep: grows the hub's degree (`n`) while holding the real
/// triangle count fixed (`k` constant), and reports wall-clock time plus
/// the static join order Phase A actually chose. Demonstrates whether the
/// *ceiling* worst-case-optimal join literature predicts for cyclic BGPs is
/// visible in practice on this codebase's current binary-join executor.
///
/// `#[ignore]`d like `tests/dblp_benchmark.rs`'s diagnostics — this is a
/// timing probe, not a correctness test, and debug-mode timings are
/// meaningless for it. Run with:
/// ```bash
/// cargo test --test join_reorder_benchmark --release -- --ignored --nocapture synthetic_triangle_scaling_sweep
/// ```
#[test]
#[ignore = "diagnostic timing sweep — run explicitly with --release --nocapture, see module doc"]
fn synthetic_triangle_scaling_sweep() {
    println!("\n=== Synthetic cyclic triangle BGP — hub-degree scaling sweep ===");
    println!("(#507, precursor measurement for #39 / JOIN_REORDERING_PLAN.md Phase D)\n");
    println!(
        "  {:>8}  {:>8}  {:>12}  {:>10}  {:>10}",
        "n", "k", "edges", "time (ms)", "triangles"
    );

    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(TRIANGLE_QUERY, &mut ctx).expect("triangle query must parse");

    for &n in &[500usize, 1000, 2000, 4000] {
        let k = 5;
        let nt = synthetic_triangle_graph(n, k);
        let datastore = load_graph(&nt);

        // Report the static join order Phase A chose, once, at the
        // smallest scale printed (it does not depend on n/k since every
        // pattern shares the same predicate and no constants — see the
        // module doc's symmetry argument).
        if n == 500 {
            let plan = explain_query(&query, &datastore);
            println!("  static join order (Phase A, order_patterns): {plan:?}\n");
        }

        let t = Instant::now();
        let rows = run_triangle_query(&datastore);
        let elapsed = t.elapsed();
        println!(
            "  {:>8}  {:>8}  {:>12}  {:>10}  {:>10}",
            n,
            k,
            2 * n + k,
            elapsed.as_millis(),
            rows
        );
        assert_eq!(rows, 3 * k, "triangle count must stay fixed as n grows");
    }
    println!(
        "\n  If time grows roughly quadratically in n while the triangle\n  \
         count stays fixed at {} rows, that confirms the O(n^2)\n  \
         intermediate-row ceiling from a binary join on a genuinely\n  \
         cyclic BGP is real on this executor (any join order is equally\n  \
         bad by symmetry — see module doc). It does not by itself say\n  \
         this matters for dagalog's actual workloads: see the #507 write-\n  \
         up (docs/plans/JOIN_REORDERING_PLAN.md Phase D section and the\n  \
         #39 comment) for why none of the 105 DBLP benchmark queries, the\n  \
         W3C SPARQL test suite, or the IMF/LUBM fixtures this repo tests\n  \
         against actually contain a cyclic join graph.",
        3 * 5
    );
}

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
//! binary join order that picks the most selective pattern first does
//! fine, which is exactly what Phase A's `join_ordering::order_patterns`
//! already does. The shape worst-case-optimal join literature targets is a
//! genuine *cycle*: `?a :e ?b . ?b :e ?c . ?c :e ?a` (a triangle on three
//! pairwise-shared variables, no single variable shared by all three
//! patterns) — provably not optimally orderable by *any* sequence of
//! binary joins, regardless of which pattern goes first.
//!
//! This file has three parts:
//! 1. [`dblp_benchmark_queries_have_no_cyclic_bgp`] — an AST-based (not
//!    regex-based) scan of every query in
//!    `tests/testdata/dblp.benchmark.tsv` confirming none of the "star"-
//!    named queries (or any other query in the suite) actually has a
//!    cyclic join graph — the DBLP benchmark corpus this repo measures
//!    against contains no instance of the shape Phase D would help with.
//! 2. [`synthetic_triangle_query_matches_brute_force_count`] /
//!    [`synthetic_triangle_scaling_sweep`] — a synthetic *adversarial*
//!    cyclic triangle (symmetric predicate, hub-degree scaling) that
//!    demonstrates the `O(n^2)` intermediate-row ceiling genuinely exists
//!    on this executor when nothing distinguishes the three patterns'
//!    selectivity.
//! 3. [`lubm_style_triangle_query_realistic`] — a *realistic* cyclic
//!    triangle modeled on LUBM's canonical Q9 shape (`?x advisor ?y . ?y
//!    teacherOf ?z . ?x takesCourse ?z`), where the three predicates have
//!    different, non-adversarial selectivity (advisor and teacherOf are
//!    close to functional), checking whether a realistic cyclic query
//!    still avoids the pathological blowup even without Phase D.

use dag_rdf::Datastore;
use sparql_parser::ast::{Query, QueryComponent, TriplePattern};
use sparql_parser::{
    NetworkPolicy, ParserContext, ProfileNode, QueryResult, bgp_has_cyclic_join_graph,
    collect_bgps, execute, execute_with_profile, parse_query,
};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::time::Instant;
use turtle::parse_ntriples;

// ── Part 1: AST-based cyclic-join-graph scan of the DBLP benchmark suite ───

/// One row of `dblp.benchmark.tsv`: name and full query text. Duplicated
/// from `tests/dblp_benchmark.rs`'s identical helper rather than shared,
/// since integration test binaries each compile as a separate crate.
fn parse_benchmark_tsv(path: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(path).expect("benchmark TSV must be readable");
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, query) = line
                .split_once('\t')
                .expect("each benchmark line must contain a tab-separated name and query");
            (name.to_string(), query.to_string())
        })
        .collect()
}

// `bgp_has_cyclic_join_graph`/`collect_bgps` moved into
// `sparql_parser::cyclic_bgp` (issue #732) so the production query-plan
// path and this benchmark suite share one implementation instead of two
// copies that could silently drift apart — see that module's doc.
// `where_clause_of` stays local: it's a trivial 7-line AST-shape match, not
// algorithmic logic worth the `#[doc(hidden)] pub` exposure the other two
// needed (`sparql_parser`'s own copy is `pub(crate)`, unreachable from this
// separately-compiled integration test crate).
fn where_clause_of(query: &Query) -> &[QueryComponent] {
    match query {
        Query::Select { where_clause, .. }
        | Query::Ask { where_clause, .. }
        | Query::Construct { where_clause, .. }
        | Query::Describe { where_clause, .. } => where_clause.as_slice(),
    }
}

/// AST-based (not regex-based) confirmation that none of the 105
/// Sparqloscope/QLever DBLP benchmark queries — in particular none of the
/// ones named "...star..." — has a genuinely cyclic BGP join graph. Only
/// queries that actually parse are analyzed; the test reports how many
/// that was out of the total, so a parse-coverage gap can't silently hide
/// as "nothing cyclic found".
#[test]
fn dblp_benchmark_queries_have_no_cyclic_bgp() {
    let tsv_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join("dblp.benchmark.tsv");
    let queries = parse_benchmark_tsv(&tsv_path);
    assert!(!queries.is_empty(), "benchmark TSV must not be empty");

    let mut analyzed = 0usize;
    let mut cyclic: Vec<String> = Vec::new();
    for (name, query_str) in &queries {
        let mut ctx = ParserContext {
            prefixes: HashMap::new(),
            base: None,
        };
        let Ok((_, query)) = parse_query(query_str, &mut ctx) else {
            continue;
        };
        analyzed += 1;
        let mut bgps = Vec::new();
        collect_bgps(where_clause_of(&query), &mut bgps);
        for patterns in &bgps {
            if bgp_has_cyclic_join_graph(patterns) {
                cyclic.push(name.clone());
            }
        }
    }

    println!(
        "dblp.benchmark.tsv: {analyzed}/{} queries parsed and analyzed for cyclic BGPs",
        queries.len()
    );
    assert!(
        analyzed > 50,
        "expected most of the 105 benchmark queries to parse (got {analyzed}) — \
         a big drop here would mean this check's coverage silently shrank"
    );
    assert!(
        cyclic.is_empty(),
        "expected no cyclic BGP join graphs in the DBLP benchmark suite, found: {cyclic:?}"
    );
}

/// Positive/negative controls for [`bgp_has_cyclic_join_graph`] itself.
/// `dblp_benchmark_queries_have_no_cyclic_bgp` finding "zero cyclic BGPs in
/// 105 queries" is only meaningful evidence if the checker can actually
/// return `true` on a genuine cycle — an earlier draft of this check
/// (pairwise cliques instead of hypergraph/GYO reduction) produced eight
/// false positives on exactly this suite (see the `BGP hypergraph`
/// comment on [`bgp_has_cyclic_join_graph`]). This test pins both
/// directions.
#[test]
fn join_graph_cycle_check_positive_and_negative_controls() {
    fn bgps_of(query_str: &str) -> Vec<Vec<&TriplePattern>> {
        // Leak the parsed AST so its borrowed `TriplePattern`s can outlive
        // this helper — fine for a test helper that's called a handful of
        // times with tiny queries.
        let mut ctx = ParserContext {
            prefixes: HashMap::new(),
            base: None,
        };
        let (_, query) = parse_query(query_str, &mut ctx).expect("control query must parse");
        let query: &'static Query = Box::leak(Box::new(query));
        let mut bgps = Vec::new();
        collect_bgps(where_clause_of(query), &mut bgps);
        bgps
    }

    // Positive controls: genuinely cyclic.
    for (label, query_str) in [
        ("adversarial triangle", TRIANGLE_QUERY),
        ("LUBM Q9-style triangle", LUBM_Q9_QUERY),
        (
            "triangle split by a FILTER into two adjacent BGP siblings",
            "PREFIX : <http://example.org/jrb#> SELECT * WHERE { \
             ?a :e ?b . FILTER(?a != ?b) ?b :e ?c . ?c :e ?a . }",
        ),
    ] {
        let bgps = bgps_of(query_str);
        assert!(
            bgps.iter().any(|p| bgp_has_cyclic_join_graph(p)),
            "expected a cyclic join graph for: {label}"
        );
    }

    // Negative controls: acyclic (star / single-pattern / multi-column-on-
    // one-shared-pair), each previously a false positive under the
    // pairwise-clique bug.
    for (label, query_str) in [
        (
            "star on one shared variable",
            "PREFIX : <http://example.org/jrb#> SELECT * WHERE { \
             ?s :p1 ?o1 . ?s :p2 ?o2 . ?s :p3 ?o3 . }",
        ),
        (
            "single pattern, three distinct variables",
            "SELECT * WHERE { ?s ?p ?o . }",
        ),
        (
            "multicolumn join on the same subject/object pair",
            "PREFIX : <http://example.org/jrb#> SELECT * WHERE { \
             ?s :p ?o . ?s :q ?o . }",
        ),
    ] {
        let bgps = bgps_of(query_str);
        assert!(
            !bgps.iter().any(|p| bgp_has_cyclic_join_graph(p)),
            "expected an acyclic join graph for: {label}"
        );
    }
}

// ── Part 2: synthetic adversarial cyclic triangle (hub-degree scaling) ─────

const EX: &str = "http://example.org/jrb#";

/// Build a synthetic N-Triples graph with a deliberately cyclic/adversarial
/// shape for the triangle query `?a :edge ?b . ?b :edge ?c . ?c :edge ?a`.
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
/// Total edges: `2*n + k` (linear in `n`). Total triangle-query rows: a
/// small, `n`-independent constant determined by `k` alone (see
/// [`brute_force_triangle_count`] for the exact count rather than a
/// hand-derived formula).
///
/// Why this is adversarial for *any* binary join order: every pattern uses
/// the same predicate, so Phase A's cardinality tie-break can't distinguish
/// them, and by symmetry every rotation of the triangle pattern picks the
/// hub as a bound value in some row. Once one pattern binds a variable to
/// `hub`, the next pattern's indexed lookup on `hub` returns `n` matches —
/// for the `n` rows where that happens, that's `O(n)` work each, i.e.
/// `O(n^2)` total intermediate rows, even though only `O(k)` of them
/// survive the final pattern's filter back down to a real triangle.
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
            let Some(cs) = out_edges.get(&b) else {
                continue;
            };
            for &c in cs {
                if out_edges
                    .get(&c)
                    .is_some_and(|a_edges| a_edges.contains(&a))
                {
                    count += 1;
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
    let mut datastore = Datastore::new(nt.len().min(u32::MAX as usize) as u32);
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
        _ => panic!("expected Select result"),
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
    // Sanity: every closing edge must actually produce a triangle (one row
    // per rotation), and no accidental triangle arises from the hub edges
    // alone, otherwise this isn't testing what the doc comment claims.
    assert_eq!(
        expected,
        3 * k,
        "expected exactly 3 rows (one per rotation) per closing edge"
    );
}

/// Print every `"Pattern"` node in a profile tree, recursively, labelled
/// with its cumulative `rows_in`/`rows_out`/time — the actual evidence of
/// intermediate-row blowup (or lack of it), not just a wall-clock number.
fn print_pattern_profile(nodes: &[ProfileNode], indent: usize) {
    for node in nodes {
        if node.kind == "Pattern" || node.kind == "BGP" {
            println!(
                "    {:indent$}{:<8} {:<40} rows_in={:>10} rows_out={:>10} time={:>8.2}ms",
                "",
                node.kind,
                node.label.clone().unwrap_or_default(),
                node.rows_in,
                node.rows_out,
                node.total_time_ms,
                indent = indent
            );
        }
        print_pattern_profile(&node.children, indent + 2);
    }
}

/// Diagnostic sweep: grows the hub's degree (`n`) while holding the real
/// triangle count fixed (`k` constant), and reports wall-clock time plus
/// each triple pattern's actual `rows_in`/`rows_out` (via
/// [`execute_with_profile`], issue #572) — the direct evidence for whether
/// the predicted `O(n^2)` intermediate-row ceiling is real on this
/// executor, not just a derived timing number.
///
/// `#[ignore]`d like `tests/dblp_benchmark.rs`'s diagnostics — this is a
/// timing probe, not a correctness test, and debug-mode timings are
/// meaningless for it. `n` is capped at 1000 (~1,000,000 intermediate
/// rows) deliberately — this measurement environment has ~3.8 GB RAM and
/// no swap (confirmed with `free -m` while writing this), shared with
/// concurrent worktree builds, and a `PartialSub`-per-row materialization
/// at `n=2000` (~4,000,000 rows) or higher risks an OOM that could take
/// down other agents' builds on the same machine — exactly the failure
/// mode the #38 comment thread hit at 2M DBLP triples on its own
/// 3.7 GB/no-swap measurement box. Only raise the cap after checking
/// `free -m` has real headroom and running under `/usr/bin/time -v` to see
/// peak RSS first. Run with:
/// ```bash
/// cargo test --test join_reorder_benchmark --release -- --ignored --nocapture synthetic_triangle_scaling_sweep
/// ```
#[test]
#[ignore = "diagnostic timing sweep — run explicitly with --release --nocapture, see module doc"]
fn synthetic_triangle_scaling_sweep() {
    println!("\n=== Synthetic cyclic triangle BGP — hub-degree scaling sweep ===");
    println!("(#507, precursor measurement for #39 / JOIN_REORDERING_PLAN.md Phase D)\n");

    let k = 5;
    for &n in &[200usize, 500, 1000] {
        let nt = synthetic_triangle_graph(n, k);
        let datastore = load_graph(&nt);
        let mut ctx = ParserContext {
            prefixes: HashMap::new(),
            base: None,
        };
        let (_, query) = parse_query(TRIANGLE_QUERY, &mut ctx).expect("triangle query must parse");

        let t = Instant::now();
        let (result, profile) =
            execute_with_profile(&query, &datastore, NetworkPolicy::Deny, None, None);
        let elapsed = t.elapsed();
        let rows = match result.expect("triangle query must execute") {
            QueryResult::Select(r) => r.rows.len(),
            _ => panic!("expected Select result"),
        };

        println!(
            "n={n:<6} k={k:<4} edges={:<8} time={:>8} ms  final_rows={rows}",
            2 * n + k,
            elapsed.as_millis()
        );
        print_pattern_profile(&profile, 0);
        println!();
        assert_eq!(rows, 3 * k, "triangle count must stay fixed as n grows");
    }
    println!(
        "If the middle pattern's rows_out grows roughly with n^2 while the\n\
         final row count stays fixed at {}, that confirms the intermediate-\n\
         row ceiling from a binary join on a genuinely cyclic, symmetric-\n\
         predicate BGP is real on this executor (every join order is\n\
         equally bad here by symmetry — see module doc). See the #507\n\
         write-up (docs/plans/JOIN_REORDERING_PLAN.md Phase D section and\n\
         the #39 comment) for how this compares to the realistic LUBM-\n\
         style query below and to the DBLP benchmark corpus (part 1 of this\n\
         file), which contains no cyclic BGP at all.",
        3 * k
    );
}

// ── Part 3: realistic cyclic triangle (LUBM Q9 shape) ──────────────────────

/// Build a small LUBM-Q9-shaped synthetic university dataset: `n_profs`
/// professors, each teaching `courses_per_prof` courses; `n_students`
/// students, each with exactly one advisor (round-robin over professors,
/// mirroring LUBM's near-functional advisor relation) and
/// `courses_taken_per_student` courses drawn deterministically from the
/// whole course pool, only sometimes overlapping with their own advisor's
/// course list — most `(student, course)` pairs do *not* close a triangle,
/// matching the sparsity real Q9 answers have.
///
/// **This is not reusing `benches/lubm.rs`'s own LUBM generator**, and that
/// is itself a finding worth recording: reading `add_students`/
/// `add_courses` there (`benches/lubm.rs:239-294`) shows every student's
/// `takesCourse` object is a uniquely-named `gradcourse{s}`/`course{s}`
/// IRI, while every professor's `teacherOf` object is a disjoint,
/// separately-named `taught{c}` IRI — the two sets never intersect by
/// construction. Running the literal Q9 query (`?x advisor ?y . ?y
/// teacherOf ?z . ?x takesCourse ?z`) against that fixture as it stands
/// today would always return **zero** rows, telling us nothing about join
/// cost. This function instead builds a deliberately overlapping dataset
/// so the query has a real, nonzero (but still sparse — see
/// [`lubm_style_triangle_query_realistic`]'s `expected > 0` assertion)
/// answer to measure.
///
/// Unlike [`synthetic_triangle_graph`], the three predicates here
/// (`advisor`, `teacherOf`, `takesCourse`) are distinct, so Phase A's
/// cardinality tie-break *can* distinguish them — this models whether a
/// real-world cyclic query, as opposed to an adversarially symmetric one,
/// is actually a problem in practice.
fn lubm_style_graph(
    n_profs: usize,
    courses_per_prof: usize,
    n_students: usize,
    courses_taken_per_student: usize,
) -> (String, usize) {
    let prof = |i: usize| format!("<{EX}prof{i}>");
    let student = |i: usize| format!("<{EX}student{i}>");
    let course = |i: usize| format!("<{EX}course{i}>");
    let advisor = format!("<{EX}advisor>");
    let teacher_of = format!("<{EX}teacherOf>");
    let takes_course = format!("<{EX}takesCourse>");

    let total_courses = n_profs * courses_per_prof;
    let mut nt = String::new();
    let mut prof_courses: Vec<Vec<usize>> = vec![Vec::new(); n_profs];
    for (p, courses) in prof_courses.iter_mut().enumerate() {
        for c in 0..courses_per_prof {
            let course_id = p * courses_per_prof + c;
            nt.push_str(&format!(
                "{} {teacher_of} {} .\n",
                prof(p),
                course(course_id)
            ));
            courses.push(course_id);
        }
    }

    let mut ground_truth = 0usize;
    for s in 0..n_students {
        let advisor_prof = s % n_profs;
        nt.push_str(&format!(
            "{} {advisor} {} .\n",
            student(s),
            prof(advisor_prof)
        ));
        // Deterministic pseudo-random spread of taken courses over the
        // whole pool, not just the advisor's own courses, so most
        // (student, course) pairs do NOT close a triangle.
        for t in 0..courses_taken_per_student {
            let course_id = (s * 7 + t * 31 + 3) % total_courses;
            nt.push_str(&format!(
                "{} {takes_course} {} .\n",
                student(s),
                course(course_id)
            ));
            if prof_courses[advisor_prof].contains(&course_id) {
                ground_truth += 1;
            }
        }
    }
    (nt, ground_truth)
}

const LUBM_Q9_QUERY: &str = concat!(
    "PREFIX : <http://example.org/jrb#> ",
    "SELECT ?x ?y ?z WHERE { ?x :advisor ?y . ?y :teacherOf ?z . ?x :takesCourse ?z . }"
);

/// Realistic (non-adversarial) cyclic triangle query, modeled on LUBM's
/// canonical Q9 shape. Correctness check against an independently computed
/// ground truth, fast enough to run un-ignored. See
/// [`lubm_style_graph`]'s doc for why this is a meaningfully different data
/// point from the symmetric adversarial case above: `advisor` is
/// functional (one row per student) and `teacherOf` has small fixed
/// fan-out per professor, so even without Phase D's worst-case-optimal
/// join, a correctly-ordered binary join (Phase A) should see intermediate
/// row counts close to linear in the input size, not quadratic.
#[test]
fn lubm_style_triangle_query_realistic() {
    let (nt, expected) = lubm_style_graph(50, 4, 500, 3);
    assert!(
        expected > 0,
        "construction must actually produce some closed triangles, \
         otherwise this test would also pass if the executor returned no rows at all"
    );
    let datastore = load_graph(&nt);
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(LUBM_Q9_QUERY, &mut ctx).expect("LUBM Q9-style query must parse");
    let actual = match execute(&query, &datastore, NetworkPolicy::Deny)
        .expect("LUBM Q9-style query must execute")
    {
        QueryResult::Select(r) => r.rows.len(),
        _ => panic!("expected Select result"),
    };
    assert_eq!(
        actual, expected,
        "LUBM Q9-style triangle query result count must match independently computed ground truth"
    );
}

/// Diagnostic: same realistic LUBM-Q9-style query at a larger scale,
/// printing the per-pattern `rows_in`/`rows_out` profile — the direct
/// comparison point against part 2's adversarial sweep. `#[ignore]`d for
/// the same reason as `synthetic_triangle_scaling_sweep`. Run with:
/// ```bash
/// cargo test --test join_reorder_benchmark --release -- --ignored --nocapture lubm_style_triangle_profile
/// ```
#[test]
#[ignore = "diagnostic timing probe — run explicitly with --release --nocapture, see module doc"]
fn lubm_style_triangle_profile() {
    println!("\n=== Realistic cyclic triangle (LUBM Q9 shape) — profile ===");
    println!("(#507, precursor measurement for #39 / JOIN_REORDERING_PLAN.md Phase D)\n");

    let (nt, expected) = lubm_style_graph(500, 6, 20_000, 3);
    let datastore = load_graph(&nt);
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(LUBM_Q9_QUERY, &mut ctx).expect("LUBM Q9-style query must parse");

    let t = Instant::now();
    let (result, profile) =
        execute_with_profile(&query, &datastore, NetworkPolicy::Deny, None, None);
    let elapsed = t.elapsed();
    let rows = match result.expect("LUBM Q9-style query must execute") {
        QueryResult::Select(r) => r.rows.len(),
        _ => panic!("expected Select result"),
    };
    println!(
        "time={} ms  final_rows={rows}  expected={expected}",
        elapsed.as_millis()
    );
    print_pattern_profile(&profile, 0);
    assert_eq!(rows, expected);
}

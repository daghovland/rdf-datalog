/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! DBLP SPARQL benchmark correctness smoke test.
//!
//! Runs the same 105-query Sparqloscope/QLever DBLP benchmark suite used by
//! the `dblp_benchmark` diagnostic (`tests/dblp_benchmark.rs`), but against a
//! tiny, committed prefix of real DBLP data
//! (`tests/testdata/dblp-sample-small.nt`, ~10k N-Triples lines) instead of
//! the large, gitignored, downloaded 15M-triple sample.
//!
//! Unlike `dblp_benchmark.rs`, this is **not** `#[ignore]`d — it runs under
//! plain `cargo test` on every CI run. The goal is not performance numbers
//! or literal result correctness (the tiny sample won't produce the same
//! row/aggregate counts as the full dataset), but a cheap regression net
//! across the whole query suite: every query should at least *parse*, and
//! the large majority should *execute* without panicking or erroring, so a
//! parser/executor regression anywhere in the 105-query surface is caught
//! immediately rather than only when someone manually runs the big ignored
//! benchmark.
//!
//! Known limitation (tracked separately, not fixed here):
//! `docs/plans/DBLP_BENCHMARK_PLAN.md` lists several SPARQL builtins
//! (`STRBEFORE`, `STRAFTER`, `STRSTARTS`, `STRENDS`, `CONTAINS`, `ABS`,
//! `CEIL`, `FLOOR`, `ROUND`, `YEAR`, `MONTH`, `DAY`) as missing from
//! `sparql_parser::execute`. As observed against this fixture, most of those
//! now execute without erroring (their correctness isn't asserted here —
//! only that they don't crash). `STRSTARTS`/`STRENDS` themselves were never
//! missing from the grammar; the two benchmark queries that use them
//! (`strstarts`, `strends`) previously hard-failed to parse for an unrelated
//! reason — the `xsd:integer(...)` numeric-cast wrapper around them hit a
//! parser bug where prefixed-name function calls (`xsd:integer(...)`, or any
//! `prefix:localname(...)`) failed to parse at all, fixed in
//! [#186](https://github.com/daghovland/rdf-datalog/issues/186). Both
//! queries now parse and execute cleanly (see `KNOWN_PARSE_GAPS` below,
//! which is now empty). The assertions are still calibrated with loose
//! headroom on the overall OK count, since `xsd:integer(...)` itself is not
//! yet implemented as a value-casting function at execution time (it
//! evaluates to unbound rather than erroring).

use dag_rdf::Datastore;
use sparql_parser::{NetworkPolicy, ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;
use turtle::parse_ntriples;

// ── Helpers (duplicated from tests/dblp_benchmark.rs — integration test
//    binaries in this workspace each carry their own small helpers; see
//    CLAUDE.md / tests/performance.rs for the established convention) ───────

/// Path to a test data file relative to the workspace root.
fn test_data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

/// One row of `dblp.benchmark.tsv`: the `name [description]` label and the
/// full SPARQL query text.
struct BenchmarkQuery {
    name: String,
    query: String,
}

/// Parse `tests/testdata/dblp.benchmark.tsv` (2-column TSV: name, query) into
/// a list of benchmark queries.
fn parse_benchmark_tsv(path: &Path) -> Vec<BenchmarkQuery> {
    let text = std::fs::read_to_string(path).expect("benchmark TSV must be readable");
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, query) = line
                .split_once('\t')
                .expect("each benchmark line must contain a tab-separated name and query");
            BenchmarkQuery {
                name: name.to_string(),
                query: query.to_string(),
            }
        })
        .collect()
}

/// Outcome of running one benchmark query against the loaded sample.
enum QueryOutcome {
    Ok { elapsed_ms: u128, rows: usize },
    ParseFail,
    ExecFail,
}

/// Run a single benchmark query against `datastore`, classifying the result.
fn run_benchmark_query(datastore: &Datastore, query_str: &str) -> QueryOutcome {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let query = match parse_query(query_str, &mut ctx) {
        Ok((_, q)) => q,
        Err(_) => return QueryOutcome::ParseFail,
    };
    let t = Instant::now();
    match execute(&query, datastore, NetworkPolicy::Deny) {
        Ok(QueryResult::Select(r)) => QueryOutcome::Ok {
            elapsed_ms: t.elapsed().as_millis(),
            rows: r.rows.len(),
        },
        Ok(QueryResult::Ask(_) | QueryResult::Construct(_) | QueryResult::Describe(_)) => {
            QueryOutcome::ExecFail
        }
        Err(_) => QueryOutcome::ExecFail,
    }
}

// ── Test ─────────────────────────────────────────────────────────────────────

/// Runs all 105 Sparqloscope/QLever DBLP benchmark queries against a tiny,
/// committed real-DBLP fixture. Fast and low-memory by construction (the
/// fixture is ~10k N-Triples lines), so this runs by default under plain
/// `cargo test` rather than being `#[ignore]`d like `dblp_benchmark.rs`.
#[test]
fn dblp_correctness_smoke() {
    let data_path = test_data("dblp-sample-small.nt");
    assert!(
        data_path.exists(),
        "committed fixture missing: {} — this file should always be present \
         in the repository (unlike the large, gitignored dblp-sample.nt)",
        data_path.display()
    );
    let tsv_path = test_data("dblp.benchmark.tsv");
    assert!(
        tsv_path.exists(),
        "committed benchmark TSV missing: {}",
        tsv_path.display()
    );

    println!("\n=== DBLP correctness smoke test (small fixture) ===");
    let t0 = Instant::now();
    let mut datastore = Datastore::new(20_000);
    let file = File::open(&data_path).expect("test data file must be readable");
    parse_ntriples(&mut datastore, BufReader::new(file)).expect("N-Triples parse must succeed");
    println!(
        "  triples loaded: {} ({} ms)",
        datastore.named_graphs.quad_count,
        t0.elapsed().as_millis()
    );
    println!();
    assert!(
        datastore.named_graphs.quad_count > 5_000,
        "expected >5,000 triples from the small DBLP fixture, got {} — \
         N-Triples loader may be silently dropping lines",
        datastore.named_graphs.quad_count
    );

    let queries = parse_benchmark_tsv(&tsv_path);
    println!("  {} benchmark queries loaded", queries.len());
    println!();
    println!(
        "  {:<55} {:<10} {:>10}  {:>8}",
        "Query", "Status", "Time (ms)", "Rows"
    );
    println!("  {}", "-".repeat(90));

    let mut ok_count = 0usize;
    let mut parse_fail_count = 0usize;
    let mut exec_fail_count = 0usize;
    let mut total_ok_ms: u128 = 0;
    let mut non_ok_names: Vec<(&str, &str)> = Vec::new();

    for q in &queries {
        match run_benchmark_query(&datastore, &q.query) {
            QueryOutcome::Ok { elapsed_ms, rows } => {
                ok_count += 1;
                total_ok_ms += elapsed_ms;
                println!(
                    "  {:<55} {:<10} {:>10}  {:>8}",
                    q.name, "OK", elapsed_ms, rows
                );
            }
            QueryOutcome::ParseFail => {
                parse_fail_count += 1;
                non_ok_names.push((q.name.as_str(), "PARSEFAIL"));
                println!(
                    "  {:<55} {:<10} {:>10}  {:>8}",
                    q.name, "PARSEFAIL", "-", "-"
                );
            }
            QueryOutcome::ExecFail => {
                exec_fail_count += 1;
                non_ok_names.push((q.name.as_str(), "EXECFAIL"));
                println!(
                    "  {:<55} {:<10} {:>10}  {:>8}",
                    q.name, "EXECFAIL", "-", "-"
                );
            }
        }
    }

    println!();
    println!("  Summary:");
    println!("    total queries:   {}", queries.len());
    println!("    OK:              {}", ok_count);
    println!("    parse failures:  {}", parse_fail_count);
    println!("    exec failures:   {}", exec_fail_count);
    if ok_count > 0 {
        println!(
            "    avg time (OK):   {:.2} ms",
            total_ok_ms as f64 / ok_count as f64
        );
    }
    if !non_ok_names.is_empty() {
        println!();
        println!("  Non-OK queries (some are expected — see KNOWN_PARSE_GAPS / the known");
        println!("  missing-builtins gap in docs/plans/DBLP_BENCHMARK_PLAN.md; anything not");
        println!("  named there is a new regression):");
        for (name, status) in &non_ok_names {
            println!("    [{status}] {name}");
        }
    }
    println!();
    report("TOTAL", t0.elapsed().as_millis());

    // Parsing is independent of the missing-builtins *execution* gap and of
    // dataset size, so any parse failure here is a real grammar regression.
    // `STRSTARTS`/`STRENDS` used to be allowlisted here — their benchmark
    // queries hard-failed to parse because of the `xsd:integer(...)` wrapper
    // hitting the prefixed-name function-call parser bug fixed in #186, not
    // because of `STRSTARTS`/`STRENDS` themselves. Now that #186 is fixed,
    // both parse cleanly and the allowlist is empty; kept as a named const
    // (rather than removed outright) so a future real parser gap has an
    // obvious place to land without restructuring this assertion.
    const KNOWN_PARSE_GAPS: &[&str] = &[];
    let unexpected_parse_fails: Vec<&str> = non_ok_names
        .iter()
        .filter(|(_, status)| *status == "PARSEFAIL")
        .map(|(name, _)| *name)
        .filter(|name| !KNOWN_PARSE_GAPS.iter().any(|gap| name.starts_with(gap)))
        .collect();
    assert!(
        unexpected_parse_fails.is_empty(),
        "unexpected SPARQL parse failures (grammar regression): {:?} — only {:?} are \
         currently known to be unparseable (see docs/plans/DBLP_BENCHMARK_PLAN.md)",
        unexpected_parse_fails,
        KNOWN_PARSE_GAPS
    );

    // Execution headroom: this is a crash/regression smoke test, not a
    // correctness gate — some builtins (e.g. STRBEFORE, ABS, YEAR) execute
    // without erroring here but aren't asserted to return correct bindings.
    // The threshold below stays loose and well below the observed pass count
    // so that it tolerates the known gaps in docs/plans/DBLP_BENCHMARK_PLAN.md
    // without needing to be bumped every time one of them is fixed
    // independently of this test, while still catching a regression that
    // knocks out a meaningful chunk of currently-passing queries.
    const MIN_OK: usize = 85;
    assert!(
        ok_count >= MIN_OK,
        "expected at least {MIN_OK}/{} benchmark queries to execute successfully, got {} \
         (parse failures: {}, exec failures: {}) — see the non-OK list above; some exec \
         failures are expected from the known missing-builtins gap (see \
         docs/plans/DBLP_BENCHMARK_PLAN.md), but a count this low suggests a new regression",
        queries.len(),
        ok_count,
        parse_fail_count,
        exec_fail_count
    );
}

/// Print a labelled timing line.
fn report(label: &str, elapsed_ms: u128) {
    println!("  {:<40} {:>8} ms", label, elapsed_ms);
}

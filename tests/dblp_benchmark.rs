/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! DBLP SPARQL benchmark diagnostic.
//!
//! Runs the 105-query Sparqloscope/QLever DBLP benchmark suite
//! (`tests/testdata/dblp.benchmark.tsv`) against a bounded in-memory sample
//! of DBLP, to find missing SPARQL functionality and slow query shapes in
//! dagalog. This is diagnostic, not a literal number comparison against
//! QLever's published (full-dataset) results — see
//! `docs/plans/DBLP_BENCHMARK_PLAN.md` for the full plan and the known gaps
//! it is expected to surface.
//!
//! Requires the DBLP sample data. Download it first:
//! ```bash
//! bash scripts/download_test_ontologies.sh
//! ```
//!
//! Then run with:
//! ```bash
//! cargo test --test dblp_benchmark -- --ignored --nocapture
//! ```
//!
//! All tests in this file are marked `#[ignore]` so they are skipped by the
//! normal `cargo test` run.

use dag_rdf::Datastore;
use sparql_parser::{ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;
use turtle::parse_ntriples;

// ── Helpers (same conventions as tests/performance.rs) ──────────────────────

/// Path to a test data file relative to the workspace root.
fn test_data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

/// Return `true` when the test data file exists; `false` to skip.
fn ensure_test_data(path: &Path) -> bool {
    if path.exists() {
        true
    } else {
        eprintln!(
            "\n[SKIP] Test data not found: {}\n\
             Run `bash scripts/download_test_ontologies.sh` to download it.\n",
            path.display()
        );
        false
    }
}

/// Print a labelled timing line.
fn report(label: &str, elapsed_ms: u128) {
    println!("  {:<40} {:>8} ms", label, elapsed_ms);
}

// ── Benchmark TSV ────────────────────────────────────────────────────────────

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
    match execute(&query, datastore) {
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

// ── Tests ────────────────────────────────────────────────────────────────────

/// Parse-only benchmark for the DBLP N-Triples sample.
///
/// Mirrors `wikidata_parse_only` in `tests/performance.rs`.
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn dblp_load_sample() {
    let path = test_data("dblp-sample.nt");
    if !ensure_test_data(&path) {
        return;
    }
    println!("\n=== DBLP sample — parse only ===");
    let t0 = Instant::now();
    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(&path).expect("test data file must be readable");
    parse_ntriples(&mut datastore, BufReader::new(file)).expect("N-Triples parse must succeed");
    let elapsed = t0.elapsed();
    let triples = datastore.named_graphs.quad_count;
    println!("  triples: {}", triples);
    println!("  elapsed: {} ms", elapsed.as_millis());
    println!(
        "  throughput: {:.0} triples/sec",
        triples as f64 / elapsed.as_secs_f64()
    );
    assert!(
        triples > 1_000_000,
        "expected >1M triples from DBLP sample, got {}",
        triples
    );
}

/// Runs the 105-query Sparqloscope/QLever DBLP benchmark suite against the
/// DBLP sample and prints a per-query report (status, time, row count) plus
/// a summary. Expected to surface the known gaps listed in
/// `docs/plans/DBLP_BENCHMARK_PLAN.md` (missing STRBEFORE/STRAFTER/
/// STRSTARTS/STRENDS/CONTAINS/YEAR/MONTH/DAY/ABS/CEIL/FLOOR/ROUND builtins).
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn dblp_benchmark_queries() {
    let data_path = test_data("dblp-sample.nt");
    if !ensure_test_data(&data_path) {
        return;
    }
    let tsv_path = test_data("dblp.benchmark.tsv");
    if !ensure_test_data(&tsv_path) {
        return;
    }

    println!("\n=== DBLP — Sparqloscope/QLever benchmark suite ===");
    let t0 = Instant::now();
    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(&data_path).expect("test data file must be readable");
    parse_ntriples(&mut datastore, BufReader::new(file)).expect("N-Triples parse must succeed");
    report("N-Triples parse", t0.elapsed().as_millis());
    println!(
        "    triples loaded:        {}",
        datastore.named_graphs.quad_count
    );
    println!();

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
                println!(
                    "  {:<55} {:<10} {:>10}  {:>8}",
                    q.name, "PARSEFAIL", "-", "-"
                );
            }
            QueryOutcome::ExecFail => {
                exec_fail_count += 1;
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
            "    avg time (OK):   {:.1} ms",
            total_ok_ms as f64 / ok_count as f64
        );
    }
    report("TOTAL", t0.elapsed().as_millis());

    assert!(
        ok_count > 0,
        "expected at least some benchmark queries to succeed"
    );
}

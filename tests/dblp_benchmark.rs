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
//!
//! STUB — red phase. Helper bodies below are intentionally `unimplemented!()`
//! pending review; see `docs/plans/DBLP_BENCHMARK_PLAN.md` step 3/4.

use dag_rdf::Datastore;
use std::path::Path;
use std::time::Instant;

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

// ── Benchmark TSV ────────────────────────────────────────────────────────────

/// One row of `dblp.benchmark.tsv`: the `name [description]` label and the
/// full SPARQL query text.
struct BenchmarkQuery {
    name: String,
    query: String,
}

/// Parse `tests/testdata/dblp.benchmark.tsv` (2-column TSV: name, query) into
/// a list of benchmark queries.
fn parse_benchmark_tsv(_path: &Path) -> Vec<BenchmarkQuery> {
    unimplemented!("split each line on the first tab into (name, query)")
}

/// Outcome of running one benchmark query against the loaded sample.
enum QueryOutcome {
    Ok { elapsed_ms: u128, rows: usize },
    ParseFail,
    ExecFail,
}

/// Run a single benchmark query against `datastore`, classifying the result.
fn run_benchmark_query(_datastore: &Datastore, _query_str: &str) -> QueryOutcome {
    unimplemented!("parse_query, then execute; classify parse/exec failures vs success")
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
    let _t0 = Instant::now();
    unimplemented!("parse_ntriples into a Datastore, report triple count + throughput")
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
    let _queries = parse_benchmark_tsv(&tsv_path);
    unimplemented!(
        "load the sample once, run every benchmark query via run_benchmark_query, \
         print a report table + summary, loose sanity assertion (ok_count > 0)"
    )
}

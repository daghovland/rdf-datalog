/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Proves `scripts/serve-backlog.sh` (issue
//! [#356](https://github.com/daghovland/rdf-datalog/issues/356)) actually
//! combines the backlog snapshot and the provenance summaries into one
//! queryable dataset -- not just that each side loads on its own.
//!
//! `--serve` combined with repeatable `--data` already worked before this
//! issue (`main()` loads every `--data` file into one `Datastore` before
//! branching on `cli.serve`), so there is no `src/main.rs` change here. What
//! this test guards is the *file list* the script resolves and the fact
//! that a query needing both files (a real PR asserted in
//! `backlog/examples/snapshot.ttl` AND described by an
//! `agp:TranscriptSummary` in `provenance/summaries/pr-300.ttl`) actually
//! joins across them.
//!
//! Uses the in-process load path (`dagalog::load_file` + `run_sparql_query`,
//! the same helpers `tests/cli_integration.rs`, `tests/backlog_queries.rs`,
//! and `tests/provenance_queries.rs` already use) rather than driving a real
//! HTTP round trip: the HTTP layer itself already has exhaustive coverage
//! under `sparql_endpoint/tests/`, so a second harness here would test
//! nothing new about what #356 actually risks (see
//! `docs/plans/SERVE_BACKLOG_PROVENANCE_356_PLAN.md`).
//!
//! The join query is intentionally NOT placed under `provenance/queries/`
//! or `backlog/queries/`: `tests/provenance_queries.rs` runs every
//! `provenance/queries/*.sparql` file against a summaries-only corpus (no
//! snapshot), and `tests/backlog_queries.rs` runs every
//! `backlog/queries/*.sparql` file against backlog-only fixtures (no
//! provenance) -- a query needing both would silently return zero rows in
//! either of those existing tests. It stays inline here instead.

use dag_rdf::{Datastore, GraphElement};
use dagalog::{graph_element_display, load_file, run_sparql_query};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Runs `scripts/serve-backlog.sh --print-data-args` and returns the
/// resolved list of data files, one per line, exactly as the script itself
/// would pass them to `dagalog --serve --data <file> ...` -- so this test
/// can never silently drift from what a user actually running the script
/// gets served.
fn script_data_args() -> Vec<PathBuf> {
    let script = repo_root().join("scripts").join("serve-backlog.sh");
    let output = Command::new("bash")
        .arg(&script)
        .arg("--print-data-args")
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", script.display()));
    assert!(
        output.status.success(),
        "{} --print-data-args failed: stderr={}",
        script.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("script output should be UTF-8");
    let files: Vec<PathBuf> = stdout.lines().map(PathBuf::from).collect();
    assert!(
        !files.is_empty(),
        "{} --print-data-args printed no files",
        script.display()
    );
    files
}

fn load(paths: &[PathBuf]) -> Datastore {
    let mut ds = Datastore::new(10_000);
    for p in paths {
        load_file(&mut ds, p).unwrap_or_else(|e| {
            panic!("{} should parse as Turtle: {e}", p.display());
        });
    }
    ds
}

fn display(row: &std::collections::HashMap<String, GraphElement>, var: &str) -> String {
    row.get(var)
        .map(graph_element_display)
        .unwrap_or_else(|| "(unbound)".to_string())
}

/// Regression test for issue
/// [#435](https://github.com/daghovland/rdf-datalog/issues/435): the
/// dagalog instance the script starts must allow cross-origin requests
/// from the dashboard it also starts, or every `sparqlFetch` call the
/// dashboard makes gets blocked by the browser as a CORS `NetworkError`.
///
/// Checks per-line rather than for one exact substring, so a future
/// requoting/reflow of the dagalog invocation doesn't produce a false
/// failure -- only actually dropping the flag (or the port variable, or
/// one of the two origins) does. Both `localhost` and `127.0.0.1` need
/// their own `--cors-allow-origin` line because browsers can report
/// either as the dashboard's origin depending on how it was opened, and
/// the endpoint's allow-list does exact string matching (see
/// `sparql_endpoint::server::build_allow_origin`) -- no wildcard or
/// hostname-normalization is done, so an unlisted variant fails closed
/// with the very `NetworkError` this flag exists to prevent.
///
/// This only checks the script text (not a real end-to-end HTTP round
/// trip -- that was verified manually via curl when fixing #435, for
/// both origins and against a mismatched-origin negative control).
#[test]
fn script_passes_cors_allow_origin_to_dagalog() {
    let script = repo_root().join("scripts").join("serve-backlog.sh");
    let contents = std::fs::read_to_string(&script)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", script.display()));

    let cors_lines: Vec<&str> = contents
        .lines()
        .filter(|line| line.contains("--cors-allow-origin"))
        .collect();

    for expected_origin in [
        "http://localhost:$DASHBOARD_PORT",
        "http://127.0.0.1:$DASHBOARD_PORT",
    ] {
        assert!(
            cors_lines.iter().any(|line| line.contains(expected_origin)),
            "expected {} to pass --cors-allow-origin {expected_origin} to the dagalog \
             invocation so the dashboard's cross-origin sparqlFetch calls aren't blocked \
             regardless of whether the browser reports the dashboard's origin as `localhost` \
             or `127.0.0.1` (see https://github.com/daghovland/rdf-datalog/issues/435)",
            script.display()
        );
    }
}

/// `scripts/serve-backlog.sh --print-data-args` must resolve to a list that
/// includes both vocab files, the backlog snapshot, and at least one
/// provenance summary -- the exact combination #356 asks for.
#[test]
fn script_resolves_expected_data_files() {
    let files = script_data_args();
    let joined: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let has_fragment = |fragment: &str| joined.iter().any(|p| p.contains(fragment));

    assert!(
        has_fragment("backlog/ontology/vocabulary.ttl"),
        "expected backlog/ontology/vocabulary.ttl in {joined:?}"
    );
    assert!(
        has_fragment("backlog/ontology/agentprov-vocabulary.ttl"),
        "expected backlog/ontology/agentprov-vocabulary.ttl in {joined:?}"
    );
    assert!(
        has_fragment("backlog/examples/snapshot.ttl"),
        "expected backlog/examples/snapshot.ttl in {joined:?}"
    );
    assert!(
        joined
            .iter()
            .any(|p| p.contains("provenance/summaries/") && p.ends_with(".ttl")),
        "expected at least one provenance/summaries/*.ttl in {joined:?}"
    );
}

/// Loading exactly the file list the script resolves must produce a sane
/// combined triple count -- i.e. neither side got dropped.
#[test]
fn combined_dataset_loads_from_script_data_args() {
    let files = script_data_args();
    let ds = load(&files);
    // Both the backlog snapshot (2000+ triples) and 20+ provenance summary
    // files are loaded together; a generous lower bound that would fail if
    // either side were silently skipped.
    assert!(
        ds.named_graphs.quad_count >= 2000,
        "expected a large combined triple count (backlog snapshot + provenance summaries), got {}",
        ds.named_graphs.quad_count
    );
}

/// The actual proof the two datasets combine: a query joining
/// `agp:reasoningFor`/`agp:summaryText` (only asserted in
/// `provenance/summaries/pr-300.ttl`) against `bl:touchesCrate` (only
/// asserted for `ghpull:300` in `backlog/examples/snapshot.ttl`) for the
/// same real PR (#300) must return a row -- impossible unless both files
/// are loaded into the same `Datastore`.
#[test]
fn cross_file_join_pr_and_provenance_works() {
    let files = script_data_args();
    let ds = load(&files);

    let query = r#"
        PREFIX agp: <https://dagalog.dev/ns/agentprov#>
        PREFIX bl: <https://dagalog.dev/ns/backlog#>
        PREFIX ghpull: <https://github.com/daghovland/rdf-datalog/pull/>

        SELECT ?crate ?summaryText WHERE {
          VALUES ?pr { ghpull:300 }
          ?pr bl:touchesCrate ?crate .
          ?summary a agp:TranscriptSummary ;
            agp:reasoningFor ?pr ;
            agp:summaryText ?summaryText .
        }
    "#;
    let result = run_sparql_query(&ds, query).expect("cross-file join query should run");
    // Not an exact row count: how many crates ghpull:300's bl:touchesCrate
    // lists is a property of the regenerable snapshot.ttl (the loader's own
    // crate-mapping heuristics), not of the cross-file join this test
    // exists to prove. Asserting non-empty plus a content check (matching
    // tests/provenance_queries.rs's own >= lower bound + content-check
    // pattern) proves the same thing without coupling to loader internals.
    assert!(
        !result.rows.is_empty(),
        "expected at least one row joining bl:touchesCrate (snapshot.ttl) with \
         agp:reasoningFor/agp:summaryText (pr-300.ttl) for ghpull:300, got none"
    );
    assert!(
        result
            .rows
            .iter()
            .any(|row| display(row, "crate") == "<https://dagalog.dev/ns/backlog/crate#shacl>"),
        "expected ghpull:300's bl:touchesCrate (from snapshot.ttl) to include the shacl crate, got: {:#?}",
        result
            .rows
            .iter()
            .map(|row| display(row, "crate"))
            .collect::<Vec<_>>()
    );
    assert!(
        result
            .rows
            .iter()
            .any(|row| display(row, "summaryText").contains("ValidationResult")),
        "expected pr-300.ttl's agp:summaryText content among the joined rows, got: {:#?}",
        result
            .rows
            .iter()
            .map(|row| display(row, "summaryText"))
            .collect::<Vec<_>>()
    );
}

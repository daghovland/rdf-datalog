/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Proves `scripts/regenerate-production-dataset.sh` (issue
//! [#565](https://github.com/daghovland/rdf-datalog/issues/565)) actually
//! produces a single loadable Turtle file combining the backlog ontology,
//! the backlog snapshot, and the provenance summaries -- the same shape
//! this session found already hand-built (once, and now stale) in this
//! host's own `data/dataset.ttl`. See
//! [`docs/plans/WIRE_BACKLOG_SNAPSHOT_565_PLAN.md`](../docs/plans/WIRE_BACKLOG_SNAPSHOT_565_PLAN.md).
//!
//! Runs with `--skip-regenerate --no-restart` so it needs neither `gh`
//! CLI/network access nor Docker -- it reuses the checked-in
//! `backlog/examples/snapshot.ttl` as the "regenerated" input and never
//! touches `docker compose`, exactly like CI would need.

use dag_rdf::{Datastore, GraphElement};
use dagalog::{graph_element_display, load_file, run_sparql_query};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Runs the script against a temp output path and returns that path (the
/// merged file), asserting the script itself succeeded.
fn run_script(out: &Path) {
    let script = repo_root()
        .join("scripts")
        .join("regenerate-production-dataset.sh");
    let output = Command::new("bash")
        .arg(&script)
        .arg("--repo-root")
        .arg(repo_root())
        .arg("--out")
        .arg(out)
        .arg("--skip-regenerate")
        .arg("--no-restart")
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", script.display()));
    assert!(
        output.status.success(),
        "{} failed: stdout={} stderr={}",
        script.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn load(path: &Path) -> Datastore {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, path)
        .unwrap_or_else(|e| panic!("{} should parse as Turtle: {e}", path.display()));
    ds
}

fn display(row: &std::collections::HashMap<String, GraphElement>, var: &str) -> String {
    row.get(var)
        .map(graph_element_display)
        .unwrap_or_else(|| "(unbound)".to_string())
}

/// The merged file must load as one Turtle document and contain both a
/// `bl:Issue` individual and an `agp:AgentSession`/`agp:summaryText`
/// individual -- i.e. all three sources (ontology + snapshot + provenance
/// summaries) actually landed in it, not just the ontology or just the
/// snapshot.
#[test]
fn merged_dataset_contains_backlog_and_provenance_data() {
    let tmp = tempfile_path("regen-prod-dataset-test.ttl");
    run_script(&tmp);

    let ds = load(&tmp);
    assert!(
        ds.named_graphs.quad_count >= 2000,
        "expected a large combined triple count (backlog snapshot + provenance summaries), got {}",
        ds.named_graphs.quad_count
    );

    let issue_query = r#"
        PREFIX bl: <https://dagalog.no/ns/backlog#>
        SELECT ?issue WHERE { ?issue a bl:Issue . } LIMIT 1
    "#;
    let result = run_sparql_query(&ds, issue_query).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        !result.rows.is_empty(),
        "expected at least one bl:Issue in the merged dataset"
    );
    assert_ne!(display(&result.rows[0], "issue"), "(unbound)");

    let session_query = r#"
        PREFIX agp: <https://dagalog.no/ns/agentprov#>
        SELECT ?session WHERE { ?session a agp:AgentSession . } LIMIT 1
    "#;
    let result = run_sparql_query(&ds, session_query).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        !result.rows.is_empty(),
        "expected at least one agp:AgentSession in the merged dataset"
    );

    // agp:summaryText's rdfs:domain is agp:TranscriptSummary (deliberately
    // not agp:AgentSession -- see agentprov-vocabulary.ttl's own comment
    // on why it has no single-class domain declared), so a real provenance
    // summary is a TranscriptSummary/Decision with summaryText, generated
    // by (prov:wasGeneratedBy) an AgentSession -- checked directly rather
    // than assuming the two attach to the same resource.
    let summary_query = r#"
        PREFIX agp: <https://dagalog.no/ns/agentprov#>
        SELECT ?summary ?text WHERE {
            ?summary a agp:TranscriptSummary .
            ?summary agp:summaryText ?text .
        } LIMIT 1
    "#;
    let result = run_sparql_query(&ds, summary_query).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        !result.rows.is_empty(),
        "expected at least one agp:TranscriptSummary with agp:summaryText in the merged dataset"
    );

    let _ = std::fs::remove_file(&tmp);
}

/// A failing/missing snapshot source must not silently produce a partial
/// merged file, and re-running after a failure must not corrupt an
/// existing `--out` file (this test only proves the second half: a
/// pre-existing `--out` file is fully replaced, atomically, by a
/// successful run -- not left as a half-written artifact from a prior
/// attempt).
#[test]
fn rerunning_replaces_output_atomically() {
    let tmp = tempfile_path("regen-prod-dataset-rerun-test.ttl");
    std::fs::write(
        &tmp,
        "this is not valid turtle and should be fully replaced",
    )
    .expect("failed to seed placeholder file");

    run_script(&tmp);

    let ds = load(&tmp);
    assert!(
        ds.named_graphs.quad_count > 0,
        "expected the placeholder content to be fully replaced by real triples"
    );

    let _ = std::fs::remove_file(&tmp);
}

fn tempfile_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("{}-{}", std::process::id(), name));
    p
}

fn run_print_data_files(script_name: &str, args: &[&str]) -> Vec<PathBuf> {
    let script = repo_root().join("scripts").join(script_name);
    let output = Command::new("bash")
        .arg(&script)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", script.display()));
    assert!(
        output.status.success(),
        "{} failed: stderr={}",
        script.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("script output should be UTF-8")
        .lines()
        .map(PathBuf::from)
        .collect()
}

/// `scripts/regenerate-production-dataset.sh --print-sources` and
/// `scripts/serve-backlog.sh --print-data-args` intentionally resolve
/// separate, hand-maintained file lists (see the plan doc, "Why not share
/// code with serve-backlog.sh") -- this test is what keeps them from
/// silently drifting apart instead of a shared implementation. Compares
/// as sets: `serve-backlog.sh` also picks up `backlog/ontology/shapes.ttl`
/// only if present (it isn't, today) and file ORDER differs between the
/// two scripts' own internal logic, neither of which should fail this
/// check.
#[test]
fn production_and_dev_scripts_resolve_the_same_source_files() {
    let prod_files = run_print_data_files(
        "regenerate-production-dataset.sh",
        &[
            "--repo-root",
            repo_root().to_str().unwrap(),
            "--print-sources",
        ],
    );
    let dev_files = run_print_data_files("serve-backlog.sh", &["--print-data-args"]);

    let mut prod_set: Vec<String> = prod_files.iter().map(|p| p.display().to_string()).collect();
    let mut dev_set: Vec<String> = dev_files.iter().map(|p| p.display().to_string()).collect();
    prod_set.sort();
    dev_set.sort();

    assert_eq!(
        prod_set, dev_set,
        "regenerate-production-dataset.sh --print-sources and serve-backlog.sh \
         --print-data-args resolved different file sets -- keep them in sync \
         (or document a deliberate difference here)"
    );
}

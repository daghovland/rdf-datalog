/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Test harness for the `provenance/` transcript-summary fixtures and their
//! SPARQL query library ([#327](https://github.com/daghovland/rdf-datalog/issues/327),
//! a sub-issue of the agent-provenance epic
//! [#306](https://github.com/daghovland/rdf-datalog/issues/306), depending
//! on [#326](https://github.com/daghovland/rdf-datalog/issues/326)), and
//! generalized for [#334](https://github.com/daghovland/rdf-datalog/issues/334)
//! so that a new `provenance/summaries/pr-<N>.ttl` file an agent writes
//! under [`docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md`](../docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md)'s
//! convention is picked up automatically -- no test code change required.
//!
//! `all_summary_files()` globs every `provenance/summaries/*.ttl` file at
//! test time instead of the previous hardcoded single-file constant. Two
//! consequences of that generalization, deliberately handled differently:
//!
//! - **Per-file SHACL conformance** (`every_summary_file_conforms_to_shapes`)
//!   validates EACH file in its own freshly-loaded `Datastore` (vocab +
//!   shapes + that one file only), not one merged graph. Merging first
//!   would let a typo'd `ghpull:NNN` reference in file B be silently
//!   "resolved" by a stub some other file A happens to declare, and would
//!   let two files' independent `ghpull:N` stubs collide/duplicate against
//!   `bl:WorkItemRequiredFieldsShape`'s cardinality checks. Per-file
//!   validation is both stricter (catches a file that isn't
//!   self-contained) and matches the actual authoring workflow (one agent
//!   writes one file per PR, in isolation).
//! - **The SPARQL query tests** load the merged set of all summary files
//!   (matching how a real deployment would query across the whole
//!   `provenance/` corpus, and how `provenance/queries/run.sh` already
//!   behaves by globbing `*.ttl`). Their assertions were relaxed from
//!   `assert_eq!` (exact counts, valid only while exactly one fixture
//!   existed) to `>=` lower bounds plus a content check that the original
//!   PR #300 fixture's specific reasoning is still present -- so adding a
//!   new summary file (like `pr-328.ttl`, added by this same issue as the
//!   second worked example) doesn't turn these tests red.
//!   `reasoning_for_pr.sparql` is the one exception: it's parameterized to
//!   PR #300 specifically, so it still returns exactly one row regardless
//!   of how many other summary files are loaded alongside it.

use dag_rdf::{Datastore, GraphElement};
use dagalog::{graph_element_display, load_file, run_sparql_query};
use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn load(paths: &[&Path]) -> Datastore {
    let mut ds = Datastore::new(10_000);
    for p in paths {
        load_file(&mut ds, p).unwrap_or_else(|e| {
            panic!("{} should parse as Turtle: {e}", p.display());
        });
    }
    ds
}

fn query_file(name: &str) -> String {
    std::fs::read_to_string(repo_path(&format!("provenance/queries/{name}.sparql")))
        .unwrap_or_else(|e| panic!("provenance/queries/{name}.sparql should be readable: {e}"))
}

/// Every `provenance/summaries/*.ttl` file, discovered by globbing the
/// directory rather than naming files individually -- see this module's
/// doc comment. Sorted for deterministic test output/ordering.
fn all_summary_files() -> Vec<PathBuf> {
    let dir = repo_path("provenance/summaries");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} should be readable: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "ttl"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "expected at least one provenance/summaries/*.ttl file"
    );
    files
}

const BL_VOCAB: &str = "backlog/ontology/vocabulary.ttl";
const BL_SHAPES: &str = "backlog/ontology/shapes.ttl";
const AGP_VOCAB: &str = "backlog/ontology/agentprov-vocabulary.ttl";
const AGP_SHAPES: &str = "backlog/ontology/agentprov-shapes.ttl";

fn display(row: &std::collections::HashMap<String, GraphElement>, var: &str) -> String {
    row.get(var)
        .map(graph_element_display)
        .unwrap_or_else(|| "(unbound)".to_string())
}

/// Every `provenance/summaries/*.ttl` file must parse cleanly as Turtle.
/// Never ignored, so a syntax regression in any (including a future
/// agent's new file) is caught by CI immediately.
#[test]
fn all_summary_files_parse() {
    for f in all_summary_files() {
        load(&[&repo_path(BL_VOCAB), &repo_path(AGP_VOCAB), &f]);
    }
}

/// Each `provenance/summaries/*.ttl` file, loaded on its OWN (not merged
/// with any other summary file -- see this module's doc comment), must
/// conform to both `agentprov-shapes.ttl` and the relevant `bl:` shapes in
/// `backlog/ontology/shapes.ttl`. This is the guard #334 asked for: a
/// future agent's malformed or non-self-contained summary fails this test
/// (and CI) rather than silently entering the graph.
#[test]
fn every_summary_file_conforms_to_shapes() {
    let shapes = load(&[&repo_path(BL_SHAPES), &repo_path(AGP_SHAPES)]);
    for f in all_summary_files() {
        let data = load(&[&repo_path(BL_VOCAB), &repo_path(AGP_VOCAB), &f]);
        let report = shacl::validate(&data, &shapes).expect("validation must not error");
        assert!(
            report.conforms,
            "{} must conform to bl:/agp: shapes on its own, got violations: {:#?}",
            f.display(),
            report.results
        );
    }
}

fn load_all_summaries() -> Datastore {
    let mut paths = vec![repo_path(BL_VOCAB), repo_path(AGP_VOCAB)];
    paths.extend(all_summary_files());
    let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    load(&refs)
}

/// Every query file must at least parse and run without error against the
/// full merged corpus of summary files. Never ignored, so a syntax
/// regression is caught by CI.
#[test]
fn all_queries_run_without_error() {
    let ds = load_all_summaries();
    for name in [
        "reasoning_for_pr",
        "reasoned_about_by_agent",
        "sessions_for_issue",
        "all_decision_points",
    ] {
        run_sparql_query(&ds, &query_file(name))
            .unwrap_or_else(|e| panic!("queries/{name}.sparql must run without error: {e}"));
    }
}

/// "Why was PR #N merged?" -- `reasoning_for_pr.sparql` (parameterized to
/// PR #300 in the shipped file) must return exactly the real distilled
/// summary text for PR #300, regardless of how many other summary files
/// are also loaded (it's parameterized to that one PR).
#[test]
fn reasoning_for_pr_returns_real_summary_text() {
    let ds = load_all_summaries();
    let result = run_sparql_query(&ds, &query_file("reasoning_for_pr")).unwrap();
    assert_eq!(
        result.rows.len(),
        1,
        "expected exactly one summary for PR #300"
    );
    let text = display(&result.rows[0], "summaryText");
    assert!(
        text.contains("sourceShape") || text.contains("qualified"),
        "expected the real PR #300 reasoning (sourceShape resolution / qualified min-max split), got: {text}"
    );
}

/// "What has agent X reasoned about?" -- `reasoned_about_by_agent.sparql`
/// (parameterized to "Claude Sonnet 5" in the shipped file) must find at
/// least the PR #300 summary; it will find every other summary file also
/// attributed to that agent (e.g. `pr-328.ttl`), so this only asserts a
/// lower bound plus that PR #300's text is among the results.
#[test]
fn reasoned_about_by_agent_finds_pr_300() {
    let ds = load_all_summaries();
    let result = run_sparql_query(&ds, &query_file("reasoned_about_by_agent")).unwrap();
    assert!(
        !result.rows.is_empty(),
        "expected at least one summary attributed to the agent"
    );
    let found_pr_300 = result
        .rows
        .iter()
        .any(|r| display(r, "summaryText").contains("sourceShape"));
    assert!(
        found_pr_300,
        "expected the PR #300 summary among the results attributed to the agent"
    );
}

/// "Which sessions worked on issue #N?" -- `sessions_for_issue.sparql`
/// (parameterized to #264, the issue PR #300 closed) must return the
/// session that used it.
#[test]
fn sessions_for_issue_finds_the_session() {
    let ds = load_all_summaries();
    let result = run_sparql_query(&ds, &query_file("sessions_for_issue")).unwrap();
    assert!(
        !result.rows.is_empty(),
        "expected at least one session that used issue #264"
    );
}

/// "All decision points across the backlog" -- `all_decision_points.sparql`
/// must flatten AT LEAST the two real decision points recorded for PR #300
/// (sourceShape resolution, qualified min/max split); other summary files
/// may or may not add their own decision points on top.
#[test]
fn all_decision_points_finds_pr_300_decisions() {
    let ds = load_all_summaries();
    let result = run_sparql_query(&ds, &query_file("all_decision_points")).unwrap();
    assert!(
        result.rows.len() >= 2,
        "expected at least the two decision points recorded for PR #300, got: {:?}",
        result
            .rows
            .iter()
            .map(|r| display(r, "summaryText"))
            .collect::<Vec<_>>()
    );
}

/// A deliberately malformed summary -- missing `agp:reasoningFor`, and its
/// `agp:summaryText` a degenerate one-line non-summary well under the
/// `sh:minLength` bound `agentprov-shapes.ttl` enforces (#334) -- must fail
/// SHACL validation. Inlined here rather than placed under
/// `provenance/summaries/` (per this module's own glob-based loader and the
/// CI check exercising the same shapes: a malformed fixture living in that
/// directory would be picked up and fail CI for the wrong reason).
#[test]
fn malformed_summary_fails_shacl_validation() {
    let mut data = load(&[&repo_path(BL_VOCAB), &repo_path(AGP_VOCAB)]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix agp: <https://dagalog.dev/ns/agentprov#> .
        @prefix prov: <http://www.w3.org/ns/prov#> .
        @prefix ex: <http://example.com/ns#> .
        ex:agent a prov:SoftwareAgent .
        ex:session a agp:AgentSession ; prov:wasAssociatedWith ex:agent .
        ex:malformed a agp:TranscriptSummary ;
            agp:summaryText "Fixed it." ;
            prov:wasAttributedTo ex:agent ;
            prov:wasGeneratedBy ex:session .
        "#
        .as_bytes(),
    )
    .expect("malformed scratch fixture should parse as Turtle");
    let shapes = load(&[&repo_path(BL_SHAPES), &repo_path(AGP_SHAPES)]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "a summary missing agp:reasoningFor with a too-short agp:summaryText must fail validation"
    );
}

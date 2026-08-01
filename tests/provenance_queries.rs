/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Test harness for the `provenance/` worked grounding example and its
//! SPARQL query library ([#327](https://github.com/daghovland/rdf-datalog/issues/327),
//! a sub-issue of the agent-provenance epic
//! [#306](https://github.com/daghovland/rdf-datalog/issues/306), depending
//! on [#326](https://github.com/daghovland/rdf-datalog/issues/326)).
//!
//! Mirrors `tests/backlog_queries.rs`'s pattern: loads the actual
//! `provenance/summaries/*.ttl` fixtures and `provenance/queries/*.sparql`
//! files (not copies), validates the fixture against
//! `backlog/ontology/agentprov-shapes.ttl` (and the relevant `bl:` shapes
//! for the referenced `bl:PullRequest`), and runs the canned queries against
//! it. See
//! [`docs/plans/AGENT_PROVENANCE_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/AGENT_PROVENANCE_PLAN.md)
//! for the full design.
//!
//! Unlike `tests/agentprov_ontology.rs`'s toy fixture, `pr-300.ttl` here is
//! REAL data: it distills the actual reasoning from PR #300
//! (<https://github.com/daghovland/rdf-datalog/pull/300>), which fixed
//! `shacl::collect_violations` to populate `ValidationResult` detail fields
//! and, after review, corrected `sourceShape` resolution and split
//! `sh:qualifiedMinCount`/`sh:qualifiedMaxCount` into independently-reported
//! constraint components.

use dag_rdf::{Datastore, GraphElement};
use dagalog::{graph_element_display, load_file, run_sparql_query};
use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn load(paths: &[&str]) -> Datastore {
    let mut ds = Datastore::new(10_000);
    for p in paths {
        load_file(&mut ds, &repo_path(p)).unwrap_or_else(|e| {
            panic!("{p} should parse as Turtle: {e}");
        });
    }
    ds
}

fn query_file(name: &str) -> String {
    std::fs::read_to_string(repo_path(&format!("provenance/queries/{name}.sparql")))
        .unwrap_or_else(|e| panic!("provenance/queries/{name}.sparql should be readable: {e}"))
}

const BL_VOCAB: &str = "backlog/ontology/vocabulary.ttl";
const BL_SHAPES: &str = "backlog/ontology/shapes.ttl";
const AGP_VOCAB: &str = "backlog/ontology/agentprov-vocabulary.ttl";
const AGP_SHAPES: &str = "backlog/ontology/agentprov-shapes.ttl";
const PR_300: &str = "provenance/summaries/pr-300.ttl";

fn display(row: &std::collections::HashMap<String, GraphElement>, var: &str) -> String {
    row.get(var)
        .map(graph_element_display)
        .unwrap_or_else(|| "(unbound)".to_string())
}

/// `pr-300.ttl` must parse cleanly as Turtle. Never ignored, so a syntax
/// regression is caught by CI immediately.
#[test]
fn pr_300_summary_parses() {
    load(&[BL_VOCAB, AGP_VOCAB, PR_300]);
}

/// `pr-300.ttl`'s `agp:TranscriptSummary` (and its `agp:AgentSession`,
/// `agp:Decision` entries, and the `bl:PullRequest` it references) must
/// conform to both `agentprov-shapes.ttl` and the relevant `bl:` shapes in
/// `backlog/ontology/shapes.ttl` (in particular the literal
/// `bl:WorkItem`-type requirement and the `bl:WorkItemRequiredFieldsShape`
/// baseline, since `pr-300.ttl` asserts a real `bl:PullRequest`).
#[test]
fn pr_300_summary_conforms_to_shapes() {
    let data = load(&[BL_VOCAB, AGP_VOCAB, PR_300]);
    let shapes = load(&[BL_SHAPES, AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        report.conforms,
        "provenance/summaries/pr-300.ttl must conform to bl:/agp: shapes, got violations: {:#?}",
        report.results
    );
}

/// Every query file must at least parse and run without error against the
/// real fixture. Never ignored, so a syntax regression is caught by CI.
#[test]
fn all_queries_run_without_error() {
    let ds = load(&[BL_VOCAB, AGP_VOCAB, PR_300]);
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
/// PR #300 in the shipped file) must return the real distilled summary
/// text, not a placeholder.
#[test]
fn reasoning_for_pr_returns_real_summary_text() {
    let ds = load(&[BL_VOCAB, AGP_VOCAB, PR_300]);
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
/// (parameterized to "Claude Sonnet 5" in the shipped file) must find the
/// PR #300 summary.
#[test]
fn reasoned_about_by_agent_finds_pr_300() {
    let ds = load(&[BL_VOCAB, AGP_VOCAB, PR_300]);
    let result = run_sparql_query(&ds, &query_file("reasoned_about_by_agent")).unwrap();
    assert_eq!(
        result.rows.len(),
        1,
        "expected exactly one summary attributed to the agent"
    );
}

/// "Which sessions worked on issue #N?" -- `sessions_for_issue.sparql`
/// (parameterized to #264, the issue PR #300 closed) must return the
/// session that used it.
#[test]
fn sessions_for_issue_finds_the_session() {
    let ds = load(&[BL_VOCAB, AGP_VOCAB, PR_300]);
    let result = run_sparql_query(&ds, &query_file("sessions_for_issue")).unwrap();
    assert!(
        !result.rows.is_empty(),
        "expected at least one session that used issue #264"
    );
}

/// "All decision points across the backlog" -- `all_decision_points.sparql`
/// must flatten the two real decision points recorded for PR #300
/// (sourceShape resolution, qualified min/max split).
#[test]
fn all_decision_points_finds_pr_300_decisions() {
    let ds = load(&[BL_VOCAB, AGP_VOCAB, PR_300]);
    let result = run_sparql_query(&ds, &query_file("all_decision_points")).unwrap();
    assert_eq!(
        result.rows.len(),
        2,
        "expected exactly two decision points recorded for PR #300, got: {:?}",
        result
            .rows
            .iter()
            .map(|r| display(r, "summaryText"))
            .collect::<Vec<_>>()
    );
}

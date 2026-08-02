/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Materializes fetched GitHub issues/PRs and workspace crates as `bl:`
//! vocabulary quads in a [`Datastore`]. One-shot regeneration only -- see
//! [`docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`](../../docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md)
//! for the full field-mapping table and deliberate scope narrowing.

use crate::crates::CrateInfo;
use crate::github::{GitHubError, GitHubSource};
use crate::model::RawIssue;
use dag_rdf::{Datastore, GraphElementId};
use ingress::{IriReference, RDF_TYPE, RDFS, RdfLiteral, RdfResource};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const BL: &str = "https://dagalog.dev/ns/backlog#";
pub const CRATE_NS: &str = "https://dagalog.dev/ns/backlog/crate#";

fn bl(local: &str) -> String {
    format!("{BL}{local}")
}

/// Labels that already have a named individual in `vocabulary.ttl` --
/// anything else gets minted fresh in the snapshot (`a bl:Label ;
/// rdfs:label "<raw name>"`).
fn known_label_individual(raw_name: &str) -> Option<&'static str> {
    match raw_name {
        "bug" => Some("Bug"),
        "enhancement" => Some("Enhancement"),
        "ready" => Some("Ready"),
        _ => None,
    }
}

/// IRI-safe local name for an arbitrary GitHub label string not already a
/// named individual, e.g. `"good first issue"` -> `"Label_good_first_issue"`.
fn label_local_name(raw_name: &str) -> String {
    let sanitized: String = raw_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("Label_{sanitized}")
}

fn closing_keyword_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s*#(\d+)").unwrap()
    })
}

fn bare_mention_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"#(\d+)").unwrap())
}

/// `#N` issue/PR numbers a PR body formally closes (via a `Closes #N`-style
/// GitHub keyword reference).
fn extract_closes(body: &str) -> HashSet<u64> {
    closing_keyword_regex()
        .captures_iter(body)
        .filter_map(|c| c.get(2)?.as_str().parse().ok())
        .collect()
}

/// Every `#N` mentioned anywhere in the body, closing keyword or not.
fn extract_mentions(body: &str) -> HashSet<u64> {
    bare_mention_regex()
        .captures_iter(body)
        .filter_map(|c| c.get(1)?.as_str().parse().ok())
        .collect()
}

/// Interns a full IRI as a `GraphElementId`.
fn iri(ds: &mut Datastore, full_iri: &str) -> GraphElementId {
    ds.add_node_resource(RdfResource::Iri(IriReference(full_iri.to_string())))
}

fn add_type(ds: &mut Datastore, subject: GraphElementId, class_local: &str) {
    let p = iri(ds, RDF_TYPE);
    let o = iri(ds, &bl(class_local));
    ds.add_triple(dag_rdf::Triple {
        subject,
        predicate: p,
        obj: o,
    });
}

fn add_label(ds: &mut Datastore, subject: GraphElementId, text: &str) {
    let p = iri(ds, &format!("{RDFS}label"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString(text.to_string()));
    ds.add_triple(dag_rdf::Triple {
        subject,
        predicate: p,
        obj: o,
    });
}

fn add_object(
    ds: &mut Datastore,
    subject: GraphElementId,
    predicate_local: &str,
    object: GraphElementId,
) {
    let p = iri(ds, &bl(predicate_local));
    ds.add_triple(dag_rdf::Triple {
        subject,
        predicate: p,
        obj: object,
    });
}

fn add_integer(ds: &mut Datastore, subject: GraphElementId, predicate_local: &str, value: i64) {
    let p = iri(ds, &bl(predicate_local));
    let o = ds.add_literal_resource(RdfLiteral::IntegerLiteral(value.into()));
    ds.add_triple(dag_rdf::Triple {
        subject,
        predicate: p,
        obj: o,
    });
}

fn add_string(ds: &mut Datastore, subject: GraphElementId, predicate_local: &str, value: &str) {
    let p = iri(ds, &bl(predicate_local));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString(value.to_string()));
    ds.add_triple(dag_rdf::Triple {
        subject,
        predicate: p,
        obj: o,
    });
}

/// Ensures a `bl:Label` resource exists for `raw_name`, minting it in the
/// snapshot (with `rdfs:label`) if it isn't one of the vocabulary's own
/// named individuals (`bl:Bug`/`bl:Enhancement`/`bl:Ready`).
fn ensure_label(ds: &mut Datastore, raw_name: &str) -> GraphElementId {
    if let Some(known) = known_label_individual(raw_name) {
        return iri(ds, &bl(known));
    }
    let local = label_local_name(raw_name);
    let id = iri(ds, &bl(&local));
    add_type(ds, id, "Label");
    add_label(ds, id, raw_name);
    id
}

/// Fetches issues/PRs (and, for PRs, changed files) from `source`, discovers
/// workspace crates from `workspace_root`'s `Cargo.toml` files, and
/// materializes the whole thing into a fresh [`Datastore`].
pub fn build_snapshot(
    source: &dyn GitHubSource,
    workspace_root: &std::path::Path,
) -> Result<Datastore, GitHubError> {
    let issues = source.list_issues()?;
    let crates = crate::crates::discover_crates(workspace_root);
    let mut ds = Datastore::new(10_000);
    load_crates(&mut ds, &crates);
    load_issues(&mut ds, source, &issues, &crates)?;
    Ok(ds)
}

fn load_crates(ds: &mut Datastore, crates: &[CrateInfo]) {
    for c in crates {
        let subj = iri(ds, &format!("{CRATE_NS}{}", c.local_name));
        add_type(ds, subj, "Crate");
        add_label(ds, subj, &c.package_name);
        add_string(ds, subj, "path", &c.path);
        for dep in &c.path_deps {
            let dep_id = iri(ds, &format!("{CRATE_NS}{dep}"));
            add_object(ds, subj, "dependsOnCrate", dep_id);
        }
    }
}

/// Loads issues/PRs. Public (not just `pub(crate)`) so tests/tools can feed
/// a `Datastore` they already built crates into, without re-discovering crates.
/// `crates` is used to resolve `bl:touchesCrate` from each PR's changed files.
pub fn load_issues(
    ds: &mut Datastore,
    source: &dyn GitHubSource,
    issues: &[RawIssue],
    crates: &[CrateInfo],
) -> Result<(), GitHubError> {
    // number -> is this an Issue (not a PR)? needed so a bare "#N" in a PR
    // body resolving to another PR is never turned into bl:closesIssue /
    // bl:relatesToIssue (both have range bl:Issue).
    let is_issue_number: HashMap<u64, bool> = issues
        .iter()
        .map(|i| (i.number, !i.is_pull_request()))
        .collect();

    // child count per parent number, to derive bl:Epic (asserted iff an
    // issue has >=1 child and no parent of its own).
    let mut child_count: HashMap<u64, u32> = HashMap::new();
    for i in issues {
        if !i.is_pull_request()
            && let Some(parent) = i.parent_number()
        {
            *child_count.entry(parent).or_insert(0) += 1;
        }
    }

    for issue in issues {
        let subj = iri(ds, &issue.html_url);
        add_label(ds, subj, &issue.title);
        add_integer(ds, subj, "number", issue.number as i64);
        let state_local = if issue.is_open() { "Open" } else { "Closed" };
        let state_id = iri(ds, &bl(state_local));
        add_object(ds, subj, "state", state_id);

        add_type(ds, subj, "WorkItem");
        if issue.is_pull_request() {
            add_type(ds, subj, "PullRequest");
        } else {
            add_type(ds, subj, "Issue");
            let has_children = child_count.get(&issue.number).copied().unwrap_or(0) > 0;
            let has_parent = issue.parent_number().is_some();
            if has_children && !has_parent {
                add_type(ds, subj, "Epic");
            }
            if let Some(parent_num) = issue.parent_number() {
                let parent_iri = format!(
                    "https://github.com/{}/issues/{}",
                    repo_slug_from_html_url(&issue.html_url),
                    parent_num
                );
                let parent_id = iri(ds, &parent_iri);
                add_object(ds, subj, "subIssueOf", parent_id);
            }
        }

        let mut has_ready = false;
        for label in &issue.labels {
            let label_id = ensure_label(ds, &label.name);
            add_object(ds, subj, "hasLabel", label_id);
            if label.name == "ready" {
                has_ready = true;
            }
        }
        // bl:Ready is both a bl:Label and a bl:WorkflowStatus -- derive
        // bl:status from the matching bl:hasLabel rather than asserting it
        // independently (vocabulary.ttl's explicit instruction). Domain is
        // bl:Issue only, never bl:PullRequest.
        if has_ready && !issue.is_pull_request() {
            let ready_id = iri(ds, &bl("Ready"));
            add_object(ds, subj, "status", ready_id);
        }

        if issue.is_pull_request() {
            if let Some(body) = &issue.body {
                let closes = extract_closes(body);
                let mentions = extract_mentions(body);
                for n in &closes {
                    if is_issue_number.get(n).copied().unwrap_or(false) {
                        let target = issue_iri(&issue.html_url, *n);
                        let target_id = iri(ds, &target);
                        add_object(ds, subj, "closesIssue", target_id);
                    }
                }
                for n in mentions.difference(&closes) {
                    if is_issue_number.get(n).copied().unwrap_or(false) {
                        let target = issue_iri(&issue.html_url, *n);
                        let target_id = iri(ds, &target);
                        add_object(ds, subj, "relatesToIssue", target_id);
                    }
                }
            }

            // bl:touchesCrate for PRs only (see plan doc: not computed for
            // issues -- no reliable GitHub-derivable signal for "expected"
            // crate impact on an open issue).
            let files = source.pr_changed_files(issue.number)?;
            let touched = touched_crate_local_names(&files, crates);
            for local in touched {
                let crate_id = iri(ds, &format!("{CRATE_NS}{local}"));
                add_object(ds, subj, "touchesCrate", crate_id);
            }
        }
    }
    Ok(())
}

/// Given changed file paths and the known crate list, returns the set of
/// touched crates' `local_name`s (matched by leading path segment). A file
/// with no crate-directory prefix (a top-level file like `Cargo.toml`, or
/// anything under `src/`) is attributed to the root package (`bl:path "."`),
/// since that crate's own sources live directly under the workspace root
/// rather than under a directory matching its own name.
fn touched_crate_local_names(files: &[String], crates: &[CrateInfo]) -> HashSet<String> {
    let mut touched = HashSet::new();
    for file in files {
        let top = file.split('/').next().unwrap_or(file);
        let mut matched = false;
        for c in crates {
            if c.path == top {
                touched.insert(c.local_name.clone());
                matched = true;
            }
        }
        if !matched
            && (top == "src" || !file.contains('/'))
            && let Some(root) = crates.iter().find(|c| c.path == ".")
        {
            touched.insert(root.local_name.clone());
        }
    }
    touched
}

/// Builds `.../issues/N` from another issue/PR's own `html_url`, reusing its
/// `owner/repo` prefix (all resources here are in the same repo).
fn issue_iri(sibling_html_url: &str, number: u64) -> String {
    format!(
        "https://github.com/{}/issues/{}",
        repo_slug_from_html_url(sibling_html_url),
        number
    )
}

/// Extracts `owner/repo` from a GitHub `html_url` like
/// `https://github.com/daghovland/rdf-datalog/pull/292`.
fn repo_slug_from_html_url(html_url: &str) -> String {
    let after_host = html_url
        .strip_prefix("https://github.com/")
        .unwrap_or(html_url);
    let parts: Vec<&str> = after_host.split('/').collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        after_host.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_closes_matches_common_keywords() {
        assert_eq!(extract_closes("Closes #258"), [258].into_iter().collect());
        assert_eq!(extract_closes("Fixes #1"), [1].into_iter().collect());
        assert_eq!(extract_closes("fixed #2"), [2].into_iter().collect());
        assert_eq!(extract_closes("Resolves #3"), [3].into_iter().collect());
        assert_eq!(
            extract_closes("no closing keyword here, just #161"),
            HashSet::new()
        );
    }

    #[test]
    fn extract_mentions_finds_bare_numbers() {
        assert_eq!(
            extract_mentions("Wires manchester_parser per #161"),
            [161].into_iter().collect()
        );
    }

    #[test]
    fn repo_slug_parses_html_url() {
        assert_eq!(
            repo_slug_from_html_url("https://github.com/daghovland/rdf-datalog/pull/292"),
            "daghovland/rdf-datalog"
        );
    }
}

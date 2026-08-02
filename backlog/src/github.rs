/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Source of GitHub data for the loader: a real `gh api` shell-out
//! ([`GhCliSource`]) plus a fixture-backed test double ([`FixtureSource`]).
//!
//! Kept behind the [`GitHubSource`] trait so `crate::loader`'s mapping logic
//! is testable against recorded JSON without ever making a live network
//! call -- see
//! [`docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`](../../docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md).

use crate::model::RawIssue;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug)]
pub struct GitHubError(pub String);

impl std::fmt::Display for GitHubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for GitHubError {}

/// Everything the loader needs to fetch from GitHub, abstracted so tests can
/// substitute recorded fixture data for a live `gh api` call.
pub trait GitHubSource {
    /// All issues and pull requests in the repo (`state=all`).
    fn list_issues(&self) -> Result<Vec<RawIssue>, GitHubError>;

    /// Filenames changed by pull request `pr_number`, for `bl:touchesCrate`.
    fn pr_changed_files(&self, pr_number: u64) -> Result<Vec<String>, GitHubError>;
}

/// Real source: shells out to the `gh` CLI, already authenticated in this
/// environment (see [#284](https://github.com/daghovland/rdf-datalog/issues/284)'s
/// brief on why this was chosen over a bespoke HTTP client).
pub struct GhCliSource {
    pub owner: String,
    pub repo: String,
}

impl GhCliSource {
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        GhCliSource {
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, GitHubError> {
        let output = Command::new("gh")
            .args(args)
            .output()
            .map_err(|e| GitHubError(format!("failed to spawn gh: {e}")))?;
        if !output.status.success() {
            return Err(GitHubError(format!(
                "gh {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|e| GitHubError(format!("gh output not utf8: {e}")))
    }
}

impl GitHubSource for GhCliSource {
    fn list_issues(&self) -> Result<Vec<RawIssue>, GitHubError> {
        let path = format!(
            "repos/{}/{}/issues?state=all&per_page=100",
            self.owner, self.repo
        );
        // `--jq '.[]'` flattens each page's JSON array into one object per
        // line (NDJSON) so `--paginate` output can be parsed without a
        // `--slurp` step -- see the plan doc.
        let stdout = self.run(&["api", "--paginate", "--jq", ".[]", &path])?;
        parse_ndjson(&stdout)
    }

    fn pr_changed_files(&self, pr_number: u64) -> Result<Vec<String>, GitHubError> {
        let path = format!(
            "repos/{}/{}/pulls/{}/files",
            self.owner, self.repo, pr_number
        );
        let stdout = self.run(&["api", "--paginate", "--jq", ".[].filename", &path])?;
        Ok(stdout.lines().map(|s| s.to_string()).collect())
    }
}

fn parse_ndjson(stdout: &str) -> Result<Vec<RawIssue>, GitHubError> {
    let mut issues = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let issue: RawIssue = serde_json::from_str(line)
            .map_err(|e| GitHubError(format!("failed to parse issue JSON line: {e}")))?;
        issues.push(issue);
    }
    Ok(issues)
}

/// Test double: recorded/fixture data instead of a live `gh api` call.
pub struct FixtureSource {
    pub issues: Vec<RawIssue>,
    pub pr_files: HashMap<u64, Vec<String>>,
}

impl FixtureSource {
    /// Parse recorded NDJSON (one issue JSON object per line, exactly the
    /// shape `GhCliSource::list_issues` produces) plus a changed-files map.
    pub fn from_ndjson(ndjson: &str, pr_files: HashMap<u64, Vec<String>>) -> Self {
        FixtureSource {
            issues: parse_ndjson(ndjson).expect("fixture NDJSON must parse"),
            pr_files,
        }
    }
}

impl GitHubSource for FixtureSource {
    fn list_issues(&self) -> Result<Vec<RawIssue>, GitHubError> {
        Ok(self.issues.clone())
    }

    fn pr_changed_files(&self, pr_number: u64) -> Result<Vec<String>, GitHubError> {
        Ok(self.pr_files.get(&pr_number).cloned().unwrap_or_default())
    }
}

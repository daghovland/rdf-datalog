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
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

/// The "Dagalog" GitHub Project (v2)'s own node id -- see
/// `scripts/set-issue-status.sh`, which writes to this same project (by
/// number 11 there; here we address it directly by node id, since reading
/// via `node(id:)` is simpler than re-deriving the id from `owner`/`repo`).
const DAGALOG_PROJECT_ID: &str = "PVT_kwHOAAbH684BbhXV";

/// One page of the Projects v2 `items` connection query used by
/// [`GhCliSource::project_status_by_number`].
#[derive(Debug, Deserialize)]
struct ProjectItemsResponse {
    data: ProjectItemsData,
}

#[derive(Debug, Deserialize)]
struct ProjectItemsData {
    node: Option<ProjectNode>,
}

#[derive(Debug, Deserialize)]
struct ProjectNode {
    items: ProjectItemsConnection,
}

#[derive(Debug, Deserialize)]
struct ProjectItemsConnection {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<ProjectItemNode>,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemNode {
    /// `null` for a draft issue (no linked Issue/PullRequest), which this
    /// repo's workflow never uses -- everything on the board is a real
    /// issue or PR (CLAUDE.md's "All issues must be made under the Dagalog
    /// project").
    content: Option<ProjectItemContent>,
    #[serde(rename = "fieldValueByName")]
    field_value_by_name: Option<StatusFieldValue>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemContent {
    number: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct StatusFieldValue {
    name: Option<String>,
}

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

    /// Issue/PR number -> raw Projects v2 `Status` field option name
    /// (`"Todo"`/`"In Progress"`/`"Done"`) for every item on the Dagalog
    /// project (#11), fetched in one batched, paginated query rather than
    /// one call per issue -- see
    /// [#447](https://github.com/daghovland/rdf-datalog/issues/447).
    /// Numbers absent from the returned map simply aren't on the project
    /// (or have no Status value set) and get no `bl:status` triple from the
    /// Project-Status derivation.
    fn project_status_by_number(&self) -> Result<HashMap<u64, String>, GitHubError>;
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

    fn project_status_by_number(&self) -> Result<HashMap<u64, String>, GitHubError> {
        // Same GraphQL shape scripts/set-issue-status.sh already uses to
        // *write* the Status field, turned around to *read* it -- reusing
        // the proven query shape rather than inventing a new one. Fetched
        // directly off the project's own `items` connection (paginated 100
        // at a time) instead of one query per issue/PR: with 400+ items in
        // this repo this is a handful of requests total, not hundreds.
        const QUERY: &str = r#"
            query($project: ID!, $cursor: String) {
              node(id: $project) {
                ... on ProjectV2 {
                  items(first: 100, after: $cursor) {
                    pageInfo { hasNextPage endCursor }
                    nodes {
                      content {
                        ... on Issue { number }
                        ... on PullRequest { number }
                      }
                      fieldValueByName(name: "Status") {
                        ... on ProjectV2ItemFieldSingleSelectValue { name }
                      }
                    }
                  }
                }
              }
            }
        "#;

        let mut status_by_number = HashMap::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut args = vec![
                "api".to_string(),
                "graphql".to_string(),
                "-f".to_string(),
                format!("query={QUERY}"),
                "-f".to_string(),
                format!("project={DAGALOG_PROJECT_ID}"),
            ];
            if let Some(c) = &cursor {
                args.push("-f".to_string());
                args.push(format!("cursor={c}"));
            }
            let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
            let stdout = self.run(&args_ref)?;
            let parsed: ProjectItemsResponse = serde_json::from_str(&stdout).map_err(|e| {
                GitHubError(format!(
                    "failed to parse project items GraphQL response: {e}"
                ))
            })?;
            let node = parsed.data.node.ok_or_else(|| {
                GitHubError(format!(
                    "Dagalog project {DAGALOG_PROJECT_ID} not found (or not visible to the authenticated `gh` user)"
                ))
            })?;

            for item in node.items.nodes {
                let Some(number) = item.content.and_then(|c| c.number) else {
                    continue;
                };
                let Some(name) = item.field_value_by_name.and_then(|f| f.name) else {
                    continue;
                };
                status_by_number.insert(number, name);
            }

            if node.items.page_info.has_next_page {
                cursor = node.items.page_info.end_cursor;
            } else {
                break;
            }
        }
        Ok(status_by_number)
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
    /// Issue/PR number -> raw Projects v2 Status option name, as
    /// [`GitHubSource::project_status_by_number`] would return from a live
    /// call. Empty by default (set via [`FixtureSource::with_project_status`])
    /// so existing fixture-based tests that don't care about Project Status
    /// keep working unchanged.
    pub project_status: HashMap<u64, String>,
}

impl FixtureSource {
    /// Parse recorded NDJSON (one issue JSON object per line, exactly the
    /// shape `GhCliSource::list_issues` produces) plus a changed-files map.
    pub fn from_ndjson(ndjson: &str, pr_files: HashMap<u64, Vec<String>>) -> Self {
        FixtureSource {
            issues: parse_ndjson(ndjson).expect("fixture NDJSON must parse"),
            pr_files,
            project_status: HashMap::new(),
        }
    }

    /// Builder-style setter for [`FixtureSource::project_status`], for tests
    /// exercising the Projects v2 Status -> `bl:status` derivation.
    pub fn with_project_status(mut self, project_status: HashMap<u64, String>) -> Self {
        self.project_status = project_status;
        self
    }
}

impl GitHubSource for FixtureSource {
    fn list_issues(&self) -> Result<Vec<RawIssue>, GitHubError> {
        Ok(self.issues.clone())
    }

    fn pr_changed_files(&self, pr_number: u64) -> Result<Vec<String>, GitHubError> {
        Ok(self.pr_files.get(&pr_number).cloned().unwrap_or_default())
    }

    fn project_status_by_number(&self) -> Result<HashMap<u64, String>, GitHubError> {
        Ok(self.project_status.clone())
    }
}

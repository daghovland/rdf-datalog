/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! The subset of a GitHub `issues` API response actually consumed by the
//! [#284](https://github.com/daghovland/rdf-datalog/issues/284) loader.
//!
//! Deliberately not a complete mirror of GitHub's schema -- only the fields
//! `crate::loader` maps to `bl:` triples. See
//! [`docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`](../../docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md)
//! for the full field-mapping table.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct RawLabel {
    pub name: String,
}

/// One row of `gh api repos/{owner}/{repo}/issues?state=all` -- covers both
/// issues and pull requests, since GitHub's Issues API returns PRs too (a PR
/// is an issue with `pull_request` non-null).
#[derive(Debug, Clone, Deserialize)]
pub struct RawIssue {
    pub number: u64,
    pub html_url: String,
    pub title: String,
    /// Present but currently unused for triple generation -- read only to
    /// extract `#N` closes/relates references. See the plan doc's
    /// "Deliberately out of scope" section for why body text itself isn't
    /// persisted as a triple (no `bl:body` property exists yet).
    #[serde(default)]
    pub body: Option<String>,
    /// `"open"` or `"closed"`.
    pub state: String,
    /// ISO-8601 timestamp, always present. Mapped to `bl:createdAt`.
    pub created_at: String,
    /// ISO-8601 timestamp, always present (GitHub bumps it on any field
    /// change, not just body/comment edits). Mapped to `bl:updatedAt`.
    pub updated_at: String,
    /// ISO-8601 timestamp, or `null`/absent while the issue/PR is still
    /// open. Mapped to `bl:closedAt` when present -- see #379.
    #[serde(default)]
    pub closed_at: Option<String>,
    #[serde(default)]
    pub labels: Vec<RawLabel>,
    /// `https://api.github.com/repos/{owner}/{repo}/issues/{number}` of the
    /// parent issue, or absent/null if this issue has no parent. Present on
    /// every issue returned by the listing endpoint -- see the plan doc for
    /// why this is used instead of a second `/sub_issues` call per parent.
    #[serde(default)]
    pub parent_issue_url: Option<String>,
    /// Non-null (any JSON value) iff this "issue" is actually a pull request.
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
}

impl RawIssue {
    pub fn is_pull_request(&self) -> bool {
        self.pull_request.is_some()
    }

    pub fn is_open(&self) -> bool {
        self.state.eq_ignore_ascii_case("open")
    }

    /// Parse the trailing `.../issues/{number}` segment off
    /// [`RawIssue::parent_issue_url`], if present.
    pub fn parent_number(&self) -> Option<u64> {
        self.parent_issue_url
            .as_deref()
            .and_then(|url| url.rsplit('/').next())
            .and_then(|s| s.parse().ok())
    }
}

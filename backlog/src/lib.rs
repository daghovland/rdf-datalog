/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! GitHub-backed loader for the dagalog-on-dagalog backlog mirror
//! ([#282](https://github.com/daghovland/rdf-datalog/issues/282)).
//!
//! Fetches this repo's issue/PR/label state from the GitHub API (via the
//! `gh` CLI) and this repo's own workspace `Cargo.toml` files, and
//! materializes both as `bl:` vocabulary quads
//! (`backlog/ontology/vocabulary.ttl`) in a [`dag_rdf::Datastore`]. A
//! one-shot "regenerate the whole snapshot" loader, not incremental --
//! see [`docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`](../../docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md).

pub mod crates;
pub mod github;
pub mod loader;
pub mod model;

pub use github::{GhCliSource, GitHubError, GitHubSource};
pub use loader::build_snapshot;

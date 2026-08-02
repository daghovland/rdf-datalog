/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! One live-network sanity check against the real GitHub API, via `gh api`.
//! Deliberately `#[ignore]`d -- never run in CI, only manually
//! (`cargo test -p backlog --test live_smoke -- --ignored`) since it needs
//! network access and `gh` auth. See
//! `docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`.

use backlog::{GhCliSource, build_snapshot};
use std::path::Path;

#[test]
#[ignore]
fn live_gh_api_smoke_test() {
    let source = GhCliSource::new("daghovland", "rdf-datalog");
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backlog/ has a parent");
    let ds = build_snapshot(&source, workspace_root).expect("live build_snapshot must succeed");
    assert!(
        ds.named_graphs.get_all_quads().count() > 0,
        "expected at least one quad from the live GitHub API"
    );
}

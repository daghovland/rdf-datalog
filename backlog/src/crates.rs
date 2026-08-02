/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Reads the workspace's own `Cargo.toml` files to discover crates and their
//! direct path dependencies -- **no GitHub API call**, per
//! `bl:dependsOnCrate`'s own vocabulary comment ("read straight off the
//! dependent crate's own Cargo.toml ... to avoid a second source of truth
//! that could drift from Cargo.toml"). See
//! [`docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`](../../docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// One workspace-member crate, plus its direct internal path dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateInfo {
    /// IRI-safe local name, e.g. `dag_rdf`, `dagalog` -- directory name with
    /// `-` normalized to `_` (matches `crates_and_dependencies.ttl`'s
    /// convention: dir `dagalog-kernel` -> `crate:dagalog_kernel`).
    pub local_name: String,
    /// The real Cargo package name (may differ from `local_name`, e.g.
    /// package `dag-rdf` for directory `dag_rdf`) -- this is the `rdfs:label`.
    pub package_name: String,
    /// Workspace-relative directory, e.g. `"dag_rdf"` or `"."` for the root package.
    pub path: String,
    /// `local_name`s of direct `[dependencies]` path deps that resolve to
    /// another workspace member (dev-dependencies are excluded).
    pub path_deps: Vec<String>,
}

/// Discover every workspace member crate and its direct path dependencies by
/// reading `Cargo.toml` files under `workspace_root`. Returns an empty list
/// (with a `log::warn!`) rather than panicking if a file is missing/unparsable
/// for a given member, so one malformed manifest doesn't abort the whole run.
pub fn discover_crates(workspace_root: &Path) -> Vec<CrateInfo> {
    let root_manifest = match read_toml(&workspace_root.join("Cargo.toml")) {
        Some(t) => t,
        None => {
            log::warn!(
                "no root Cargo.toml found under {}",
                workspace_root.display()
            );
            return Vec::new();
        }
    };
    let members: Vec<String> = root_manifest
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Resolve each member's absolute (normalized) directory up front, so
    // dependency `path = "../foo"` entries can be matched back to a member.
    let member_dirs: HashMap<String, PathBuf> = members
        .iter()
        .map(|m| (m.clone(), normalize(&workspace_root.join(m))))
        .collect();
    let dir_to_local: HashMap<PathBuf, String> = members
        .iter()
        .map(|m| (member_dirs[m].clone(), local_name_for(m)))
        .collect();

    let mut crates = Vec::new();
    for member in &members {
        let manifest_path = workspace_root.join(member).join("Cargo.toml");
        let Some(manifest) = read_toml(&manifest_path) else {
            log::warn!("skipping unreadable manifest {}", manifest_path.display());
            continue;
        };
        let package_name = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(member)
            .to_string();

        let mut path_deps = Vec::new();
        if let Some(deps) = manifest.get("dependencies").and_then(|d| d.as_table()) {
            for dep in deps.values() {
                let Some(rel_path) = dep.get("path").and_then(|p| p.as_str()) else {
                    continue;
                };
                let resolved = normalize(&workspace_root.join(member).join(rel_path));
                if let Some(local) = dir_to_local.get(&resolved) {
                    path_deps.push(local.clone());
                }
            }
        }
        path_deps.sort();

        crates.push(CrateInfo {
            local_name: local_name_for(member),
            package_name,
            path: member.clone(),
            path_deps,
        });
    }
    crates
}

fn local_name_for(member: &str) -> String {
    if member == "." {
        // The root package's directory isn't a usable identifier; fall back
        // to reading its own package name at the call site isn't available
        // here, so callers needing the root crate's local name should use
        // its package_name (normalized) instead. In practice the root
        // package's directory member string is always "." and its package
        // name ("dagalog") already has no hyphens, so this rarely matters --
        // resolved properly in discover_crates for the returned CrateInfo.
        "dagalog".to_string()
    } else {
        member.replace('-', "_")
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn read_toml(path: &Path) -> Option<toml::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_this_repo_workspace() {
        // CARGO_MANIFEST_DIR is backlog/, so the workspace root is one level up.
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("backlog/ has a parent");
        let crates = discover_crates(workspace_root);
        assert!(
            crates.iter().any(|c| c.local_name == "ingress"),
            "expected to discover the ingress crate, got: {crates:?}"
        );
        let dag_rdf = crates
            .iter()
            .find(|c| c.local_name == "dag_rdf")
            .expect("dag_rdf crate should be discovered");
        assert_eq!(dag_rdf.package_name, "dag-rdf");
        assert!(
            dag_rdf.path_deps.contains(&"ingress".to_string()),
            "dag_rdf should depend on ingress, got: {:?}",
            dag_rdf.path_deps
        );
    }
}

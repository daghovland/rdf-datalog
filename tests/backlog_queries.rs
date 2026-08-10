/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Test harness for the dagalog-on-dagalog backlog SPARQL query library
//! ([#282](https://github.com/daghovland/rdf-datalog/issues/282),
//! [#286](https://github.com/daghovland/rdf-datalog/issues/286)).
//!
//! Loads the actual query files under `backlog/queries/` (not copies) and
//! runs them against `backlog/examples/` -- the same fixtures #294/#296
//! used -- standing in for a real loader ([#284](https://github.com/daghovland/rdf-datalog/issues/284))
//! snapshot until that exists.

use dag_rdf::{Datastore, GraphElement};
use dagalog::{graph_element_display, load_file, run_sparql_query};
use std::path::{Path, PathBuf};

fn backlog_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("backlog")
        .join(relative)
}

fn load(paths: &[&str]) -> Datastore {
    let mut ds = Datastore::new(10_000);
    for p in paths {
        load_file(&mut ds, &backlog_path(p)).unwrap_or_else(|e| {
            panic!("{p} should parse as Turtle: {e}");
        });
    }
    ds
}

fn query_file(name: &str) -> String {
    std::fs::read_to_string(backlog_path(&format!("queries/{name}.sparql")))
        .unwrap_or_else(|e| panic!("queries/{name}.sparql should be readable: {e}"))
}

const VOCAB: &str = "ontology/vocabulary.ttl";
const VALID_SNAPSHOT: &str = "examples/valid_backlog_snapshot.ttl";
const CRATES: &str = "examples/crates_and_dependencies.ttl";
const PROJECT_AND_STATUS: &str = "examples/project_and_status.ttl";

/// Extract the bound value of `var` in `row` as its display string (e.g.
/// `"267"^^<...integer>` for a number literal, or the literal's own display
/// for a plain string) -- for asserting on the `?number`/`?crateName`
/// projections these queries use.
fn display(row: &std::collections::HashMap<String, GraphElement>, var: &str) -> String {
    row.get(var)
        .map(graph_element_display)
        .unwrap_or_else(|| "(unbound)".to_string())
}

/// Every query file must at least parse and run without error against the
/// real fixtures. Never ignored, so a syntax regression is caught by CI.
#[test]
fn all_queries_run_without_error() {
    let ds = load(&[VOCAB, VALID_SNAPSHOT, CRATES, PROJECT_AND_STATUS]);
    for name in [
        "ready_not_started",
        "epics_with_no_subissues",
        "epics_all_children_closed_but_open",
        "crates_with_open_bugs",
        "crate_dependents",
        "work_items_touching_crate",
    ] {
        run_sparql_query(&ds, &query_file(name))
            .unwrap_or_else(|e| panic!("queries/{name}.sparql must run without error: {e}"));
    }
}

/// `ready_not_started.sparql` must include real ready-and-open issues
/// (#267, #283) and must NOT include a closed issue that also carries the
/// ready label (#262: closed, but was labeled ready before it was worked).
#[test]
fn ready_not_started_excludes_closed_issues() {
    let ds = load(&[VOCAB, VALID_SNAPSHOT, CRATES, PROJECT_AND_STATUS]);
    let result = run_sparql_query(&ds, &query_file("ready_not_started")).unwrap();
    let numbers: Vec<String> = result.rows.iter().map(|r| display(r, "number")).collect();
    assert!(
        numbers.iter().any(|n| n.contains("267")),
        "expected open+ready issue #267 in results, got: {numbers:?}"
    );
    assert!(
        numbers.iter().any(|n| n.contains("283")),
        "expected open+ready issue #283 in results, got: {numbers:?}"
    );
    assert!(
        !numbers.iter().any(|n| n.contains("262")),
        "closed issue #262 must not appear even though it carries the ready label, got: {numbers:?}"
    );
}

/// `epics_with_no_subissues.sparql` returns nothing against the real
/// fixtures (every real epic already has at least one child) -- but must
/// still correctly match a synthetic epic that genuinely has zero children,
/// proving the query isn't vacuously/accidentally always empty.
#[test]
fn epics_with_no_subissues_matches_a_real_empty_epic() {
    let mut ds = load(&[VOCAB, VALID_SNAPSHOT, CRATES, PROJECT_AND_STATUS]);
    assert!(
        run_sparql_query(&ds, &query_file("epics_with_no_subissues"))
            .unwrap()
            .rows
            .is_empty(),
        "no real epic in the fixtures currently has zero children"
    );

    turtle::parse_turtle(
        &mut ds,
        r#"
        @prefix bl: <https://dagalog.no/ns/backlog#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:freshEpic a bl:Issue, bl:Epic, bl:WorkItem ; rdfs:label "fresh epic" ;
          bl:number 9001 ; bl:state bl:Open .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");

    let result = run_sparql_query(&ds, &query_file("epics_with_no_subissues")).unwrap();
    let numbers: Vec<String> = result.rows.iter().map(|r| display(r, "number")).collect();
    assert!(
        numbers.iter().any(|n| n.contains("9001")),
        "the synthetic childless epic must be matched, got: {numbers:?}"
    );
}

/// `epics_all_children_closed_but_open.sparql` must match #25 (RML epic:
/// open, its one child #257 is closed) and must NOT match #267/#282/#178
/// (each still has at least one open child in the fixtures).
#[test]
fn epics_all_children_closed_but_open_matches_only_rml_epic() {
    let ds = load(&[VOCAB, VALID_SNAPSHOT, CRATES, PROJECT_AND_STATUS]);
    let result = run_sparql_query(&ds, &query_file("epics_all_children_closed_but_open")).unwrap();
    let numbers: Vec<String> = result.rows.iter().map(|r| display(r, "number")).collect();
    assert_eq!(
        numbers.len(),
        1,
        "expected exactly one matching epic, got: {numbers:?}"
    );
    assert!(
        numbers[0].contains("25"),
        "expected epic #25 (RML pipeline), got: {numbers:?}"
    );
}

/// `crates_with_open_bugs.sparql` must return exactly the four crates
/// touched by an open Issue in the fixtures: shacl, dagalog, dagalog-kernel,
/// sparql-endpoint.
#[test]
fn crates_with_open_bugs_matches_expected_set() {
    let ds = load(&[VOCAB, VALID_SNAPSHOT, CRATES, PROJECT_AND_STATUS]);
    let result = run_sparql_query(&ds, &query_file("crates_with_open_bugs")).unwrap();
    let mut names: Vec<String> = result
        .rows
        .iter()
        .map(|r| display(r, "crateName"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "\"dagalog\"".to_string(),
            "\"dagalog-kernel\"".to_string(),
            "\"shacl\"".to_string(),
            "\"sparql-endpoint\"".to_string(),
        ],
        "unexpected crate set: {names:?}"
    );
}

/// `crate_dependents.sparql` (parameterized to "shacl" in the shipped file)
/// must return exactly its two known direct dependents.
#[test]
fn crate_dependents_of_shacl() {
    let ds = load(&[VOCAB, CRATES]);
    let result = run_sparql_query(&ds, &query_file("crate_dependents")).unwrap();
    let mut names: Vec<String> = result
        .rows
        .iter()
        .map(|r| display(r, "dependentName"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "\"dagalog-kernel\"".to_string(),
            "\"sparql-endpoint\"".to_string(),
        ],
        "unexpected dependent set: {names:?}"
    );
}

/// `work_items_touching_crate.sparql` (parameterized to "shacl") must
/// return every issue/PR that touches the shacl crate in the fixtures --
/// both the still-open bug reports and the closed fixing PRs.
#[test]
fn work_items_touching_shacl() {
    let ds = load(&[VOCAB, VALID_SNAPSHOT, CRATES, PROJECT_AND_STATUS]);
    let result = run_sparql_query(&ds, &query_file("work_items_touching_crate")).unwrap();
    let numbers: Vec<String> = result.rows.iter().map(|r| display(r, "number")).collect();
    for expected in [
        "264", "266", "271", "272", "273", "276", "280", "289", "290",
    ] {
        assert!(
            numbers.iter().any(|n| n.contains(expected)),
            "expected #{expected} touching shacl in results, got: {numbers:?}"
        );
    }
}

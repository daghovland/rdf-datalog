/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Test harness for the dagalog-on-dagalog backlog vocabulary/shapes
//! ([#282](https://github.com/daghovland/rdf-datalog/issues/282)).
//!
//! Loads `backlog/ontology/vocabulary.ttl` and `backlog/ontology/shapes.ttl`
//! (the actual ontology + SHACL shapes, not copies) against the example
//! fixtures in `backlog/examples/`, standing in for a real loader ([#284](https://github.com/daghovland/rdf-datalog/issues/284))
//! snapshot until that exists. This is a stopgap, not #285's full scope --
//! see the discussion on [#285](https://github.com/daghovland/rdf-datalog/issues/285): nothing previously caught a
//! regression here (a broken Turtle file, a shape that stopped firing) until
//! #284/#285 built real code with its own test suite. This file exists so
//! that gap isn't open indefinitely.

use dag_rdf::Datastore;
use dagalog::load_file;
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

const VOCAB: &str = "ontology/vocabulary.ttl";
const SHAPES: &str = "ontology/shapes.ttl";
const VALID_SNAPSHOT: &str = "examples/valid_backlog_snapshot.ttl";
const CRATES: &str = "examples/crates_and_dependencies.ttl";
const PROJECT_AND_STATUS: &str = "examples/project_and_status.ttl";
const INVALID_ORPHAN: &str = "examples/invalid_orphan_issue.ttl";
const REAL_GAP_274: &str = "examples/real_gap_standalone_issue_274.ttl";

/// Every ontology/example Turtle file parses cleanly. Never ignored, so a
/// syntax regression is caught by CI immediately.
#[test]
fn backlog_files_parse() {
    load(&[
        VOCAB,
        SHAPES,
        VALID_SNAPSHOT,
        CRATES,
        PROJECT_AND_STATUS,
        INVALID_ORPHAN,
        REAL_GAP_274,
    ]);
}

/// The real, valid slice of this repo's backlog (everything except the two
/// deliberately-invalid negative fixtures) must conform to every shape in
/// `shapes.ttl`. This is the main regression guard: if a future ontology or
/// fixture edit breaks conformance, this test fails.
#[test]
fn valid_fixtures_conform() {
    let data = load(&[VOCAB, VALID_SNAPSHOT, CRATES, PROJECT_AND_STATUS]);
    let shapes = load(&[SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        report.conforms,
        "valid backlog fixtures must conform to shapes.ttl, got violations: {:#?}",
        report.results
    );
}

/// The two deliberately-invalid fixtures (a fictional orphan issue, and the
/// real #274 gap) must be caught by `bl:IssueIsEpicXorHasParentShape`, and
/// nothing else in the corpus should also violate when they're included.
#[test]
fn known_invalid_fixtures_are_caught() {
    let data = load(&[
        VOCAB,
        VALID_SNAPSHOT,
        CRATES,
        PROJECT_AND_STATUS,
        INVALID_ORPHAN,
        REAL_GAP_274,
    ]);
    let shapes = load(&[SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms, "known-invalid fixtures must not conform");
    assert_eq!(
        report.results.len(),
        2,
        "expected exactly the two known violations (fictional #999999, real #274), got: {:#?}",
        report.results
    );
}

/// A synthetic issue with no `bl:WorkItem` type must violate
/// `bl:RequiresWorkItemTypeShape` -- proving that shape actually fires
/// rather than being vacuously satisfied. See shapes.ttl's header comment:
/// `sh:class` follows `rdfs:subClassOf` in this engine (per #265/PR #290),
/// so this shape deliberately uses a raw `rdf:type`/`sh:hasValue` check
/// instead, which this test guards against regressing back to `sh:class`.
#[test]
fn missing_workitem_type_is_a_violation() {
    let mut data = load(&[VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix bl: <https://dagalog.no/ns/backlog#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:noWorkItem a bl:Issue ; rdfs:label "Missing WorkItem type" ;
          bl:number 1 ; bl:state bl:Open ; bl:subIssueOf ex:someEpic .
        ex:someEpic a bl:Issue, bl:Epic, bl:WorkItem ; rdfs:label "epic" ;
          bl:number 2 ; bl:state bl:Open .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "an Issue missing an explicit bl:WorkItem type must violate RequiresWorkItemTypeShape"
    );
}

/// A synthetic node typed both `bl:Issue` and `bl:PullRequest` must violate
/// `bl:IssueAndPullRequestMutuallyExclusiveShape`.
#[test]
fn issue_and_pull_request_are_mutually_exclusive() {
    let mut data = load(&[VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix bl: <https://dagalog.no/ns/backlog#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:both a bl:Issue, bl:PullRequest, bl:WorkItem ; rdfs:label "both types" ;
          bl:number 3 ; bl:state bl:Open ; bl:subIssueOf ex:someEpic2 .
        ex:someEpic2 a bl:Issue, bl:Epic, bl:WorkItem ; rdfs:label "epic2" ;
          bl:number 4 ; bl:state bl:Open .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "a node typed both bl:Issue and bl:PullRequest must violate IssueAndPullRequestMutuallyExclusiveShape"
    );
}

/// A synthetic issue that is `bl:status bl:InProgress` but `bl:state
/// bl:Closed` must violate `bl:InProgressImpliesOpenShape`.
#[test]
fn in_progress_implies_open() {
    let mut data = load(&[VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix bl: <https://dagalog.no/ns/backlog#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:contradiction a bl:Issue, bl:WorkItem ; rdfs:label "contradiction" ;
          bl:number 5 ; bl:state bl:Closed ; bl:status bl:InProgress ;
          bl:subIssueOf ex:someEpic3 .
        ex:someEpic3 a bl:Issue, bl:Epic, bl:WorkItem ; rdfs:label "epic3" ;
          bl:number 6 ; bl:state bl:Open .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "an Issue that is bl:status bl:InProgress but bl:state bl:Closed must violate InProgressImpliesOpenShape"
    );
}

/// A synthetic work item missing its title/number/state must violate
/// `bl:WorkItemRequiredFieldsShape` (3 violations: one per missing field).
#[test]
fn missing_required_fields_are_violations() {
    let mut data = load(&[VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix bl: <https://dagalog.no/ns/backlog#> .
        @prefix ex: <http://example.com/ns#> .
        ex:incomplete a bl:Issue, bl:Epic, bl:WorkItem .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "a work item with no label/number/state must violate WorkItemRequiredFieldsShape"
    );
    assert_eq!(
        report.results.len(),
        3,
        "expected 3 violations (missing label, number, state), got: {:#?}",
        report.results
    );
}

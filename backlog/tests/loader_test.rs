/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Loader tests against recorded fixture data
//! (`backlog/tests/fixtures/repo_slice.ndjson`, trimmed real `gh api`
//! output) -- never a live network call. See
//! `docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`.

use backlog::github::FixtureSource;
use backlog::loader::{BL, CRATE_NS, build_snapshot};
use dag_rdf::{Datastore, GraphElementId};
use ingress::{IriReference, RdfResource};
use std::collections::HashMap;
use std::path::Path;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backlog/ has a parent")
        .to_path_buf()
}

fn fixture_source() -> FixtureSource {
    let ndjson = include_str!("fixtures/repo_slice.ndjson");
    let mut pr_files = HashMap::new();
    pr_files.insert(
        276,
        vec![
            "shacl/src/evaluate.rs".to_string(),
            "shacl/src/translate.rs".to_string(),
        ],
    );
    pr_files.insert(
        292,
        vec![
            "CLAUDE.md".to_string(),
            "dagalog-kernel/src/cell/manchester.rs".to_string(),
            "src/lib.rs".to_string(),
        ],
    );
    FixtureSource::from_ndjson(ndjson, pr_files)
}

fn lookup(ds: &Datastore, full_iri: &str) -> Option<GraphElementId> {
    ds.resources
        .resource_map
        .get(&ingress::GraphElement::NodeOrEdge(RdfResource::Iri(
            IriReference(full_iri.to_string()),
        )))
        .copied()
}

fn has_triple(ds: &Datastore, s: &str, p: &str, o: &str) -> bool {
    let (Some(s), Some(p), Some(o)) = (lookup(ds, s), lookup(ds, p), lookup(ds, o)) else {
        return false;
    };
    ds.contains_triple(&dag_rdf::Triple {
        subject: s,
        predicate: p,
        obj: o,
    })
}

const RDF_TYPE: &str = ingress::RDF_TYPE;
fn rdfs_label() -> String {
    format!("{}label", ingress::RDFS)
}

#[test]
fn issue_gets_title_number_state() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    let subj = "https://github.com/daghovland/rdf-datalog/issues/284";
    let id = lookup(&ds, subj).expect("issue #284 must be interned");
    let label_pred = lookup(&ds, &rdfs_label()).expect("rdfs:label must be interned");
    let titles: Vec<_> = ds
        .get_triples_with_subject_predicate(id, label_pred)
        .map(|t| ds.resources.get_graph_element(t.obj).to_string())
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("GitHub API loader")),
        "expected #284's title as rdfs:label, got: {titles:?}"
    );

    let number_pred = lookup(&ds, &format!("{BL}number")).unwrap();
    let numbers: Vec<_> = ds
        .get_triples_with_subject_predicate(id, number_pred)
        .map(|t| ds.resources.get_graph_element(t.obj).to_string())
        .collect();
    assert_eq!(numbers.len(), 1, "expected exactly one bl:number triple");

    assert!(
        has_triple(&ds, subj, &format!("{BL}state"), &format!("{BL}Open")),
        "#284 is open on GitHub, expected bl:state bl:Open"
    );
    assert!(
        has_triple(&ds, subj, RDF_TYPE, &format!("{BL}Issue")),
        "expected explicit a bl:Issue"
    );
    assert!(
        has_triple(&ds, subj, RDF_TYPE, &format!("{BL}WorkItem")),
        "expected explicit a bl:WorkItem (rdfs:subClassOf is not free)"
    );
}

#[test]
fn labels_become_resources() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    let subj = "https://github.com/daghovland/rdf-datalog/issues/258";
    assert!(has_triple(
        &ds,
        subj,
        &format!("{BL}hasLabel"),
        &format!("{BL}Bug")
    ));
    assert!(has_triple(
        &ds,
        subj,
        &format!("{BL}hasLabel"),
        &format!("{BL}Ready")
    ));
    // bl:Ready doubles as bl:WorkflowStatus -- bl:status must be derived,
    // not duplicated as an independent fact.
    assert!(has_triple(
        &ds,
        subj,
        &format!("{BL}status"),
        &format!("{BL}Ready")
    ));
}

#[test]
fn pull_requests_are_typed_and_never_get_status() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    let pr = "https://github.com/daghovland/rdf-datalog/pull/276";
    assert!(has_triple(&ds, pr, RDF_TYPE, &format!("{BL}PullRequest")));
    assert!(has_triple(&ds, pr, RDF_TYPE, &format!("{BL}WorkItem")));
    assert!(!has_triple(&ds, pr, RDF_TYPE, &format!("{BL}Issue")));

    let pr_id = lookup(&ds, pr).unwrap();
    let status_pred = lookup(&ds, &format!("{BL}status"));
    if let Some(status_pred) = status_pred {
        assert_eq!(
            ds.get_triples_with_subject_predicate(pr_id, status_pred)
                .count(),
            0,
            "a PullRequest must never get bl:status (domain bl:Issue only)"
        );
    }
}

#[test]
fn sub_issue_of_derived_from_parent_issue_url() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    assert!(has_triple(
        &ds,
        "https://github.com/daghovland/rdf-datalog/issues/258",
        &format!("{BL}subIssueOf"),
        "https://github.com/daghovland/rdf-datalog/issues/267"
    ));
    assert!(has_triple(
        &ds,
        "https://github.com/daghovland/rdf-datalog/issues/284",
        &format!("{BL}subIssueOf"),
        "https://github.com/daghovland/rdf-datalog/issues/282"
    ));
}

#[test]
fn epic_derived_for_parentless_issue_with_children() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    // #282 has no parent and has children (#283, #284) in the fixture -> Epic.
    assert!(has_triple(
        &ds,
        "https://github.com/daghovland/rdf-datalog/issues/282",
        RDF_TYPE,
        &format!("{BL}Epic")
    ));
    // #267 HAS a parent in the real data (issue #65) even though it also has
    // a child (#258) in this fixture slice -- a genuine mid-tree node, which
    // must NOT be asserted bl:Epic (see MODELING_NOTES.md "Epic modeling").
    assert!(!has_triple(
        &ds,
        "https://github.com/daghovland/rdf-datalog/issues/267",
        RDF_TYPE,
        &format!("{BL}Epic")
    ));
}

#[test]
fn pr_closes_issue_via_closing_keyword() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    assert!(has_triple(
        &ds,
        "https://github.com/daghovland/rdf-datalog/pull/276",
        &format!("{BL}closesIssue"),
        "https://github.com/daghovland/rdf-datalog/issues/258"
    ));
}

#[test]
fn pr_relates_to_issue_without_closing_keyword() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    let pr = "https://github.com/daghovland/rdf-datalog/pull/292";
    let issue_161 = "https://github.com/daghovland/rdf-datalog/issues/161";
    assert!(
        has_triple(&ds, pr, &format!("{BL}relatesToIssue"), issue_161),
        "PR #292 mentions #161 without a closing keyword -> bl:relatesToIssue"
    );
    assert!(
        !has_triple(&ds, pr, &format!("{BL}closesIssue"), issue_161),
        "PR #292 must NOT bl:closesIssue #161 -- it deliberately leaves it open (real #292/#161 case)"
    );
}

#[test]
fn crates_are_discovered_with_dependencies() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    let ingress_crate = format!("{CRATE_NS}ingress");
    let dag_rdf_crate = format!("{CRATE_NS}dag_rdf");
    assert!(has_triple(
        &ds,
        &dag_rdf_crate,
        RDF_TYPE,
        &format!("{BL}Crate")
    ));
    assert!(has_triple(
        &ds,
        &dag_rdf_crate,
        &format!("{BL}dependsOnCrate"),
        &ingress_crate
    ));
}

#[test]
fn touches_crate_derived_from_pr_changed_files() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    assert!(has_triple(
        &ds,
        "https://github.com/daghovland/rdf-datalog/pull/276",
        &format!("{BL}touchesCrate"),
        &format!("{CRATE_NS}shacl")
    ));
    let pr292 = "https://github.com/daghovland/rdf-datalog/pull/292";
    assert!(has_triple(
        &ds,
        pr292,
        &format!("{BL}touchesCrate"),
        &format!("{CRATE_NS}dagalog_kernel")
    ));
    assert!(
        has_triple(
            &ds,
            pr292,
            &format!("{BL}touchesCrate"),
            &format!("{CRATE_NS}dagalog")
        ),
        "CLAUDE.md/src/lib.rs must be attributed to the root crate (bl:path \".\")"
    );
}

#[test]
fn touches_file_derived_from_pr_changed_files() {
    let source = fixture_source();
    let ds = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    let pr276 = "https://github.com/daghovland/rdf-datalog/pull/276";
    let pr276_id = lookup(&ds, pr276).expect("PR #276 must be interned");
    let touches_file_pred = lookup(&ds, &format!("{BL}touchesFile"))
        .expect("bl:touchesFile must be interned by the loader");
    let files: Vec<String> = ds
        .get_triples_with_subject_predicate(pr276_id, touches_file_pred)
        .map(|t| ds.resources.get_graph_element(t.obj).to_string())
        .collect();
    assert!(
        files.iter().any(|f| f.contains("shacl/src/evaluate.rs")),
        "expected bl:touchesFile shacl/src/evaluate.rs on PR #276, got: {files:?}"
    );
    assert!(
        files.iter().any(|f| f.contains("shacl/src/translate.rs")),
        "expected bl:touchesFile shacl/src/translate.rs on PR #276, got: {files:?}"
    );
    assert_eq!(
        files.len(),
        2,
        "expected exactly the two files fixture_source() records for PR #276, got: {files:?}"
    );
}

/// The generated snapshot must conform to `backlog/ontology/shapes.ttl`,
/// loaded through this repo's own `shacl` crate -- mirrors
/// `tests/backlog_ontology.rs`'s existing pattern, catching structural
/// mistakes fixture-assertion tests alone would miss.
#[test]
fn generated_snapshot_conforms_to_shapes() {
    let source = fixture_source();
    let mut data = build_snapshot(&source, &workspace_root()).expect("build_snapshot must succeed");
    turtle::parse_turtle(
        &mut data,
        std::fs::File::open(workspace_root().join("backlog/ontology/vocabulary.ttl")).unwrap(),
    )
    .expect("vocabulary.ttl must parse");

    let mut shapes = Datastore::new(10_000);
    turtle::parse_turtle(
        &mut shapes,
        std::fs::File::open(workspace_root().join("backlog/ontology/shapes.ttl")).unwrap(),
    )
    .expect("shapes.ttl must parse");

    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        report.conforms,
        "loader output for this fixture slice must conform to shapes.ttl, got violations: {:#?}",
        report.results
    );
}

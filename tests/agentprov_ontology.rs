/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Test harness for the agent-provenance / transcript-summary vocabulary
//! ([#326](https://github.com/daghovland/rdf-datalog/issues/326), a
//! sub-issue of the agent-provenance epic
//! [#306](https://github.com/daghovland/rdf-datalog/issues/306)).
//!
//! Mirrors `tests/backlog_ontology.rs`'s pattern: loads the actual
//! `backlog/ontology/agentprov-vocabulary.ttl` and
//! `backlog/ontology/agentprov-shapes.ttl` files (not copies) against a
//! small grounding fixture defined inline here, and checks conformance via
//! this repo's own `shacl` crate. See
//! [`docs/plans/AGENT_PROVENANCE_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/AGENT_PROVENANCE_PLAN.md)
//! for the full design this mirrors.
//!
//! The grounding fixture here is a toy example only, exercising every
//! class/property the vocabulary declares -- it is NOT real transcript
//! data. Real, repo-specific transcript summaries live under `provenance/`
//! (out of scope for #326; see #327).

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

const AGP_VOCAB: &str = "ontology/agentprov-vocabulary.ttl";
const AGP_SHAPES: &str = "ontology/agentprov-shapes.ttl";
const BL_VOCAB: &str = "ontology/vocabulary.ttl";

/// A toy grounding fixture for a fictional PR's transcript summary,
/// exercising every class/property the plan doc specifies: `agp:AgentSession`,
/// `agp:TranscriptSummary`, `agp:Decision`, and all listed properties
/// (`agp:summaryText`, `agp:reasoningFor`, `agp:transcriptRef`,
/// `agp:decisionPoint`, `agp:alternative`, `agp:parentSession`), plus the
/// reused PROV-O terms (`prov:Activity`/`prov:Entity`/`prov:SoftwareAgent`/
/// `prov:Person` and the relation properties).
const VALID_FIXTURE: &str = r#"
@prefix agp: <https://dagalog.dev/ns/agentprov#> .
@prefix bl: <https://dagalog.dev/ns/backlog#> .
@prefix prov: <http://www.w3.org/ns/prov#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix ex: <http://example.com/ns#> .

ex:pr999 a bl:PullRequest, bl:WorkItem ;
    rdfs:label "Fictional PR for grounding the agentprov vocabulary" ;
    bl:number 999 ;
    bl:state bl:Open .

ex:claudeSonnet5 a prov:SoftwareAgent ;
    rdfs:label "Claude Sonnet 5" .

ex:dag a prov:Person ;
    rdfs:label "Dag Hovland" .

ex:parentSession a agp:AgentSession ;
    prov:wasAssociatedWith ex:claudeSonnet5 ;
    prov:startedAtTime "2026-07-31T09:00:00Z"^^xsd:dateTime .

ex:session1 a agp:AgentSession ;
    prov:wasAssociatedWith ex:claudeSonnet5 ;
    prov:used ex:pr999 ;
    agp:parentSession ex:parentSession ;
    prov:startedAtTime "2026-07-31T09:05:00Z"^^xsd:dateTime ;
    prov:endedAtTime "2026-07-31T10:00:00Z"^^xsd:dateTime .

ex:decision1 a agp:Decision ;
    agp:summaryText "Chose to model agp:AgentSession as a subclass of prov:Activity." ;
    agp:alternative "Model sessions as a bespoke class unrelated to PROV-O." ;
    agp:alternative "Reuse prov:Activity directly with no subclass." .

ex:summary1 a agp:TranscriptSummary ;
    agp:summaryText "This session designed the agentprov vocabulary, reusing PROV-O terms directly rather than reinventing them." ;
    agp:reasoningFor ex:pr999 ;
    agp:transcriptRef "session_01example" ;
    agp:decisionPoint ex:decision1 ;
    prov:wasGeneratedBy ex:session1 ;
    prov:wasAttributedTo ex:claudeSonnet5 .

ex:claudeSonnet5 prov:actedOnBehalfOf ex:dag .
"#;

fn load_valid_fixture() -> Datastore {
    let mut ds = load(&[BL_VOCAB, AGP_VOCAB]);
    turtle::parse_turtle(&mut ds, VALID_FIXTURE.as_bytes())
        .expect("grounding fixture should parse as Turtle");
    ds
}

/// The vocabulary and shapes files parse cleanly as Turtle. Never ignored,
/// so a syntax regression is caught by CI immediately.
#[test]
fn agentprov_files_parse() {
    load(&[BL_VOCAB, AGP_VOCAB, AGP_SHAPES]);
}

/// The inline grounding fixture (a toy PR + session + summary + decision
/// point) must conform to every shape in `agentprov-shapes.ttl`.
#[test]
fn valid_fixture_conforms() {
    let data = load_valid_fixture();
    let shapes = load(&[AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        report.conforms,
        "grounding fixture must conform to agentprov-shapes.ttl, got violations: {:#?}",
        report.results
    );
}

/// A `agp:TranscriptSummary` missing `agp:reasoningFor` must violate
/// `agp:TranscriptSummaryRequiredFieldsShape`.
#[test]
fn summary_without_reasoning_for_is_a_violation() {
    let mut data = load(&[BL_VOCAB, AGP_VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix agp: <https://dagalog.dev/ns/agentprov#> .
        @prefix prov: <http://www.w3.org/ns/prov#> .
        @prefix ex: <http://example.com/ns#> .
        ex:agent a prov:SoftwareAgent .
        ex:session a agp:AgentSession ; prov:wasAssociatedWith ex:agent .
        ex:orphanSummary a agp:TranscriptSummary ;
            agp:summaryText "No reasoningFor here." ;
            prov:wasAttributedTo ex:agent ;
            prov:wasGeneratedBy ex:session .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "a TranscriptSummary missing agp:reasoningFor must violate TranscriptSummaryRequiredFieldsShape"
    );
}

/// An `agp:AgentSession` missing `prov:wasAssociatedWith` must violate
/// `agp:SessionHasAgentShape`.
#[test]
fn session_without_agent_is_a_violation() {
    let mut data = load(&[BL_VOCAB, AGP_VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix agp: <https://dagalog.dev/ns/agentprov#> .
        @prefix ex: <http://example.com/ns#> .
        ex:orphanSession a agp:AgentSession .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "an AgentSession missing prov:wasAssociatedWith must violate SessionHasAgentShape"
    );
}

/// An `agp:TranscriptSummary` missing `prov:wasGeneratedBy` entirely must
/// violate `agp:SummaryGeneratedByShape` (the `sh:minCount` half).
#[test]
fn summary_without_generating_session_is_a_violation() {
    let mut data = load(&[BL_VOCAB, AGP_VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix agp: <https://dagalog.dev/ns/agentprov#> .
        @prefix bl: <https://dagalog.dev/ns/backlog#> .
        @prefix prov: <http://www.w3.org/ns/prov#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:pr1000 a bl:PullRequest, bl:WorkItem ; rdfs:label "another fictional PR" ;
            bl:number 1000 ; bl:state bl:Open .
        ex:agent a prov:SoftwareAgent .
        ex:ungenerated a agp:TranscriptSummary ;
            agp:summaryText "No wasGeneratedBy here." ;
            agp:reasoningFor ex:pr1000 ;
            prov:wasAttributedTo ex:agent .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "a TranscriptSummary missing prov:wasGeneratedBy (an AgentSession) must violate SummaryGeneratedByShape"
    );
}

/// An `agp:TranscriptSummary` with an `agp:abstractText` at or under the
/// 160-char cap conforms.
#[test]
fn abstract_text_under_max_length_conforms() {
    let mut data = load(&[BL_VOCAB, AGP_VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix agp: <https://dagalog.dev/ns/agentprov#> .
        @prefix bl: <https://dagalog.dev/ns/backlog#> .
        @prefix prov: <http://www.w3.org/ns/prov#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:pr1002 a bl:PullRequest, bl:WorkItem ; rdfs:label "short-abstract PR" ;
            bl:number 1002 ; bl:state bl:Open .
        ex:agent a prov:SoftwareAgent .
        ex:session a agp:AgentSession ; prov:wasAssociatedWith ex:agent .
        ex:summaryWithAbstract a agp:TranscriptSummary ;
            agp:summaryText "This summary has a short abstract alongside its full text." ;
            agp:abstractText "Fix short abstract, well under the cap." ;
            agp:reasoningFor ex:pr1002 ;
            prov:wasAttributedTo ex:agent ;
            prov:wasGeneratedBy ex:session .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        report.conforms,
        "an agp:abstractText at/under 160 chars must conform, got violations: {:#?}",
        report.results
    );
}

/// An `agp:TranscriptSummary` whose `agp:abstractText` exceeds the 160-char
/// cap must violate `agp:TranscriptSummaryRequiredFieldsShape`.
#[test]
fn abstract_text_over_max_length_is_a_violation() {
    let mut data = load(&[BL_VOCAB, AGP_VOCAB]);
    let too_long = "x".repeat(161);
    let ttl = format!(
        r#"
        @prefix agp: <https://dagalog.dev/ns/agentprov#> .
        @prefix bl: <https://dagalog.dev/ns/backlog#> .
        @prefix prov: <http://www.w3.org/ns/prov#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:pr1003 a bl:PullRequest, bl:WorkItem ; rdfs:label "long-abstract PR" ;
            bl:number 1003 ; bl:state bl:Open .
        ex:agent a prov:SoftwareAgent .
        ex:session a agp:AgentSession ; prov:wasAssociatedWith ex:agent .
        ex:summaryWithLongAbstract a agp:TranscriptSummary ;
            agp:summaryText "This summary has an abstract that is too long for the cap." ;
            agp:abstractText "{too_long}" ;
            agp:reasoningFor ex:pr1003 ;
            prov:wasAttributedTo ex:agent ;
            prov:wasGeneratedBy ex:session .
        "#
    );
    turtle::parse_turtle(&mut data, ttl.as_bytes()).expect("scratch fixture should parse");
    let shapes = load(&[AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "an agp:abstractText over 160 chars must violate TranscriptSummaryRequiredFieldsShape"
    );
}

/// An `agp:TranscriptSummary` whose `prov:wasGeneratedBy` points at
/// something that is a `prov:Activity` but NOT typed `agp:AgentSession`
/// must also violate `agp:SummaryGeneratedByShape` (the `sh:class` half --
/// proving that half actually fires, not just the `sh:minCount` half
/// exercised by `summary_without_generating_session_is_a_violation` above.
/// See shapes.ttl's own header comment on the `sh:class` trap this guards
/// against regressing into.)
#[test]
fn summary_generated_by_non_agent_session_is_a_violation() {
    let mut data = load(&[BL_VOCAB, AGP_VOCAB]);
    turtle::parse_turtle(
        &mut data,
        r#"
        @prefix agp: <https://dagalog.dev/ns/agentprov#> .
        @prefix bl: <https://dagalog.dev/ns/backlog#> .
        @prefix prov: <http://www.w3.org/ns/prov#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix ex: <http://example.com/ns#> .
        ex:pr1001 a bl:PullRequest, bl:WorkItem ; rdfs:label "yet another fictional PR" ;
            bl:number 1001 ; bl:state bl:Open .
        ex:agent a prov:SoftwareAgent .
        ex:notASession a prov:Activity .
        ex:wronglyGenerated a agp:TranscriptSummary ;
            agp:summaryText "wasGeneratedBy points at a plain prov:Activity, not an agp:AgentSession." ;
            agp:reasoningFor ex:pr1001 ;
            prov:wasAttributedTo ex:agent ;
            prov:wasGeneratedBy ex:notASession .
        "#
        .as_bytes(),
    )
    .expect("scratch fixture should parse");
    let shapes = load(&[AGP_SHAPES]);
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "a TranscriptSummary generated by a plain prov:Activity (not an agp:AgentSession) must violate SummaryGeneratedByShape"
    );
}

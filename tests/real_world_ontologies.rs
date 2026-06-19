/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests loading real-world W3C ontologies.
//!
//! Tests verify that each ontology file parses without error and produces
//! a non-empty triple store. Ontology files are vendored in `tests/testdata/`:
//!
//! | File              | Source                                          |
//! |-------------------|-------------------------------------------------|
//! | `prov-o.ttl`      | <https://www.w3.org/ns/prov-o> (W3C)           |
//! | `dcterms.ttl`     | <https://www.dublincore.org/specifications/dublin-core/dcmi-terms/> |
//! | `owl-time.ttl`    | <https://www.w3.org/2006/time> (W3C)           |
//!
//! All tests run without `#[ignore]`.

use dag_rdf::Datastore;
use dagalog::load_file;
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn load(file: &str) -> Datastore {
    let path = testdata(file);
    let mut ds = Datastore::new(50_000);
    load_file(&mut ds, &path).unwrap_or_else(|e| panic!("failed to load {}: {}", file, e));
    ds
}

// ── W3C PROV-O (Provenance Ontology) ─────────────────────────────────────────
//
// The W3C PROV-O ontology defines the provenance data model in OWL.
// Source: https://www.w3.org/ns/prov-o (W3C Recommendation, 30 April 2013)

/// PROV-O Turtle file parses without error and produces triples.
///
/// Source: https://www.w3.org/ns/prov-o
#[test]
fn prov_o_parses() {
    let ds = load("prov-o.ttl");
    assert!(
        ds.named_graphs.quad_count > 0,
        "PROV-O should contain triples"
    );
}

/// PROV-O contains key provenance classes (prov:Entity, prov:Activity, prov:Agent).
///
/// Source: https://www.w3.org/ns/prov-o
#[test]
fn prov_o_contains_core_classes() {
    use dagalog::run_sparql_query;
    let ds = load("prov-o.ttl");
    let result = run_sparql_query(
        &ds,
        "PREFIX owl: <http://www.w3.org/2002/07/owl#>
         SELECT ?class WHERE { ?class a owl:Class }",
    )
    .expect("SPARQL query should execute");
    assert!(
        result.rows.len() >= 3,
        "PROV-O should define at least 3 OWL classes, found {}",
        result.rows.len()
    );
}

/// PROV-O OWL-RL reasoning does not crash.
///
/// Source: https://www.w3.org/ns/prov-o
#[test]
fn prov_o_owlrl_reasoning() {
    let path = testdata("prov-o.ttl");
    let mut ds = Datastore::new(100_000);
    dagalog::apply_ontologies(&mut ds, &[path]).expect("PROV-O OWL-RL reasoning should not crash");
    assert!(ds.named_graphs.quad_count > 0);
}

// ── Dublin Core Metadata Terms ────────────────────────────────────────────────
//
// DCMI Metadata Terms — the foundational metadata vocabulary.
// Source: https://www.dublincore.org/specifications/dublin-core/dcmi-terms/

/// Dublin Core Terms Turtle file parses without error.
///
/// Source: https://www.dublincore.org/specifications/dublin-core/dcmi-terms/dublin_core_terms.ttl
#[test]
fn dublin_core_terms_parses() {
    let ds = load("dcterms.ttl");
    assert!(
        ds.named_graphs.quad_count > 0,
        "Dublin Core Terms should contain triples"
    );
}

/// Dublin Core Terms contains standard DC properties (title, creator, date…).
///
/// Source: https://www.dublincore.org/specifications/dublin-core/dcmi-terms/
#[test]
fn dublin_core_terms_contains_core_properties() {
    use dagalog::run_sparql_query;
    let ds = load("dcterms.ttl");
    let result = run_sparql_query(
        &ds,
        "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
         PREFIX owl: <http://www.w3.org/2002/07/owl#>
         SELECT ?prop WHERE {
           { ?prop a rdf:Property } UNION { ?prop a owl:AnnotationProperty }
         }",
    )
    .expect("SPARQL query should execute");
    assert!(
        result.rows.len() >= 15,
        "Dublin Core Terms should define at least 15 properties, found {}",
        result.rows.len()
    );
}

// ── W3C OWL-Time ─────────────────────────────────────────────────────────────
//
// OWL-Time is an OWL-2 ontology for temporal concepts.
// Source: https://www.w3.org/2006/time (W3C Recommendation, 19 October 2017)

/// OWL-Time Turtle file parses without error.
///
/// Source: https://www.w3.org/2006/time
#[test]
fn owl_time_parses() {
    let ds = load("owl-time.ttl");
    assert!(
        ds.named_graphs.quad_count > 0,
        "OWL-Time should contain triples"
    );
}

/// OWL-Time contains temporal classes (time:Instant, time:Interval, time:Duration).
///
/// Source: https://www.w3.org/2006/time
#[test]
fn owl_time_contains_temporal_classes() {
    use dagalog::run_sparql_query;
    let ds = load("owl-time.ttl");
    let result = run_sparql_query(
        &ds,
        "PREFIX owl: <http://www.w3.org/2002/07/owl#>
         SELECT ?class WHERE { ?class a owl:Class }",
    )
    .expect("SPARQL query should execute");
    assert!(
        result.rows.len() >= 5,
        "OWL-Time should define at least 5 OWL classes, found {}",
        result.rows.len()
    );
}

/// OWL-Time OWL-RL reasoning does not crash.
///
/// Source: https://www.w3.org/2006/time
#[test]
fn owl_time_owlrl_reasoning() {
    let path = testdata("owl-time.ttl");
    let mut ds = Datastore::new(100_000);
    dagalog::apply_ontologies(&mut ds, &[path])
        .expect("OWL-Time OWL-RL reasoning should not crash");
    assert!(ds.named_graphs.quad_count > 0);
}

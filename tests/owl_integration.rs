/*
Copyright (C) 2025,2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests translated from DagSemTools `Api.Tests/TestApiOntology.cs`.
//!
//! Each test corresponds to a `[Fact]` or `[Theory]` in the original C# suite.
//! Tests that require functionality not yet implemented (Tableau, ALC) are
//! marked `#[ignore]`.
//!
//! Run just this file: `cargo test --test owl_integration`

use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use dagalog::load_file;
use datalog::evaluate_rules;
use owl2rl2datalog::owl2datalog;
use rdf_owl_translator::rdf2owl;
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn load_and_extract_rules(name: &str) -> (Datastore, usize) {
    let mut ds = Datastore::new(500_000);
    load_file(&mut ds, &testdata(name)).expect("ontology must load");
    let ontology_doc = rdf2owl(&mut ds);
    let axiom_count = ontology_doc.ontology.axioms.len();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    let rule_count = rules.len();
    evaluate_rules(rules, &mut ds);
    let _ = axiom_count;
    (ds, rule_count)
}

fn has_triple(ds: &Datastore, subj: &str, pred: &str, obj: &str) -> bool {
    let s = ds
        .resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            subj.to_string(),
        ))))
        .copied();
    let p = ds
        .resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            pred.to_string(),
        ))))
        .copied();
    let o = ds
        .resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            obj.to_string(),
        ))))
        .copied();
    match (s, p, o) {
        (Some(s), Some(p), Some(o)) => !ds
            .quads_matching(None, Some(s), Some(p), Some(o))
            .is_empty(),
        _ => false,
    }
}

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

// ── TestApiOntology.LoadEmptyOntologyWorks ────────────────────────────────────

/// Translated from `LoadEmptyOntologyWorks`.
/// empty.owl is 0 bytes; our rdf2owl correctly returns 0 axioms (no implicit
/// axioms are added unlike in the original C# implementation).
#[test]
fn load_empty_ontology_does_not_panic() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("empty.owl")).unwrap();
    let ontology_doc = rdf2owl(&mut ds);
    // File is empty so axiom count is 0; this just verifies no panic
    let _ = ontology_doc.ontology.axioms.len();
}

// ── TestApiOntology.LoadSubClassFromRestriction ───────────────────────────────

/// Translated from `LoadSubClassFromRestriction`.
#[test]
fn load_subclass_restriction_extracts_axioms() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("subclass_of_restriction.owl")).unwrap();
    let ontology_doc = rdf2owl(&mut ds);
    assert!(
        !ontology_doc.ontology.axioms.is_empty(),
        "subclass_of_restriction.owl should yield OWL axioms"
    );
}

// ── TestApiOntology.EqualityReasoningWorks ────────────────────────────────────

/// Translated from `EqualityReasoningWorks`.
///
/// equality.owl: `ind1 rdf:type SomeClass` and `ind1 owl:sameAs ind2`.
/// After reasoning, `ind2` should also be typed.
#[test]
fn equality_reasoning_works() {
    let (ds, _) = load_and_extract_rules("equality.owl");

    const IND1: &str = "https://example.com/vocab#ind1";
    const IND2: &str = "https://example.com/vocab#ind2";

    // ind1 must be typed (it was explicitly asserted)
    let ind1_typed = !ds
        .quads_matching(
            None,
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    IND1.to_string(),
                ))))
                .copied(),
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    RDF_TYPE.to_string(),
                ))))
                .copied(),
            None,
        )
        .is_empty();
    assert!(ind1_typed, "ind1 must have an rdf:type");

    // After reasoning via owl:sameAs, ind2 should also be typed
    let ind2_typed = !ds
        .quads_matching(
            None,
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    IND2.to_string(),
                ))))
                .copied(),
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    RDF_TYPE.to_string(),
                ))))
                .copied(),
            None,
        )
        .is_empty();
    assert!(
        ind2_typed,
        "ind2 should be typed after owl:sameAs equality reasoning"
    );
}

// ── TestApiOntology.LoadIntersection ─────────────────────────────────────────

/// Translated from `LoadIntersection`.
#[test]
fn load_intersection_extracts_axioms() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("intersection.owl.ttl")).unwrap();
    let ontology_doc = rdf2owl(&mut ds);
    assert!(
        !ontology_doc.ontology.axioms.is_empty(),
        "intersection.owl.ttl should yield OWL axioms"
    );
}

// ── TestApiOntology.ReasoningExampleWorks [Theory × 10] ──────────────────────
//
// Each ontology contains data about `http://example.org/x` and rules that
// should infer `x rdf:type A` after materialisation.
// `http://example.org/notx` should NOT get rdf:type A.

const EXAMPLE_X: &str = "http://example.org/x";
const EXAMPLE_A: &str = "http://example.org/A";
const EXAMPLE_NOTX: &str = "http://example.org/notx";

fn assert_reasoning_example(name: &str) {
    let (ds, rule_count) = load_and_extract_rules(name);
    assert!(
        rule_count > 0,
        "{}: expected at least one Datalog rule",
        name
    );

    let x_has_type_a = has_triple(&ds, EXAMPLE_X, RDF_TYPE, EXAMPLE_A);
    assert!(
        x_has_type_a,
        "{}: expected x rdf:type A after reasoning, but it was not inferred",
        name
    );

    let notx_has_type_a = has_triple(&ds, EXAMPLE_NOTX, RDF_TYPE, EXAMPLE_A);
    assert!(
        !notx_has_type_a,
        "{}: notx should NOT have rdf:type A, but it was incorrectly inferred",
        name
    );
}

#[test]
fn reasoning_example_min_qualified_union() {
    assert_reasoning_example("minQualifiedUnion.ttl");
}

#[test]
fn reasoning_example_some_values_from_inverse() {
    assert_reasoning_example("someValuesFromInverse.ttl");
}

#[test]
fn reasoning_example_intersection_of_classes() {
    assert_reasoning_example("intersectionOfClassesWorks.ttl");
}

#[test]
fn reasoning_example_intersection_of_restrictions() {
    assert_reasoning_example("intersectionOfRestrictionsWorks.ttl");
}

#[test]
fn reasoning_example_some_values_example() {
    assert_reasoning_example("someValuesExample.ttl");
}

#[test]
fn reasoning_example_min_qualified() {
    assert_reasoning_example("minQualified.ttl");
}

#[test]
fn reasoning_example_min_qualified_simple_union() {
    assert_reasoning_example("minQualifiedSimpleUnion.ttl");
}

#[test]
fn reasoning_example_simple_union() {
    assert_reasoning_example("simpleUnion.ttl");
}

#[test]
fn reasoning_example_darling() {
    assert_reasoning_example("darlingExample.ttl");
}

#[test]
fn reasoning_example_qualified_cardinality_intersection() {
    assert_reasoning_example("qualifiedCardinalityIntersection.ttl");
}

// ── TestApiOntology.DescriptorFromImfOntologyNonCyclic ───────────────────────

/// Translated from `DescriptorFromImfOntologyNonCyclic`.
/// Verifies that loading cycle-imf-test.ttl and applying rules does not panic.
#[test]
fn descriptor_from_imf_ontology_non_cyclic() {
    let mut ds = Datastore::new(100_000);
    load_file(&mut ds, &testdata("cycle-imf-test.ttl")).unwrap();
    let ontology_doc = rdf2owl(&mut ds);
    assert!(
        !ontology_doc.ontology.axioms.is_empty(),
        "expected axioms from cycle-imf-test.ttl"
    );
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    assert!(
        !rules.is_empty(),
        "expected Datalog rules from cycle-imf-test.ttl"
    );
    // Must not panic during materialisation
    evaluate_rules(rules, &mut ds);
}

// ── TestApiOntology.MaxQualifiedCardinalityIsIgnored ─────────────────────────

/// Translated from `MaxQualifiedCardinalityIsIgnored`.
/// Loads minimal-loop-test.ttl (contains maxQualifiedCardinality) and verifies
/// that reasoning completes without panicking.
#[test]
fn max_qualified_cardinality_is_ignored() {
    let mut ds = Datastore::new(100_000);
    load_file(&mut ds, &testdata("minimal-loop-test.ttl")).unwrap();
    let ontology_doc = rdf2owl(&mut ds);
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    // Must not panic
    evaluate_rules(rules, &mut ds);
}

// ── TestApiOntology.DuplicateRulesWorks ──────────────────────────────────────

/// Translated from `DuplicateRulesWorks`.
///
/// duplicate_rules.datalog has the same rule twice.
/// The stratifier should deduplicate it, producing exactly 1 unique rule in output.
#[test]
fn duplicate_rules_are_deduplicated() {
    let mut ds = Datastore::new(10_000);
    let rules = datalog_parser::parse_file(&testdata("duplicate_rules.datalog"), &mut ds).unwrap();

    // The raw parse gives 2 rules (the file has the rule twice)
    assert_eq!(
        rules.len(),
        2,
        "parse should give 2 rules before deduplication"
    );

    // The stratifier deduplicates: unique(rules) should be 1
    let partitioner = datalog::stratifier::RulePartitioner::new(rules);
    let ordered = partitioner.order_rules();
    let total_unique: usize = ordered.iter().map(|stratum| stratum.len()).sum();
    assert_eq!(
        total_unique, 1,
        "stratifier should deduplicate to 1 unique rule, got {}",
        total_unique
    );
}

// ── TestApiOntology.LoadIDOOntologyWorks ─────────────────────────────────────

/// Translated from `LoadIDOOntologyWorks`.
///
/// LIS-14.ttl is the ISO 15926-14 (LIS) industrial ontology. Loading it should
/// extract axioms and apply reasoning without errors.
/// Marked ignore because it is a large file (~several MB).
#[test]
#[ignore = "large file (LIS-14.ttl) — run explicitly if available"]
fn load_ido_ontology_works() {
    let path = testdata("LIS-14.ttl");
    if !path.exists() {
        eprintln!("[SKIP] LIS-14.ttl not found");
        return;
    }
    let mut ds = Datastore::new(1_000_000);
    load_file(&mut ds, &path).unwrap();
    let ontology_doc = rdf2owl(&mut ds);
    assert!(
        !ontology_doc.ontology.axioms.is_empty(),
        "LIS-14.ttl should yield OWL axioms"
    );
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    assert!(!rules.is_empty(), "LIS-14.ttl should yield Datalog rules");
    evaluate_rules(rules, &mut ds);
}

// ── Tests that cannot be translated (not implemented) ────────────────────────
//
// TableauWorks / Imf2AlcWorks: the Tableau (ALC) reasoner is not implemented
// in the Rust project (alc_tableau crate deferred in docs/architecture/PLAN.md).
//
// ParseImfOntologyWorks / LoadImfOntologyWorks: require downloading the full
// IMF ontology — covered by the ignored tests in tests/performance.rs.
//
// TestSparqlSelectExpressions / TestSparqlSubquery: SELECT expressions and
// subqueries are not yet implemented in the SPARQL engine.

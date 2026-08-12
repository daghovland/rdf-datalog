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
use rdf_owl_translator::{TranslatorError, rdf2owl};
use std::path::Path;
use turtle::parse_turtle;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn load_and_extract_rules(name: &str) -> (Datastore, usize) {
    let mut ds = Datastore::new(500_000);
    load_file(&mut ds, &testdata(name)).expect("ontology must load");
    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let axiom_count = ontology_doc.ontology.axioms.len();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    let rule_count = rules.len();
    evaluate_rules(rules, &mut ds).unwrap();
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
    let ontology_doc = rdf2owl(&mut ds).unwrap();
    // File is empty so axiom count is 0; this just verifies no panic
    let _ = ontology_doc.ontology.axioms.len();
}

// ── TestApiOntology.LoadSubClassFromRestriction ───────────────────────────────

/// Translated from `LoadSubClassFromRestriction`.
#[test]
fn load_subclass_restriction_extracts_axioms() {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata("subclass_of_restriction.owl")).unwrap();
    let ontology_doc = rdf2owl(&mut ds).unwrap();
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
    let ontology_doc = rdf2owl(&mut ds).unwrap();
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
    let ontology_doc = rdf2owl(&mut ds).unwrap();
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
    evaluate_rules(rules, &mut ds).unwrap();
}

// ── TestApiOntology.MaxQualifiedCardinalityIsIgnored ─────────────────────────

/// Translated from `MaxQualifiedCardinalityIsIgnored`.
/// Loads minimal-loop-test.ttl (contains maxQualifiedCardinality) and verifies
/// that reasoning completes without panicking.
#[test]
fn max_qualified_cardinality_is_ignored() {
    let mut ds = Datastore::new(100_000);
    load_file(&mut ds, &testdata("minimal-loop-test.ttl")).unwrap();
    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    // Must not panic
    evaluate_rules(rules, &mut ds).unwrap();
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
    let ordered = partitioner.order_rules().unwrap();
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
    let ontology_doc = rdf2owl(&mut ds).unwrap();
    assert!(
        !ontology_doc.ontology.axioms.is_empty(),
        "LIS-14.ttl should yield OWL axioms"
    );
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    assert!(!rules.is_empty(), "LIS-14.ttl should yield Datalog rules");
    evaluate_rules(rules, &mut ds).unwrap();
}

// ── Regression tests for issue #298 ──────────────────────────────────────────
//
// `ObjectMaxCardinality(0, prop)` on a class in super-concept position used to
// discard `prop` entirely and translate `C ⊑ ≤0 R` as `C ⊑ ⊥` unconditionally
// — i.e. "no instance of C exists at all" rather than the correct "instances
// of C have no R-successors". This made the whole reasoning pipeline panic on
// *any* instance of such a class, even one with zero R-edges (which trivially
// satisfies "at most 0 R-successors").
//
// See <https://github.com/daghovland/rdf-datalog/issues/298>.

/// The exact non-violating repro from the issue: `ex:Leaf` has a
/// `maxCardinality 0` restriction on `ex:hasChild`, and `ex:n1 a ex:Leaf` has
/// NO `ex:hasChild` edges at all. This must succeed without panicking —
/// `ex:n1` trivially satisfies "at most 0 hasChild edges".
#[test]
fn maxcardinality_zero_without_violation_does_not_panic() {
    let (ds, rule_count) = load_and_extract_rules("maxcardinality0.ttl");
    assert!(
        rule_count > 0,
        "the maxCardinality 0 restriction should yield at least one Datalog rule"
    );

    const N1: &str = "http://example.com/ns#n1";
    const LEAF: &str = "http://example.com/ns#Leaf";
    let n1_is_leaf = !ds
        .quads_matching(
            None,
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    N1.to_string(),
                ))))
                .copied(),
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    RDF_TYPE.to_string(),
                ))))
                .copied(),
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    LEAF.to_string(),
                ))))
                .copied(),
        )
        .is_empty();
    assert!(
        n1_is_leaf,
        "ex:n1 should still be a valid, trivially-conforming ex:Leaf"
    );
}

/// The violating case: `ex:n2 a ex:Leaf` but `ex:n2` DOES have an
/// `ex:hasChild` edge, which genuinely violates the `maxCardinality 0`
/// restriction. This must still be detected as a contradiction.
///
/// The reasoner signals a genuine contradiction via
/// `Err(datalog::ReasoningError::Contradiction)` rather than a `panic!` —
/// see [#301](https://github.com/daghovland/rdf-datalog/issues/301). The
/// important thing this regression test guards is that the contradiction is
/// *still detected at all*: a naive fix for the non-violating case above
/// could easily regress into silently accepting genuine violations too.
#[test]
fn maxcardinality_zero_violation_is_still_detected() {
    let mut ds = Datastore::new(500_000);
    load_file(&mut ds, &testdata("maxcardinality0_violation.ttl")).expect("ontology must load");
    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    let result = evaluate_rules(rules, &mut ds);
    assert!(
        matches!(result, Err(datalog::ReasoningError::Contradiction(_))),
        "expected a Contradiction error, got {result:?}"
    );
}

// ── Malformed rdf:List handling ───────────────────────────────────────────────
//
// `rdf_owl_translator::get_rdf_list_elements` used to `panic!` on a
// structurally malformed `rdf:List` encoding (a node with the wrong number
// of `rdf:first`/`rdf:rest` triples, or a cycle) reachable from
// `owl:intersectionOf`/`owl:unionOf`/`owl:members`/property chains etc. —
// crashing the whole `--serve` process on a load of otherwise-syntactically-
// valid Turtle. It now returns `Err(TranslatorError::MalformedRdfList)`
// instead. See [#363](https://github.com/daghovland/rdf-datalog/issues/363).

/// An `owl:intersectionOf` list node with `rdf:first` but no `rdf:rest` at
/// all (instead of the required exactly-one `rdf:rest`, even to `rdf:nil`).
/// `rdf2owl` — the entry point a real `--ontology`/`--serve` load goes
/// through — must return a clean `Err` rather than crash.
#[test]
fn malformed_intersection_of_list_returns_err_not_panic() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix ex:   <http://example.org/> .

ex:A a owl:Class .
ex:B a owl:Class .

_:cls a owl:Class ;
    owl:intersectionOf _:list1 .

_:list1 rdf:first ex:A .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let result = rdf2owl(&mut ds);

    match result {
        Err(TranslatorError::MalformedRdfList(_)) => {}
        Ok(_) => panic!("expected Err(MalformedRdfList), got Ok"),
        Err(other) => panic!("expected Err(MalformedRdfList), got Err({other:?})"),
    }
}

/// An `owl:NamedIndividual` declaration whose subject is (malformed) a
/// literal rather than an IRI/blank node. `rdf2owl` must return a clean
/// `Err`, not panic. See <https://github.com/daghovland/rdf-datalog/issues/363>.
///
/// Turtle's grammar itself forbids a literal in subject position, so this
/// malformed shape can only arise from data built directly against the
/// `Datastore` API (e.g. a non-Turtle ingress path) rather than from a
/// `parse_turtle` fixture — the triples are constructed by hand here.
#[test]
fn named_individual_declaration_with_literal_subject_returns_err_not_panic() {
    const RDF_TYPE_IRI: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const OWL_NAMED_INDIVIDUAL_IRI: &str = "http://www.w3.org/2002/07/owl#NamedIndividual";

    let mut ds = Datastore::new(1_000);
    let subj = ds.add_literal_resource(dag_rdf::RdfLiteral::LiteralString(
        "not an individual".to_string(),
    ));
    let pred = ds.add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE_IRI.to_string())));
    let obj = ds.add_node_resource(RdfResource::Iri(IriReference(
        OWL_NAMED_INDIVIDUAL_IRI.to_string(),
    )));
    ds.add_triple(dag_rdf::ingress::Triple {
        subject: subj,
        predicate: pred,
        obj,
    });

    let result = rdf2owl(&mut ds);

    match result {
        Err(TranslatorError::InvalidIndividual(_)) => {}
        Ok(_) => panic!("expected Err(InvalidIndividual), got Ok"),
        Err(other) => panic!("expected Err(InvalidIndividual), got Err({other:?})"),
    }
}

/// An `owl:sameAs` axiom with a literal in individual position must return a
/// clean `Err`, not panic. See
/// <https://github.com/daghovland/rdf-datalog/issues/363>.
#[test]
fn same_as_with_literal_individual_returns_err_not_panic() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix ex:   <http://example.org/> .

ex:a owl:sameAs "not an individual" .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let result = rdf2owl(&mut ds);

    match result {
        Err(TranslatorError::InvalidIndividual(_)) => {}
        Ok(_) => panic!("expected Err(InvalidIndividual), got Ok"),
        Err(other) => panic!("expected Err(InvalidIndividual), got Err({other:?})"),
    }
}

// ── OWL 2 RL prp-spo1: rdfs:subPropertyOf propagation to ABox ────────────────
//
// See <https://github.com/daghovland/rdf-datalog/issues/451>.
//
// `object_property_axiom2datalog` used to silently drop
// `ObjectPropertyAxiom::SubObjectPropertyOf` (fell through to `_ => vec![]`),
// so a `P rdfs:subPropertyOf Q` axiom was never compiled into the datalog
// rule `Q[?s, ?o] :- P[?s, ?o]` (OWL 2 RL rule `prp-spo1`), and subproperty
// hierarchy never propagated to the ABox.

/// The exact reproducer from issue #451: `:hasTerminal rdfs:subPropertyOf
/// :adjacentTo`, plus data `:block1 :hasTerminal :terminal1`, must
/// materialise `:block1 :adjacentTo :terminal1`.
#[test]
fn sub_object_property_of_propagates_to_abox() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex:   <http://example.org/> .

ex:hasTerminal a owl:ObjectProperty ;
    rdfs:subPropertyOf ex:adjacentTo .
ex:adjacentTo a owl:ObjectProperty .

ex:block1 ex:hasTerminal ex:terminal1 .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    evaluate_rules(rules, &mut ds).unwrap();

    assert!(
        has_triple(
            &ds,
            "http://example.org/block1",
            "http://example.org/adjacentTo",
            "http://example.org/terminal1",
        ),
        "expected :block1 :adjacentTo :terminal1 to be derived via prp-spo1"
    );
}

/// This test exercises only the simple `SubObjectPropertyExpression` case
/// (`P rdfs:subPropertyOf Q`); `PropertyExpressionChain` sub-property axioms
/// (`prp-spo2`) are exercised separately below.
#[test]
fn sub_object_property_expression_does_not_regress_plain_case() {
    // Sanity check: a *reflexive-looking* but otherwise ordinary hierarchy
    // (P subPropertyOf P, i.e. self-subproperty) must not loop or panic.
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex:   <http://example.org/> .

ex:knows a owl:ObjectProperty ;
    rdfs:subPropertyOf ex:knows .

ex:a ex:knows ex:b .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    evaluate_rules(rules, &mut ds).unwrap();

    assert!(has_triple(
        &ds,
        "http://example.org/a",
        "http://example.org/knows",
        "http://example.org/b",
    ));
}

/// Adjacent gap fixed alongside #451: the data-property twin of prp-spo1.
/// `DataPropertyAxiom::SubDataPropertyOf` has no `PropertyExpressionChain`
/// equivalent (data properties can't be chained in OWL 2), so it's a
/// strictly simpler fix than the object-property case and was folded into
/// the same PR.
#[test]
fn sub_data_property_of_propagates_to_abox() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex:   <http://example.org/> .

ex:hasNickname a owl:DatatypeProperty ;
    rdfs:subPropertyOf ex:hasName .
ex:hasName a owl:DatatypeProperty .

ex:person1 ex:hasNickname "Al" .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    evaluate_rules(rules, &mut ds).unwrap();

    let s = ds
        .resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            "http://example.org/person1".to_string(),
        ))))
        .copied()
        .unwrap();
    let p = ds
        .resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            "http://example.org/hasName".to_string(),
        ))))
        .copied()
        .unwrap();
    assert!(
        !ds.quads_matching(None, Some(s), Some(p), None).is_empty(),
        "expected :person1 :hasName \"Al\" to be derived via prp-spo1 (data property variant)"
    );
}

// ── OWL 2 RL prp-spo2: property chain sub-property propagation ──────────────
//
// See <https://github.com/daghovland/rdf-datalog/issues/456>. Follow-up from
// #451/#455 above, which left `SubPropertyExpression::PropertyExpressionChain`
// unimplemented. `owl:propertyChainAxiom` (backed by an `rdf:List`) is the RDF
// syntax for a `PropertyExpressionChain` axiom, so these are genuine
// end-to-end Turtle tests, unlike the #455-era comment claiming no such
// parser path existed.

/// The issue's own worked example: `hasParent ∘ hasParent ⊑ hasGrandparent`
/// (chain length 2, same property repeated). `:a hasParent :b`, `:b hasParent
/// :c` must entail `:a hasGrandparent :c`.
#[test]
fn property_chain_of_length_two_propagates_to_abox() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix ex:   <http://example.org/> .

ex:hasGrandparent a owl:ObjectProperty ;
    owl:propertyChainAxiom ( ex:hasParent ex:hasParent ) .
ex:hasParent a owl:ObjectProperty .

ex:a ex:hasParent ex:b .
ex:b ex:hasParent ex:c .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    evaluate_rules(rules, &mut ds).unwrap();

    assert!(
        has_triple(
            &ds,
            "http://example.org/a",
            "http://example.org/hasGrandparent",
            "http://example.org/c",
        ),
        "expected :a :hasGrandparent :c to be derived via prp-spo2"
    );
    // The two-hop intermediate should not itself satisfy the grandparent
    // relation directly from :a (sanity check the join isn't accidentally
    // matching a single hop).
    assert!(!has_triple(
        &ds,
        "http://example.org/a",
        "http://example.org/hasGrandparent",
        "http://example.org/b",
    ));
}

/// A chain of length 1 (`PropertyExpressionChain(vec![P])`) degenerates to
/// the same shape as prp-spo1's simple case; the general n-ary loop must
/// handle n=1 correctly without special-casing.
#[test]
fn property_chain_of_length_one_degenerates_to_simple_subproperty() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix ex:   <http://example.org/> .

ex:adjacentTo a owl:ObjectProperty ;
    owl:propertyChainAxiom ( ex:hasTerminal ) .
ex:hasTerminal a owl:ObjectProperty .

ex:block1 ex:hasTerminal ex:terminal1 .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    evaluate_rules(rules, &mut ds).unwrap();

    assert!(
        has_triple(
            &ds,
            "http://example.org/block1",
            "http://example.org/adjacentTo",
            "http://example.org/terminal1",
        ),
        "expected a length-1 chain to behave like plain prp-spo1"
    );
}

/// A longer chain (length 3) with three distinct properties, to confirm
/// variable naming (`x0..xn`) doesn't collide across chain positions and the
/// join is a genuine multi-atom join, not just two hops.
#[test]
fn property_chain_of_length_three_with_distinct_properties() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix ex:   <http://example.org/> .

ex:connectsTo a owl:ObjectProperty ;
    owl:propertyChainAxiom ( ex:p1 ex:p2 ex:p3 ) .
ex:p1 a owl:ObjectProperty .
ex:p2 a owl:ObjectProperty .
ex:p3 a owl:ObjectProperty .

ex:n0 ex:p1 ex:n1 .
ex:n1 ex:p2 ex:n2 .
ex:n2 ex:p3 ex:n3 .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    evaluate_rules(rules, &mut ds).unwrap();

    assert!(
        has_triple(
            &ds,
            "http://example.org/n0",
            "http://example.org/connectsTo",
            "http://example.org/n3",
        ),
        "expected :n0 :connectsTo :n3 to be derived via a 3-atom prp-spo2 join"
    );
}

/// An empty chain (`owl:propertyChainAxiom ()`, i.e. the list is `rdf:nil`)
/// has no corresponding entailment (there is no `x0`/`xn` pair to relate) and
/// must not derive anything or panic.
#[test]
fn property_chain_empty_list_derives_nothing() {
    let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix ex:   <http://example.org/> .

ex:hasGrandparent a owl:ObjectProperty ;
    owl:propertyChainAxiom () .
ex:hasParent a owl:ObjectProperty .

ex:a ex:hasParent ex:b .
ex:b ex:hasParent ex:c .
"#;
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse should succeed");

    let ontology_doc = rdf2owl(&mut ds).unwrap();
    let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    evaluate_rules(rules, &mut ds).unwrap();

    assert!(!has_triple(
        &ds,
        "http://example.org/a",
        "http://example.org/hasGrandparent",
        "http://example.org/c",
    ));
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

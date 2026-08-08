/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for rdf_owl_translator.
//! Mirrors DagSemTools `TestApiOntology.cs`.

use dag_rdf::Datastore;
use owl_ontology::{Axiom, ClassAxiom, ClassExpression, Individual};
use rdf_owl_translator::rdf2owl;
use std::fs::File;
use std::io::BufReader;
use turtle::parse_turtle;

fn parse_and_translate(path: &str) -> Vec<Axiom> {
    let file = File::open(path).unwrap_or_else(|_| panic!("Cannot open {}", path));
    let reader = BufReader::new(file);
    let mut datastore = Datastore::new(100_000);
    parse_turtle(&mut datastore, reader).expect("Turtle parse failed");
    let doc = rdf2owl(&mut datastore).expect("rdf2owl should succeed on well-formed test fixture");
    doc.ontology.axioms
}

#[test]
fn translate_intersection_of_classes() {
    let axioms = parse_and_translate("tests/data/intersectionOfClassesWorks.ttl");
    assert!(
        !axioms.is_empty(),
        "Expected axioms from intersectionOfClassesWorks.ttl"
    );
    // Should contain at least a SubClassOf axiom and class declarations
    let has_subclass = axioms
        .iter()
        .any(|ax| matches!(ax, Axiom::AxiomClassAxiom(_)));
    assert!(has_subclass, "Expected at least one class axiom");
}

#[test]
fn translate_some_values_example() {
    let axioms = parse_and_translate("tests/data/someValuesExample.ttl");
    assert!(
        !axioms.is_empty(),
        "Expected axioms from someValuesExample.ttl"
    );
    let has_subclass = axioms
        .iter()
        .any(|ax| matches!(ax, Axiom::AxiomClassAxiom(_)));
    assert!(
        has_subclass,
        "Expected at least one class axiom (SubClassOf restriction)"
    );
}

#[test]
fn translate_owl_intersection() {
    let axioms = parse_and_translate("tests/data/intersection.owl.ttl");
    assert!(
        !axioms.is_empty(),
        "Expected axioms from intersection.owl.ttl"
    );
}

#[test]
fn translate_min_qualified_cardinality() {
    let axioms = parse_and_translate("tests/data/minQualified.ttl");
    assert!(!axioms.is_empty(), "Expected axioms from minQualified.ttl");
}

#[test]
fn translate_simple_union() {
    let axioms = parse_and_translate("tests/data/simpleUnion.ttl");
    assert!(!axioms.is_empty(), "Expected axioms from simpleUnion.ttl");
}

#[test]
fn translate_some_values_from_inverse() {
    let axioms = parse_and_translate("tests/data/someValuesFromInverse.ttl");
    assert!(
        !axioms.is_empty(),
        "Expected axioms from someValuesFromInverse.ttl"
    );
}

/// Regression test for #363: `owl:hasSelf "1"^^xsd:boolean` (the XSD-legal
/// but non-canonical lexical form for `true`) must be accepted by
/// `try_get_bool_literal` and resolve to `ObjectHasSelf`, not silently fall
/// back to `owl:Thing` (which is what happens when the value can't be
/// parsed as boolean) and must not panic the process.
#[test]
fn translate_has_self_numeric_lexical_form() {
    let axioms = parse_and_translate("tests/data/hasSelfNumericLexicalForm.ttl");
    assert!(
        !axioms.is_empty(),
        "Expected axioms from hasSelfNumericLexicalForm.ttl"
    );
    let has_self_restriction = axioms.iter().any(|ax| {
        matches!(
            ax,
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                _,
                _,
                ClassExpression::ObjectHasSelf(_)
            ))
        )
    });
    assert!(
        has_self_restriction,
        "Expected an ObjectHasSelf restriction from owl:hasSelf \"1\"^^xsd:boolean, got: {:?}",
        axioms
    );
}

/// Regression test for #363: two blank-node `owl:intersectionOf` class
/// expressions whose member lists reference each other form a cycle in the
/// anonymous-class-expression dependency graph. `rdf2owl` must return a
/// clean `Err` instead of panicking the process.
#[test]
fn translate_cyclic_intersection_of_classes_returns_err() {
    let file = File::open("tests/data/cyclicIntersectionOfClasses.ttl")
        .expect("Cannot open tests/data/cyclicIntersectionOfClasses.ttl");
    let reader = BufReader::new(file);
    let mut datastore = Datastore::new(1_000);
    parse_turtle(&mut datastore, reader).expect("Turtle parse failed");

    let result = rdf2owl(&mut datastore);
    assert!(
        result.is_err(),
        "Expected rdf2owl to return Err on a cyclic class-expression dependency graph, got {:?}",
        result.map(|doc| doc.ontology.axioms)
    );
}

/// Fix coverage for #363: an `owl:oneOf` list containing one literal among
/// otherwise-valid individual members must not panic the translation. The
/// malformed member is skipped (with a `log::warn!`, not mechanically
/// asserted here — this crate has no log-capturing test utility) and the
/// resulting `ObjectOneOf` contains only the valid individuals.
#[test]
fn translate_one_of_skips_malformed_member() {
    let axioms = parse_and_translate("tests/data/oneOfWithMalformedMember.ttl");
    let one_of = axioms.iter().find_map(|ax| match ax {
        Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(_, ces)) => {
            ces.iter().find_map(|ce| match ce {
                ClassExpression::ObjectOneOf(individuals) => Some(individuals),
                _ => None,
            })
        }
        _ => None,
    });
    let individuals = one_of.unwrap_or_else(|| {
        panic!(
            "Expected an ObjectOneOf class expression, got: {:?}",
            axioms
        )
    });
    assert_eq!(
        individuals.len(),
        2,
        "Expected the malformed literal member to be skipped, got: {:?}",
        individuals
    );
    assert!(
        individuals
            .iter()
            .all(|ind| matches!(ind, Individual::NamedIndividual(_)))
    );
}

/// Fix coverage for #363: an `owl:oneOf` list whose *only* member is
/// malformed must not silently collapse to an empty `ObjectOneOf` (which
/// denotes the empty class — a much bigger silent semantic change than
/// skipping one bad member out of several). It falls back to `owl:Thing`
/// instead, with a `log::warn!` (not mechanically asserted here).
#[test]
fn translate_one_of_falls_back_to_owl_thing_when_all_members_malformed() {
    let axioms = parse_and_translate("tests/data/oneOfAllMembersMalformed.ttl");
    let falls_back_to_thing = axioms.iter().any(|ax| match ax {
        Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(_, ces)) => {
            ces.iter().any(|ce| match ce {
                ClassExpression::ClassName(class) => {
                    class.0.0 == "http://www.w3.org/2002/07/owl#Thing"
                }
                _ => false,
            })
        }
        _ => false,
    });
    assert!(
        falls_back_to_thing,
        "Expected the all-malformed owl:oneOf to fall back to owl:Thing, got: {:?}",
        axioms
    );
    let has_empty_one_of = axioms.iter().any(|ax| match ax {
        Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(_, ces)) => ces
            .iter()
            .any(|ce| matches!(ce, ClassExpression::ObjectOneOf(v) if v.is_empty())),
        _ => false,
    });
    assert!(
        !has_empty_one_of,
        "Did not expect an empty ObjectOneOf, got: {:?}",
        axioms
    );
}

/// Regression test for #363: an ordinary, well-formed `owl:oneOf` still
/// produces an `ObjectOneOf` with all its members, unaffected by the fix to
/// `try_get_individual`.
#[test]
fn translate_one_of_well_formed() {
    let axioms = parse_and_translate("tests/data/oneOfWellFormed.ttl");
    let one_of = axioms.iter().find_map(|ax| match ax {
        Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(_, ces)) => {
            ces.iter().find_map(|ce| match ce {
                ClassExpression::ObjectOneOf(individuals) => Some(individuals),
                _ => None,
            })
        }
        _ => None,
    });
    let individuals = one_of.unwrap_or_else(|| {
        panic!(
            "Expected an ObjectOneOf class expression, got: {:?}",
            axioms
        )
    });
    assert_eq!(individuals.len(), 2, "Expected both members present");
}

/// Fix coverage for #363: an `owl:hasValue` restriction on an object
/// property whose `owl:hasValue` object is (malformed) a literal must not
/// panic the translation. The restriction falls back to `owl:Thing` for that
/// class expression (with a `log::warn!`, not mechanically asserted here).
#[test]
fn translate_has_value_falls_back_to_owl_thing_on_malformed_object() {
    let axioms = parse_and_translate("tests/data/hasValueMalformedObjectProperty.ttl");
    let falls_back_to_thing = axioms.iter().any(|ax| match ax {
        Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(_, _, ClassExpression::ClassName(class))) => {
            class.0.0 == "http://www.w3.org/2002/07/owl#Thing"
        }
        _ => false,
    });
    assert!(
        falls_back_to_thing,
        "Expected the malformed owl:hasValue restriction to fall back to owl:Thing, got: {:?}",
        axioms
    );
    let has_value_restriction = axioms.iter().any(|ax| {
        matches!(
            ax,
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                _,
                _,
                ClassExpression::ObjectHasValue(_, _)
            ))
        )
    });
    assert!(
        !has_value_restriction,
        "Did not expect an ObjectHasValue restriction from a malformed owl:hasValue object, got: {:?}",
        axioms
    );
}

/// Regression test for #363: an ordinary, well-formed `owl:hasValue`
/// restriction on an object property still produces an `ObjectHasValue`
/// restriction, unaffected by the fix to `try_get_individual`.
#[test]
fn translate_has_value_well_formed() {
    let axioms = parse_and_translate("tests/data/hasValueWellFormed.ttl");
    let has_value_restriction = axioms.iter().any(|ax| {
        matches!(
            ax,
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                _,
                _,
                ClassExpression::ObjectHasValue(_, _)
            ))
        )
    });
    assert!(
        has_value_restriction,
        "Expected an ObjectHasValue restriction, got: {:?}",
        axioms
    );
}

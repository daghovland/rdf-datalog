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
use owl_ontology::{Axiom, ClassAxiom, ClassExpression};
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

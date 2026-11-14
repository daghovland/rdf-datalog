/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Cross-syntax round-trip test (issue
//! [#749](https://github.com/daghovland/rdf-datalog/issues/749)):
//!
//! Both OWL 2 Functional-Style Syntax (`owl_functional_parser`, epic
//! [#182](https://github.com/daghovland/rdf-datalog/issues/182), closed) and
//! Manchester Syntax (`manchester_parser`, epic
//! [#178](https://github.com/daghovland/rdf-datalog/issues/178)) parse and
//! serialize the *same* `owl_ontology::Ontology` type. This test checks that
//! an ontology survives being parsed in one syntax, serialized in the
//! *other*, and re-parsed — in both directions:
//!
//! - Manchester text -> `Ontology` -> Functional-Style text -> `Ontology`
//! - Functional-Style text -> `Ontology` -> Manchester text -> `Ontology`
//!
//! Comparison is axiom-set equality (`HashSet<Axiom>`), the same bar used by
//! the existing same-syntax round-trip suites this test mirrors:
//! - `owl_functional_parser/tests/serialize_roundtrip.rs`
//! - `manchester_parser/tests/serialize_roundtrip.rs`
//!
//! Since this crate lives at the top level of the workspace (it exercises
//! two sibling crates, so it doesn't naturally belong inside either one's own
//! `tests/` dir), it follows the convention already used by
//! `tests/manchester_roundtrip.rs` for the same reason.
//!
//! The fixture below deliberately has no anonymous individuals: an anonymous
//! individual's numeric id is assigned by first-occurrence order within
//! `ontology.axioms`, and the two serializers group/order axioms differently
//! (Manchester's serializer groups axioms by frame subject in
//! first-occurrence order; the Functional-Style serializer emits
//! `ontology.axioms` as-is, one `Keyword(...)` per axiom, no grouping pass).
//! A fixture with more than one anonymous individual could see its ids
//! legitimately renumbered differently by each serializer's grouping, which
//! would require the same kind of renumbering-before-comparison machinery
//! `tests/manchester_roundtrip.rs` uses for *its* multi-anonymous-individual
//! case — out of scope for this issue's "representative ontology fixture"
//! ask, so the fixture here stays within the no-anonymous-individual range
//! both existing serializer-only round-trip suites also use for their own
//! richest "full document" fixtures.

use owl_ontology::Axiom;
use std::collections::HashSet;

fn axiom_set(axioms: &[Axiom]) -> HashSet<Axiom> {
    axioms.iter().cloned().collect()
}

/// A representative multi-construct ontology, written once and parsed by
/// both syntaxes' own fixture dialects below (the two functions'
/// `MANCHESTER`/`FUNCTIONAL` constants describe the *same* ontology, which
/// is what makes a cross-syntax comparison meaningful). Covers: class
/// declarations, `SubClassOf`/`EquivalentClasses`/`DisjointClasses`, object
/// and data property declarations with domain/range/characteristics/
/// sub-property/equivalent/inverse, a nested class expression
/// (intersection + existential restriction), and ABox assertions (class,
/// object property, data property, same/different individuals).
mod fixture {
    pub const MANCHESTER: &str = r#"
        Prefix: : <http://example.org/onto#>
        Prefix: xsd: <http://www.w3.org/2001/XMLSchema#>
        Ontology: <http://example.org/onto>

        Class: Food

        Class: Pizza
            SubClassOf: Food
            EquivalentTo: Food and (hasTopping some Topping)
            DisjointWith: Drink

        Class: Drink
        Class: Topping

        ObjectProperty: hasBaseTopping

        ObjectProperty: hasTopping
            Domain: Pizza
            Range: Topping
            SubPropertyOf: hasBaseTopping
            Characteristics: InverseFunctional

        ObjectProperty: isToppingOf
            InverseOf: hasTopping

        DataProperty: hasAge
            Domain: Person
            Range: xsd:integer
            Characteristics: Functional

        Class: Person

        Individual: Margherita
            Types: Pizza
            Facts: hasTopping Mozzarella

        Individual: Mozzarella
            Types: Topping

        Individual: Alice
            Types: Person
            Facts: hasAge "30"^^xsd:integer
            SameAs: Alicia
            DifferentFrom: Bob

        Individual: Alicia
        Individual: Bob
    "#;

    pub const FUNCTIONAL: &str = r#"
        Ontology(<http://example.org/onto>
            Declaration(Class(<http://example.org/onto#Food>))
            Declaration(Class(<http://example.org/onto#Pizza>))
            Declaration(Class(<http://example.org/onto#Drink>))
            Declaration(Class(<http://example.org/onto#Topping>))
            Declaration(Class(<http://example.org/onto#Person>))
            Declaration(ObjectProperty(<http://example.org/onto#hasBaseTopping>))
            Declaration(ObjectProperty(<http://example.org/onto#hasTopping>))
            Declaration(ObjectProperty(<http://example.org/onto#isToppingOf>))
            Declaration(DataProperty(<http://example.org/onto#hasAge>))
            Declaration(NamedIndividual(<http://example.org/onto#Margherita>))
            Declaration(NamedIndividual(<http://example.org/onto#Mozzarella>))
            Declaration(NamedIndividual(<http://example.org/onto#Alice>))
            Declaration(NamedIndividual(<http://example.org/onto#Alicia>))
            Declaration(NamedIndividual(<http://example.org/onto#Bob>))

            SubClassOf(<http://example.org/onto#Pizza> <http://example.org/onto#Food>)
            EquivalentClasses(<http://example.org/onto#Pizza> ObjectIntersectionOf(<http://example.org/onto#Food> ObjectSomeValuesFrom(<http://example.org/onto#hasTopping> <http://example.org/onto#Topping>)))
            DisjointClasses(<http://example.org/onto#Pizza> <http://example.org/onto#Drink>)

            ObjectPropertyDomain(<http://example.org/onto#hasTopping> <http://example.org/onto#Pizza>)
            ObjectPropertyRange(<http://example.org/onto#hasTopping> <http://example.org/onto#Topping>)
            SubObjectPropertyOf(<http://example.org/onto#hasTopping> <http://example.org/onto#hasBaseTopping>)
            InverseFunctionalObjectProperty(<http://example.org/onto#hasTopping>)
            InverseObjectProperties(<http://example.org/onto#isToppingOf> <http://example.org/onto#hasTopping>)

            DataPropertyDomain(<http://example.org/onto#hasAge> <http://example.org/onto#Person>)
            DataPropertyRange(<http://example.org/onto#hasAge> <http://www.w3.org/2001/XMLSchema#integer>)
            FunctionalDataProperty(<http://example.org/onto#hasAge>)

            ClassAssertion(<http://example.org/onto#Pizza> <http://example.org/onto#Margherita>)
            ObjectPropertyAssertion(<http://example.org/onto#hasTopping> <http://example.org/onto#Margherita> <http://example.org/onto#Mozzarella>)
            ClassAssertion(<http://example.org/onto#Topping> <http://example.org/onto#Mozzarella>)
            ClassAssertion(<http://example.org/onto#Person> <http://example.org/onto#Alice>)
            DataPropertyAssertion(<http://example.org/onto#hasAge> <http://example.org/onto#Alice> "30"^^<http://www.w3.org/2001/XMLSchema#integer>)
            SameIndividual(<http://example.org/onto#Alice> <http://example.org/onto#Alicia>)
            DifferentIndividuals(<http://example.org/onto#Alice> <http://example.org/onto#Bob>)
        )
    "#;
}

/// Parse `MANCHESTER` via `manchester_parser`, serialize via
/// `owl_functional_parser::serialize`, re-parse via `owl_functional_parser`,
/// and compare axiom sets against the independently-hand-written
/// `FUNCTIONAL` fixture describing the same ontology — which doubles as the
/// "would this actually fail if something were broken" sanity check: the two
/// fixtures are written independently (different concrete syntaxes, by
/// hand), so for the test to pass at all, `manchester_parser::parse`,
/// `owl_functional_parser::serialize`, and `owl_functional_parser::parse`
/// must all agree on the same `Ontology` structure.
#[test]
fn manchester_to_functional_roundtrip_preserves_axioms() {
    let from_manchester = manchester_parser::parse(fixture::MANCHESTER)
        .unwrap_or_else(|e| panic!("Manchester parse failed: {e}"));

    let functional_text = owl_functional_parser::serialize(&from_manchester);
    let reparsed_as_functional = owl_functional_parser::parse(&functional_text).unwrap_or_else(
        |e| panic!("re-parsing Manchester-derived Functional-Style text failed: {e}\n---\n{functional_text}"),
    );

    let expected = owl_functional_parser::parse(fixture::FUNCTIONAL)
        .unwrap_or_else(|e| panic!("hand-written Functional-Style fixture failed to parse: {e}"));

    let original_set = axiom_set(&from_manchester.axioms);
    let round_tripped_set = axiom_set(&reparsed_as_functional.axioms);
    let expected_set = axiom_set(&expected.axioms);

    assert_eq!(
        original_set, round_tripped_set,
        "axiom sets differ after Manchester -> Functional-Style -> re-parse round trip; \
         serialized Functional-Style text was:\n{functional_text}"
    );
    assert_eq!(
        round_tripped_set, expected_set,
        "Manchester-derived axiom set does not match the independently-authored \
         Functional-Style fixture describing the same ontology"
    );
}

/// The reverse direction: parse `FUNCTIONAL` via `owl_functional_parser`,
/// serialize via `manchester_parser::serialize`, re-parse via
/// `manchester_parser`, and compare axiom sets against the independently
/// hand-written `MANCHESTER` fixture.
#[test]
fn functional_to_manchester_roundtrip_preserves_axioms() {
    let from_functional = owl_functional_parser::parse(fixture::FUNCTIONAL)
        .unwrap_or_else(|e| panic!("Functional-Style parse failed: {e}"));

    let manchester_text = manchester_parser::serialize(&from_functional);
    let reparsed_as_manchester = manchester_parser::parse(&manchester_text).unwrap_or_else(|e| {
        panic!(
            "re-parsing Functional-Style-derived Manchester text failed: {e}\n---\n{manchester_text}"
        )
    });

    let expected = manchester_parser::parse(fixture::MANCHESTER)
        .unwrap_or_else(|e| panic!("hand-written Manchester fixture failed to parse: {e}"));

    let original_set = axiom_set(&from_functional.axioms);
    let round_tripped_set = axiom_set(&reparsed_as_manchester.axioms);
    let expected_set = axiom_set(&expected.axioms);

    assert_eq!(
        original_set, round_tripped_set,
        "axiom sets differ after Functional-Style -> Manchester -> re-parse round trip; \
         serialized Manchester text was:\n{manchester_text}"
    );
    assert_eq!(
        round_tripped_set, expected_set,
        "Functional-Style-derived axiom set does not match the independently-authored \
         Manchester fixture describing the same ontology"
    );
}

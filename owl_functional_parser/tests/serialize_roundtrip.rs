/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Round-trip tests for `owl_functional_parser::serialize` (issue
//! [#181](https://github.com/daghovland/rdf-datalog/issues/181)).
//!
//! Pattern, per `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`'s
//! "Serialiser" section: parse a `.ofn` snippet, serialize the resulting
//! `Ontology`, re-parse the serialized text, and compare the *axiom set* (a
//! `HashSet<Axiom>`) of the original and the round-tripped `Ontology` — not
//! the exact text, since `owl_ontology::Axiom` doesn't guarantee any
//! particular literal formatting round-trips byte-for-byte (e.g. numeric
//! literal lexical forms). `Ontology` carries no `PartialEq`/`Eq` of its own,
//! so the comparison is built from `ontology.axioms` (the built-in
//! declarations from `all_axioms()` are deliberately excluded — they are not
//! serialized, and both sides would otherwise trivially agree on them
//! anyway).
//!
//! Unlike `manchester_parser`'s equivalent suite, axiom order (and therefore
//! anonymous-individual first-occurrence order) is preserved exactly by this
//! serializer (no entity-grouping pass — see the plan doc), so fixtures with
//! more than one anonymous individual are not avoided here.
//!
//! Tests are `#[ignore]` pending implementation, per this repo's TDD
//! protocol (`CLAUDE.md`): unignore one at a time.

use owl_ontology::Axiom;
use std::collections::HashSet;

/// Parses `input`, serializes the result, re-parses the serialization, and
/// asserts the axiom sets of the original and round-tripped ontologies are
/// equal. Returns the serialized text so callers can add extra assertions.
fn assert_roundtrip(input: &str) -> String {
    let onto = owl_functional_parser::parse(input).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let original: HashSet<Axiom> = onto.axioms.iter().cloned().collect();
    let text = owl_functional_parser::serialize(&onto);
    let reparsed = owl_functional_parser::parse(&text)
        .unwrap_or_else(|e| panic!("re-parse of serialized output failed: {e}\n---\n{text}"));
    let round_tripped: HashSet<Axiom> = reparsed.axioms.into_iter().collect();
    assert_eq!(
        original, round_tripped,
        "axiom sets differ after round-trip; serialized text was:\n{text}"
    );
    text
}

// ── Header ────────────────────────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_empty_unnamed_ontology() {
    assert_roundtrip("Ontology()");
}

#[test]
#[ignore] // #181
fn roundtrips_named_ontology_with_version_iri() {
    let text = assert_roundtrip(
        "Ontology(<http://example.org/onto> <http://example.org/onto/1.0.0>)",
    );
    assert!(text.contains("http://example.org/onto"));
}

#[test]
#[ignore] // #181
fn roundtrips_ontology_with_import() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             Import(<http://example.org/other>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_ontology_level_annotation() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             Annotation(<http://www.w3.org/2000/01/rdf-schema#label> \"My Ontology\")\n\
         )",
    );
}

// ── Declarations ─────────────────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_all_six_declaration_kinds() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             Declaration(Class(<http://example.org/Pizza>))\n\
             Declaration(Datatype(<http://example.org/MyDatatype>))\n\
             Declaration(ObjectProperty(<http://example.org/hasTopping>))\n\
             Declaration(DataProperty(<http://example.org/hasAge>))\n\
             Declaration(AnnotationProperty(<http://example.org/comment>))\n\
             Declaration(NamedIndividual(<http://example.org/fido>))\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_declaration_with_annotation() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             Declaration(Annotation(<http://www.w3.org/2000/01/rdf-schema#label> \"Pizza\") Class(<http://example.org/Pizza>))\n\
         )",
    );
}

// ── Class expressions (via SubClassOf) ──────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_intersection_union_complement() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubClassOf(<http://example.org/Pizza> ObjectIntersectionOf(<http://example.org/Food> ObjectComplementOf(<http://example.org/Drink>)))\n\
             SubClassOf(<http://example.org/Pizza> ObjectUnionOf(<http://example.org/Food> <http://example.org/Drink>))\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_object_one_of() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubClassOf(<http://example.org/Weekday> ObjectOneOf(<http://example.org/Mon> <http://example.org/Tue>))\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_object_restrictions() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubClassOf(<http://example.org/Pizza> ObjectSomeValuesFrom(<http://example.org/hasTopping> <http://example.org/Topping>))\n\
             SubClassOf(<http://example.org/Pizza> ObjectAllValuesFrom(<http://example.org/hasTopping> <http://example.org/Topping>))\n\
             SubClassOf(<http://example.org/Pizza> ObjectHasValue(<http://example.org/hasTopping> <http://example.org/Mozzarella>))\n\
             SubClassOf(<http://example.org/Pizza> ObjectHasSelf(<http://example.org/likes>))\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_object_cardinalities_qualified_and_unqualified() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubClassOf(<http://example.org/Pizza> ObjectMinCardinality(1 <http://example.org/hasTopping>))\n\
             SubClassOf(<http://example.org/Pizza> ObjectMaxCardinality(3 <http://example.org/hasTopping> <http://example.org/Topping>))\n\
             SubClassOf(<http://example.org/Pizza> ObjectExactCardinality(2 <http://example.org/hasTopping>))\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_data_restrictions() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubClassOf(<http://example.org/Adult> DataSomeValuesFrom(<http://example.org/hasAge> <http://www.w3.org/2001/XMLSchema#integer>))\n\
             SubClassOf(<http://example.org/Adult> DataHasValue(<http://example.org/hasAge> \"42\"^^<http://www.w3.org/2001/XMLSchema#integer>))\n\
             SubClassOf(<http://example.org/Adult> DataMinCardinality(1 <http://example.org/hasAge>))\n\
             SubClassOf(<http://example.org/Adult> DataMaxQualifiedCardinality(3 <http://example.org/hasAge> <http://www.w3.org/2001/XMLSchema#integer>))\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_nested_class_expression() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             EquivalentClasses(<http://example.org/Pizza> ObjectIntersectionOf(<http://example.org/Food> ObjectSomeValuesFrom(<http://example.org/hasTopping> <http://example.org/Topping>)))\n\
         )",
    );
}

// ── Class axioms ─────────────────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_subclassof_with_annotation() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubClassOf(Annotation(<http://www.w3.org/2000/01/rdf-schema#label> \"why\") <http://example.org/Dog> <http://example.org/Animal>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_equivalent_and_disjoint_classes_nary() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             EquivalentClasses(<http://example.org/Pizza> <http://example.org/Food> <http://example.org/Meal>)\n\
             DisjointClasses(<http://example.org/Pizza> <http://example.org/Drink> <http://example.org/Dessert>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_disjoint_union() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             DisjointUnion(<http://example.org/Animal> <http://example.org/Dog> <http://example.org/Cat>)\n\
         )",
    );
}

// ── Object property axioms ──────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_object_property_domain_range() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             ObjectPropertyDomain(<http://example.org/hasTopping> <http://example.org/Pizza>)\n\
             ObjectPropertyRange(<http://example.org/hasTopping> <http://example.org/Topping>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_object_property_characteristics() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             FunctionalObjectProperty(<http://example.org/hasTopping>)\n\
             InverseFunctionalObjectProperty(<http://example.org/hasTopping>)\n\
             ReflexiveObjectProperty(<http://example.org/hasTopping>)\n\
             IrreflexiveObjectProperty(<http://example.org/hasTopping>)\n\
             SymmetricObjectProperty(<http://example.org/hasTopping>)\n\
             AsymmetricObjectProperty(<http://example.org/hasTopping>)\n\
             TransitiveObjectProperty(<http://example.org/hasTopping>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_sub_object_property_of() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubObjectPropertyOf(<http://example.org/hasDog> <http://example.org/hasPet>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_sub_object_property_of_with_chain() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubObjectPropertyOf(ObjectPropertyChain(<http://example.org/hasParent> <http://example.org/hasParent>) <http://example.org/hasGrandparent>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_equivalent_and_disjoint_object_properties() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             EquivalentObjectProperties(<http://example.org/hasTopping> <http://example.org/hasIngredient>)\n\
             DisjointObjectProperties(<http://example.org/hasTopping> <http://example.org/hasCrust>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_inverse_object_properties_and_expression() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             InverseObjectProperties(<http://example.org/hasTopping> <http://example.org/isToppingOf>)\n\
             SubObjectPropertyOf(<http://example.org/isBaseOf> ObjectInverseOf(<http://example.org/hasBase>))\n\
         )",
    );
}

// ── Data property axioms + DatatypeDefinition ───────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_data_property_axioms() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubDataPropertyOf(<http://example.org/hasBaseAge> <http://example.org/hasAge>)\n\
             EquivalentDataProperties(<http://example.org/hasAge> <http://example.org/hasYears>)\n\
             DisjointDataProperties(<http://example.org/hasAge> <http://example.org/hasName>)\n\
             DataPropertyDomain(<http://example.org/hasAge> <http://example.org/Person>)\n\
             DataPropertyRange(<http://example.org/hasAge> <http://www.w3.org/2001/XMLSchema#integer>)\n\
             FunctionalDataProperty(<http://example.org/hasAge>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_datatype_definition() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             DatatypeDefinition(<http://example.org/AdultAge> <http://www.w3.org/2001/XMLSchema#integer>)\n\
         )",
    );
}

// ── Compound data ranges ─────────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_compound_data_ranges() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             DataPropertyRange(<http://example.org/hasAge> DataIntersectionOf(<http://www.w3.org/2001/XMLSchema#integer> DataComplementOf(<http://www.w3.org/2001/XMLSchema#negativeInteger>)))\n\
             DataPropertyRange(<http://example.org/hasGrade> DataOneOf(\"A\" \"B\" \"C\"))\n\
             DataPropertyRange(<http://example.org/hasScore> DatatypeRestriction(<http://www.w3.org/2001/XMLSchema#integer> <http://www.w3.org/2001/XMLSchema#minInclusive> \"0\" <http://www.w3.org/2001/XMLSchema#maxInclusive> \"100\"))\n\
         )",
    );
}

// ── HasKey ───────────────────────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_has_key() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             HasKey(<http://example.org/Person> (<http://example.org/hasSSN>) (<http://example.org/hasName>))\n\
         )",
    );
}

// ── Assertions ───────────────────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_all_assertion_kinds() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             ClassAssertion(<http://example.org/Dog> <http://example.org/fido>)\n\
             ObjectPropertyAssertion(<http://example.org/hasPet> <http://example.org/alice> <http://example.org/fido>)\n\
             NegativeObjectPropertyAssertion(<http://example.org/hasPet> <http://example.org/alice> <http://example.org/rex>)\n\
             DataPropertyAssertion(<http://example.org/hasAge> <http://example.org/alice> \"30\")\n\
             NegativeDataPropertyAssertion(<http://example.org/hasAge> <http://example.org/alice> \"5\")\n\
             SameIndividual(<http://example.org/alice> <http://example.org/alicia>)\n\
             DifferentIndividuals(<http://example.org/alice> <http://example.org/bob>)\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_anonymous_individuals() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             ClassAssertion(<http://example.org/Person> _:x)\n\
             ClassAssertion(<http://example.org/Person> _:y)\n\
             ObjectPropertyAssertion(<http://example.org/knows> _:x _:y)\n\
         )",
    );
}

// ── Annotation axioms ────────────────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_annotation_assertion_iri_and_literal_and_anon_subject() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             AnnotationAssertion(<http://www.w3.org/2000/01/rdf-schema#label> <http://example.org/Pizza> \"Pizza\")\n\
             AnnotationAssertion(<http://www.w3.org/2000/01/rdf-schema#seeAlso> <http://example.org/Pizza> <http://example.org/Food>)\n\
             AnnotationAssertion(<http://www.w3.org/2000/01/rdf-schema#label> _:x \"Anon\")\n\
         )",
    );
}

#[test]
#[ignore] // #181
fn roundtrips_sub_annotation_property_of_domain_and_range() {
    assert_roundtrip(
        "Ontology(<http://example.org/onto>\n\
             SubAnnotationPropertyOf(<http://example.org/myAnnotation> <http://www.w3.org/2000/01/rdf-schema#label>)\n\
             AnnotationPropertyDomain(<http://example.org/myAnnotation> <http://example.org/Pizza>)\n\
             AnnotationPropertyRange(<http://example.org/myAnnotation> <http://example.org/Food>)\n\
         )",
    );
}

// ── Full-document integration ───────────────────────────────────────────

#[test]
#[ignore] // #181
fn roundtrips_pizza_style_multi_axiom_ontology() {
    assert_roundtrip(
        "Ontology(<http://example.org/pizza> <http://example.org/pizza/1.0>\n\
             Declaration(Class(<http://example.org/Pizza>))\n\
             Declaration(Class(<http://example.org/Food>))\n\
             Declaration(ObjectProperty(<http://example.org/hasTopping>))\n\
             SubClassOf(<http://example.org/Pizza> <http://example.org/Food>)\n\
             EquivalentClasses(<http://example.org/Pizza> ObjectIntersectionOf(<http://example.org/Food> ObjectSomeValuesFrom(<http://example.org/hasTopping> <http://example.org/Topping>)))\n\
             ObjectPropertyDomain(<http://example.org/hasTopping> <http://example.org/Pizza>)\n\
             ObjectPropertyRange(<http://example.org/hasTopping> <http://example.org/Topping>)\n\
             InverseFunctionalObjectProperty(<http://example.org/hasTopping>)\n\
             ClassAssertion(<http://example.org/Pizza> <http://example.org/Margherita>)\n\
             DataPropertyAssertion(<http://example.org/hasCalories> <http://example.org/Margherita> \"250\"^^<http://www.w3.org/2001/XMLSchema#integer>)\n\
         )",
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Round-trip tests for `manchester_parser::serialize` (issue
//! [#160](https://github.com/daghovland/rdf-datalog/issues/160)).
//!
//! Pattern, per `docs/plans/MANCHESTER_SYNTAX_PLAN.md`'s ask: parse a `.omn`
//! snippet, serialize the resulting `Ontology`, re-parse the serialized text,
//! and compare the *axiom set* (a `HashSet<Axiom>`) of the original and the
//! round-tripped `Ontology` — not the exact text, since frame
//! grouping/ordering is not guaranteed to survive serialization. `Ontology`
//! carries no `PartialEq`/`Eq` of its own, so the comparison is built from
//! `ontology.axioms` (the built-in declarations from `all_axioms()` are
//! deliberately excluded — they are not serialized, and both sides would
//! otherwise trivially agree on them anyway).
//!
//! Anonymous-individual ids (`Individual::AnonymousIndividual(u32)`) are
//! assigned by first-occurrence order during parsing; the serializer walks
//! axioms in their original order so ids are expected to line up. Fixtures
//! with more than one anonymous individual are avoided here to sidestep
//! relying on that ordering guarantee across a grouped/reordered emission.

use owl_ontology::Axiom;
use std::collections::HashSet;

/// Parses `input`, serializes the result, re-parses the serialization, and
/// asserts the axiom sets of the original and round-tripped ontologies are
/// equal. Returns the serialized text so callers can add extra assertions.
fn assert_roundtrip(input: &str) -> String {
    let onto = manchester_parser::parse(input).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let original: HashSet<Axiom> = onto.axioms.iter().cloned().collect();
    let text = manchester_parser::serialize(&onto);
    let reparsed = manchester_parser::parse(&text)
        .unwrap_or_else(|e| panic!("re-parse of serialized output failed: {e}\n---\n{text}"));
    let round_tripped: HashSet<Axiom> = reparsed.axioms.into_iter().collect();
    assert_eq!(
        original, round_tripped,
        "axiom sets differ after round-trip; serialized text was:\n{text}"
    );
    text
}

// ── Phase 1: ontology header ────────────────────────────────────────────

#[test]
fn roundtrips_empty_unnamed_ontology() {
    assert_roundtrip("Ontology:");
}

#[test]
fn roundtrips_named_ontology_with_version_iri() {
    let text =
        assert_roundtrip("Ontology: <http://example.org/onto> <http://example.org/onto/1.0.0>");
    assert!(text.contains("http://example.org/onto"));
}

#[test]
fn roundtrips_ontology_with_import() {
    assert_roundtrip(
        r#"
        Ontology: <http://example.org/onto>
        Import: <http://example.org/other>
        "#,
    );
}

// ── Phase 2: simple Class: frame ────────────────────────────────────────

#[test]
fn roundtrips_class_subclassof() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        Class: :Pizza
            SubClassOf: :Food
        "#,
    );
}

#[test]
fn roundtrips_class_equivalentto_and_disjointwith() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        Class: :Pizza
            EquivalentTo: :Food
            DisjointWith: :Drink
        "#,
    );
}

#[test]
fn roundtrips_bare_class_declaration_with_no_axioms() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        Class: :Pizza
        "#,
    );
}

// ── Phase 3: class-expression nesting ────────────────────────────────────

#[test]
fn roundtrips_and_or_not_class_expressions() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        Class: :Pizza
            SubClassOf: :Food and (:Cheap or :Vegetarian) and not :Meat
        "#,
    );
}

#[test]
fn roundtrips_object_restrictions_some_only_value_self() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :hasTopping
        Class: :Pizza
            SubClassOf: :hasTopping some :Topping
            SubClassOf: :hasTopping only :Topping
            SubClassOf: :hasTopping value :Mozzarella
            SubClassOf: :hasTopping Self
        "#,
    );
}

#[test]
fn roundtrips_object_cardinality_restrictions_qualified_and_unqualified() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :hasTopping
        Class: :Pizza
            SubClassOf: :hasTopping min 1
            SubClassOf: :hasTopping max 3 :Topping
            SubClassOf: :hasTopping exactly 2 :Topping
        "#,
    );
}

#[test]
fn roundtrips_data_restrictions() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Prefix: xsd: <http://www.w3.org/2001/XMLSchema#>
        Ontology: <http://example.org/onto>
        DataProperty: :hasAge
        Class: :Adult
            SubClassOf: :hasAge some xsd:integer
            SubClassOf: :hasAge value "42"^^xsd:integer
            SubClassOf: :hasAge min 1
        "#,
    );
}

#[test]
fn roundtrips_object_one_of() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        Class: :Weekday
            EquivalentTo: { :Mon, :Tue }
        "#,
    );
}

// ── Phase 4: ObjectProperty: / DataProperty: frames ──────────────────────

#[test]
fn roundtrips_object_property_frame_sections() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :hasTopping
            Domain: :Pizza
            Range: :Topping
            Characteristics: Functional, InverseFunctional
        ObjectProperty: :isToppingOf
            InverseOf: :hasTopping
        "#,
    );
}

#[test]
fn roundtrips_object_property_subpropertyof_equivalentto_disjointwith() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :hasBaseTopping
        ObjectProperty: :hasTopping
            SubPropertyOf: :hasBaseTopping
            EquivalentTo: :hasIngredient
            DisjointWith: :hasCrust
        "#,
    );
}

#[test]
fn roundtrips_object_property_inverse_expression() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :hasTopping
        ObjectProperty: :isToppingOf
            EquivalentTo: inverse :hasTopping
        "#,
    );
}

#[test]
fn roundtrips_data_property_frame_sections() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Prefix: xsd: <http://www.w3.org/2001/XMLSchema#>
        Ontology: <http://example.org/onto>
        DataProperty: :hasBaseAge
        DataProperty: :hasAge
            Domain: :Person
            Range: xsd:integer
            Characteristics: Functional
            SubPropertyOf: :hasBaseAge
            EquivalentTo: :hasYears
            DisjointWith: :hasName
        "#,
    );
}

// ── Phase 5: Individual: frame ───────────────────────────────────────────

#[test]
fn roundtrips_individual_types_and_facts() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :knows
        DataProperty: :hasAge
        Individual: :Alice
            Types: :Person
            Facts: :knows :Bob, :hasAge "30"^^xsd:integer
        "#,
    );
}

#[test]
fn roundtrips_individual_negative_facts() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :knows
        Individual: :Alice
            Facts: not :knows :Carol
        "#,
    );
}

#[test]
fn roundtrips_individual_sameas_and_differentfrom() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        Individual: :Alice
            SameAs: :Alicia
            DifferentFrom: :Bob
        "#,
    );
}

#[test]
fn roundtrips_single_anonymous_individual() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        Individual: _:x
            Types: :Person
        "#,
    );
}

// ── Phase 6: AnnotationProperty: frame + Annotations: sections ───────────

#[test]
fn roundtrips_annotation_property_frame() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        AnnotationProperty: :myAnnotation
            Domain: :Pizza
            Range: :Food
            SubPropertyOf: rdfs:label
        "#,
    );
}

#[test]
fn roundtrips_class_frame_with_declaration_annotations() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Prefix: rdfs: <http://www.w3.org/2000/01/rdf-schema#>
        Ontology: <http://example.org/onto>
        Class: :Pizza
            Annotations: rdfs:label "Pizza"
            SubClassOf: :Food
        "#,
    );
}

// ── Phase 7: top-level misc (n-ary forms) ────────────────────────────────

#[test]
fn roundtrips_misc_equivalent_and_disjoint_classes_with_three_members() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        EquivalentClasses: :Pizza, :Food, :Meal
        DisjointClasses: :Pizza, :Drink, :Dessert
        "#,
    );
}

/// A top-level `misc` axiom with its own `Annotations:` section, preceding
/// (not per-item within) the n-ary member list — the trickiest boundary in
/// the misc grammar, since the annotation list and the member list are both
/// comma-separated (see `frame.rs::misc`'s `opt_annotations` then
/// `separated_list1` pairing).
#[test]
fn roundtrips_misc_equivalent_classes_with_annotations_and_three_members() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Prefix: rdfs: <http://www.w3.org/2000/01/rdf-schema#>
        Ontology: <http://example.org/onto>
        EquivalentClasses: Annotations: rdfs:label "why" :Pizza, :Food, :Meal
        "#,
    );
}

#[test]
fn roundtrips_misc_same_and_different_individuals_with_three_members() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        SameIndividual: :Alice, :Alicia, :A
        DifferentIndividuals: :Alice, :Bob, :Carol
        "#,
    );
}

#[test]
fn roundtrips_misc_equivalent_and_disjoint_object_properties() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        ObjectProperty: :hasTopping
        ObjectProperty: :hasIngredient
        ObjectProperty: :hasCrust
        EquivalentProperties: :hasTopping, :hasIngredient
        DisjointProperties: :hasTopping, :hasCrust
        "#,
    );
}

#[test]
fn roundtrips_misc_equivalent_and_disjoint_data_properties() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/onto#>
        Ontology: <http://example.org/onto>
        DataProperty: :hasAge
        DataProperty: :hasYears
        DataProperty: :hasName
        EquivalentProperties: :hasAge, :hasYears
        DisjointProperties: :hasAge, :hasName
        "#,
    );
}

// ── Phase 8: full-document integration ───────────────────────────────────

#[test]
fn roundtrips_multi_frame_document() {
    assert_roundtrip(
        r#"
        Prefix: : <http://example.org/pizza#>
        Prefix: xsd: <http://www.w3.org/2001/XMLSchema#>
        Ontology: <http://example.org/pizza> <http://example.org/pizza/1.0.0>

        Class: :Food

        Class: :Pizza
            SubClassOf: :Food
            EquivalentTo: :Food and (:hasTopping some :Topping)

        Class: :Topping

        ObjectProperty: :hasTopping
            Domain: :Pizza
            Range: :Topping
            Characteristics: InverseFunctional

        DataProperty: :hasCalories
            Domain: :Pizza
            Range: xsd:integer

        Individual: :Margherita
            Types: :Pizza
            Facts: :hasCalories "250"^^xsd:integer
        "#,
    );
}

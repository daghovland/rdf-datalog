/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! End-to-end tests for the OWL 2 Manchester Syntax parser
//! ([#139](https://github.com/daghovland/rdf-datalog/issues/139)), following
//! `turtle/tests/rdf12.rs`'s pattern: each test documents one grammar
//! production or frame section, parsing a small `.omn` snippet via
//! `manchester_parser::parse` and asserting on the resulting
//! `owl_ontology::Ontology`.
//!
//! See `docs/plans/MANCHESTER_SYNTAX_PLAN.md` for the grammar subset in
//! scope. Tests marked `#[ignore] // #157` document deferred grammar
//! (tracked in [#157](https://github.com/daghovland/rdf-datalog/issues/157))
//! and are expected to keep failing until that follow-up is implemented.

use ingress::{IriReference, OntologyVersion, RDFS, XSD};
use owl_ontology::{
    AnnotationAxiom, AnnotationValue, Assertion, Axiom, ClassAxiom, ClassExpression,
    DataPropertyAxiom, DataRange, Entity, FullIri, Individual, ObjectPropertyAxiom,
    ObjectPropertyExpression,
};

const EX: &str = "http://example.org/";

fn iri(local: &str) -> FullIri {
    FullIri(IriReference(format!("{EX}{local}")))
}

fn xsd(local: &str) -> FullIri {
    FullIri(IriReference(format!("{XSD}{local}")))
}

fn cls(local: &str) -> ClassExpression {
    ClassExpression::ClassName(iri(local))
}

fn obj_prop(local: &str) -> ObjectPropertyExpression {
    ObjectPropertyExpression::NamedObjectProperty(iri(local))
}

/// Preamble shared by every test: declares the default `:` prefix as `EX`.
fn doc(body: &str) -> String {
    format!("Prefix: : <{EX}>\nOntology:\n{body}\n")
}

// ── Phase 1: ontology header ─────────────────────────────────────────────

#[test]
fn header_unnamed_empty_ontology() {
    let onto = manchester_parser::parse("Ontology:").expect("parse failed");
    assert_eq!(onto.version, OntologyVersion::UnNamedOntology);
    assert!(onto.axioms.is_empty());
    assert!(onto.annotations.is_empty());
    assert!(onto.directly_imports_documents.is_empty());
}

#[test]
fn header_named_ontology_no_version() {
    let onto = manchester_parser::parse("Ontology: <http://example.org/onto>").unwrap();
    assert_eq!(
        onto.version,
        OntologyVersion::NamedOntology(IriReference("http://example.org/onto".to_string()))
    );
}

#[test]
fn header_named_ontology_with_version() {
    let onto = manchester_parser::parse(
        "Ontology: <http://example.org/onto> <http://example.org/onto/1.0>",
    )
    .unwrap();
    assert_eq!(
        onto.version,
        OntologyVersion::VersionedOntology {
            ontology_iri: IriReference("http://example.org/onto".to_string()),
            version_iri: IriReference("http://example.org/onto/1.0".to_string()),
        }
    );
}

#[test]
fn header_import() {
    let onto = manchester_parser::parse(
        "Ontology: <http://example.org/onto>\nImport: <http://example.org/other>",
    )
    .unwrap();
    assert_eq!(
        onto.directly_imports_documents,
        vec![IriReference("http://example.org/other".to_string())]
    );
}

#[test]
fn header_annotations_with_custom_prefix() {
    let onto = manchester_parser::parse(&format!(
        "Prefix: ex: <{EX}>\nOntology: <http://example.org/onto>\nAnnotations: ex:createdBy \"Dag\""
    ))
    .unwrap();
    assert_eq!(onto.annotations.len(), 1);
    let (prop, value) = &onto.annotations[0];
    assert_eq!(*prop, iri("createdBy"));
    match value {
        AnnotationValue::LiteralAnnotation(_) => {}
        other => panic!("expected LiteralAnnotation, got {other:?}"),
    }
}

// ── Phase 2/3: class expressions (via minimal Class: frames) ─────────────

#[test]
fn class_atomic_subclassof() {
    let onto = manchester_parser::parse(&doc("Class: Pizza SubClassOf: Food")).unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("Pizza"),
                cls("Food")
            )))
    );
}

#[test]
fn class_and_or_not() {
    let onto = manchester_parser::parse(&doc(
        "Class: Pizza SubClassOf: Food and Cheap\nClass: NonPizza SubClassOf: not Pizza\nClass: FoodOrDrink SubClassOf: Food or Drink",
    ))
    .unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("Pizza"),
                ClassExpression::ObjectIntersectionOf(vec![cls("Food"), cls("Cheap")])
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("NonPizza"),
                ClassExpression::ObjectComplementOf(Box::new(cls("Pizza")))
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("FoodOrDrink"),
                ClassExpression::ObjectUnionOf(vec![cls("Food"), cls("Drink")])
            )))
    );
}

#[test]
fn class_object_restrictions_some_only_value_self() {
    let onto = manchester_parser::parse(&doc(
        "Class: Pizza SubClassOf: hasTopping some Mozzarella\nClass: Pizza SubClassOf: hasTopping only Mozzarella\nClass: Narcissist SubClassOf: likes value Self",
    ));
    let onto = onto.unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("Pizza"),
                ClassExpression::ObjectSomeValuesFrom(
                    obj_prop("hasTopping"),
                    Box::new(cls("Mozzarella"))
                )
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("Pizza"),
                ClassExpression::ObjectAllValuesFrom(
                    obj_prop("hasTopping"),
                    Box::new(cls("Mozzarella"))
                )
            )))
    );
}

#[test]
fn class_object_self_restriction() {
    let onto = manchester_parser::parse(&doc("Class: Narcissist SubClassOf: likes Self")).unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("Narcissist"),
                ClassExpression::ObjectHasSelf(obj_prop("likes"))
            )))
    );
}

#[test]
fn class_object_cardinalities_qualified_and_unqualified() {
    let onto = manchester_parser::parse(&doc(
        "Class: C SubClassOf: hasTopping min 2\nClass: C SubClassOf: hasTopping min 2 Mozzarella\nClass: C SubClassOf: hasTopping max 3 Mozzarella\nClass: C SubClassOf: hasTopping exactly 1 Mozzarella",
    ))
    .unwrap();
    use num_bigint::BigInt;
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("C"),
                ClassExpression::ObjectMinCardinality(BigInt::from(2), obj_prop("hasTopping"))
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("C"),
                ClassExpression::ObjectMinQualifiedCardinality(
                    BigInt::from(2),
                    obj_prop("hasTopping"),
                    Box::new(cls("Mozzarella"))
                )
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("C"),
                ClassExpression::ObjectExactQualifiedCardinality(
                    BigInt::from(1),
                    obj_prop("hasTopping"),
                    Box::new(cls("Mozzarella"))
                )
            )))
    );
}

#[test]
fn class_one_of_and_parens() {
    let onto = manchester_parser::parse(&doc(
        "Class: Continent SubClassOf: { Europe, Asia }\nClass: C SubClassOf: (Food or Drink) and Cheap",
    ))
    .unwrap();
    let has_one_of = onto.axioms.iter().any(|a| {
        matches!(
            a,
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(_, _, ClassExpression::ObjectOneOf(v))) if v.len() == 2
        )
    });
    assert!(has_one_of, "expected an ObjectOneOf axiom");
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("C"),
                ClassExpression::ObjectIntersectionOf(vec![
                    ClassExpression::ObjectUnionOf(vec![cls("Food"), cls("Drink")]),
                    cls("Cheap"),
                ])
            )))
    );
}

#[test]
fn class_data_restriction_some_and_value() {
    // `hasAge` is declared via its own `DataProperty:` frame so the
    // object/data-property disambiguation (see `class_expr.rs` module docs)
    // resolves `some` to the data-property reading.
    let onto = manchester_parser::parse(&doc(
        "DataProperty: hasAge\nClass: Adult SubClassOf: hasAge some xsd:integer\nClass: Teen SubClassOf: hasAge value \"15\"^^xsd:integer",
    ))
    .unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("Adult"),
                ClassExpression::DataSomeValuesFrom(
                    vec![iri("hasAge")],
                    DataRange::NamedDataRange(xsd("integer"))
                )
            )))
    );
    let has_data_value = onto.axioms.iter().any(|a| {
        matches!(
            a,
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(_, _, ClassExpression::DataHasValue(p, _))) if *p == iri("hasAge")
        )
    });
    assert!(has_data_value, "expected a DataHasValue axiom");
}

// ── Phase 4: Class: frame sections ───────────────────────────────────────

#[test]
fn class_frame_equivalentto_and_disjointwith() {
    let onto = manchester_parser::parse(&doc(
        "Class: Pizza EquivalentTo: CheesyFood\nClass: Pizza DisjointWith: Drink",
    ))
    .unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                vec![],
                vec![cls("Pizza"), cls("CheesyFood")]
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                vec![],
                vec![cls("Pizza"), cls("Drink")]
            )))
    );
}

#[test]
fn class_frame_declaration_and_annotations() {
    let onto = manchester_parser::parse(&format!(
        "Prefix: rdfs: <{RDFS}>\n{}",
        doc("Class: Pizza Annotations: rdfs:label \"Pizza\"")
    ))
    .unwrap();
    let decl_found = onto.axioms.iter().any(|a| {
        matches!(
            a,
            Axiom::AxiomDeclaration((anns, Entity::ClassDeclaration(c)))
                if *c == iri("Pizza") && anns.len() == 1
        )
    });
    assert!(
        decl_found,
        "expected an annotated ClassDeclaration for Pizza"
    );
}

// ── Phase 5: ObjectProperty:/DataProperty: frames ────────────────────────

#[test]
fn object_property_domain_range_and_characteristics() {
    let onto = manchester_parser::parse(&doc(
        "ObjectProperty: hasTopping\n    Domain: Pizza\n    Range: Topping\n    Characteristics: InverseFunctional, Transitive",
    ))
    .unwrap();
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::ObjectPropertyDomain(obj_prop("hasTopping"), cls("Pizza"))
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::ObjectPropertyRange(obj_prop("hasTopping"), cls("Topping"))
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::InverseFunctionalObjectProperty(vec![], obj_prop("hasTopping"))
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::TransitiveObjectProperty(vec![], obj_prop("hasTopping"))
    )));
}

#[test]
fn object_property_subpropertyof_equivalentto_disjointwith_inverseof() {
    let onto = manchester_parser::parse(&doc(
        "ObjectProperty: hasBaseTopping\n    SubPropertyOf: hasTopping\n    EquivalentTo: hasIngredient\n    DisjointWith: excludesTopping\n    InverseOf: isBaseToppingOf",
    ))
    .unwrap();
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::SubObjectPropertyOf(
            vec![],
            owl_ontology::SubPropertyExpression::SubObjectPropertyExpression(obj_prop(
                "hasBaseTopping"
            )),
            obj_prop("hasTopping"),
        )
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::EquivalentObjectProperties(
            vec![],
            vec![obj_prop("hasBaseTopping"), obj_prop("hasIngredient")]
        )
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::DisjointObjectProperties(
            vec![],
            vec![obj_prop("hasBaseTopping"), obj_prop("excludesTopping")]
        )
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::InverseObjectProperties(
            vec![],
            obj_prop("hasBaseTopping"),
            obj_prop("isBaseToppingOf"),
        )
    )));
}

#[test]
fn object_property_inverse_in_restriction() {
    let onto = manchester_parser::parse(&doc("Class: C SubClassOf: inverse hasTopping some Pizza"))
        .unwrap();
    let expected_prop =
        ObjectPropertyExpression::InverseObjectProperty(Box::new(obj_prop("hasTopping")));
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("C"),
                ClassExpression::ObjectSomeValuesFrom(expected_prop, Box::new(cls("Pizza")))
            )))
    );
}

#[test]
fn data_property_domain_range_functional_subpropertyof() {
    let onto = manchester_parser::parse(&doc(
        "DataProperty: hasAge\n    Domain: Person\n    Range: xsd:integer\n    Characteristics: Functional\nDataProperty: hasYearsOld\n    SubPropertyOf: hasAge\n    EquivalentTo: hasAgeInYears\n    DisjointWith: hasWeight",
    ))
    .unwrap();
    assert!(onto.axioms.contains(&Axiom::AxiomDataPropertyAxiom(
        DataPropertyAxiom::DataPropertyDomain(vec![], iri("hasAge"), cls("Person"))
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomDataPropertyAxiom(
        DataPropertyAxiom::DataPropertyRange(
            vec![],
            iri("hasAge"),
            DataRange::NamedDataRange(xsd("integer"))
        )
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomDataPropertyAxiom(
        DataPropertyAxiom::FunctionalDataProperty(vec![], iri("hasAge"))
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomDataPropertyAxiom(
        DataPropertyAxiom::SubDataPropertyOf(vec![], iri("hasYearsOld"), iri("hasAge"))
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomDataPropertyAxiom(
        DataPropertyAxiom::EquivalentDataProperties(
            vec![],
            vec![iri("hasYearsOld"), iri("hasAgeInYears")]
        )
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomDataPropertyAxiom(
        DataPropertyAxiom::DisjointDataProperties(
            vec![],
            vec![iri("hasYearsOld"), iri("hasWeight")]
        )
    )));
}

// ── Phase 6: Individual: frame ────────────────────────────────────────────

#[test]
fn individual_types_and_sameas_differentfrom() {
    let onto = manchester_parser::parse(&doc(
        "Individual: Alice\n    Types: Person\n    SameAs: AliceSmith\n    DifferentFrom: Bob",
    ))
    .unwrap();
    let alice = Individual::NamedIndividual(iri("Alice"));
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomAssertion(Assertion::ClassAssertion(
                vec![],
                cls("Person"),
                alice.clone()
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomAssertion(Assertion::SameIndividual(
                vec![],
                vec![
                    alice.clone(),
                    Individual::NamedIndividual(iri("AliceSmith"))
                ]
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomAssertion(Assertion::DifferentIndividuals(
                vec![],
                vec![alice, Individual::NamedIndividual(iri("Bob"))]
            )))
    );
}

#[test]
fn individual_facts_positive_and_negative() {
    let onto = manchester_parser::parse(&doc(
        "Individual: Alice\n    Facts: hasFriend Bob, not hasFriend Carol, hasAge \"30\"^^xsd:integer, not hasAge \"99\"^^xsd:integer",
    ))
    .unwrap();
    let alice = Individual::NamedIndividual(iri("Alice"));
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(
                vec![],
                obj_prop("hasFriend"),
                alice.clone(),
                Individual::NamedIndividual(iri("Bob")),
            )))
    );
    assert!(onto.axioms.contains(&Axiom::AxiomAssertion(
        Assertion::NegativeObjectPropertyAssertion(
            vec![],
            obj_prop("hasFriend"),
            alice.clone(),
            Individual::NamedIndividual(iri("Carol")),
        )
    )));
    let has_data_fact = onto.axioms.iter().any(|a| {
        matches!(a, Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(_, p, i, _)) if *p == iri("hasAge") && *i == alice)
    });
    assert!(has_data_fact, "expected a positive DataPropertyAssertion");
    let has_negative_data_fact = onto.axioms.iter().any(|a| {
        matches!(a, Axiom::AxiomAssertion(Assertion::NegativeDataPropertyAssertion(_, p, i, _)) if *p == iri("hasAge") && *i == alice)
    });
    assert!(
        has_negative_data_fact,
        "expected a negative DataPropertyAssertion"
    );
}

#[test]
fn individual_anonymous_node_id() {
    let onto = manchester_parser::parse(&doc("Individual: _:x Types: Person")).unwrap();
    let has_anon_person = onto.axioms.iter().any(|a| {
        matches!(
            a,
            Axiom::AxiomAssertion(Assertion::ClassAssertion(_, expr, Individual::AnonymousIndividual(_)))
                if *expr == cls("Person")
        )
    });
    assert!(
        has_anon_person,
        "expected an anonymous individual Types: assertion"
    );
}

// ── Phase 7: AnnotationProperty: frame ────────────────────────────────────

#[test]
fn annotation_property_domain_range_subpropertyof() {
    let onto = manchester_parser::parse(&format!(
        "Prefix: rdfs: <{RDFS}>\n{}",
        doc("AnnotationProperty: createdBy\n    Domain: OntologyThing\n    Range: Agent\n    SubPropertyOf: rdfs:seeAlso")
    ))
    .unwrap();
    assert!(onto.axioms.contains(&Axiom::AxiomAnnotationAxiom(
        AnnotationAxiom::AnnotationPropertyDomain(vec![], iri("createdBy"), iri("OntologyThing"))
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomAnnotationAxiom(
        AnnotationAxiom::AnnotationPropertyRange(vec![], iri("createdBy"), iri("Agent"))
    )));
    let expected_super = FullIri(IriReference(format!("{RDFS}seeAlso")));
    assert!(onto.axioms.contains(&Axiom::AxiomAnnotationAxiom(
        AnnotationAxiom::SubAnnotationPropertyOf(vec![], iri("createdBy"), expected_super)
    )));
}

// ── Phase 8: top-level misc + full-document integration ──────────────────

#[test]
fn misc_equivalentclasses_and_disjointclasses() {
    let onto = manchester_parser::parse(&doc(
        "EquivalentClasses: Pizza, CheesyFood, ItalianFood\nDisjointClasses: Pizza, Drink, Dessert",
    ))
    .unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                vec![],
                vec![cls("Pizza"), cls("CheesyFood"), cls("ItalianFood")]
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                vec![],
                vec![cls("Pizza"), cls("Drink"), cls("Dessert")]
            )))
    );
}

#[test]
fn misc_equivalent_and_disjoint_properties() {
    let onto = manchester_parser::parse(&doc(
        "DataProperty: hasAge\nEquivalentProperties: hasAge, hasYearsOld\nObjectProperty: hasTopping\nDisjointProperties: hasTopping, excludesTopping",
    ))
    .unwrap();
    assert!(onto.axioms.contains(&Axiom::AxiomDataPropertyAxiom(
        DataPropertyAxiom::EquivalentDataProperties(
            vec![],
            vec![iri("hasAge"), iri("hasYearsOld")]
        )
    )));
    assert!(onto.axioms.contains(&Axiom::AxiomObjectPropertyAxiom(
        ObjectPropertyAxiom::DisjointObjectProperties(
            vec![],
            vec![obj_prop("hasTopping"), obj_prop("excludesTopping")]
        )
    )));
}

#[test]
fn misc_sameindividual_and_differentindividuals() {
    let onto = manchester_parser::parse(&doc(
        "SameIndividual: Alice, AliceSmith\nDifferentIndividuals: Alice, Bob, Carol",
    ))
    .unwrap();
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomAssertion(Assertion::SameIndividual(
                vec![],
                vec![
                    Individual::NamedIndividual(iri("Alice")),
                    Individual::NamedIndividual(iri("AliceSmith"))
                ]
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomAssertion(Assertion::DifferentIndividuals(
                vec![],
                vec![
                    Individual::NamedIndividual(iri("Alice")),
                    Individual::NamedIndividual(iri("Bob")),
                    Individual::NamedIndividual(iri("Carol")),
                ]
            )))
    );
}

#[test]
fn full_pizza_like_ontology_integration() {
    let text = format!(
        r#"Prefix: : <{EX}>
Prefix: owl: <http://www.w3.org/2002/07/owl#>
Prefix: xsd: <http://www.w3.org/2001/XMLSchema#>

Ontology: <http://example.org/pizza>

Class: Food

Class: Pizza
    SubClassOf: Food
    EquivalentTo: Food and (hasTopping some Mozzarella)
    DisjointWith: Drink

ObjectProperty: hasTopping
    Domain: Pizza
    Range: Food
    Characteristics: InverseFunctional

DataProperty: hasCalories
    Domain: Pizza
    Range: xsd:integer
    Characteristics: Functional

Individual: Margherita
    Types: Pizza
    Facts: hasTopping Mozzarella, hasCalories "800"^^xsd:integer
"#
    );
    let onto = manchester_parser::parse(&text).unwrap();
    assert_eq!(
        onto.version,
        OntologyVersion::NamedOntology(IriReference("http://example.org/pizza".to_string()))
    );
    // One declaration each for Food, Pizza, hasTopping, hasCalories, Margherita.
    let decl_count = onto
        .axioms
        .iter()
        .filter(|a| matches!(a, Axiom::AxiomDeclaration(_)))
        .count();
    assert_eq!(
        decl_count, 5,
        "expected 5 entity declarations, got: {:?}",
        onto.axioms
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                cls("Pizza"),
                cls("Food")
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                vec![],
                vec![
                    cls("Pizza"),
                    ClassExpression::ObjectIntersectionOf(vec![
                        cls("Food"),
                        ClassExpression::ObjectSomeValuesFrom(
                            obj_prop("hasTopping"),
                            Box::new(cls("Mozzarella"))
                        ),
                    ])
                ]
            )))
    );
    assert!(
        onto.axioms
            .contains(&Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(
                vec![],
                obj_prop("hasTopping"),
                Individual::NamedIndividual(iri("Margherita")),
                Individual::NamedIndividual(iri("Mozzarella")),
            )))
    );
}

// ── Deferred grammar — tracked in #157 ────────────────────────────────────
//
// These document grammar productions this parser deliberately does not
// support yet (see docs/plans/MANCHESTER_SYNTAX_PLAN.md's scope table).
// They are `#[ignore]`d and expected to keep failing (return `Err`, or parse
// but silently drop the construct) until #157 is implemented.

#[test]
#[ignore] // #157: DisjointUnionOf: is not parsed.
fn deferred_disjoint_union_of() {
    let onto = manchester_parser::parse(&doc("Class: Topping DisjointUnionOf: Mozzarella, Tomato"))
        .unwrap();
    assert!(onto.axioms.iter().any(|a| matches!(
        a,
        Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(_, _, _))
    )));
}

#[test]
#[ignore] // #157: HasKey: is not parsed.
fn deferred_has_key() {
    let onto = manchester_parser::parse(&doc("Class: Person HasKey: hasSSN")).unwrap();
    assert!(
        onto.axioms
            .iter()
            .any(|a| matches!(a, Axiom::AxiomHasKey(_, _, _, _)))
    );
}

#[test]
#[ignore] // #157: SubPropertyChain: (object property chains) is not parsed.
fn deferred_subproperty_chain() {
    let onto = manchester_parser::parse(&doc(
        "ObjectProperty: hasGrandparent SubPropertyChain: hasParent o hasParent",
    ))
    .unwrap();
    assert!(onto.axioms.iter().any(|a| matches!(
        a,
        Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
            _,
            owl_ontology::SubPropertyExpression::PropertyExpressionChain(_),
            _
        ))
    )));
}

#[test]
#[ignore] // #157: compound data ranges (and/or/not/oneOf on data ranges) are not parsed.
fn deferred_compound_data_range() {
    let onto = manchester_parser::parse(&doc(
        "DataProperty: hasRating Range: xsd:integer and xsd:positiveInteger",
    ))
    .unwrap();
    assert!(onto.axioms.iter().any(|a| matches!(
        a,
        Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyRange(
            _,
            _,
            DataRange::DataIntersectionOf(_)
        ))
    )));
}

#[test]
#[ignore] // #157: datatype facet restrictions (Datatype[facet value,...]) are not parsed.
fn deferred_datatype_facet_restriction() {
    let onto =
        manchester_parser::parse(&doc("DataProperty: hasName Range: xsd:string[minLength 1]"))
            .unwrap();
    assert!(onto.axioms.iter().any(|a| matches!(
        a,
        Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyRange(
            _,
            _,
            DataRange::DatatypeRestriction(_, _)
        ))
    )));
}

#[test]
#[ignore] // #157: SWRL `Rule:` frames are not parsed.
fn deferred_swrl_rule_frame() {
    manchester_parser::parse(&doc(
        "Rule: Person(?p), hasAge(?p, ?a), greaterThan(?a, 18) -> Adult(?p)",
    ))
    .expect("SWRL Rule: frames are not yet supported (#157)");
}

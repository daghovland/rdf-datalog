/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `owl_functional_parser::parse`, one `.ofn` snippet
//! per test. See `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md` for the
//! phase this coverage maps to; the crate's own unit tests (in `src/*.rs`)
//! cover the individual grammar productions in more depth. This file
//! exercises them end-to-end through the public `parse` entry point.

use ingress::IriReference;
use owl_ontology::{
    Assertion, Axiom, ClassAxiom, ClassExpression, DataPropertyAxiom, Entity, FullIri, Individual,
    ObjectPropertyAxiom, ObjectPropertyExpression,
};

fn iri(s: &str) -> FullIri {
    FullIri(IriReference(s.to_string()))
}

const PREFIX: &str = "Prefix(:=<http://example.org/pizza#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nPrefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n";

fn parse_body(body: &str) -> owl_ontology::Ontology {
    let src = format!("{PREFIX}Ontology({body})");
    owl_functional_parser::parse(&src).unwrap_or_else(|e| panic!("parse failed: {e}\nsrc: {src}"))
}

// --- Phase 1: ontology header ---------------------------------------------

#[test]
fn empty_unnamed_ontology() {
    let onto = owl_functional_parser::parse("Ontology()").unwrap();
    assert_eq!(onto.version, ingress::OntologyVersion::UnNamedOntology);
    assert!(onto.axioms.is_empty());
}

#[test]
fn named_ontology_with_version_iri() {
    let onto = owl_functional_parser::parse(
        "Ontology(<http://example.org/pizza> <http://example.org/pizza/1.0>)",
    )
    .unwrap();
    assert_eq!(
        onto.try_get_ontology_iri(),
        Some(&IriReference("http://example.org/pizza".to_string()))
    );
    assert_eq!(
        onto.try_get_version_iri(),
        Some(&IriReference("http://example.org/pizza/1.0".to_string()))
    );
}

#[test]
fn import_declaration() {
    let onto = parse_body("Import(<http://example.org/imported>)");
    assert_eq!(
        onto.directly_imports_documents,
        vec![IriReference("http://example.org/imported".to_string())]
    );
}

// --- Phase 2: declarations --------------------------------------------------

#[test]
fn class_declaration() {
    let onto = parse_body("Declaration(Class(:Pizza))");
    assert_eq!(
        onto.axioms,
        vec![Axiom::AxiomDeclaration((
            vec![],
            Entity::ClassDeclaration(iri("http://example.org/pizza#Pizza"))
        ))]
    );
}

#[test]
fn named_individual_declaration() {
    let onto = parse_body("Declaration(NamedIndividual(:fido))");
    assert_eq!(
        onto.axioms,
        vec![Axiom::AxiomDeclaration((
            vec![],
            Entity::NamedIndividualDeclaration(Individual::NamedIndividual(iri(
                "http://example.org/pizza#fido"
            )))
        ))]
    );
}

// --- Phase 4/5: class expressions + class axioms ----------------------------

#[test]
fn sub_class_of_with_nested_class_expression() {
    let onto = parse_body(
        "SubClassOf(:Pizza ObjectIntersectionOf(:Food ObjectSomeValuesFrom(:hasTopping :Topping)))",
    );
    match &onto.axioms[0] {
        Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(_, sub, sup)) => {
            assert_eq!(
                *sub,
                ClassExpression::ClassName(iri("http://example.org/pizza#Pizza"))
            );
            assert!(matches!(sup, ClassExpression::ObjectIntersectionOf(v) if v.len() == 2));
        }
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
fn disjoint_union() {
    let onto = parse_body("DisjointUnion(:Pizza :MeatPizza :VegPizza)");
    assert!(matches!(
        &onto.axioms[0],
        Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(_, _, ces)) if ces.len() == 2
    ));
}

// --- Phase 6: object property axioms ----------------------------------------

#[test]
fn object_property_domain_and_range() {
    let onto = parse_body(
        "ObjectPropertyDomain(:hasTopping :Pizza)\nObjectPropertyRange(:hasTopping :Topping)",
    );
    assert_eq!(onto.axioms.len(), 2);
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyDomain(_, _))
    ));
}

#[test]
fn inverse_object_property_expression_in_axiom() {
    let onto = parse_body("InverseObjectProperties(:hasTopping :isToppingOf)");
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::InverseObjectProperties(_, _, _))
    ));
    let onto2 = parse_body("SubObjectPropertyOf(ObjectInverseOf(:isToppingOf) :hasTopping)");
    match &onto2.axioms[0] {
        Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
            _,
            owl_ontology::SubPropertyExpression::SubObjectPropertyExpression(
                ObjectPropertyExpression::InverseObjectProperty(_),
            ),
            _,
        )) => {}
        other => panic!("expected inverse-property LHS, got {other:?}"),
    }
}

// --- Phase 7: data property axioms + DatatypeDefinition ---------------------

#[test]
fn functional_data_property_and_range() {
    let onto =
        parse_body("FunctionalDataProperty(:hasAge)\nDataPropertyRange(:hasAge xsd:integer)");
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::FunctionalDataProperty(_, _))
    ));
}

#[test]
fn datatype_definition() {
    let onto = parse_body("DatatypeDefinition(:AdultAge xsd:integer)");
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomDatatypeDefinition(_, _, _)
    ));
}

// --- Phase 8: assertions -----------------------------------------------------

#[test]
fn class_assertion_and_object_property_assertion() {
    let onto = parse_body(
        "ClassAssertion(:Pizza :margherita)\nObjectPropertyAssertion(:hasTopping :margherita :cheese)",
    );
    assert_eq!(onto.axioms.len(), 2);
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomAssertion(Assertion::ClassAssertion(_, _, _))
    ));
}

#[test]
fn negative_property_assertions() {
    let onto = parse_body(
        "NegativeObjectPropertyAssertion(:hasTopping :margherita :anchovy)\n\
         NegativeDataPropertyAssertion(:hasAge :margherita \"0\")",
    );
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomAssertion(Assertion::NegativeObjectPropertyAssertion(_, _, _, _))
    ));
    assert!(matches!(
        onto.axioms[1],
        Axiom::AxiomAssertion(Assertion::NegativeDataPropertyAssertion(_, _, _, _))
    ));
}

#[test]
fn same_and_different_individuals() {
    let onto = parse_body(
        "SameIndividual(:margherita :classicMargherita)\nDifferentIndividuals(:margherita :hawaiian)",
    );
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomAssertion(Assertion::SameIndividual(_, _))
    ));
    assert!(matches!(
        onto.axioms[1],
        Axiom::AxiomAssertion(Assertion::DifferentIndividuals(_, _))
    ));
}

#[test]
fn anonymous_individual_round_trips_within_one_document() {
    let onto = parse_body("ClassAssertion(:Pizza _:x)\nDataPropertyAssertion(:hasAge _:x \"1\")");
    let ind_a = match &onto.axioms[0] {
        Axiom::AxiomAssertion(Assertion::ClassAssertion(_, _, i)) => i.clone(),
        other => panic!("expected ClassAssertion, got {other:?}"),
    };
    let ind_b = match &onto.axioms[1] {
        Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(_, _, i, _)) => i.clone(),
        other => panic!("expected DataPropertyAssertion, got {other:?}"),
    };
    assert_eq!(
        ind_a, ind_b,
        "same _:x label must resolve to the same anonymous individual id"
    );
}

// --- Phase 9: annotation axioms + axiomAnnotations --------------------------

#[test]
fn annotation_assertion_and_sub_annotation_property_of() {
    let onto = parse_body(
        "AnnotationAssertion(rdfs:label :Pizza \"Pizza\")\nSubAnnotationPropertyOf(rdfs:label rdfs:comment)",
    );
    assert_eq!(onto.axioms.len(), 2);
}

#[test]
fn axiom_annotations_attach_to_sub_class_of() {
    let onto = parse_body("SubClassOf(Annotation(rdfs:label \"why\") :Pizza :Food)");
    match &onto.axioms[0] {
        Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(anns, _, _)) => {
            assert_eq!(anns.len(), 1);
        }
        other => panic!("expected annotated SubClassOf, got {other:?}"),
    }
}

// --- Phase 10: full-document integration ------------------------------------

#[test]
fn pizza_style_multi_axiom_ontology() {
    let src = "\
Prefix(:=<http://example.org/pizza#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://example.org/pizza>
    Declaration(Class(:Pizza))
    Declaration(Class(:Food))
    Declaration(Class(:Topping))
    Declaration(ObjectProperty(:hasTopping))
    Declaration(NamedIndividual(:margherita))
    SubClassOf(:Pizza :Food)
    EquivalentClasses(:Pizza ObjectIntersectionOf(:Food ObjectSomeValuesFrom(:hasTopping :Topping)))
    ObjectPropertyDomain(:hasTopping :Pizza)
    ObjectPropertyRange(:hasTopping :Topping)
    InverseFunctionalObjectProperty(:hasTopping)
    ClassAssertion(:Pizza :margherita)
)";
    let onto = owl_functional_parser::parse(src).unwrap();
    assert_eq!(
        onto.try_get_ontology_iri(),
        Some(&IriReference("http://example.org/pizza".to_string()))
    );
    assert_eq!(onto.axioms.len(), 11);
}

// --- Phase 11: extended constructs (beyond #180's mandated tier) ------------

#[test]
fn compound_data_range() {
    let onto = parse_body(
        "DataPropertyRange(:hasAge DataIntersectionOf(xsd:integer xsd:nonNegativeInteger))",
    );
    assert!(matches!(
        onto.axioms[0],
        Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyRange(_, _, _))
    ));
}

#[test]
fn object_property_chain_as_sub_object_property_of_lhs() {
    let onto = parse_body(
        "SubObjectPropertyOf(ObjectPropertyChain(:hasParent :hasParent) :hasGrandparent)",
    );
    match &onto.axioms[0] {
        Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
            _,
            owl_ontology::SubPropertyExpression::PropertyExpressionChain(chain),
            _,
        )) => assert_eq!(chain.len(), 2),
        other => panic!("expected chain SubObjectPropertyOf, got {other:?}"),
    }
}

#[test]
fn has_key() {
    let onto = parse_body("HasKey(:Person (:hasSSN) ())");
    assert!(matches!(onto.axioms[0], Axiom::AxiomHasKey(_, _, _, _)));
}

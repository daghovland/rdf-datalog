/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `owl_xml_parser::parse`, one OWL/XML snippet per
//! test. See `docs/plans/OWL_XML_PLAN.md` for the phase this coverage maps
//! to (phases 1-6, this issue's #605 scope only: ontology header, prefixes,
//! imports, ontology-level annotations, declarations — no class/property/
//! ABox axioms yet, those are #606-608).

use ingress::IriReference;
use owl_ontology::{Axiom, ClassAxiom, ClassExpression, Entity, FullIri, Individual};

fn iri(s: &str) -> FullIri {
    FullIri(IriReference(s.to_string()))
}

const NS: &str = r#"xmlns="http://www.w3.org/2002/07/owl#" xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#" xmlns:xsd="http://www.w3.org/2001/XMLSchema#""#;

fn wrap(attrs: &str, body: &str) -> String {
    format!(r#"<?xml version="1.0"?><Ontology {NS} {attrs}>{body}</Ontology>"#)
}

// --- Phase 2: ontology header ----------------------------------------------

#[test]
fn empty_unnamed_ontology() {
    let onto = owl_xml_parser::parse(&wrap("", "")).unwrap();
    assert_eq!(onto.version, ingress::OntologyVersion::UnNamedOntology);
    assert!(onto.axioms.is_empty());
}

#[test]
fn named_ontology() {
    let onto =
        owl_xml_parser::parse(&wrap(r#"ontologyIRI="http://example.org/pizza""#, "")).unwrap();
    assert_eq!(
        onto.try_get_ontology_iri(),
        Some(&IriReference("http://example.org/pizza".to_string()))
    );
    assert_eq!(onto.try_get_version_iri(), None);
}

#[test]
fn named_ontology_with_version_iri() {
    let onto = owl_xml_parser::parse(&wrap(
        r#"ontologyIRI="http://example.org/pizza" versionIRI="http://example.org/pizza/1.0""#,
        "",
    ))
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

// --- Phase 3: Prefix + Import -----------------------------------------------

#[test]
fn import_declaration() {
    let onto = owl_xml_parser::parse(&wrap("", r#"<Import>http://example.org/imported</Import>"#))
        .unwrap();
    assert_eq!(
        onto.directly_imports_documents,
        vec![IriReference("http://example.org/imported".to_string())]
    );
}

#[test]
fn multiple_imports() {
    let onto = owl_xml_parser::parse(&wrap(
        "",
        r#"<Import>http://example.org/a</Import><Import>http://example.org/b</Import>"#,
    ))
    .unwrap();
    assert_eq!(
        onto.directly_imports_documents,
        vec![
            IriReference("http://example.org/a".to_string()),
            IriReference("http://example.org/b".to_string()),
        ]
    );
}

#[test]
fn prefix_declaration_used_by_declaration_abbreviated_iri() {
    let src = wrap(
        r#"ontologyIRI="http://example.org/pizza""#,
        r#"<Prefix name="" IRI="http://example.org/pizza#"/>
           <Declaration><Class abbreviatedIRI=":Pizza"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 1);
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::ClassDeclaration(c))) => {
            assert_eq!(*c, iri("http://example.org/pizza#Pizza"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

#[test]
fn prefix_declaration_non_default_prefix() {
    let src = wrap(
        "",
        r#"<Prefix name="owl" IRI="http://www.w3.org/2002/07/owl#"/>
           <Declaration><Class abbreviatedIRI="owl:Thing"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::ClassDeclaration(c))) => {
            assert_eq!(*c, iri("http://www.w3.org/2002/07/owl#Thing"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

// --- Phase 4: Declaration, all six Entity variants --------------------------

#[test]
fn declaration_class_full_iri() {
    let src = wrap(
        "",
        r#"<Declaration><Class IRI="http://example.org/pizza#Pizza"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 1);
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((anns, Entity::ClassDeclaration(c))) => {
            assert!(anns.is_empty());
            assert_eq!(*c, iri("http://example.org/pizza#Pizza"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

#[test]
fn declaration_object_property() {
    let src = wrap(
        "",
        r#"<Declaration><ObjectProperty IRI="http://example.org/pizza#hasTopping"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::ObjectPropertyDeclaration(p))) => {
            assert_eq!(*p, iri("http://example.org/pizza#hasTopping"));
        }
        other => panic!("expected AxiomDeclaration(ObjectPropertyDeclaration), got {other:?}"),
    }
}

#[test]
fn declaration_data_property() {
    let src = wrap(
        "",
        r#"<Declaration><DataProperty IRI="http://example.org/pizza#hasCalories"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::DataPropertyDeclaration(p))) => {
            assert_eq!(*p, iri("http://example.org/pizza#hasCalories"));
        }
        other => panic!("expected AxiomDeclaration(DataPropertyDeclaration), got {other:?}"),
    }
}

#[test]
fn declaration_annotation_property() {
    let src = wrap(
        "",
        r#"<Declaration><AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::AnnotationPropertyDeclaration(p))) => {
            assert_eq!(*p, iri("http://www.w3.org/2000/01/rdf-schema#comment"));
        }
        other => panic!("expected AxiomDeclaration(AnnotationPropertyDeclaration), got {other:?}"),
    }
}

#[test]
fn declaration_named_individual() {
    let src = wrap(
        "",
        r#"<Declaration><NamedIndividual IRI="http://example.org/pizza#margherita1"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((
            _,
            Entity::NamedIndividualDeclaration(owl_ontology::Individual::NamedIndividual(i)),
        )) => {
            assert_eq!(*i, iri("http://example.org/pizza#margherita1"));
        }
        other => panic!("expected AxiomDeclaration(NamedIndividualDeclaration), got {other:?}"),
    }
}

#[test]
fn declaration_datatype() {
    let src = wrap(
        "",
        r#"<Declaration><Datatype IRI="http://www.w3.org/2001/XMLSchema#positiveInteger"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::DatatypeDeclaration(d))) => {
            assert_eq!(*d, iri("http://www.w3.org/2001/XMLSchema#positiveInteger"));
        }
        other => panic!("expected AxiomDeclaration(DatatypeDeclaration), got {other:?}"),
    }
}

#[test]
fn multiple_declarations_preserve_order() {
    let src = wrap(
        "",
        r#"<Declaration><Class IRI="http://example.org/pizza#Pizza"/></Declaration>
           <Declaration><Class IRI="http://example.org/pizza#Food"/></Declaration>
           <Declaration><ObjectProperty IRI="http://example.org/pizza#hasTopping"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 3);
}

// --- Phase 5: annotations (ontology-level, and on a Declaration) -----------

#[test]
fn ontology_level_annotation() {
    let src = wrap(
        "",
        r#"<Annotation>
             <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
             <Literal>An example ontology</Literal>
           </Annotation>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.annotations.len(), 1);
    assert_eq!(
        onto.annotations[0].0,
        iri("http://www.w3.org/2000/01/rdf-schema#comment")
    );
}

#[test]
fn declaration_with_leading_annotation() {
    let src = wrap(
        "",
        r#"<Declaration>
             <Annotation>
               <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
               <Literal>A pizza</Literal>
             </Annotation>
             <Class IRI="http://example.org/pizza#Pizza"/>
           </Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 1);
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((anns, Entity::ClassDeclaration(c))) => {
            assert_eq!(anns.len(), 1);
            assert_eq!(*c, iri("http://example.org/pizza#Pizza"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

// --- Phase 6: full-document integration -------------------------------------

#[test]
fn pizza_style_header_integration() {
    let src = format!(
        r#"<?xml version="1.0"?><Ontology {NS} ontologyIRI="http://example.org/pizza">
           <Prefix name="" IRI="http://example.org/pizza#"/>
           <Prefix name="owl" IRI="http://www.w3.org/2002/07/owl#"/>
           <Import>http://example.org/imported</Import>
           <Annotation>
             <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
             <Literal>An example ontology</Literal>
           </Annotation>
           <Declaration><Class abbreviatedIRI=":Pizza"/></Declaration>
           <Declaration><Class abbreviatedIRI=":Food"/></Declaration>
           <Declaration><ObjectProperty abbreviatedIRI=":hasTopping"/></Declaration>
           </Ontology>"#
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(
        onto.try_get_ontology_iri(),
        Some(&IriReference("http://example.org/pizza".to_string()))
    );
    assert_eq!(
        onto.directly_imports_documents,
        vec![IriReference("http://example.org/imported".to_string())]
    );
    assert_eq!(onto.annotations.len(), 1);
    assert_eq!(onto.axioms.len(), 3);
}

// === #606: class expressions and class axioms ==============================
// See docs/plans/OWL_XML_PLAN.md's "#606" section for the grammar subset.

fn class_axiom(onto: &owl_ontology::Ontology) -> &ClassAxiom {
    match &onto.axioms[0] {
        Axiom::AxiomClassAxiom(ca) => ca,
        other => panic!("expected AxiomClassAxiom, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn sub_class_of_named_classes() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <Class IRI="http://example.org/pizza#Food"/>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 1);
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(anns, sub, sup) => {
            assert!(anns.is_empty());
            assert_eq!(*sub, ClassExpression::ClassName(iri("http://example.org/pizza#Pizza")));
            assert_eq!(*sup, ClassExpression::ClassName(iri("http://example.org/pizza#Food")));
        }
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn equivalent_classes_three_way() {
    let src = wrap(
        "",
        r#"<EquivalentClasses>
             <Class IRI="http://example.org/pizza#A"/>
             <Class IRI="http://example.org/pizza#B"/>
             <Class IRI="http://example.org/pizza#C"/>
           </EquivalentClasses>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::EquivalentClasses(anns, ces) => {
            assert!(anns.is_empty());
            assert_eq!(ces.len(), 3);
        }
        other => panic!("expected EquivalentClasses, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn disjoint_classes() {
    let src = wrap(
        "",
        r#"<DisjointClasses>
             <Class IRI="http://example.org/pizza#Meat"/>
             <Class IRI="http://example.org/pizza#Vegetable"/>
           </DisjointClasses>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::DisjointClasses(anns, ces) => {
            assert!(anns.is_empty());
            assert_eq!(ces.len(), 2);
        }
        other => panic!("expected DisjointClasses, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn disjoint_union() {
    let src = wrap(
        "",
        r#"<DisjointUnion>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <Class IRI="http://example.org/pizza#MeatPizza"/>
             <Class IRI="http://example.org/pizza#VegetablePizza"/>
           </DisjointUnion>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::DisjointUnion(anns, class, ces) => {
            assert!(anns.is_empty());
            assert_eq!(*class, iri("http://example.org/pizza#Pizza"));
            assert_eq!(ces.len(), 2);
        }
        other => panic!("expected DisjointUnion, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn object_intersection_union_complement_operands() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <ObjectIntersectionOf>
               <Class IRI="http://example.org/pizza#Food"/>
               <ObjectComplementOf>
                 <Class IRI="http://example.org/pizza#Meat"/>
               </ObjectComplementOf>
             </ObjectIntersectionOf>
             <ObjectUnionOf>
               <Class IRI="http://example.org/pizza#Veggie"/>
               <Class IRI="http://example.org/pizza#Vegan"/>
             </ObjectUnionOf>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, sub, sup) => {
            match sub {
                ClassExpression::ObjectIntersectionOf(v) => {
                    assert_eq!(v.len(), 2);
                    assert!(matches!(v[1], ClassExpression::ObjectComplementOf(_)));
                }
                other => panic!("expected ObjectIntersectionOf, got {other:?}"),
            }
            match sup {
                ClassExpression::ObjectUnionOf(v) => assert_eq!(v.len(), 2),
                other => panic!("expected ObjectUnionOf, got {other:?}"),
            }
        }
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn object_one_of_named_individuals() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <ObjectOneOf>
               <NamedIndividual IRI="http://example.org/pizza#Alice"/>
               <NamedIndividual IRI="http://example.org/pizza#Bob"/>
             </ObjectOneOf>
             <Class IRI="http://example.org/pizza#Person"/>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, sub, _) => match sub {
            ClassExpression::ObjectOneOf(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(
                    v[0],
                    Individual::NamedIndividual(iri("http://example.org/pizza#Alice"))
                );
            }
            other => panic!("expected ObjectOneOf, got {other:?}"),
        },
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn object_one_of_rejects_anonymous_individual() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <ObjectOneOf>
               <AnonymousIndividual nodeID="x"/>
             </ObjectOneOf>
             <Class IRI="http://example.org/pizza#Person"/>
           </SubClassOf>"#,
    );
    match owl_xml_parser::parse(&src) {
        Err(e) => assert!(e.contains("608")),
        Ok(_) => panic!("expected an error for AnonymousIndividual (see #608)"),
    }
}

#[test]
#[ignore] // #606
fn object_some_and_all_values_from() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <ObjectIntersectionOf>
               <ObjectSomeValuesFrom>
                 <ObjectProperty IRI="http://example.org/pizza#hasTopping"/>
                 <Class IRI="http://example.org/pizza#Topping"/>
               </ObjectSomeValuesFrom>
               <ObjectAllValuesFrom>
                 <ObjectProperty IRI="http://example.org/pizza#hasTopping"/>
                 <Class IRI="http://example.org/pizza#Topping"/>
               </ObjectAllValuesFrom>
             </ObjectIntersectionOf>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, _, sup) => match sup {
            ClassExpression::ObjectIntersectionOf(v) => {
                assert!(matches!(v[0], ClassExpression::ObjectSomeValuesFrom(_, _)));
                assert!(matches!(v[1], ClassExpression::ObjectAllValuesFrom(_, _)));
            }
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        },
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn object_has_value_and_has_self_and_inverse_of() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <ObjectIntersectionOf>
               <ObjectHasValue>
                 <ObjectProperty IRI="http://example.org/pizza#hasTopping"/>
                 <NamedIndividual IRI="http://example.org/pizza#Mushroom"/>
               </ObjectHasValue>
               <ObjectHasSelf>
                 <ObjectInverseOf>
                   <ObjectProperty IRI="http://example.org/pizza#likes"/>
                 </ObjectInverseOf>
               </ObjectHasSelf>
             </ObjectIntersectionOf>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, _, sup) => match sup {
            ClassExpression::ObjectIntersectionOf(v) => {
                assert!(matches!(v[0], ClassExpression::ObjectHasValue(_, _)));
                match &v[1] {
                    ClassExpression::ObjectHasSelf(p) => assert!(matches!(
                        p,
                        owl_ontology::ObjectPropertyExpression::InverseObjectProperty(_)
                    )),
                    other => panic!("expected ObjectHasSelf, got {other:?}"),
                }
            }
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        },
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn object_cardinalities_qualified_and_unqualified() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <ObjectIntersectionOf>
               <ObjectMinCardinality cardinality="1">
                 <ObjectProperty IRI="http://example.org/pizza#hasTopping"/>
               </ObjectMinCardinality>
               <ObjectMaxCardinality cardinality="3">
                 <ObjectProperty IRI="http://example.org/pizza#hasTopping"/>
                 <Class IRI="http://example.org/pizza#Topping"/>
               </ObjectMaxCardinality>
               <ObjectExactCardinality cardinality="2">
                 <ObjectProperty IRI="http://example.org/pizza#hasTopping"/>
               </ObjectExactCardinality>
             </ObjectIntersectionOf>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, _, sup) => match sup {
            ClassExpression::ObjectIntersectionOf(v) => {
                assert!(matches!(
                    v[0],
                    ClassExpression::ObjectMinCardinality(ref n, _) if *n == num_bigint::BigInt::from(1)
                ));
                assert!(matches!(
                    v[1],
                    ClassExpression::ObjectMaxQualifiedCardinality(ref n, _, _) if *n == num_bigint::BigInt::from(3)
                ));
                assert!(matches!(
                    v[2],
                    ClassExpression::ObjectExactCardinality(ref n, _) if *n == num_bigint::BigInt::from(2)
                ));
            }
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        },
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn data_some_and_all_values_from_with_compound_range() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <ObjectIntersectionOf>
               <DataSomeValuesFrom>
                 <DataProperty IRI="http://example.org/pizza#hasCalories"/>
                 <DataUnionOf>
                   <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
                   <Datatype IRI="http://www.w3.org/2001/XMLSchema#decimal"/>
                 </DataUnionOf>
               </DataSomeValuesFrom>
               <DataAllValuesFrom>
                 <DataProperty IRI="http://example.org/pizza#hasCalories"/>
                 <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
               </DataAllValuesFrom>
             </ObjectIntersectionOf>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, _, sup) => match sup {
            ClassExpression::ObjectIntersectionOf(v) => {
                match &v[0] {
                    ClassExpression::DataSomeValuesFrom(props, dr) => {
                        assert_eq!(props.len(), 1);
                        assert!(matches!(dr, owl_ontology::DataRange::DataUnionOf(_)));
                    }
                    other => panic!("expected DataSomeValuesFrom, got {other:?}"),
                }
                assert!(matches!(v[1], ClassExpression::DataAllValuesFrom(_, _)));
            }
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        },
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn data_has_value_and_cardinalities_with_datatype_restriction() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <ObjectIntersectionOf>
               <DataHasValue>
                 <DataProperty IRI="http://example.org/pizza#hasCalories"/>
                 <Literal>500</Literal>
               </DataHasValue>
               <DataMinCardinality cardinality="1">
                 <DataProperty IRI="http://example.org/pizza#hasCalories"/>
               </DataMinCardinality>
               <DataMaxCardinality cardinality="1">
                 <DataProperty IRI="http://example.org/pizza#hasCalories"/>
                 <DatatypeRestriction>
                   <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
                   <FacetRestriction facet="http://www.w3.org/2001/XMLSchema#minInclusive">
                     <Literal>0</Literal>
                   </FacetRestriction>
                 </DatatypeRestriction>
               </DataMaxCardinality>
             </ObjectIntersectionOf>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, _, sup) => match sup {
            ClassExpression::ObjectIntersectionOf(v) => {
                assert!(matches!(v[0], ClassExpression::DataHasValue(_, _)));
                assert!(matches!(
                    v[1],
                    ClassExpression::DataMinCardinality(ref n, _) if *n == num_bigint::BigInt::from(1)
                ));
                match &v[2] {
                    ClassExpression::DataMaxQualifiedCardinality(n, _, dr) => {
                        assert_eq!(*n, num_bigint::BigInt::from(1));
                        assert!(matches!(dr, owl_ontology::DataRange::DatatypeRestriction(_, facets) if facets.len() == 1));
                    }
                    other => panic!("expected DataMaxQualifiedCardinality, got {other:?}"),
                }
            }
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        },
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn deeply_nested_class_expression() {
    // Union containing an intersection containing a restriction -- the
    // exact shape #606 flags as most bug-prone for hand-rolled
    // recursive-descent XML parsers.
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <ObjectUnionOf>
               <Class IRI="http://example.org/pizza#Food"/>
               <ObjectIntersectionOf>
                 <Class IRI="http://example.org/pizza#Meal"/>
                 <ObjectSomeValuesFrom>
                   <ObjectProperty IRI="http://example.org/pizza#hasTopping"/>
                   <ObjectOneOf>
                     <NamedIndividual IRI="http://example.org/pizza#Mushroom"/>
                   </ObjectOneOf>
                 </ObjectSomeValuesFrom>
               </ObjectIntersectionOf>
             </ObjectUnionOf>
           </SubClassOf>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match class_axiom(&onto) {
        ClassAxiom::SubClassOf(_, _, sup) => match sup {
            ClassExpression::ObjectUnionOf(v) => {
                assert_eq!(v.len(), 2);
                match &v[1] {
                    ClassExpression::ObjectIntersectionOf(inner) => {
                        assert_eq!(inner.len(), 2);
                        match &inner[1] {
                            ClassExpression::ObjectSomeValuesFrom(_, filler) => {
                                assert!(matches!(**filler, ClassExpression::ObjectOneOf(_)));
                            }
                            other => panic!("expected ObjectSomeValuesFrom, got {other:?}"),
                        }
                    }
                    other => panic!("expected ObjectIntersectionOf, got {other:?}"),
                }
            }
            other => panic!("expected ObjectUnionOf, got {other:?}"),
        },
        other => panic!("expected SubClassOf, got {other:?}"),
    }
}

#[test]
#[ignore] // #606
fn class_axiom_annotation_children_deferred_to_608() {
    let src = wrap(
        "",
        r#"<SubClassOf>
             <Annotation>
               <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
               <Literal>why</Literal>
             </Annotation>
             <Class IRI="http://example.org/pizza#Pizza"/>
             <Class IRI="http://example.org/pizza#Food"/>
           </SubClassOf>"#,
    );
    match owl_xml_parser::parse(&src) {
        Err(e) => assert!(e.contains("608")),
        Ok(_) => panic!("expected an error for axiom-level Annotation (see #608)"),
    }
}

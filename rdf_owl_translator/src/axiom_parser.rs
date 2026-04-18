/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Translates individual RDF triples into OWL 2 axioms.
//! Implements Table 16/17 from <https://www.w3.org/TR/owl2-mapping-to-rdf/>.
//! Mirrors `DagSemTools.RdfOwlTranslator.AxiomParser`.

use crate::class_expression_parser::OntologyDeclarations;
use crate::ingress::{WellKnownIds, get_rdf_list_elements, try_get_individual};
use dag_rdf::datastore::Datastore;
use dag_rdf::ingress::Triple;
use ingress::*;
use owl_ontology::*;

/// Get any axiom annotations for a triple (Table 17 in the spec).
fn get_axiom_annotations(
    datastore: &Datastore,
    ids: &WellKnownIds,
    decls: &OntologyDeclarations,
    triple: &Triple,
) -> Vec<Annotation> {
    datastore
        .get_triples_with_object_predicate(triple.subject, ids.owl_annotated_source_id)
        .filter(|src_tr| {
            let ax = src_tr.subject;
            datastore.contains_triple(&Triple {
                subject: ax,
                predicate: ids.rdf_type_id,
                obj: ids.owl_axiom_id,
            }) && datastore.contains_triple(&Triple {
                subject: ax,
                predicate: ids.owl_annotated_property_id,
                obj: triple.predicate,
            }) && datastore.contains_triple(&Triple {
                subject: ax,
                predicate: ids.owl_annotated_target_id,
                obj: triple.obj,
            })
        })
        .flat_map(|src_tr| decls.get_annotations(src_tr.subject))
        .collect()
}

/// Extract a single OWL axiom from an RDF triple, if applicable.
/// Returns None if the triple doesn't encode an OWL axiom.
pub fn extract_axiom(
    datastore: &Datastore,
    ids: &WellKnownIds,
    decls: &OntologyDeclarations,
    triple: &Triple,
) -> Option<Axiom> {
    let res = &datastore.resources;

    let predicate_iri = res.get_named_resource(triple.predicate)?;
    let predicate_str = predicate_iri.0.as_str();

    let axiom_anns = get_axiom_annotations(datastore, ids, decls, triple);

    match predicate_str {
        // ── rdf:type ────────────────────────────────────────────────────────
        s if s == RDF_TYPE => {
            let obj_iri = res.get_named_resource(triple.obj)?;
            match obj_iri.0.as_str() {
                s if s == OWL_CLASS => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::ClassDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                s if s == RDFS_DATATYPE => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::DatatypeDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                s if s == OWL_OBJECT_PROPERTY => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::ObjectPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                s if s == OWL_DATATYPE_PROPERTY => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::DataPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                s if s == OWL_ANNOTATION_PROPERTY => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::AnnotationPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                s if s == OWL_NAMED_INDIVIDUAL => {
                    let subj_gel = res.get_graph_element(triple.subject);
                    let individual = try_get_individual(subj_gel);
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::NamedIndividualDeclaration(individual),
                    )))
                }
                s if s == OWL_ALL_DISJOINT_CLASSES => {
                    // owl:members list of class expressions
                    let members_triples: Vec<Triple> = datastore
                        .get_triples_with_subject_predicate(triple.subject, ids.owl_members_id)
                        .collect();
                    match members_triples.as_slice() {
                        [] => None,
                        [mt] => {
                            let list = get_rdf_list_elements(
                                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                                ids,
                                mt.obj,
                            );
                            let ces: Vec<ClassExpression> = list
                                .iter()
                                .map(|&id| decls.class_expression(id, res))
                                .collect();
                            Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                                axiom_anns,
                                ces,
                            )))
                        }
                        _ => panic!("Multiple owl:members on owl:AllDisjointClasses"),
                    }
                }
                s if s == OWL_ALL_DISJOINT_PROPERTIES => {
                    let members_triples: Vec<Triple> = datastore
                        .get_triples_with_subject_predicate(triple.subject, ids.owl_members_id)
                        .collect();
                    match members_triples.as_slice() {
                        [] => None,
                        [mt] => {
                            let list = get_rdf_list_elements(
                                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                                ids,
                                mt.obj,
                            );
                            let opes: Vec<ObjectPropertyExpression> = list
                                .iter()
                                .map(|&id| decls.object_property_expression(id, res))
                                .collect();
                            Some(Axiom::AxiomObjectPropertyAxiom(
                                ObjectPropertyAxiom::DisjointObjectProperties(axiom_anns, opes),
                            ))
                        }
                        _ => panic!("Multiple owl:members on owl:AllDisjointProperties"),
                    }
                }
                s if s == OWL_FUNCTIONAL_PROPERTY => {
                    Some(decls.object_or_data_property(
                        triple.subject,
                        res,
                        |ope| Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::FunctionalObjectProperty(axiom_anns.clone(), ope)),
                        |dp| Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::FunctionalDataProperty(axiom_anns.clone(), dp)),
                    ))
                }
                s if s == OWL_INVERSE_FUNCTIONAL_PROPERTY => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::InverseFunctionalObjectProperty(axiom_anns, ope),
                    ))
                }
                s if s == OWL_REFLEXIVE_PROPERTY => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::ReflexiveObjectProperty(axiom_anns, ope),
                    ))
                }
                s if s == OWL_IRREFLEXIVE_PROPERTY => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::IrreflexiveObjectProperty(axiom_anns, ope),
                    ))
                }
                s if s == OWL_SYMMETRIC_PROPERTY => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::SymmetricObjectProperty(axiom_anns, ope),
                    ))
                }
                s if s == OWL_ASYMMETRIC_PROPERTY => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::AsymmetricObjectProperty(axiom_anns, ope),
                    ))
                }
                s if s == OWL_TRANSITIVE_PROPERTY => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::TransitiveObjectProperty(axiom_anns, ope),
                    ))
                }
                _ => {
                    // ClassAssertion: :x rdf:type C
                    let ce = decls.class_expression(triple.obj, res);
                    let subj_gel = res.get_graph_element(triple.subject);
                    let individual = try_get_individual(subj_gel);
                    Some(Axiom::AxiomAssertion(Assertion::ClassAssertion(
                        axiom_anns,
                        ce,
                        individual,
                    )))
                }
            }
        }

        // ── rdfs:subClassOf ─────────────────────────────────────────────────
        s if s == RDFS_SUB_CLASS_OF => {
            let sub_ce = decls.class_expression(triple.subject, res);
            let sup_ce = decls.class_expression(triple.obj, res);
            Some(Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                axiom_anns,
                sub_ce,
                sup_ce,
            )))
        }

        // ── owl:equivalentClass ─────────────────────────────────────────────
        s if s == OWL_EQUIVALENT_CLASS => {
            // Could be class or datatype definition
            if let Some(dr) = decls.data_ranges.get(&triple.obj) {
                // Datatype definition: subj ≡ dr
                if let Some(DataRange::NamedDataRange(dtype)) = decls.data_ranges.get(&triple.subject) {
                    return Some(Axiom::AxiomDatatypeDefinition(
                        axiom_anns,
                        dtype.clone(),
                        dr.clone(),
                    ));
                }
            }
            let sub_ce = decls.class_expression(triple.subject, res);
            let obj_ce = decls.class_expression(triple.obj, res);
            Some(Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                axiom_anns,
                vec![sub_ce, obj_ce],
            )))
        }

        // ── owl:disjointWith ────────────────────────────────────────────────
        s if s == OWL_DISJOINT_WITH => {
            let sub_ce = decls.class_expression(triple.subject, res);
            let obj_ce = decls.class_expression(triple.obj, res);
            Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                axiom_anns,
                vec![sub_ce, obj_ce],
            )))
        }

        // ── owl:disjointUnionOf ─────────────────────────────────────────────
        s if s == OWL_DISJOINT_UNION_OF => {
            let class_iri = res.get_named_resource(triple.subject)?;
            let list = get_rdf_list_elements(
                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                ids,
                triple.obj,
            );
            let ces: Vec<ClassExpression> = list.iter().map(|&id| decls.class_expression(id, res)).collect();
            Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(
                axiom_anns,
                FullIri(class_iri.clone()),
                ces,
            )))
        }

        // ── rdfs:subPropertyOf ──────────────────────────────────────────────
        s if s == RDFS_SUB_PROPERTY_OF => {
            let obj_ope = decls.object_property_expression(triple.obj, res);
            Some(decls.object_or_data_property(
                triple.subject,
                res,
                |ope| {
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
                        axiom_anns.clone(),
                        SubPropertyExpression::SubObjectPropertyExpression(ope),
                        obj_ope.clone(),
                    ))
                },
                |dp| {
                    let sup_dp = decls.data_property_expression(triple.obj, res);
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::SubDataPropertyOf(
                        axiom_anns.clone(),
                        dp,
                        sup_dp,
                    ))
                },
            ))
        }

        // ── owl:propertyChainAxiom ──────────────────────────────────────────
        s if s == OWL_PROPERTY_CHAIN_AXIOM => {
            let list = get_rdf_list_elements(
                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                ids,
                triple.obj,
            );
            let chain: Vec<ObjectPropertyExpression> =
                list.iter().map(|&id| decls.object_property_expression(id, res)).collect();
            let sup = decls.object_property_expression(triple.subject, res);
            Some(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::SubObjectPropertyOf(
                    axiom_anns,
                    SubPropertyExpression::PropertyExpressionChain(chain),
                    sup,
                ),
            ))
        }

        // ── owl:equivalentProperty ─────────────────────────────────────────
        s if s == OWL_EQUIVALENT_PROPERTY => {
            Some(decls.object_or_data_property(
                triple.subject,
                res,
                |ope| {
                    let obj_ope = decls.object_property_expression(triple.obj, res);
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::EquivalentObjectProperties(
                        axiom_anns.clone(),
                        vec![ope, obj_ope],
                    ))
                },
                |dp| {
                    let obj_dp = decls.data_property_expression(triple.obj, res);
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::EquivalentDataProperties(
                        axiom_anns.clone(),
                        vec![dp, obj_dp],
                    ))
                },
            ))
        }

        // ── owl:propertyDisjointWith ────────────────────────────────────────
        s if s == OWL_PROPERTY_DISJOINT_WITH => {
            let ope1 = decls.object_property_expression(triple.subject, res);
            let ope2 = decls.object_property_expression(triple.obj, res);
            Some(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::DisjointObjectProperties(axiom_anns, vec![ope1, ope2]),
            ))
        }

        // ── rdfs:domain ─────────────────────────────────────────────────────
        s if s == RDFS_DOMAIN => {
            let range_ce = decls.class_expression(triple.obj, res);
            Some(decls.object_or_data_property(
                triple.subject,
                res,
                |ope| {
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyDomain(
                        ope, range_ce.clone(),
                    ))
                },
                |dp| {
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyDomain(
                        axiom_anns.clone(),
                        dp,
                        range_ce.clone(),
                    ))
                },
            ))
        }

        // ── rdfs:range ──────────────────────────────────────────────────────
        s if s == RDFS_RANGE => {
            Some(decls.object_or_data_property(
                triple.subject,
                res,
                |ope| {
                    let obj_ce = decls.class_expression(triple.obj, res);
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyRange(
                        ope, obj_ce,
                    ))
                },
                |dp| {
                    let obj_dr = decls.data_range(triple.obj, res);
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyRange(
                        axiom_anns.clone(),
                        dp,
                        obj_dr,
                    ))
                },
            ))
        }

        // ── owl:inverseOf ───────────────────────────────────────────────────
        s if s == OWL_OBJECT_INVERSE_OF => {
            let ope1 = decls.object_property_expression(triple.subject, res);
            let ope2 = decls.object_property_expression(triple.obj, res);
            Some(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::InverseObjectProperties(axiom_anns, ope1, ope2),
            ))
        }

        // ── owl:sameAs ──────────────────────────────────────────────────────
        s if s == OWL_SAME_AS => {
            let ind1 = try_get_individual(res.get_graph_element(triple.subject));
            let ind2 = try_get_individual(res.get_graph_element(triple.obj));
            Some(Axiom::AxiomAssertion(Assertion::SameIndividual(
                axiom_anns,
                vec![ind1, ind2],
            )))
        }

        // ── Data / Object property assertions ───────────────────────────────
        _ => {
            // Check if predicate is a known object or data property
            let is_obj_prop = decls.object_property_expressions.contains_key(&triple.predicate);
            let is_data_prop = decls.data_property_expressions.contains_key(&triple.predicate);

            if is_obj_prop {
                let ope = decls.object_property_expression(triple.predicate, res);
                let subj_ind = try_get_individual(res.get_graph_element(triple.subject));
                let obj_ind = try_get_individual(res.get_graph_element(triple.obj));
                Some(Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(
                    axiom_anns,
                    ope,
                    subj_ind,
                    obj_ind,
                )))
            } else if is_data_prop {
                let dp = decls.data_property_expression(triple.predicate, res);
                let subj_ind = try_get_individual(res.get_graph_element(triple.subject));
                let obj_gel = res.get_graph_element(triple.obj).clone();
                Some(Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(
                    axiom_anns,
                    dp,
                    subj_ind,
                    obj_gel,
                )))
            } else {
                None
            }
        }
    }
}

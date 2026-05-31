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
use dag_rdf::GraphElementId;
use dag_rdf::datastore::Datastore;
use dag_rdf::ingress::Triple;
use owl_ontology::*;

/// All predicate IDs that `extract_axiom` handles in its non-wildcard arms.
///
/// Co-located with `extract_axiom` so the list and the match stay in sync.
/// `rdf2owl` iterates exactly these predicates (plus declared object/data
/// property predicates) to avoid scanning triples that can never produce axioms.
pub fn axiom_structural_predicate_ids(ids: &WellKnownIds) -> Vec<GraphElementId> {
    vec![
        ids.rdf_type_id,
        ids.rdfs_sub_class_of_id,
        ids.owl_equivalent_class_id,
        ids.owl_disjoint_with_id,
        ids.owl_disjoint_union_of_id,
        ids.rdfs_sub_property_of_id,
        ids.owl_property_chain_axiom_id,
        ids.owl_equivalent_property_id,
        ids.owl_property_disjoint_with_id,
        ids.rdfs_domain_id,
        ids.rdfs_range_id,
        ids.owl_object_inverse_of_id,
        ids.owl_same_as_id,
    ]
}

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
///
/// Dispatch is by `GraphElementId` (u32) rather than IRI string for O(1) matching.
pub fn extract_axiom(
    datastore: &Datastore,
    ids: &WellKnownIds,
    decls: &OntologyDeclarations,
    triple: &Triple,
) -> Option<Axiom> {
    let res = &datastore.resources;
    let axiom_anns = get_axiom_annotations(datastore, ids, decls, triple);

    match triple.predicate {
        // ── rdf:type ────────────────────────────────────────────────────────
        p if p == ids.rdf_type_id => {
            match triple.obj {
                o if o == ids.owl_class_id => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::ClassDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.rdfs_datatype_id => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::DatatypeDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_object_property_id => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::ObjectPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_datatype_property_id => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::DataPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_annotation_property_id => {
                    let subj_iri = res.get_named_resource(triple.subject)?;
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::AnnotationPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_named_individual_id => {
                    let subj_gel = res.get_graph_element(triple.subject);
                    let individual = try_get_individual(subj_gel);
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::NamedIndividualDeclaration(individual),
                    )))
                }
                o if o == ids.owl_all_disjoint_classes_id => {
                    let members_triples: Vec<Triple> = datastore
                        .get_triples_with_subject_predicate(triple.subject, ids.owl_members_id)
                        .collect();
                    match members_triples.as_slice() {
                        [] => None,
                        [mt] => {
                            let list = get_rdf_list_elements(
                                &|s, p| {
                                    datastore.get_triples_with_subject_predicate(s, p).collect()
                                },
                                ids,
                                mt.obj,
                            );
                            let ces: Vec<ClassExpression> = list
                                .iter()
                                .map(|&id| decls.class_expression(id, res))
                                .collect();
                            Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                                axiom_anns, ces,
                            )))
                        }
                        _ => panic!("Multiple owl:members on owl:AllDisjointClasses"),
                    }
                }
                o if o == ids.owl_all_disjoint_properties_id => {
                    let members_triples: Vec<Triple> = datastore
                        .get_triples_with_subject_predicate(triple.subject, ids.owl_members_id)
                        .collect();
                    match members_triples.as_slice() {
                        [] => None,
                        [mt] => {
                            let list = get_rdf_list_elements(
                                &|s, p| {
                                    datastore.get_triples_with_subject_predicate(s, p).collect()
                                },
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
                o if o == ids.owl_functional_property_id => Some(decls.object_or_data_property(
                    triple.subject,
                    res,
                    |ope| {
                        Axiom::AxiomObjectPropertyAxiom(
                            ObjectPropertyAxiom::FunctionalObjectProperty(axiom_anns.clone(), ope),
                        )
                    },
                    |dp| {
                        Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::FunctionalDataProperty(
                            axiom_anns.clone(),
                            dp,
                        ))
                    },
                )),
                o if o == ids.owl_inverse_functional_property_id => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::InverseFunctionalObjectProperty(axiom_anns, ope),
                    ))
                }
                o if o == ids.owl_reflexive_property_id => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::ReflexiveObjectProperty(axiom_anns, ope),
                    ))
                }
                o if o == ids.owl_irreflexive_property_id => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::IrreflexiveObjectProperty(axiom_anns, ope),
                    ))
                }
                o if o == ids.owl_symmetric_property_id => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::SymmetricObjectProperty(axiom_anns, ope),
                    ))
                }
                o if o == ids.owl_asymmetric_property_id => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::AsymmetricObjectProperty(axiom_anns, ope),
                    ))
                }
                o if o == ids.owl_transitive_property_id => {
                    let ope = decls.object_property_expression(triple.subject, res);
                    Some(Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::TransitiveObjectProperty(axiom_anns, ope),
                    ))
                }
                _ => {
                    // ClassAssertion: :x rdf:type C
                    // Preserve original semantics: blank-node objects return None.
                    res.get_named_resource(triple.obj)?;
                    let ce = decls.class_expression(triple.obj, res);
                    let subj_gel = res.get_graph_element(triple.subject);
                    let individual = try_get_individual(subj_gel);
                    Some(Axiom::AxiomAssertion(Assertion::ClassAssertion(
                        axiom_anns, ce, individual,
                    )))
                }
            }
        }

        // ── rdfs:subClassOf ─────────────────────────────────────────────────
        p if p == ids.rdfs_sub_class_of_id => {
            let sub_ce = decls.class_expression(triple.subject, res);
            let sup_ce = decls.class_expression(triple.obj, res);
            Some(Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                axiom_anns, sub_ce, sup_ce,
            )))
        }

        // ── owl:equivalentClass ─────────────────────────────────────────────
        p if p == ids.owl_equivalent_class_id => {
            if let (Some(dr), Some(DataRange::NamedDataRange(dtype))) = (
                decls.data_ranges.get(&triple.obj),
                decls.data_ranges.get(&triple.subject),
            ) {
                return Some(Axiom::AxiomDatatypeDefinition(
                    axiom_anns,
                    dtype.clone(),
                    dr.clone(),
                ));
            }
            let sub_ce = decls.class_expression(triple.subject, res);
            let obj_ce = decls.class_expression(triple.obj, res);
            Some(Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                axiom_anns,
                vec![sub_ce, obj_ce],
            )))
        }

        // ── owl:disjointWith ────────────────────────────────────────────────
        p if p == ids.owl_disjoint_with_id => {
            let sub_ce = decls.class_expression(triple.subject, res);
            let obj_ce = decls.class_expression(triple.obj, res);
            Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                axiom_anns,
                vec![sub_ce, obj_ce],
            )))
        }

        // ── owl:disjointUnionOf ─────────────────────────────────────────────
        p if p == ids.owl_disjoint_union_of_id => {
            let class_iri = res.get_named_resource(triple.subject)?;
            let list = get_rdf_list_elements(
                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                ids,
                triple.obj,
            );
            let ces: Vec<ClassExpression> = list
                .iter()
                .map(|&id| decls.class_expression(id, res))
                .collect();
            Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(
                axiom_anns,
                FullIri(class_iri.clone()),
                ces,
            )))
        }

        // ── rdfs:subPropertyOf ──────────────────────────────────────────────
        p if p == ids.rdfs_sub_property_of_id => {
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
        p if p == ids.owl_property_chain_axiom_id => {
            let list = get_rdf_list_elements(
                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                ids,
                triple.obj,
            );
            let chain: Vec<ObjectPropertyExpression> = list
                .iter()
                .map(|&id| decls.object_property_expression(id, res))
                .collect();
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
        p if p == ids.owl_equivalent_property_id => Some(decls.object_or_data_property(
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
        )),

        // ── owl:propertyDisjointWith ────────────────────────────────────────
        p if p == ids.owl_property_disjoint_with_id => {
            let ope1 = decls.object_property_expression(triple.subject, res);
            let ope2 = decls.object_property_expression(triple.obj, res);
            Some(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::DisjointObjectProperties(axiom_anns, vec![ope1, ope2]),
            ))
        }

        // ── rdfs:domain ─────────────────────────────────────────────────────
        p if p == ids.rdfs_domain_id => {
            let range_ce = decls.class_expression(triple.obj, res);
            Some(decls.object_or_data_property(
                triple.subject,
                res,
                |ope| {
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyDomain(
                        ope,
                        range_ce.clone(),
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
        p if p == ids.rdfs_range_id => Some(decls.object_or_data_property(
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
        )),

        // ── owl:inverseOf ───────────────────────────────────────────────────
        p if p == ids.owl_object_inverse_of_id => {
            let ope1 = decls.object_property_expression(triple.subject, res);
            let ope2 = decls.object_property_expression(triple.obj, res);
            Some(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::InverseObjectProperties(axiom_anns, ope1, ope2),
            ))
        }

        // ── owl:sameAs ──────────────────────────────────────────────────────
        p if p == ids.owl_same_as_id => {
            let ind1 = try_get_individual(res.get_graph_element(triple.subject));
            let ind2 = try_get_individual(res.get_graph_element(triple.obj));
            Some(Axiom::AxiomAssertion(Assertion::SameIndividual(
                axiom_anns,
                vec![ind1, ind2],
            )))
        }

        // ── Data / Object property assertions ───────────────────────────────
        _ => {
            let is_obj_prop = decls
                .object_property_expressions
                .contains_key(&triple.predicate);
            let is_data_prop = decls
                .data_property_expressions
                .contains_key(&triple.predicate);

            if is_obj_prop {
                let ope = decls.object_property_expression(triple.predicate, res);
                let subj_ind = try_get_individual(res.get_graph_element(triple.subject));
                let obj_ind = try_get_individual(res.get_graph_element(triple.obj));
                Some(Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(
                    axiom_anns, ope, subj_ind, obj_ind,
                )))
            } else if is_data_prop {
                let dp = decls.data_property_expression(triple.predicate, res);
                let subj_ind = try_get_individual(res.get_graph_element(triple.subject));
                let obj_gel = res.get_graph_element(triple.obj).clone();
                Some(Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(
                    axiom_anns, dp, subj_ind, obj_gel,
                )))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class_expression_parser::OntologyDeclarations;
    use crate::ingress::WellKnownIds;
    use dag_rdf::Datastore;

    fn sorted_axiom_debug_strings(axioms: &[Axiom]) -> Vec<String> {
        let mut v: Vec<String> = axioms.iter().map(|a| format!("{a:?}")).collect();
        v.sort();
        v
    }

    /// Verify that the indexed path produces the same axiom multiset as a direct
    /// full-scan on the same data.  Run against a variety of axiom types to catch
    /// missing entries in `axiom_structural_predicate_ids`.
    #[test]
    fn indexed_matches_full_scan() {
        let ttl = r#"
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex:   <http://example.org/> .

<http://example.org/ont> a owl:Ontology .

ex:Animal       a owl:Class .
ex:Dog          a owl:Class .
ex:Cat          a owl:Class .
ex:hasOwner     a owl:ObjectProperty .
ex:hasWeight    a owl:DatatypeProperty .
ex:label        a owl:AnnotationProperty .
ex:Fido         a owl:NamedIndividual .
ex:Whiskers     a owl:NamedIndividual .

ex:Dog          rdfs:subClassOf ex:Animal .
ex:Cat          rdfs:subClassOf ex:Animal .
ex:Dog          owl:disjointWith ex:Cat .
ex:Dog          owl:equivalentClass ex:Canine .
ex:hasOwner     rdfs:domain ex:Pet .
ex:hasOwner     rdfs:range  ex:Person .

ex:Fido         a ex:Dog .
ex:Fido         owl:sameAs ex:FidoAlt .
"#;
        let mut ds = Datastore::new(10_000);
        turtle_parser::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

        let ids = WellKnownIds::new(&mut ds.resources);
        let decls = OntologyDeclarations::build(&ds, &ids);

        // Indexed path
        use std::collections::HashSet;
        let mut pred_ids: HashSet<GraphElementId> =
            axiom_structural_predicate_ids(&ids).into_iter().collect();
        pred_ids.extend(decls.object_property_expressions.keys().copied());
        pred_ids.extend(decls.data_property_expressions.keys().copied());
        let indexed: Vec<Axiom> = pred_ids
            .iter()
            .flat_map(|&p| {
                ds.get_triples_with_predicate(p)
                    .filter_map(|t| extract_axiom(&ds, &ids, &decls, &t))
                    .collect::<Vec<_>>()
            })
            .collect();

        // Full-scan path (reference)
        let full_scan: Vec<Axiom> = ds
            .named_graphs
            .get_all_quads()
            .filter(|q| q.triple_id == dag_rdf::DEFAULT_GRAPH_ELEMENT_ID)
            .filter_map(|q| {
                let triple = dag_rdf::ingress::Triple {
                    subject: q.subject,
                    predicate: q.predicate,
                    obj: q.obj,
                };
                extract_axiom(&ds, &ids, &decls, &triple)
            })
            .collect();

        assert_eq!(
            sorted_axiom_debug_strings(&indexed),
            sorted_axiom_debug_strings(&full_scan),
            "indexed and full-scan paths produced different axiom multisets"
        );
    }
}

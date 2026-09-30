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
use crate::error::TranslatorError;
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
        ids.owl_different_from_id,
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
/// Returns `Ok(None)` if the triple doesn't encode an OWL axiom, and `Err`
/// if it does but one of the `rdf:List`s it references (`owl:members` on
/// `AllDisjointClasses`/`AllDisjointProperties`, `owl:disjointUnionOf`,
/// `owl:propertyChainAxiom`) is malformed. See
/// <https://github.com/daghovland/rdf-datalog/issues/363>.
///
/// Dispatch is by `GraphElementId` (u32) rather than IRI string for O(1) matching.
pub fn extract_axiom(
    datastore: &Datastore,
    ids: &WellKnownIds,
    decls: &OntologyDeclarations,
    triple: &Triple,
) -> Result<Option<Axiom>, TranslatorError> {
    let res = &datastore.resources;
    let axiom_anns = get_axiom_annotations(datastore, ids, decls, triple);

    Ok(match triple.predicate {
        // ── rdf:type ────────────────────────────────────────────────────────
        p if p == ids.rdf_type_id => {
            match triple.obj {
                o if o == ids.owl_class_id => {
                    let Some(subj_iri) = res.get_named_resource(triple.subject) else {
                        return Ok(None);
                    };
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::ClassDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.rdfs_datatype_id => {
                    let Some(subj_iri) = res.get_named_resource(triple.subject) else {
                        return Ok(None);
                    };
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::DatatypeDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_object_property_id => {
                    let Some(subj_iri) = res.get_named_resource(triple.subject) else {
                        return Ok(None);
                    };
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::ObjectPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_datatype_property_id => {
                    let Some(subj_iri) = res.get_named_resource(triple.subject) else {
                        return Ok(None);
                    };
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::DataPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_annotation_property_id => {
                    let Some(subj_iri) = res.get_named_resource(triple.subject) else {
                        return Ok(None);
                    };
                    Some(Axiom::AxiomDeclaration((
                        vec![],
                        Entity::AnnotationPropertyDeclaration(FullIri(subj_iri.clone())),
                    )))
                }
                o if o == ids.owl_named_individual_id => {
                    let subj_gel = res.get_graph_element(triple.subject);
                    let individual = try_get_individual(subj_gel)?;
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
                            )?;
                            let ces: Vec<ClassExpression> = list
                                .iter()
                                .map(|&id| decls.class_expression(id, res))
                                .collect();
                            Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                                axiom_anns, ces,
                            )))
                        }
                        _ => {
                            return Err(TranslatorError::MultipleOwlMembers(format!(
                                "owl:AllDisjointClasses {} has {} owl:members triples, expected at most 1",
                                triple.subject,
                                members_triples.len()
                            )));
                        }
                    }
                }
                // `owl:AllDisjointProperties` is the identical RDF encoding
                // for both n>2 `DisjointObjectProperties` and n>2
                // `DisjointDataProperties` (per #513/PR #669) -- the mapping
                // spec gives no way to tell them apart from the blank-node
                // structure alone. Disambiguate per-member using each
                // property's own declared kind, mirroring
                // `OntologyDeclarations::object_or_data_property`'s existing
                // pattern (used below for `owl:FunctionalProperty`): only
                // when *every* member is declared a data property (and none
                // an object property) is this read back as
                // `DisjointDataProperties`; an undeclared or mixed member set
                // defaults to `DisjointObjectProperties`, the pre-existing
                // behavior. See
                // https://github.com/daghovland/rdf-datalog/issues/668.
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
                            )?;
                            let all_data_properties = !list.is_empty()
                                && list
                                    .iter()
                                    .all(|id| decls.data_property_expressions.contains_key(id))
                                && list
                                    .iter()
                                    .all(|id| !decls.object_property_expressions.contains_key(id));
                            if all_data_properties {
                                let dps: Vec<DataProperty> = list
                                    .iter()
                                    .map(|&id| decls.data_property_expression(id, res))
                                    .collect();
                                Some(Axiom::AxiomDataPropertyAxiom(
                                    DataPropertyAxiom::DisjointDataProperties(axiom_anns, dps),
                                ))
                            } else {
                                let opes: Vec<ObjectPropertyExpression> = list
                                    .iter()
                                    .map(|&id| decls.object_property_expression(id, res))
                                    .collect();
                                Some(Axiom::AxiomObjectPropertyAxiom(
                                    ObjectPropertyAxiom::DisjointObjectProperties(axiom_anns, opes),
                                ))
                            }
                        }
                        _ => {
                            return Err(TranslatorError::MultipleOwlMembers(format!(
                                "owl:AllDisjointProperties {} has {} owl:members triples, expected at most 1",
                                triple.subject,
                                members_triples.len()
                            )));
                        }
                    }
                }
                // `owl:AllDifferent` (n>2 `DifferentIndividuals`), per
                // https://github.com/daghovland/rdf-datalog/issues/667 —
                // the read-back counterpart of #513's write-side
                // `Translator::all_disjoint` encoding.
                o if o == ids.owl_all_different_id => {
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
                            )?;
                            let individuals: Vec<Individual> = list
                                .iter()
                                .map(|&id| try_get_individual(res.get_graph_element(id)))
                                .collect::<Result<_, _>>()?;
                            Some(Axiom::AxiomAssertion(Assertion::DifferentIndividuals(
                                axiom_anns,
                                individuals,
                            )))
                        }
                        _ => {
                            return Err(TranslatorError::MultipleOwlMembers(format!(
                                "owl:AllDifferent {} has {} owl:members triples, expected at most 1",
                                triple.subject,
                                members_triples.len()
                            )));
                        }
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
                // `<iri> rdf:type owl:Ontology`: the ontology header
                // declaration, already consumed by
                // `translator::extract_ontology_name` into the resulting
                // `Ontology`'s `version` field
                // ([#515](https://github.com/daghovland/rdf-datalog/issues/515)).
                // Not a per-axiom construct — must not also fall through to
                // the generic ClassAssertion arm below, which would
                // spuriously assert "this ontology IRI is an individual of
                // class owl:Ontology".
                o if o == ids.owl_ontology_id => None,
                _ => {
                    // ClassAssertion: :x rdf:type C
                    // Preserve original semantics: blank-node objects return None.
                    if res.get_named_resource(triple.obj).is_none() {
                        return Ok(None);
                    }
                    let ce = decls.class_expression(triple.obj, res);
                    let subj_gel = res.get_graph_element(triple.subject);
                    let individual = try_get_individual(subj_gel)?;
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
                return Ok(Some(Axiom::AxiomDatatypeDefinition(
                    axiom_anns,
                    dtype.clone(),
                    dr.clone(),
                )));
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
            let Some(class_iri) = res.get_named_resource(triple.subject) else {
                return Ok(None);
            };
            let list = get_rdf_list_elements(
                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                ids,
                triple.obj,
            )?;
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
            )?;
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
                        axiom_anns.clone(),
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
                    axiom_anns.clone(),
                    ope,
                    obj_ce,
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
            let ind1 = try_get_individual(res.get_graph_element(triple.subject))?;
            let ind2 = try_get_individual(res.get_graph_element(triple.obj))?;
            Some(Axiom::AxiomAssertion(Assertion::SameIndividual(
                axiom_anns,
                vec![ind1, ind2],
            )))
        }

        // ── owl:differentFrom ───────────────────────────────────────────────
        p if p == ids.owl_different_from_id => {
            let ind1 = try_get_individual(res.get_graph_element(triple.subject))?;
            let ind2 = try_get_individual(res.get_graph_element(triple.obj))?;
            Some(Axiom::AxiomAssertion(Assertion::DifferentIndividuals(
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
                let subj_ind = try_get_individual(res.get_graph_element(triple.subject))?;
                let obj_ind = try_get_individual(res.get_graph_element(triple.obj))?;
                Some(Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(
                    axiom_anns, ope, subj_ind, obj_ind,
                )))
            } else if is_data_prop {
                let dp = decls.data_property_expression(triple.predicate, res);
                let subj_ind = try_get_individual(res.get_graph_element(triple.subject))?;
                let obj_gel = res.get_graph_element(triple.obj).clone();
                Some(Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(
                    axiom_anns, dp, subj_ind, obj_gel,
                )))
            } else {
                None
            }
        }
    })
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
        turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

        let ids = WellKnownIds::new(&mut ds.resources);
        let decls = OntologyDeclarations::build(&ds, &ids).unwrap();

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
                    .filter_map(|t| extract_axiom(&ds, &ids, &decls, &t).unwrap())
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
                extract_axiom(&ds, &ids, &decls, &triple).unwrap()
            })
            .collect();

        assert_eq!(
            sorted_axiom_debug_strings(&indexed),
            sorted_axiom_debug_strings(&full_scan),
            "indexed and full-scan paths produced different axiom multisets"
        );
    }

    /// Build a `Datastore` from an inline Turtle fixture, plus the
    /// `rdf:type` triple whose subject is `subject_iri` and whose object is
    /// `type_iri` — the triple `extract_axiom` dispatches on for
    /// `owl:AllDisjointClasses`/`owl:AllDisjointProperties`.
    fn extract_type_axiom(
        ttl: &str,
        subject_iri: &str,
        type_iri: &str,
    ) -> Result<Option<Axiom>, TranslatorError> {
        let mut ds = Datastore::new(1_000);
        turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();
        let ids = WellKnownIds::new(&mut ds.resources);
        let decls = OntologyDeclarations::build(&ds, &ids).unwrap();

        let triple = ds
            .get_triples_with_predicate(ids.rdf_type_id)
            .find(|tr| {
                ds.resources
                    .get_named_resource(tr.subject)
                    .map(|i| i.0.as_str())
                    == Some(subject_iri)
                    && ds
                        .resources
                        .get_named_resource(tr.obj)
                        .map(|i| i.0.as_str())
                        == Some(type_iri)
            })
            .unwrap_or_else(|| panic!("no rdf:type triple found for {subject_iri} a {type_iri}"));
        extract_axiom(&ds, &ids, &decls, &triple)
    }

    #[test]
    fn all_disjoint_classes_single_members_triple_succeeds() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:A a owl:Class .
ex:B a owl:Class .

ex:Disj a owl:AllDisjointClasses ;
    owl:members ( ex:A ex:B ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Disj",
            "http://www.w3.org/2002/07/owl#AllDisjointClasses",
        );
        assert!(
            matches!(
                result,
                Ok(Some(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                    _,
                    _
                ))))
            ),
            "expected a DisjointClasses axiom, got {result:?}"
        );
    }

    #[test]
    fn all_disjoint_classes_multiple_members_triples_returns_err() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:A a owl:Class .
ex:B a owl:Class .
ex:C a owl:Class .

ex:Disj a owl:AllDisjointClasses ;
    owl:members ( ex:A ex:B ) ;
    owl:members ( ex:B ex:C ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Disj",
            "http://www.w3.org/2002/07/owl#AllDisjointClasses",
        );
        assert!(
            matches!(result, Err(TranslatorError::MultipleOwlMembers(_))),
            "expected MultipleOwlMembers error, got {result:?}"
        );
    }

    #[test]
    fn all_disjoint_properties_single_members_triple_succeeds() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:p a owl:ObjectProperty .
ex:q a owl:ObjectProperty .

ex:Disj a owl:AllDisjointProperties ;
    owl:members ( ex:p ex:q ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Disj",
            "http://www.w3.org/2002/07/owl#AllDisjointProperties",
        );
        assert!(
            matches!(
                result,
                Ok(Some(Axiom::AxiomObjectPropertyAxiom(
                    ObjectPropertyAxiom::DisjointObjectProperties(_, _)
                )))
            ),
            "expected a DisjointObjectProperties axiom, got {result:?}"
        );
    }

    #[test]
    fn all_disjoint_properties_multiple_members_triples_returns_err() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:p a owl:ObjectProperty .
ex:q a owl:ObjectProperty .
ex:r a owl:ObjectProperty .

ex:Disj a owl:AllDisjointProperties ;
    owl:members ( ex:p ex:q ) ;
    owl:members ( ex:q ex:r ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Disj",
            "http://www.w3.org/2002/07/owl#AllDisjointProperties",
        );
        assert!(
            matches!(result, Err(TranslatorError::MultipleOwlMembers(_))),
            "expected MultipleOwlMembers error, got {result:?}"
        );
    }

    // ── owl:AllDisjointProperties object- vs. data-property disambiguation
    // (#668) ─────────────────────────────────────────────────────────────
    //
    // `owl:AllDisjointProperties` is the identical RDF encoding for both n>2
    // `DisjointObjectProperties` and n>2 `DisjointDataProperties` (per
    // https://github.com/daghovland/rdf-datalog/issues/513's write side, PR
    // #669) -- the only available signal on read-back is each member's own
    // declared property kind.

    #[test]
    fn all_disjoint_properties_all_data_properties_returns_disjoint_data_properties() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:firstName a owl:DatatypeProperty .
ex:lastName a owl:DatatypeProperty .
ex:ssn a owl:DatatypeProperty .

ex:Disj a owl:AllDisjointProperties ;
    owl:members ( ex:firstName ex:lastName ex:ssn ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Disj",
            "http://www.w3.org/2002/07/owl#AllDisjointProperties",
        );
        match &result {
            Ok(Some(Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DisjointDataProperties(
                _,
                props,
            )))) => {
                assert_eq!(props.len(), 3, "expected 3 data properties, got {props:?}");
            }
            _ => panic!("expected a DisjointDataProperties axiom, got {result:?}"),
        }
    }

    #[test]
    fn all_disjoint_properties_undeclared_defaults_to_disjoint_object_properties() {
        // No rdf:type declarations at all for the members -- ambiguous input,
        // so read-back falls back to the pre-existing DisjointObjectProperties
        // behavior (matching `OntologyDeclarations::object_or_data_property`'s
        // own undeclared-member fallback elsewhere in this file).
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:Disj a owl:AllDisjointProperties ;
    owl:members ( ex:p ex:q ex:r ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Disj",
            "http://www.w3.org/2002/07/owl#AllDisjointProperties",
        );
        assert!(
            matches!(
                result,
                Ok(Some(Axiom::AxiomObjectPropertyAxiom(
                    ObjectPropertyAxiom::DisjointObjectProperties(_, _)
                )))
            ),
            "expected a DisjointObjectProperties axiom (fallback), got {result:?}"
        );
    }

    /// Round-trip: write an n-ary `DisjointDataProperties` axiom (with
    /// explicit `DataPropertyDeclaration`s so the members are unambiguously
    /// declared data properties, per #668) out via `owl2rl2datalog::owl2rdf`
    /// (#513's `owl:AllDisjointProperties` blank-node encoding), then read it
    /// back via `extract_axiom` and confirm it comes back as
    /// `DisjointDataProperties`, not `DisjointObjectProperties`.
    #[test]
    fn disjoint_data_properties_round_trips_through_owl2rdf() {
        use ingress::{IriReference, OntologyVersion};
        use owl_ontology::Ontology;
        use owl2rl2datalog::owl_to_rdf::owl2rdf;

        fn dp(name: &str) -> FullIri {
            FullIri(IriReference(format!("http://example.org/{name}")))
        }

        let axioms = vec![
            Axiom::AxiomDeclaration((vec![], Entity::DataPropertyDeclaration(dp("firstName")))),
            Axiom::AxiomDeclaration((vec![], Entity::DataPropertyDeclaration(dp("lastName")))),
            Axiom::AxiomDeclaration((vec![], Entity::DataPropertyDeclaration(dp("ssn")))),
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DisjointDataProperties(
                vec![],
                vec![dp("firstName"), dp("lastName"), dp("ssn")],
            )),
        ];
        let ontology = Ontology::new(vec![], OntologyVersion::UnNamedOntology, vec![], axioms);

        let mut ds = Datastore::new(100);
        let report = owl2rdf(&mut ds, &ontology);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);

        let ids = WellKnownIds::new(&mut ds.resources);
        let decls = OntologyDeclarations::build(&ds, &ids).unwrap();

        let all_disjoint_properties_type_id = ids.owl_all_disjoint_properties_id;
        let triple = ds
            .get_triples_with_predicate(ids.rdf_type_id)
            .find(|tr| tr.obj == all_disjoint_properties_type_id)
            .expect("owl2rdf must emit an owl:AllDisjointProperties rdf:type triple");

        let result = extract_axiom(&ds, &ids, &decls, &triple).unwrap();
        match result {
            Some(Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DisjointDataProperties(
                _,
                props,
            ))) => {
                assert_eq!(
                    props.len(),
                    3,
                    "expected 3 data properties round-tripped, got {props:?}"
                );
            }
            other => panic!("expected a DisjointDataProperties axiom, got {other:?}"),
        }
    }

    // ── owl:AllDifferent (n>2 DifferentIndividuals) read-back (#667) ───────
    //
    // Mirrors the `owl:AllDisjointClasses`/`owl:AllDisjointProperties` tests
    // above: a single `owl:members` triple succeeds, more than one is an
    // error (`TranslatorError::MultipleOwlMembers`).

    #[test]
    fn all_different_single_members_triple_succeeds() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:Alice a owl:NamedIndividual .
ex:Bob a owl:NamedIndividual .
ex:Carol a owl:NamedIndividual .

ex:Diff a owl:AllDifferent ;
    owl:members ( ex:Alice ex:Bob ex:Carol ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Diff",
            "http://www.w3.org/2002/07/owl#AllDifferent",
        );
        match &result {
            Ok(Some(Axiom::AxiomAssertion(Assertion::DifferentIndividuals(_, individuals)))) => {
                assert_eq!(
                    individuals.len(),
                    3,
                    "expected 3 individuals, got {individuals:?}"
                );
            }
            _ => panic!("expected a DifferentIndividuals axiom, got {result:?}"),
        }
    }

    #[test]
    fn all_different_multiple_members_triples_returns_err() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:Alice a owl:NamedIndividual .
ex:Bob a owl:NamedIndividual .
ex:Carol a owl:NamedIndividual .

ex:Diff a owl:AllDifferent ;
    owl:members ( ex:Alice ex:Bob ) ;
    owl:members ( ex:Bob ex:Carol ) .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Diff",
            "http://www.w3.org/2002/07/owl#AllDifferent",
        );
        assert!(
            matches!(result, Err(TranslatorError::MultipleOwlMembers(_))),
            "expected MultipleOwlMembers error, got {result:?}"
        );
    }

    #[test]
    fn all_different_no_members_triple_returns_none() {
        let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix ex:  <http://example.org/> .

ex:Diff a owl:AllDifferent .
"#;
        let result = extract_type_axiom(
            ttl,
            "http://example.org/Diff",
            "http://www.w3.org/2002/07/owl#AllDifferent",
        );
        assert!(
            matches!(result, Ok(None)),
            "expected Ok(None), got {result:?}"
        );
    }

    /// Round-trip: write an n-ary `DifferentIndividuals` axiom out via
    /// `owl2rl2datalog::owl2rdf` (#513's `owl:AllDifferent` blank-node
    /// encoding), then read it back via `extract_axiom` and confirm the same
    /// set of individuals comes back.
    #[test]
    fn all_different_round_trips_through_owl2rdf() {
        use ingress::{IriReference, OntologyVersion};
        use owl_ontology::{Individual, Ontology};
        use owl2rl2datalog::owl_to_rdf::owl2rdf;

        let axiom = Axiom::AxiomAssertion(Assertion::DifferentIndividuals(
            vec![],
            vec![
                Individual::NamedIndividual(FullIri(IriReference(
                    "http://example.org/alice".to_owned(),
                ))),
                Individual::NamedIndividual(FullIri(IriReference(
                    "http://example.org/bob".to_owned(),
                ))),
                Individual::NamedIndividual(FullIri(IriReference(
                    "http://example.org/carol".to_owned(),
                ))),
            ],
        ));
        let ontology = Ontology::new(
            vec![],
            OntologyVersion::UnNamedOntology,
            vec![],
            vec![axiom],
        );

        let mut ds = Datastore::new(100);
        let report = owl2rdf(&mut ds, &ontology);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);

        let ids = WellKnownIds::new(&mut ds.resources);
        let decls = OntologyDeclarations::build(&ds, &ids).unwrap();

        let all_different_type_id = ids.owl_all_different_id;
        let triple = ds
            .get_triples_with_predicate(ids.rdf_type_id)
            .find(|tr| tr.obj == all_different_type_id)
            .expect("owl2rdf must emit an owl:AllDifferent rdf:type triple");

        let result = extract_axiom(&ds, &ids, &decls, &triple).unwrap();
        match result {
            Some(Axiom::AxiomAssertion(Assertion::DifferentIndividuals(_, individuals))) => {
                assert_eq!(
                    individuals.len(),
                    3,
                    "expected 3 individuals round-tripped, got {individuals:?}"
                );
            }
            other => panic!("expected a DifferentIndividuals axiom, got {other:?}"),
        }
    }
}

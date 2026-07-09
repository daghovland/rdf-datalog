/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Parses anonymous OWL class expressions and restrictions from RDF triples.
//! Mirrors `DagSemTools.RdfOwlTranslator.ClassExpressionParser`.

use crate::ingress::{
    WellKnownIds, get_rdf_list_elements, topological_sort, try_get_bool_literal,
    try_get_individual, try_get_non_negative_integer_literal,
};
use dag_rdf::datastore::Datastore;
use dag_rdf::ingress::Triple;
use dag_rdf::{GraphElement, GraphElementId, IriReference, RdfResource};
use ingress::*;
use num_bigint::BigInt;
use owl_ontology::*;
use std::collections::HashMap;

/// The IRI for `owl:Thing` — used as a fallback when a class expression
/// cannot be resolved (e.g. unsupported anonymous expression types).
const OWL_THING_IRI: &str = "http://www.w3.org/2002/07/owl#Thing";

/// Holds the parsed OWL declarations (CE, DR, OPE, DPE, AP, Individuals, Annotations).
/// Built up by `ClassExpressionParser`.
pub struct OntologyDeclarations {
    pub class_expressions: HashMap<GraphElementId, ClassExpression>,
    pub data_ranges: HashMap<GraphElementId, DataRange>,
    pub object_property_expressions: HashMap<GraphElementId, ObjectPropertyExpression>,
    pub data_property_expressions: HashMap<GraphElementId, DataProperty>,
    pub annotation_properties: HashMap<GraphElementId, AnnotationProperty>,
    // Kept for future use in assertion axiom extraction.
    #[allow(dead_code)]
    pub(crate) individuals: HashMap<GraphElementId, Individual>,
    pub annotations: HashMap<GraphElementId, Vec<Annotation>>,
}

impl OntologyDeclarations {
    /// Build initial declarations from the triple store.
    pub fn build(datastore: &Datastore, ids: &WellKnownIds) -> Self {
        let class_expressions = build_class_declarations(datastore, ids);
        let data_ranges = build_data_range_declarations(datastore, ids);
        let object_property_expressions = build_object_property_declarations(datastore, ids);
        let data_property_expressions = build_data_property_declarations(datastore, ids);
        let annotation_properties = build_annotation_property_declarations(datastore, ids);
        let individuals = build_individual_declarations(datastore, ids);
        let annotations = build_annotations(datastore, ids, &annotation_properties, &individuals);

        let mut decls = OntologyDeclarations {
            class_expressions,
            data_ranges,
            object_property_expressions,
            data_property_expressions,
            annotation_properties,
            individuals,
            annotations,
        };

        // Parse anonymous class expressions and restrictions in a single unified
        // topological sort so cross-type dependencies (e.g. an owl:Class node whose
        // member list contains owl:Restriction blank nodes) are resolved correctly.
        parse_anonymous_exprs(datastore, ids, &mut decls);

        decls
    }

    pub fn get_annotations(&self, id: GraphElementId) -> Vec<Annotation> {
        self.annotations.get(&id).cloned().unwrap_or_default()
    }

    /// Get a class expression for `id`.
    ///
    /// Returns the parsed class expression if known, a named-class expression
    /// for IRI resources, or `owl:Thing` as a conservative fallback for
    /// unresolved anonymous blank nodes (logging a warning).
    pub fn class_expression(
        &self,
        id: GraphElementId,
        resources: &dag_rdf::GraphElementManager,
    ) -> ClassExpression {
        if let Some(ce) = self.class_expressions.get(&id) {
            return ce.clone();
        }
        if self.data_ranges.contains_key(&id) {
            log::warn!(
                "OWL: {:?} used as class but declared as data range — using owl:Thing",
                resources.get_graph_element(id)
            );
            return ClassExpression::ClassName(FullIri(IriReference(OWL_THING_IRI.to_string())));
        }
        match resources.get_resource(id) {
            Some(RdfResource::Iri(iri)) => ClassExpression::ClassName(FullIri(iri.clone())),
            Some(RdfResource::AnonymousBlankNode(_)) => {
                log::warn!(
                    "OWL: blank node {:?} used as class without definition — \
                     using owl:Thing (unsupported anonymous class expression type)",
                    resources.get_graph_element(id)
                );
                ClassExpression::ClassName(FullIri(IriReference(OWL_THING_IRI.to_string())))
            }
            None => {
                log::warn!(
                    "OWL: unknown resource {} used as class — using owl:Thing",
                    id
                );
                ClassExpression::ClassName(FullIri(IriReference(OWL_THING_IRI.to_string())))
            }
        }
    }

    pub fn data_range(
        &self,
        id: GraphElementId,
        resources: &dag_rdf::GraphElementManager,
    ) -> DataRange {
        if let Some(dr) = self.data_ranges.get(&id) {
            return dr.clone();
        }
        log::warn!(
            "OWL: {:?} used as data range but not declared — using owl:Literal",
            resources.get_graph_element(id)
        );
        DataRange::NamedDataRange(FullIri(IriReference(
            "http://www.w3.org/2002/07/owl#Literal".to_string(),
        )))
    }

    pub fn object_property_expression(
        &self,
        id: GraphElementId,
        resources: &dag_rdf::GraphElementManager,
    ) -> ObjectPropertyExpression {
        if let Some(ope) = self.object_property_expressions.get(&id) {
            return ope.clone();
        }
        if self.data_property_expressions.contains_key(&id)
            || self.annotation_properties.contains_key(&id)
        {
            log::warn!(
                "OWL: {:?} used as object property but declared as data/annotation property — \
                 treating as named object property",
                resources.get_graph_element(id)
            );
        }
        match resources.get_named_resource(id) {
            Some(iri) => ObjectPropertyExpression::NamedObjectProperty(FullIri(iri.clone())),
            None => {
                log::warn!(
                    "OWL: {:?} used as object property but not found — using owl:topObjectProperty",
                    resources.get_graph_element(id)
                );
                ObjectPropertyExpression::NamedObjectProperty(FullIri(IriReference(
                    "http://www.w3.org/2002/07/owl#topObjectProperty".to_string(),
                )))
            }
        }
    }

    pub fn data_property_expression(
        &self,
        id: GraphElementId,
        resources: &dag_rdf::GraphElementManager,
    ) -> DataProperty {
        if let Some(dp) = self.data_property_expressions.get(&id) {
            return dp.clone();
        }
        if self.object_property_expressions.contains_key(&id)
            || self.annotation_properties.contains_key(&id)
        {
            log::warn!(
                "OWL: {:?} used as data property but declared as object/annotation property — \
                 treating as named data property",
                resources.get_graph_element(id)
            );
        }
        match resources.get_named_resource(id) {
            Some(iri) => FullIri(iri.clone()),
            None => {
                log::warn!(
                    "OWL: {:?} used as data property but not found — using owl:topDataProperty",
                    resources.get_graph_element(id)
                );
                FullIri(IriReference(
                    "http://www.w3.org/2002/07/owl#topDataProperty".to_string(),
                ))
            }
        }
    }

    /// Returns the object or data property axiom (whichever is declared).
    pub fn object_or_data_property<T>(
        &self,
        id: GraphElementId,
        resources: &dag_rdf::GraphElementManager,
        object_fn: impl Fn(ObjectPropertyExpression) -> T,
        data_fn: impl Fn(DataProperty) -> T,
    ) -> T {
        let is_obj = self.object_property_expressions.contains_key(&id);
        let is_data = self.data_property_expressions.contains_key(&id);
        match (is_obj, is_data) {
            (true, false) => object_fn(self.object_property_expression(id, resources)),
            (false, true) => data_fn(self.data_property_expression(id, resources)),
            _ => {
                // Fallback: try IRI-based heuristic, default to object property
                if let Some(iri) = resources.get_named_resource(id) {
                    object_fn(ObjectPropertyExpression::NamedObjectProperty(FullIri(
                        iri.clone(),
                    )))
                } else {
                    log::warn!(
                        "OWL: {:?} not declared as object or data property — \
                         using owl:topObjectProperty",
                        resources.get_graph_element(id)
                    );
                    object_fn(ObjectPropertyExpression::NamedObjectProperty(FullIri(
                        IriReference("http://www.w3.org/2002/07/owl#topObjectProperty".to_string()),
                    )))
                }
            }
        }
    }
}

// ── Initial declaration builders ───────────────────────────────────────────

fn get_initial_declarations(
    datastore: &Datastore,
    ids: &WellKnownIds,
    type_id: GraphElementId,
) -> Vec<(GraphElementId, IriReference)> {
    let mut result: Vec<(GraphElementId, IriReference)> = datastore
        .get_triples_with_object_predicate(type_id, ids.rdf_type_id)
        .filter_map(|tr| {
            datastore
                .resources
                .get_named_resource(tr.subject)
                .map(|iri| (tr.subject, iri.clone()))
        })
        .collect();

    // Also collect axiom-annotated declarations (second part of Table 7)
    let ann_decls: Vec<(GraphElementId, IriReference)> = datastore
        .get_triples_with_object_predicate(type_id, ids.owl_annotated_target_id)
        .filter(|tr| {
            let ax = tr.subject;
            datastore.contains_triple(&dag_rdf::ingress::Triple {
                subject: ax,
                predicate: ids.rdf_type_id,
                obj: ids.owl_axiom_id,
            }) && datastore.contains_triple(&dag_rdf::ingress::Triple {
                subject: ax,
                predicate: ids.owl_annotated_property_id,
                obj: ids.rdf_type_id,
            })
        })
        .flat_map(|tr| {
            datastore
                .get_triples_with_subject_predicate(tr.subject, ids.owl_annotated_source_id)
                .filter_map(|src_tr| {
                    datastore
                        .resources
                        .get_named_resource(src_tr.obj)
                        .map(|iri| (src_tr.obj, iri.clone()))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    result.extend(ann_decls);
    result
}

fn build_class_declarations(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> HashMap<GraphElementId, ClassExpression> {
    let mut map: HashMap<GraphElementId, ClassExpression> =
        get_initial_declarations(datastore, ids, ids.owl_class_id)
            .into_iter()
            .map(|(id, iri)| (id, ClassExpression::ClassName(FullIri(iri))))
            .collect();

    // Also treat subjects and objects of rdfs:subClassOf as classes
    for tr in datastore.get_triples_with_predicate(ids.rdfs_sub_class_of_id) {
        for &class_id in &[tr.subject, tr.obj] {
            if let Some(iri) = datastore.resources.get_named_resource(class_id) {
                map.entry(class_id)
                    .or_insert_with(|| ClassExpression::ClassName(FullIri(iri.clone())));
            }
        }
    }

    map
}

fn build_data_range_declarations(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> HashMap<GraphElementId, DataRange> {
    let mut map: HashMap<GraphElementId, DataRange> =
        get_initial_declarations(datastore, ids, ids.rdfs_datatype_id)
            .into_iter()
            .map(|(id, iri)| (id, DataRange::NamedDataRange(FullIri(iri))))
            .collect();

    // rdfs:Literal is always a datatype
    if let Some(iri) = datastore.resources.get_named_resource(ids.rdfs_literal_id) {
        map.entry(ids.rdfs_literal_id)
            .or_insert_with(|| DataRange::NamedDataRange(FullIri(iri.clone())));
    }

    map
}

fn build_object_property_declarations(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> HashMap<GraphElementId, ObjectPropertyExpression> {
    let mut map: HashMap<GraphElementId, ObjectPropertyExpression> =
        get_initial_declarations(datastore, ids, ids.owl_object_property_id)
            .into_iter()
            .map(|(id, iri)| {
                (
                    id,
                    ObjectPropertyExpression::NamedObjectProperty(FullIri(iri)),
                )
            })
            .collect();

    // Also detect inverse object properties: _:x owl:inverseOf y
    for tr in datastore.get_triples_with_predicate(ids.owl_object_inverse_of_id) {
        if let Some(obj_iri) = datastore.resources.get_named_resource(tr.obj) {
            let inv = ObjectPropertyExpression::InverseObjectProperty(Box::new(
                ObjectPropertyExpression::NamedObjectProperty(FullIri(obj_iri.clone())),
            ));
            map.entry(tr.subject).or_insert(inv);
        }
    }

    map
}

fn build_data_property_declarations(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> HashMap<GraphElementId, DataProperty> {
    get_initial_declarations(datastore, ids, ids.owl_datatype_property_id)
        .into_iter()
        .map(|(id, iri)| (id, FullIri(iri)))
        .collect()
}

fn build_annotation_property_declarations(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> HashMap<GraphElementId, AnnotationProperty> {
    get_initial_declarations(datastore, ids, ids.owl_annotation_property_id)
        .into_iter()
        .map(|(id, iri)| (id, FullIri(iri)))
        .collect()
}

fn build_individual_declarations(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> HashMap<GraphElementId, Individual> {
    get_initial_declarations(datastore, ids, ids.owl_named_individual_id)
        .into_iter()
        .map(|(id, iri)| (id, Individual::NamedIndividual(FullIri(iri))))
        .collect()
}

fn build_annotations(
    datastore: &Datastore,
    _ids: &WellKnownIds,
    annotation_properties: &HashMap<GraphElementId, AnnotationProperty>,
    individuals: &HashMap<GraphElementId, Individual>,
) -> HashMap<GraphElementId, Vec<Annotation>> {
    let mut map: HashMap<GraphElementId, Vec<Annotation>> = HashMap::new();

    for (&ann_prop_id, ann_prop) in annotation_properties {
        for tr in datastore.get_triples_with_predicate(ann_prop_id) {
            let annotated_obj = tr.subject;
            let obj_gel = datastore.resources.get_graph_element(tr.obj);
            let ann_value = create_annotation_value(individuals, tr.obj, obj_gel);
            map.entry(annotated_obj)
                .or_default()
                .push((ann_prop.clone(), ann_value));
        }
    }

    map
}

fn create_annotation_value(
    individuals: &HashMap<GraphElementId, Individual>,
    res_id: GraphElementId,
    gel: &GraphElement,
) -> AnnotationValue {
    if let Some(ind) = individuals.get(&res_id) {
        return AnnotationValue::IndividualAnnotation(ind.clone());
    }
    match gel {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            AnnotationValue::IriAnnotation(FullIri(iri.clone()))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => {
            AnnotationValue::IndividualAnnotation(Individual::AnonymousIndividual(*n))
        }
        GraphElement::GraphLiteral(_lit) => AnnotationValue::LiteralAnnotation(gel.clone()),
        // Triple terms cannot appear as OWL annotation values; treat as literal placeholder (#143).
        GraphElement::TripleTerm(_) => AnnotationValue::LiteralAnnotation(gel.clone()),
    }
}

// ── Anonymous class expressions and restrictions (Table 13 in the spec) ────

fn is_owl_axiom_predicate(pred_id: GraphElementId, ids: &WellKnownIds) -> bool {
    pred_id == ids.owl_intersection_of_id
        || pred_id == ids.owl_union_of_id
        || pred_id == ids.owl_complement_of_id
        || pred_id == ids.owl_one_of_id
}

/// Callback type for lazily building a [`ClassExpression`] once all its
/// dependencies have been resolved via topological sort.
type ClassExprBuilder =
    Box<dyn Fn(&OntologyDeclarations, &dag_rdf::GraphElementManager) -> ClassExpression>;

/// A lazily-built anonymous node (either an `owl:Class` or `owl:Restriction`
/// blank node) with its dependency set.  All nodes are collected into one list
/// and sorted together so cross-type dependencies (e.g. an `owl:Class`
/// intersection whose member list contains `owl:Restriction` blank nodes) are
/// resolved in the right order.
struct AnonExpr {
    id: GraphElementId,
    predecessors: Vec<GraphElementId>,
    builder: ClassExprBuilder,
}

fn is_anon_blank_node(datastore: &Datastore, id: GraphElementId) -> bool {
    matches!(
        datastore.resources.get_resource(id),
        Some(RdfResource::AnonymousBlankNode(_))
    )
}

/// Collect builders for anonymous `owl:Class` nodes (intersectionOf, unionOf,
/// complementOf, oneOf).  Dependencies include **all** anonymous blank-node
/// list members, regardless of whether they appear in `class_expressions`
/// yet — the unified topological sort handles ordering across both class
/// expressions and restrictions.
fn collect_anon_class_exprs(datastore: &Datastore, ids: &WellKnownIds) -> Vec<AnonExpr> {
    let anon_class_subjects: Vec<GraphElementId> = datastore
        .get_triples_with_object_predicate(ids.owl_class_id, ids.rdf_type_id)
        .filter_map(|tr| match datastore.resources.get_resource(tr.subject) {
            Some(RdfResource::AnonymousBlankNode(_)) => Some(tr.subject),
            _ => None,
        })
        .collect();

    let mut exprs: Vec<AnonExpr> = Vec::new();

    for subj in anon_class_subjects {
        let triples: Vec<Triple> = datastore.get_triples_with_subject(subj).collect();
        let defining: Vec<&Triple> = triples
            .iter()
            .filter(|tr| is_owl_axiom_predicate(tr.predicate, ids))
            .collect();

        let defining_tr = match defining.as_slice() {
            [tr] => *tr,
            [] => {
                log::warn!(
                    "Anonymous owl:Class {} without defining expression – skipped",
                    subj
                );
                continue;
            }
            [first, ..] => {
                log::warn!(
                    "Anonymous owl:Class {} with multiple defining expressions — using first",
                    subj
                );
                *first
            }
        };

        let pred_id = defining_tr.predicate;
        let obj_id = defining_tr.obj;

        let (predecessors, builder): (Vec<GraphElementId>, ClassExprBuilder) = if pred_id
            == ids.owl_complement_of_id
        {
            let dep = obj_id;
            let preds = if is_anon_blank_node(datastore, dep) {
                vec![dep]
            } else {
                vec![]
            };
            (
                preds,
                Box::new(move |decls: &OntologyDeclarations, res| {
                    ClassExpression::ObjectComplementOf(Box::new(decls.class_expression(dep, res)))
                }),
            )
        } else if pred_id == ids.owl_one_of_id {
            let list_items = get_rdf_list_elements(
                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                ids,
                obj_id,
            );
            (
                vec![],
                Box::new(move |_decls: &OntologyDeclarations, res| {
                    let individuals: Vec<Individual> = list_items
                        .iter()
                        .map(|&id| try_get_individual(res.get_graph_element(id)))
                        .collect();
                    ClassExpression::ObjectOneOf(individuals)
                }),
            )
        } else {
            // intersectionOf or unionOf: list of class expressions
            let list_items = get_rdf_list_elements(
                &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
                ids,
                obj_id,
            );
            // Include ALL anonymous blank-node list members as predecessors.
            // Named resources and already-declared class expressions are not
            // in the node set so topological_sort will ignore them safely.
            let deps: Vec<GraphElementId> = list_items
                .iter()
                .filter(|&&dep| is_anon_blank_node(datastore, dep))
                .copied()
                .collect();

            if pred_id == ids.owl_intersection_of_id {
                (
                    deps,
                    Box::new(move |decls: &OntologyDeclarations, res| {
                        ClassExpression::ObjectIntersectionOf(
                            list_items
                                .iter()
                                .map(|&id| decls.class_expression(id, res))
                                .collect(),
                        )
                    }),
                )
            } else {
                // unionOf
                (
                    deps,
                    Box::new(move |decls: &OntologyDeclarations, res| {
                        ClassExpression::ObjectUnionOf(
                            list_items
                                .iter()
                                .map(|&id| decls.class_expression(id, res))
                                .collect(),
                        )
                    }),
                )
            }
        };

        exprs.push(AnonExpr {
            id: subj,
            predecessors,
            builder,
        });
    }

    exprs
}

/// Collect builders for anonymous `owl:Restriction` nodes.
fn collect_anon_restriction_exprs(datastore: &Datastore, ids: &WellKnownIds) -> Vec<AnonExpr> {
    let restriction_subjects: Vec<GraphElementId> = datastore
        .get_triples_with_object_predicate(ids.owl_restriction_id, ids.rdf_type_id)
        .filter_map(|tr| match datastore.resources.get_resource(tr.subject) {
            Some(RdfResource::AnonymousBlankNode(_)) => Some(tr.subject),
            other => {
                log::warn!(
                    "owl:Restriction used on non-blank-node {:?} — skipped",
                    other
                );
                None
            }
        })
        .collect();

    restriction_subjects
        .into_iter()
        .filter_map(|subj| {
            let triples: Vec<Triple> = datastore
                .get_triples_with_subject(subj)
                .filter(|tr| tr.predicate != ids.rdf_type_id)
                .collect();

            let pred_iris: Vec<String> = triples
                .iter()
                .filter_map(|tr| {
                    datastore
                        .resources
                        .get_named_resource(tr.predicate)
                        .map(|iri| iri.0.clone())
                })
                .collect();

            build_restriction(datastore, ids, subj, &triples, &pred_iris)
        })
        .collect()
}

/// Parse all anonymous class expressions and restrictions in one unified
/// topological sort.  This correctly handles cross-type dependencies such as
/// an `owl:Class` intersection whose member list contains `owl:Restriction`
/// blank nodes (and vice-versa).
fn parse_anonymous_exprs(
    datastore: &Datastore,
    ids: &WellKnownIds,
    decls: &mut OntologyDeclarations,
) {
    let mut all: Vec<AnonExpr> = collect_anon_class_exprs(datastore, ids);
    all.extend(collect_anon_restriction_exprs(datastore, ids));

    // Deduplicate: a node that is both owl:Class and owl:Restriction typed
    // (unusual but defensive) should only appear once.
    {
        let mut seen = std::collections::HashSet::new();
        all.retain(|e| seen.insert(e.id));
    }

    let ids_vec: Vec<GraphElementId> = all.iter().map(|e| e.id).collect();
    let pred_map: HashMap<GraphElementId, Vec<GraphElementId>> =
        all.iter().map(|e| (e.id, e.predecessors.clone())).collect();
    let builder_map: HashMap<GraphElementId, usize> =
        all.iter().enumerate().map(|(i, e)| (e.id, i)).collect();

    let sorted = topological_sort(&ids_vec, &pred_map);

    for id in sorted {
        if let Some(&idx) = builder_map.get(&id) {
            let expr = (all[idx].builder)(decls, &datastore.resources);
            decls.class_expressions.insert(id, expr);
        }
    }
}

fn find_triple_obj(triples: &[Triple], predicate_id: GraphElementId) -> Option<GraphElementId> {
    triples
        .iter()
        .find(|tr| tr.predicate == predicate_id)
        .map(|tr| tr.obj)
}

fn require_triple_obj(
    triples: &[Triple],
    predicate_id: GraphElementId,
    desc: &str,
) -> Option<GraphElementId> {
    let result = find_triple_obj(triples, predicate_id);
    if result.is_none() {
        log::warn!("Missing {} in OWL restriction — skipping restriction", desc);
    }
    result
}

fn require_cardinality(
    triples: &[Triple],
    card_pred_id: GraphElementId,
    resources: &dag_rdf::GraphElementManager,
) -> Option<BigInt> {
    let obj_id = require_triple_obj(triples, card_pred_id, "cardinality")?;
    let gel = resources.get_graph_element(obj_id);
    let result = try_get_non_negative_integer_literal(gel);
    if result.is_none() {
        log::warn!(
            "Invalid cardinality value: {:?} — skipping restriction",
            gel
        );
    }
    result
}

fn build_restriction(
    datastore: &Datastore,
    ids: &WellKnownIds,
    subj: GraphElementId,
    triples: &[Triple],
    pred_iris: &[String],
) -> Option<AnonExpr> {
    // Determine which kind of restriction this is based on predicates present
    let has = |iri: &str| pred_iris.iter().any(|s| s == iri);

    if has(OWL_ON_PROPERTIES) {
        // Data(All|Some)ValuesFrom with multiple properties
        let on_props_id =
            require_triple_obj(triples, ids.owl_on_properties_id, "owl:onProperties")?;
        let list_items = get_rdf_list_elements(
            &|s, p| datastore.get_triples_with_subject_predicate(s, p).collect(),
            ids,
            on_props_id,
        );
        let values_from_id = if has(OWL_ALL_VALUES_FROM) {
            require_triple_obj(triples, ids.owl_all_values_from_id, "owl:allValuesFrom")?
        } else if has(OWL_SOME_VALUES_FROM) {
            require_triple_obj(triples, ids.owl_some_values_from_id, "owl:someValuesFrom")?
        } else {
            log::warn!("owl:onProperties without allValuesFrom or someValuesFrom — skipping");
            return None;
        };
        let use_all = has(OWL_ALL_VALUES_FROM);
        Some(AnonExpr {
            id: subj,
            predecessors: vec![],
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                let props: Vec<DataProperty> = list_items
                    .iter()
                    .map(|&id| decls.data_property_expression(id, res))
                    .collect();
                let dr = decls.data_range(values_from_id, res);
                if use_all {
                    ClassExpression::DataAllValuesFrom(props, dr)
                } else {
                    ClassExpression::DataSomeValuesFrom(props, dr)
                }
            }),
        })
    } else if has(OWL_SOME_VALUES_FROM) {
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let z = require_triple_obj(triples, ids.owl_some_values_from_id, "owl:someValuesFrom")?;
        let deps = if is_anon_blank_node(datastore, z) {
            vec![z]
        } else {
            vec![]
        };
        Some(AnonExpr {
            id: subj,
            predecessors: deps,
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                decls.object_or_data_property(
                    y,
                    res,
                    |ope| {
                        ClassExpression::ObjectSomeValuesFrom(
                            ope,
                            Box::new(decls.class_expression(z, res)),
                        )
                    },
                    |dp| ClassExpression::DataSomeValuesFrom(vec![dp], decls.data_range(z, res)),
                )
            }),
        })
    } else if has(OWL_ALL_VALUES_FROM) {
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let z = require_triple_obj(triples, ids.owl_all_values_from_id, "owl:allValuesFrom")?;
        let deps = if is_anon_blank_node(datastore, z) {
            vec![z]
        } else {
            vec![]
        };
        Some(AnonExpr {
            id: subj,
            predecessors: deps,
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                decls.object_or_data_property(
                    y,
                    res,
                    |ope| {
                        ClassExpression::ObjectAllValuesFrom(
                            ope,
                            Box::new(decls.class_expression(z, res)),
                        )
                    },
                    |dp| ClassExpression::DataAllValuesFrom(vec![dp], decls.data_range(z, res)),
                )
            }),
        })
    } else if has(OWL_HAS_VALUE) {
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let z_id = require_triple_obj(triples, ids.owl_has_value_id, "owl:hasValue")?;
        let z_gel = datastore.resources.get_graph_element(z_id).clone();
        Some(AnonExpr {
            id: subj,
            predecessors: vec![],
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                decls.object_or_data_property(
                    y,
                    res,
                    |ope| ClassExpression::ObjectHasValue(ope, try_get_individual(&z_gel)),
                    |dp| ClassExpression::DataHasValue(dp, z_gel.clone()),
                )
            }),
        })
    } else if has(OWL_HAS_SELF) {
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let self_id = require_triple_obj(triples, ids.owl_has_self_id, "owl:hasSelf")?;
        Some(AnonExpr {
            id: subj,
            predecessors: vec![],
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                let gel = res.get_graph_element(self_id);
                let has_self = match try_get_bool_literal(gel) {
                    Some(v) => v,
                    None => {
                        log::warn!(
                            "owl:hasSelf value is not boolean: {:?} — using owl:Thing",
                            gel
                        );
                        return ClassExpression::ClassName(FullIri(IriReference(
                            OWL_THING_IRI.to_string(),
                        )));
                    }
                };
                if has_self {
                    let ope = decls.object_property_expression(y, res);
                    ClassExpression::ObjectHasSelf(ope)
                } else {
                    log::warn!("owl:hasSelf with false value — using owl:Thing");
                    ClassExpression::ClassName(FullIri(IriReference(OWL_THING_IRI.to_string())))
                }
            }),
        })
    } else if has(OWL_ON_CLASS) {
        // Qualified cardinality on object property
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let z = require_triple_obj(triples, ids.owl_on_class_id, "owl:onClass")?;
        let deps = if is_anon_blank_node(datastore, z) {
            vec![z]
        } else {
            vec![]
        };
        let (card_id, card_type) = if has(OWL_MAX_QUALIFIED_CARDINALITY) {
            (ids.owl_max_qualified_cardinality_id, 0u8)
        } else if has(OWL_MIN_QUALIFIED_CARDINALITY) {
            (ids.owl_min_qualified_cardinality_id, 1u8)
        } else if has(OWL_QUALIFIED_CARDINALITY) {
            (ids.owl_qualified_cardinality_id, 2u8)
        } else {
            log::warn!("owl:onClass without qualified cardinality — skipping");
            return None;
        };
        let n = require_cardinality(triples, card_id, &datastore.resources)?;
        Some(AnonExpr {
            id: subj,
            predecessors: deps,
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                let ope = decls.object_property_expression(y, res);
                let ce = Box::new(decls.class_expression(z, res));
                match card_type {
                    0 => ClassExpression::ObjectMaxQualifiedCardinality(n.clone(), ope, ce),
                    1 => ClassExpression::ObjectMinQualifiedCardinality(n.clone(), ope, ce),
                    _ => ClassExpression::ObjectExactQualifiedCardinality(n.clone(), ope, ce),
                }
            }),
        })
    } else if has(OWL_ON_DATA_RANGE) {
        // Qualified cardinality on data property
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let z = require_triple_obj(triples, ids.owl_on_data_range_id, "owl:onDataRange")?;
        let (card_id, card_type) = if has(OWL_MAX_QUALIFIED_CARDINALITY) {
            (ids.owl_max_qualified_cardinality_id, 0u8)
        } else if has(OWL_MIN_QUALIFIED_CARDINALITY) {
            (ids.owl_min_qualified_cardinality_id, 1u8)
        } else if has(OWL_QUALIFIED_CARDINALITY) {
            (ids.owl_qualified_cardinality_id, 2u8)
        } else {
            log::warn!("owl:onDataRange without qualified cardinality — skipping");
            return None;
        };
        let n = require_cardinality(triples, card_id, &datastore.resources)?;
        Some(AnonExpr {
            id: subj,
            predecessors: vec![],
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                let dp = decls.data_property_expression(y, res);
                let dr = decls.data_range(z, res);
                match card_type {
                    0 => ClassExpression::DataMaxQualifiedCardinality(n.clone(), dp, dr),
                    1 => ClassExpression::DataMinQualifiedCardinality(n.clone(), dp, dr),
                    _ => ClassExpression::DataExactQualifiedCardinality(n.clone(), dp, dr),
                }
            }),
        })
    } else if has(OWL_MIN_CARDINALITY) {
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let n = require_cardinality(triples, ids.owl_min_cardinality_id, &datastore.resources)?;
        Some(AnonExpr {
            id: subj,
            predecessors: vec![],
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                let ope = decls.object_property_expression(y, res);
                ClassExpression::ObjectMinCardinality(n.clone(), ope)
            }),
        })
    } else if has(OWL_MAX_CARDINALITY) {
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let n = require_cardinality(triples, ids.owl_max_cardinality_id, &datastore.resources)?;
        Some(AnonExpr {
            id: subj,
            predecessors: vec![],
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                let ope = decls.object_property_expression(y, res);
                ClassExpression::ObjectMaxCardinality(n.clone(), ope)
            }),
        })
    } else if has(OWL_CARDINALITY) {
        let y = require_triple_obj(triples, ids.owl_on_property_id, "owl:onProperty")?;
        let n = require_cardinality(triples, ids.owl_cardinality_id, &datastore.resources)?;
        Some(AnonExpr {
            id: subj,
            predecessors: vec![],
            builder: Box::new(move |decls: &OntologyDeclarations, res| {
                let ope = decls.object_property_expression(y, res);
                ClassExpression::ObjectExactQualifiedCardinality(
                    n.clone(),
                    ope,
                    Box::new(ClassExpression::ClassName(FullIri(IriReference(
                        OWL_THING.to_string(),
                    )))),
                )
            }),
        })
    } else {
        log::warn!(
            "Invalid owl:Restriction {}: no recognized restriction predicate found — skipping",
            subj
        );
        None
    }
}

use std::collections::HashMap;

use ingress::{GraphElement, IriReference, RdfResource};

use crate::ast::{MappingDocument, ObjectMap, TermMap, TermType, TriplesMap};
use crate::plan::{
    FormatFunction, GenerationLogic, JoinAlgorithm, JoinCondition, LogicalJoin, LogicalPlan,
    LogicalProjection, LogicalScan, OutputAttr, TermPattern,
};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

pub fn translate(mapping: &MappingDocument) -> Vec<LogicalPlan> {
    let parent_by_id: HashMap<&IriReference, &TriplesMap> =
        mapping.triples_maps.iter().map(|tm| (&tm.id, tm)).collect();

    let mut plans = Vec::new();
    for tm in &mapping.triples_maps {
        translate_triples_map(tm, &parent_by_id, &mut plans);
    }
    plans
}

fn make_scan(tm: &TriplesMap) -> LogicalPlan {
    LogicalPlan::Scan(LogicalScan {
        source: tm.logical_source.source.clone(),
        reference_formulation: tm.logical_source.reference_formulation.clone(),
        iterator: tm.logical_source.iterator.clone(),
    })
}

fn term_map_to_logic(
    term_map: &TermMap,
    term_type: TermType,
    om: Option<&ObjectMap>,
) -> GenerationLogic {
    let language = om.and_then(|o| o.language.clone());
    let datatype = om.and_then(|o| o.datatype.clone());
    match term_map {
        TermMap::Constant(elem) => GenerationLogic::Constant(elem.clone()),
        TermMap::Template(t) => GenerationLogic::Dynamic(FormatFunction {
            pattern: TermPattern::Template(t.clone()),
            term_type,
            language,
            datatype,
        }),
        TermMap::Reference(r) => GenerationLogic::Dynamic(FormatFunction {
            pattern: TermPattern::Reference(r.clone()),
            term_type,
            language,
            datatype,
        }),
    }
}

fn translate_triples_map(
    tm: &TriplesMap,
    parent_by_id: &HashMap<&IriReference, &TriplesMap>,
    plans: &mut Vec<LogicalPlan>,
) {
    let subject_logic = term_map_to_logic(&tm.subject_map.term_map, tm.subject_map.term_type, None);

    // One plan per predicate × object pair in each PredicateObjectMap
    for pom in &tm.predicate_object_maps {
        for (pred_map, pred_type) in &pom.predicate_maps {
            let pred_logic = term_map_to_logic(pred_map, *pred_type, None);
            for obj_map in &pom.object_maps {
                let (input, obj_logic) = match &obj_map.parent_triples_map {
                    Some(parent_id) => {
                        let parent_tm = parent_by_id.get(parent_id).unwrap_or_else(|| {
                            panic!("unknown rml:parentTriplesMap {parent_id:?}")
                        });
                        let conditions: Vec<JoinCondition> = obj_map
                            .join_conditions
                            .iter()
                            .map(|jc| JoinCondition {
                                left_column: jc.child.clone(),
                                right_column: jc.parent.clone(),
                            })
                            .collect();
                        let join_obj_logic = term_map_to_logic(
                            &parent_tm.subject_map.term_map,
                            parent_tm.subject_map.term_type,
                            None,
                        );
                        let join = LogicalPlan::Join(LogicalJoin {
                            left: Box::new(make_scan(tm)),
                            right: Box::new(make_scan(parent_tm)),
                            conditions,
                            algorithm: JoinAlgorithm::HashJoin,
                        });
                        (join, join_obj_logic)
                    }
                    None => (
                        make_scan(tm),
                        term_map_to_logic(&obj_map.term_map, obj_map.term_type, Some(obj_map)),
                    ),
                };

                let mut attrs = vec![
                    (OutputAttr::Subject, subject_logic.clone()),
                    (OutputAttr::Predicate, pred_logic.clone()),
                    (OutputAttr::Object, obj_logic),
                ];

                // Graph from subject map or predicate-object map (first wins)
                if let Some(gm) = tm
                    .subject_map
                    .graph_maps
                    .iter()
                    .chain(pom.graph_maps.iter())
                    .next()
                {
                    let g_logic = term_map_to_logic(&gm.term_map, TermType::Iri, None);
                    attrs.push((OutputAttr::Graph, g_logic));
                }

                plans.push(LogicalPlan::Projection(LogicalProjection {
                    input: Box::new(input),
                    attrs,
                }));
            }
        }
    }

    // rml:class shorthands: one plan per class, predicate = rdf:type, object = class IRI
    for class_iri in &tm.subject_map.classes {
        let pred_elem =
            GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(RDF_TYPE.to_string())));
        let obj_elem = GraphElement::NodeOrEdge(RdfResource::Iri(class_iri.clone()));
        let attrs = vec![
            (OutputAttr::Subject, subject_logic.clone()),
            (OutputAttr::Predicate, GenerationLogic::Constant(pred_elem)),
            (OutputAttr::Object, GenerationLogic::Constant(obj_elem)),
        ];
        plans.push(LogicalPlan::Projection(LogicalProjection {
            input: Box::new(make_scan(tm)),
            attrs,
        }));
    }
}

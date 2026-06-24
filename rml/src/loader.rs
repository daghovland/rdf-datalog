use std::path::Path;

use dag_rdf::{Datastore, GraphElementId, RdfLiteral, RdfResource};
use ingress::{GraphElement, IriReference};
use turtle::parse_turtle;

use crate::RmlError;
use crate::ast::{
    GraphMap, JoinConditionRef, LogicalSource, LogicalSourceRef, MappingDocument, ObjectMap,
    PredicateObjectMap, ReferenceFormulation, SubjectMap, TermMap, TermType, TriplesMap,
};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RML: &str = "http://w3id.org/rml/";
const QL: &str = "http://semweb.mmlab.be/ns/ql#";

fn rml(local: &str) -> String {
    format!("{RML}{local}")
}

fn ql(local: &str) -> String {
    format!("{QL}{local}")
}

/// Load an RML mapping from a Turtle file on disk.
pub fn load_mapping(path: &Path) -> Result<MappingDocument, RmlError> {
    let content = std::fs::read_to_string(path)?;
    load_mapping_from_str(&content)
}

/// Load an RML mapping from a Turtle string (convenience for tests).
pub fn load_mapping_from_str(turtle: &str) -> Result<MappingDocument, RmlError> {
    let mut ds = Datastore::new(1_000);
    parse_turtle(&mut ds, turtle.as_bytes()).map_err(|e| RmlError::MappingParse(e.to_string()))?;
    extract_mapping(&ds)
}

fn get_id(ds: &Datastore, iri: &str) -> Option<GraphElementId> {
    let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())));
    ds.resources.resource_map.get(&elem).copied()
}

fn get_literal_string(ds: &Datastore, id: GraphElementId) -> Option<&str> {
    match ds.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some(s.as_str()),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            Some(literal.as_str())
        }
        _ => None,
    }
}

fn get_iri(ds: &Datastore, id: GraphElementId) -> Option<&str> {
    match ds.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s))) => Some(s.as_str()),
        _ => None,
    }
}

fn first_obj(
    ds: &Datastore,
    subject: GraphElementId,
    predicate_iri: &str,
) -> Option<GraphElementId> {
    let pred = get_id(ds, predicate_iri)?;
    ds.get_triples_with_subject_predicate(subject, pred)
        .next()
        .map(|t| t.obj)
}

fn all_objs(ds: &Datastore, subject: GraphElementId, predicate_iri: &str) -> Vec<GraphElementId> {
    let Some(pred) = get_id(ds, predicate_iri) else {
        return vec![];
    };
    ds.get_triples_with_subject_predicate(subject, pred)
        .map(|t| t.obj)
        .collect()
}

fn extract_term_map(ds: &Datastore, node: GraphElementId) -> TermMap {
    if let Some(s) = first_obj(ds, node, &rml("template")).and_then(|id| get_literal_string(ds, id))
    {
        return TermMap::Template(s.to_string());
    }
    if let Some(s) =
        first_obj(ds, node, &rml("reference")).and_then(|id| get_literal_string(ds, id))
    {
        return TermMap::Reference(s.to_string());
    }
    if let Some(const_id) = first_obj(ds, node, &rml("constant")) {
        let elem = ds.resources.get_graph_element(const_id).clone();
        return TermMap::Constant(elem);
    }
    TermMap::Reference(String::new())
}

fn extract_term_type(ds: &Datastore, node: GraphElementId, default: TermType) -> TermType {
    let Some(tt_id) = first_obj(ds, node, &rml("termType")) else {
        return default;
    };
    match get_iri(ds, tt_id) {
        Some(s) if s == rml("IRI") => TermType::Iri,
        Some(s) if s == rml("BlankNode") => TermType::BlankNode,
        Some(s) if s == rml("Literal") => TermType::Literal,
        _ => default,
    }
}

fn extract_graph_maps(ds: &Datastore, node: GraphElementId) -> Vec<GraphMap> {
    all_objs(ds, node, &rml("graphMap"))
        .into_iter()
        .map(|gm_node| {
            let term_map = extract_term_map(ds, gm_node);
            GraphMap { term_map }
        })
        .collect()
}

fn extract_join_conditions(ds: &Datastore, om_id: GraphElementId) -> Vec<JoinConditionRef> {
    all_objs(ds, om_id, &rml("joinCondition"))
        .into_iter()
        .filter_map(|jc_id| {
            let child =
                first_obj(ds, jc_id, &rml("child")).and_then(|id| get_literal_string(ds, id))?;
            let parent =
                first_obj(ds, jc_id, &rml("parent")).and_then(|id| get_literal_string(ds, id))?;
            Some(JoinConditionRef {
                child: child.to_string(),
                parent: parent.to_string(),
            })
        })
        .collect()
}

fn extract_logical_source(
    ds: &Datastore,
    tm_id: GraphElementId,
) -> Result<LogicalSource, RmlError> {
    let ls_id =
        first_obj(ds, tm_id, &rml("logicalSource")).ok_or_else(|| RmlError::MissingProperty {
            subject: format!("{tm_id:?}"),
            property: "rml:logicalSource".to_string(),
        })?;

    let source_id =
        first_obj(ds, ls_id, &rml("source")).ok_or_else(|| RmlError::MissingProperty {
            subject: format!("{ls_id:?}"),
            property: "rml:source".to_string(),
        })?;
    let source_str =
        get_literal_string(ds, source_id).ok_or_else(|| RmlError::MissingProperty {
            subject: format!("{ls_id:?}"),
            property: "rml:source (string value)".to_string(),
        })?;

    let reference_formulation = {
        let rf_id = first_obj(ds, ls_id, &rml("referenceFormulation"));
        match rf_id.and_then(|id| get_iri(ds, id)) {
            Some(s) if s == rml("CSV") => ReferenceFormulation::Csv,
            Some(s) if s == rml("JSONPath") || s == ql("JSONPath") => {
                ReferenceFormulation::JsonPath
            }
            Some(s) if s == rml("XPath") || s == ql("XPath") => ReferenceFormulation::XPath,
            _ => ReferenceFormulation::Csv,
        }
    };

    let iterator = first_obj(ds, ls_id, &rml("iterator"))
        .and_then(|id| get_literal_string(ds, id))
        .map(|s| s.to_string());

    Ok(LogicalSource {
        source: LogicalSourceRef::File(source_str.into()),
        reference_formulation,
        iterator,
    })
}

fn extract_subject_map(ds: &Datastore, tm_id: GraphElementId) -> Result<SubjectMap, RmlError> {
    let sm_id =
        first_obj(ds, tm_id, &rml("subjectMap")).ok_or_else(|| RmlError::MissingProperty {
            subject: format!("{tm_id:?}"),
            property: "rml:subjectMap".to_string(),
        })?;

    let term_map = extract_term_map(ds, sm_id);
    let term_type = extract_term_type(ds, sm_id, TermType::Iri);

    let classes = all_objs(ds, sm_id, &rml("class"))
        .into_iter()
        .filter_map(|id| get_iri(ds, id).map(|s| IriReference(s.to_string())))
        .collect();

    let graph_maps = extract_graph_maps(ds, sm_id);

    Ok(SubjectMap {
        term_map,
        term_type,
        classes,
        graph_maps,
    })
}

fn extract_predicate_object_maps(ds: &Datastore, tm_id: GraphElementId) -> Vec<PredicateObjectMap> {
    all_objs(ds, tm_id, &rml("predicateObjectMap"))
        .into_iter()
        .map(|pom_id| {
            // Predicate maps: rml:predicate shorthand or rml:predicateMap
            let mut predicate_maps: Vec<(TermMap, TermType)> = Vec::new();

            // rml:predicate shorthand — constant IRI predicate
            for pred_id in all_objs(ds, pom_id, &rml("predicate")) {
                let elem = ds.resources.get_graph_element(pred_id).clone();
                predicate_maps.push((TermMap::Constant(elem), TermType::Iri));
            }
            // rml:predicateMap (full form)
            for pm_id in all_objs(ds, pom_id, &rml("predicateMap")) {
                let tm = extract_term_map(ds, pm_id);
                let tt = extract_term_type(ds, pm_id, TermType::Iri);
                predicate_maps.push((tm, tt));
            }

            // Object maps: rml:object shorthand or rml:objectMap
            let mut object_maps: Vec<ObjectMap> = Vec::new();

            for obj_id in all_objs(ds, pom_id, &rml("object")) {
                let elem = ds.resources.get_graph_element(obj_id).clone();
                object_maps.push(ObjectMap {
                    term_map: TermMap::Constant(elem),
                    term_type: TermType::Iri,
                    language: None,
                    datatype: None,
                    parent_triples_map: None,
                    join_conditions: vec![],
                });
            }
            for om_id in all_objs(ds, pom_id, &rml("objectMap")) {
                let language = first_obj(ds, om_id, &rml("language"))
                    .and_then(|id| get_literal_string(ds, id))
                    .map(|s| s.to_string());
                let datatype = first_obj(ds, om_id, &rml("datatype"))
                    .and_then(|id| get_iri(ds, id))
                    .map(|s| IriReference(s.to_string()));
                let term_map = extract_term_map(ds, om_id);
                let default_tt = if language.is_some() || datatype.is_some() {
                    TermType::Literal
                } else {
                    match &term_map {
                        TermMap::Reference(_) => TermType::Literal,
                        _ => TermType::Iri,
                    }
                };
                let term_type = extract_term_type(ds, om_id, default_tt);
                let parent_triples_map = first_obj(ds, om_id, &rml("parentTriplesMap"))
                    .and_then(|id| get_iri(ds, id))
                    .map(|s| IriReference(s.to_string()));
                let join_conditions = extract_join_conditions(ds, om_id);

                object_maps.push(ObjectMap {
                    term_map,
                    term_type,
                    language,
                    datatype,
                    parent_triples_map,
                    join_conditions,
                });
            }

            let graph_maps = extract_graph_maps(ds, pom_id);

            PredicateObjectMap {
                predicate_maps,
                object_maps,
                graph_maps,
            }
        })
        .collect()
}

fn extract_mapping(ds: &Datastore) -> Result<MappingDocument, RmlError> {
    let Some(rdf_type_id) = get_id(ds, RDF_TYPE) else {
        return Ok(MappingDocument {
            triples_maps: vec![],
        });
    };
    let Some(triples_map_type_id) = get_id(ds, &rml("TriplesMap")) else {
        return Ok(MappingDocument {
            triples_maps: vec![],
        });
    };

    let tm_ids: Vec<GraphElementId> = ds
        .get_triples_with_predicate(rdf_type_id)
        .filter(|t| t.obj == triples_map_type_id)
        .map(|t| t.subject)
        .collect();

    let mut triples_maps = Vec::new();
    for tm_id in tm_ids {
        let id = match ds.resources.get_graph_element(tm_id) {
            GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => iri.clone(),
            _ => IriReference(format!("_:tm{tm_id:?}")),
        };
        let logical_source = extract_logical_source(ds, tm_id)?;
        let subject_map = extract_subject_map(ds, tm_id)?;
        let predicate_object_maps = extract_predicate_object_maps(ds, tm_id);
        triples_maps.push(TriplesMap {
            id,
            logical_source,
            subject_map,
            predicate_object_maps,
        });
    }

    Ok(MappingDocument { triples_maps })
}

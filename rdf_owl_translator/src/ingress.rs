/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Shared helper types and functions.
//! Mirrors `DagSemTools.RdfOwlTranslator.Ingress`.

use dag_rdf::ingress::Triple;
use dag_rdf::{
    GraphElement, GraphElementId, GraphElementManager, IriReference, RdfLiteral, RdfResource,
};
use ingress::*;
use num_bigint::BigInt;
use owl_ontology::{FullIri, Individual};
use std::collections::{HashMap, VecDeque};

/// All pre-computed resource IDs for the well-known OWL/RDF/RDFS IRIs.
/// Built once at the start of translation.
pub struct WellKnownIds {
    pub rdf_type_id: GraphElementId,
    pub rdf_nil_id: GraphElementId,
    pub rdf_first_id: GraphElementId,
    pub rdf_rest_id: GraphElementId,
    pub rdfs_literal_id: GraphElementId,
    pub rdfs_sub_class_of_id: GraphElementId,
    pub rdfs_sub_property_of_id: GraphElementId,
    pub rdfs_datatype_id: GraphElementId,
    pub rdfs_domain_id: GraphElementId,
    pub rdfs_range_id: GraphElementId,
    pub owl_ontology_id: GraphElementId,
    pub owl_version_iri_id: GraphElementId,
    pub owl_import_id: GraphElementId,
    pub owl_class_id: GraphElementId,
    pub owl_restriction_id: GraphElementId,
    pub owl_on_property_id: GraphElementId,
    pub owl_on_properties_id: GraphElementId,
    pub owl_on_class_id: GraphElementId,
    pub owl_on_data_range_id: GraphElementId,
    pub owl_some_values_from_id: GraphElementId,
    pub owl_all_values_from_id: GraphElementId,
    pub owl_intersection_of_id: GraphElementId,
    pub owl_union_of_id: GraphElementId,
    pub owl_complement_of_id: GraphElementId,
    pub owl_one_of_id: GraphElementId,
    pub owl_has_value_id: GraphElementId,
    pub owl_has_self_id: GraphElementId,
    pub owl_qualified_cardinality_id: GraphElementId,
    pub owl_min_qualified_cardinality_id: GraphElementId,
    pub owl_max_qualified_cardinality_id: GraphElementId,
    pub owl_cardinality_id: GraphElementId,
    pub owl_min_cardinality_id: GraphElementId,
    pub owl_max_cardinality_id: GraphElementId,
    pub owl_axiom_id: GraphElementId,
    pub owl_members_id: GraphElementId,
    pub owl_annotated_source_id: GraphElementId,
    pub owl_annotated_property_id: GraphElementId,
    pub owl_annotated_target_id: GraphElementId,
    pub owl_object_inverse_of_id: GraphElementId,
    pub owl_object_property_id: GraphElementId,
    pub owl_datatype_property_id: GraphElementId,
    pub owl_annotation_property_id: GraphElementId,
    pub owl_named_individual_id: GraphElementId,
    pub owl_equivalent_class_id: GraphElementId,
    pub owl_disjoint_with_id: GraphElementId,
    pub owl_disjoint_union_of_id: GraphElementId,
    pub owl_equivalent_property_id: GraphElementId,
    pub owl_property_disjoint_with_id: GraphElementId,
    pub owl_functional_property_id: GraphElementId,
    pub owl_inverse_functional_property_id: GraphElementId,
    pub owl_reflexive_property_id: GraphElementId,
    pub owl_irreflexive_property_id: GraphElementId,
    pub owl_symmetric_property_id: GraphElementId,
    pub owl_asymmetric_property_id: GraphElementId,
    pub owl_transitive_property_id: GraphElementId,
    pub owl_property_chain_axiom_id: GraphElementId,
    pub owl_all_disjoint_classes_id: GraphElementId,
    pub owl_all_disjoint_properties_id: GraphElementId,
    pub owl_negative_property_assertion_id: GraphElementId,
    pub owl_all_different_id: GraphElementId,
    pub owl_annotation_id: GraphElementId,
    pub owl_same_as_id: GraphElementId,
}

fn iri_id(res: &mut GraphElementManager, iri: &str) -> GraphElementId {
    res.add_node_resource(RdfResource::Iri(IriReference(iri.to_string())))
}

impl WellKnownIds {
    pub fn new(res: &mut GraphElementManager) -> Self {
        WellKnownIds {
            rdf_type_id: iri_id(res, RDF_TYPE),
            rdf_nil_id: iri_id(res, RDF_NIL),
            rdf_first_id: iri_id(res, RDF_FIRST),
            rdf_rest_id: iri_id(res, RDF_REST),
            rdfs_literal_id: iri_id(res, RDFS_LITERAL),
            rdfs_sub_class_of_id: iri_id(res, RDFS_SUB_CLASS_OF),
            rdfs_sub_property_of_id: iri_id(res, RDFS_SUB_PROPERTY_OF),
            rdfs_datatype_id: iri_id(res, RDFS_DATATYPE),
            rdfs_domain_id: iri_id(res, RDFS_DOMAIN),
            rdfs_range_id: iri_id(res, RDFS_RANGE),
            owl_ontology_id: iri_id(res, OWL_ONTOLOGY),
            owl_version_iri_id: iri_id(res, OWL_VERSION_IRI),
            owl_import_id: iri_id(res, OWL_IMPORT),
            owl_class_id: iri_id(res, OWL_CLASS),
            owl_restriction_id: iri_id(res, OWL_RESTRICTION),
            owl_on_property_id: iri_id(res, OWL_ON_PROPERTY),
            owl_on_properties_id: iri_id(res, OWL_ON_PROPERTIES),
            owl_on_class_id: iri_id(res, OWL_ON_CLASS),
            owl_on_data_range_id: iri_id(res, OWL_ON_DATA_RANGE),
            owl_some_values_from_id: iri_id(res, OWL_SOME_VALUES_FROM),
            owl_all_values_from_id: iri_id(res, OWL_ALL_VALUES_FROM),
            owl_intersection_of_id: iri_id(res, OWL_INTERSECTION_OF),
            owl_union_of_id: iri_id(res, OWL_UNION_OF),
            owl_complement_of_id: iri_id(res, OWL_COMPLEMENT_OF),
            owl_one_of_id: iri_id(res, OWL_ONE_OF),
            owl_has_value_id: iri_id(res, OWL_HAS_VALUE),
            owl_has_self_id: iri_id(res, OWL_HAS_SELF),
            owl_qualified_cardinality_id: iri_id(res, OWL_QUALIFIED_CARDINALITY),
            owl_min_qualified_cardinality_id: iri_id(res, OWL_MIN_QUALIFIED_CARDINALITY),
            owl_max_qualified_cardinality_id: iri_id(res, OWL_MAX_QUALIFIED_CARDINALITY),
            owl_cardinality_id: iri_id(res, OWL_CARDINALITY),
            owl_min_cardinality_id: iri_id(res, OWL_MIN_CARDINALITY),
            owl_max_cardinality_id: iri_id(res, OWL_MAX_CARDINALITY),
            owl_axiom_id: iri_id(res, OWL_AXIOM),
            owl_members_id: iri_id(res, OWL_MEMBERS),
            owl_annotated_source_id: iri_id(res, OWL_ANNOTATED_SOURCE),
            owl_annotated_property_id: iri_id(res, OWL_ANNOTATED_PROPERTY),
            owl_annotated_target_id: iri_id(res, OWL_ANNOTATED_TARGET),
            owl_object_inverse_of_id: iri_id(res, OWL_OBJECT_INVERSE_OF),
            owl_object_property_id: iri_id(res, OWL_OBJECT_PROPERTY),
            owl_datatype_property_id: iri_id(res, OWL_DATATYPE_PROPERTY),
            owl_annotation_property_id: iri_id(res, OWL_ANNOTATION_PROPERTY),
            owl_named_individual_id: iri_id(res, OWL_NAMED_INDIVIDUAL),
            owl_equivalent_class_id: iri_id(res, OWL_EQUIVALENT_CLASS),
            owl_disjoint_with_id: iri_id(res, OWL_DISJOINT_WITH),
            owl_disjoint_union_of_id: iri_id(res, OWL_DISJOINT_UNION_OF),
            owl_equivalent_property_id: iri_id(res, OWL_EQUIVALENT_PROPERTY),
            owl_property_disjoint_with_id: iri_id(res, OWL_PROPERTY_DISJOINT_WITH),
            owl_functional_property_id: iri_id(res, OWL_FUNCTIONAL_PROPERTY),
            owl_inverse_functional_property_id: iri_id(res, OWL_INVERSE_FUNCTIONAL_PROPERTY),
            owl_reflexive_property_id: iri_id(res, OWL_REFLEXIVE_PROPERTY),
            owl_irreflexive_property_id: iri_id(res, OWL_IRREFLEXIVE_PROPERTY),
            owl_symmetric_property_id: iri_id(res, OWL_SYMMETRIC_PROPERTY),
            owl_asymmetric_property_id: iri_id(res, OWL_ASYMMETRIC_PROPERTY),
            owl_transitive_property_id: iri_id(res, OWL_TRANSITIVE_PROPERTY),
            owl_property_chain_axiom_id: iri_id(res, OWL_PROPERTY_CHAIN_AXIOM),
            owl_all_disjoint_classes_id: iri_id(res, OWL_ALL_DISJOINT_CLASSES),
            owl_all_disjoint_properties_id: iri_id(res, OWL_ALL_DISJOINT_PROPERTIES),
            owl_negative_property_assertion_id: iri_id(res, OWL_NEGATIVE_PROPERTY_ASSERTION),
            owl_all_different_id: iri_id(res, OWL_ALL_DIFFERENT),
            owl_annotation_id: iri_id(res, OWL_ANNOTATION),
            owl_same_as_id: iri_id(res, OWL_SAME_AS),
        }
    }
}

/// Traverse an RDF list and return its elements in order.
pub fn get_rdf_list_elements(
    triples: &dyn Fn(GraphElementId, GraphElementId) -> Vec<Triple>,
    ids: &WellKnownIds,
    list_id: GraphElementId,
) -> Vec<GraphElementId> {
    let mut result = Vec::new();
    let mut current = list_id;
    let mut visited = Vec::new();

    loop {
        if current == ids.rdf_nil_id {
            break;
        }
        if visited.contains(&current) {
            panic!("Cyclic RDF list");
        }
        visited.push(current);

        let first = triples(current, ids.rdf_first_id);
        let head = match first.as_slice() {
            [tr] => tr.obj,
            _ => panic!("Invalid RDF list: wrong number of rdf:first triples"),
        };

        let rest = triples(current, ids.rdf_rest_id);
        let tail = match rest.as_slice() {
            [tr] => tr.obj,
            _ => panic!("Invalid RDF list: wrong number of rdf:rest triples"),
        };

        result.push(head);
        current = tail;
    }
    result
}

/// Turn a graph element into an OWL Individual. Panics if it is a literal.
pub fn try_get_individual(gel: &GraphElement) -> Individual {
    match gel {
        GraphElement::GraphLiteral(lit) => {
            panic!("Invalid OWL: literal {:?} used as individual", lit)
        }
        GraphElement::NodeOrEdge(res) => match res {
            RdfResource::Iri(iri) => Individual::NamedIndividual(FullIri(iri.clone())),
            RdfResource::AnonymousBlankNode(n) => Individual::AnonymousIndividual(*n),
        },
        // Triple terms cannot be OWL individuals; full RDF 1.2 support tracked in #143.
        GraphElement::TripleTerm(_) => {
            panic!(
                "Invalid OWL: triple term used as individual (RDF 1.2 not yet supported, see issue #143)"
            )
        }
    }
}

/// Extract a literal from a graph element. Panics if it is not a literal.
pub fn try_get_literal(gel: &GraphElement) -> &RdfLiteral {
    match gel {
        GraphElement::GraphLiteral(lit) => lit,
        other => panic!("{:?} used as a literal but is not one", other),
    }
}

/// Parse a non-negative integer from a graph element.
pub fn try_get_non_negative_integer_literal(gel: &GraphElement) -> Option<BigInt> {
    match gel {
        GraphElement::NodeOrEdge(_) | GraphElement::TripleTerm(_) => None,
        GraphElement::GraphLiteral(lit) => match lit {
            RdfLiteral::IntegerLiteral(n) => Some(n.clone()),
            RdfLiteral::TypedLiteral { type_iri, literal } => {
                let t = type_iri.0.as_str();
                if t == XSD_INT || t == XSD_INTEGER || t == XSD_NON_NEGATIVE_INTEGER {
                    literal.parse::<BigInt>().ok()
                } else {
                    None
                }
            }
            _ => None,
        },
    }
}

/// Parse a boolean from a graph element.
pub fn try_get_bool_literal(gel: &GraphElement) -> Option<bool> {
    match gel {
        GraphElement::NodeOrEdge(_) | GraphElement::TripleTerm(_) => None,
        GraphElement::GraphLiteral(lit) => match lit {
            RdfLiteral::BooleanLiteral(b) => Some(*b),
            RdfLiteral::TypedLiteral { type_iri, literal } if type_iri.0 == XSD_BOOLEAN => {
                match literal.as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    other => panic!("Invalid xsd:boolean value: {}", other),
                }
            }
            _ => None,
        },
    }
}

/// Topological sort using Kahn's algorithm.
pub fn topological_sort(
    nodes: &[GraphElementId],
    predecessors: &HashMap<GraphElementId, Vec<GraphElementId>>,
) -> Vec<GraphElementId> {
    let node_set: std::collections::HashSet<GraphElementId> = nodes.iter().copied().collect();

    let mut in_degree: HashMap<GraphElementId, usize> = nodes.iter().map(|&n| (n, 0)).collect();
    let mut successors: HashMap<GraphElementId, Vec<GraphElementId>> = HashMap::new();

    for &node in nodes {
        let preds = predecessors.get(&node).map(|v| v.as_slice()).unwrap_or(&[]);
        for &pred in preds {
            if node_set.contains(&pred) {
                *in_degree.entry(node).or_insert(0) += 1;
                successors.entry(pred).or_default().push(node);
            }
        }
    }

    let mut queue: VecDeque<GraphElementId> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(n, _)| *n)
        .collect();

    let mut result = Vec::with_capacity(nodes.len());
    while let Some(node) = queue.pop_front() {
        result.push(node);
        if let Some(succs) = successors.get(&node) {
            for &succ in succs {
                let deg = in_degree.entry(succ).or_insert(0);
                if *deg > 0 {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(succ);
                    }
                }
            }
        }
    }

    if result.len() != nodes.len() {
        panic!("Cycle detected in OWL class expression dependency graph");
    }
    result
}

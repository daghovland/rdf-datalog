/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Shared helper types and functions.
//! Mirrors `DagSemTools.RdfOwlTranslator.Ingress`.

use crate::error::TranslatorError;
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
///
/// Returns [`TranslatorError::MalformedRdfList`] instead of panicking when
/// the RDF encoding of the list is structurally invalid: a cycle (an
/// `rdf:rest` chain that revisits a node), or a list node with != 1
/// `rdf:first`/`rdf:rest` triples. See
/// <https://github.com/daghovland/rdf-datalog/issues/363>.
pub fn get_rdf_list_elements(
    triples: &dyn Fn(GraphElementId, GraphElementId) -> Vec<Triple>,
    ids: &WellKnownIds,
    list_id: GraphElementId,
) -> Result<Vec<GraphElementId>, TranslatorError> {
    let mut result = Vec::new();
    let mut current = list_id;
    let mut visited = Vec::new();

    loop {
        if current == ids.rdf_nil_id {
            break;
        }
        if visited.contains(&current) {
            return Err(TranslatorError::MalformedRdfList(format!(
                "cyclic rdf:List at node {current}"
            )));
        }
        visited.push(current);

        let first = triples(current, ids.rdf_first_id);
        let head = match first.as_slice() {
            [tr] => tr.obj,
            other => {
                return Err(TranslatorError::MalformedRdfList(format!(
                    "node {} has {} rdf:first triples, expected exactly 1",
                    current,
                    other.len()
                )));
            }
        };

        let rest = triples(current, ids.rdf_rest_id);
        let tail = match rest.as_slice() {
            [tr] => tr.obj,
            other => {
                return Err(TranslatorError::MalformedRdfList(format!(
                    "node {} has {} rdf:rest triples, expected exactly 1",
                    current,
                    other.len()
                )));
            }
        };

        result.push(head);
        current = tail;
    }
    Ok(result)
}

/// Turn a graph element into an OWL Individual.
///
/// Returns [`TranslatorError::InvalidIndividual`] instead of panicking when
/// `gel` is a literal, or an RDF 1.2 triple term (triple terms cannot be OWL
/// individuals at all; full RDF 1.2 support tracked in
/// <https://github.com/daghovland/rdf-datalog/issues/143>). See
/// <https://github.com/daghovland/rdf-datalog/issues/363>.
pub fn try_get_individual(gel: &GraphElement) -> Result<Individual, TranslatorError> {
    match gel {
        GraphElement::GraphLiteral(lit) => Err(TranslatorError::InvalidIndividual(format!(
            "literal {lit:?} used as individual"
        ))),
        GraphElement::NodeOrEdge(res) => match res {
            RdfResource::Iri(iri) => Ok(Individual::NamedIndividual(FullIri(iri.clone()))),
            RdfResource::AnonymousBlankNode(n) => Ok(Individual::AnonymousIndividual(*n)),
        },
        GraphElement::TripleTerm(_) => Err(TranslatorError::InvalidIndividual(
            "triple term used as individual (RDF 1.2 not yet supported, see issue #143)"
                .to_string(),
        )),
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
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                }
            }
            _ => None,
        },
    }
}

/// Topological sort using Kahn's algorithm.
///
/// Returns [`TranslatorError::CyclicDependency`] instead of panicking when
/// `predecessors` describes a cycle among `nodes` (e.g. two anonymous OWL
/// class expressions whose builders reference each other).
pub fn topological_sort(
    nodes: &[GraphElementId],
    predecessors: &HashMap<GraphElementId, Vec<GraphElementId>>,
) -> Result<Vec<GraphElementId>, TranslatorError> {
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
        return Err(TranslatorError::CyclicDependency(format!(
            "cycle detected in OWL class expression dependency graph: {} of {} nodes sorted",
            result.len(),
            nodes.len()
        )));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;

    /// Build a well-formed 2-element `rdf:List` (a . b . rdf:nil), returning
    /// `(datastore, ids, list_head_id, [element_a_id, element_b_id])`.
    fn well_formed_list() -> (Datastore, WellKnownIds, GraphElementId, [GraphElementId; 2]) {
        let mut ds = Datastore::new(1_000);
        let ids = WellKnownIds::new(&mut ds.resources);

        let elem_a = ds.resources.create_unnamed_anon_resource();
        let elem_b = ds.resources.create_unnamed_anon_resource();
        let node1 = ds.resources.create_unnamed_anon_resource();
        let node2 = ds.resources.create_unnamed_anon_resource();

        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_first_id,
            obj: elem_a,
        });
        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_rest_id,
            obj: node2,
        });
        ds.add_triple(Triple {
            subject: node2,
            predicate: ids.rdf_first_id,
            obj: elem_b,
        });
        ds.add_triple(Triple {
            subject: node2,
            predicate: ids.rdf_rest_id,
            obj: ids.rdf_nil_id,
        });

        (ds, ids, node1, [elem_a, elem_b])
    }

    fn triples_fn(ds: &Datastore) -> impl Fn(GraphElementId, GraphElementId) -> Vec<Triple> + '_ {
        move |s, p| ds.get_triples_with_subject_predicate(s, p).collect()
    }

    #[test]
    fn get_rdf_list_elements_returns_elements_in_order_for_well_formed_list() {
        let (ds, ids, head, [elem_a, elem_b]) = well_formed_list();
        let result = get_rdf_list_elements(&triples_fn(&ds), &ids, head);
        assert_eq!(result, Ok(vec![elem_a, elem_b]));
    }

    #[test]
    fn get_rdf_list_elements_returns_err_on_cycle() {
        let mut ds = Datastore::new(1_000);
        let ids = WellKnownIds::new(&mut ds.resources);

        let elem_a = ds.resources.create_unnamed_anon_resource();
        let node1 = ds.resources.create_unnamed_anon_resource();
        let node2 = ds.resources.create_unnamed_anon_resource();

        // node1 -> node2 -> node1 (cycle, never reaches rdf:nil)
        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_first_id,
            obj: elem_a,
        });
        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_rest_id,
            obj: node2,
        });
        ds.add_triple(Triple {
            subject: node2,
            predicate: ids.rdf_first_id,
            obj: elem_a,
        });
        ds.add_triple(Triple {
            subject: node2,
            predicate: ids.rdf_rest_id,
            obj: node1,
        });

        let result = get_rdf_list_elements(&triples_fn(&ds), &ids, node1);
        assert!(matches!(result, Err(TranslatorError::MalformedRdfList(_))));
    }

    #[test]
    fn get_rdf_list_elements_returns_err_on_wrong_number_of_first_triples() {
        let mut ds = Datastore::new(1_000);
        let ids = WellKnownIds::new(&mut ds.resources);

        let elem_a = ds.resources.create_unnamed_anon_resource();
        let elem_c = ds.resources.create_unnamed_anon_resource();
        let node1 = ds.resources.create_unnamed_anon_resource();

        // node1 has TWO rdf:first triples.
        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_first_id,
            obj: elem_a,
        });
        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_first_id,
            obj: elem_c,
        });
        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_rest_id,
            obj: ids.rdf_nil_id,
        });

        let result = get_rdf_list_elements(&triples_fn(&ds), &ids, node1);
        assert!(matches!(result, Err(TranslatorError::MalformedRdfList(_))));
    }

    #[test]
    fn get_rdf_list_elements_returns_err_on_zero_rest_triples() {
        let mut ds = Datastore::new(1_000);
        let ids = WellKnownIds::new(&mut ds.resources);

        let elem_a = ds.resources.create_unnamed_anon_resource();
        let node1 = ds.resources.create_unnamed_anon_resource();

        // node1 has rdf:first but no rdf:rest at all.
        ds.add_triple(Triple {
            subject: node1,
            predicate: ids.rdf_first_id,
            obj: elem_a,
        });

        let result = get_rdf_list_elements(&triples_fn(&ds), &ids, node1);
        assert!(matches!(result, Err(TranslatorError::MalformedRdfList(_))));
    }

    #[test]
    fn topological_sort_returns_err_on_cycle() {
        let mut ds = Datastore::new(1_000);
        let node_a = ds.resources.create_unnamed_anon_resource();
        let node_b = ds.resources.create_unnamed_anon_resource();

        // node_a depends on node_b, and node_b depends on node_a: a genuine
        // cycle with no valid topological order.
        let mut predecessors: HashMap<GraphElementId, Vec<GraphElementId>> = HashMap::new();
        predecessors.insert(node_a, vec![node_b]);
        predecessors.insert(node_b, vec![node_a]);

        let result = topological_sort(&[node_a, node_b], &predecessors);
        assert!(matches!(result, Err(TranslatorError::CyclicDependency(_))));
    }

    #[test]
    fn topological_sort_returns_ok_on_acyclic_input() {
        let mut ds = Datastore::new(1_000);
        let node_a = ds.resources.create_unnamed_anon_resource();
        let node_b = ds.resources.create_unnamed_anon_resource();
        let node_c = ds.resources.create_unnamed_anon_resource();

        // node_c depends on node_b, node_b depends on node_a: a valid chain.
        let mut predecessors: HashMap<GraphElementId, Vec<GraphElementId>> = HashMap::new();
        predecessors.insert(node_b, vec![node_a]);
        predecessors.insert(node_c, vec![node_b]);

        let result = topological_sort(&[node_a, node_b, node_c], &predecessors)
            .expect("acyclic input should sort successfully");

        // node_a must come before node_b, which must come before node_c.
        let pos_a = result.iter().position(|&n| n == node_a).unwrap();
        let pos_b = result.iter().position(|&n| n == node_b).unwrap();
        let pos_c = result.iter().position(|&n| n == node_c).unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
        assert_eq!(result.len(), 3);
    }

    fn typed_bool_literal(lexical: &str) -> GraphElement {
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
            type_iri: IriReference(XSD_BOOLEAN.to_string()),
            literal: lexical.to_string(),
        })
    }

    #[test]
    fn try_get_bool_literal_accepts_true_false() {
        assert_eq!(
            try_get_bool_literal(&typed_bool_literal("true")),
            Some(true)
        );
        assert_eq!(
            try_get_bool_literal(&typed_bool_literal("false")),
            Some(false)
        );
    }

    #[test]
    fn try_get_bool_literal_accepts_xsd_lexical_1_and_0() {
        assert_eq!(try_get_bool_literal(&typed_bool_literal("1")), Some(true));
        assert_eq!(try_get_bool_literal(&typed_bool_literal("0")), Some(false));
    }

    #[test]
    fn try_get_bool_literal_returns_none_for_invalid_lexical_form() {
        assert_eq!(try_get_bool_literal(&typed_bool_literal("yes")), None);
    }

    #[test]
    fn try_get_bool_literal_returns_none_for_non_boolean_literal() {
        let gel = GraphElement::GraphLiteral(RdfLiteral::LiteralString("hello".to_string()));
        assert_eq!(try_get_bool_literal(&gel), None);
    }

    #[test]
    fn try_get_individual_ok_on_iri_resource() {
        let gel = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            "http://example.org/x".to_string(),
        )));
        assert_eq!(
            try_get_individual(&gel),
            Ok(Individual::NamedIndividual(FullIri(IriReference(
                "http://example.org/x".to_string()
            ))))
        );
    }

    #[test]
    fn try_get_individual_ok_on_anonymous_blank_node() {
        let gel = GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(42));
        assert_eq!(
            try_get_individual(&gel),
            Ok(Individual::AnonymousIndividual(42))
        );
    }

    #[test]
    fn try_get_individual_err_on_literal() {
        let gel = GraphElement::GraphLiteral(RdfLiteral::LiteralString("hello".to_string()));
        assert!(matches!(
            try_get_individual(&gel),
            Err(TranslatorError::InvalidIndividual(_))
        ));
    }

    #[test]
    fn try_get_individual_err_on_triple_term() {
        use dag_rdf::TripleTermKey;

        let gel = GraphElement::TripleTerm(TripleTermKey {
            subject: 1,
            predicate: 2,
            obj: 3,
        });
        assert!(matches!(
            try_get_individual(&gel),
            Err(TranslatorError::InvalidIndividual(_))
        ));
    }
}

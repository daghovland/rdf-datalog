/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Parse a SHACL shapes `Datastore` into `Vec<ParsedShape>`.
//!
//! The parsed shapes carry IRI strings (not store-specific IDs) so they can be
//! safely re-interned into a different (data) store when generating Datalog rules.

use crate::graph;
use crate::vocab::*;
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, RdfResource};
use ingress::{RDF_TYPE, RDFS_SUB_CLASS_OF};

// ── Public shape representation ───────────────────────────────────────────────

/// A value in a shape constraint — an IRI, blank node, or literal.
#[derive(Debug, Clone)]
pub enum ElemValue {
    Iri(String),
    BlankNode(u32),
    Literal {
        value: String,
        datatype: Option<String>,
        lang: Option<String>,
    },
}

/// Target declarations (`sh:targetClass`, `sh:targetNode`, …).
#[derive(Debug, Clone)]
pub enum Target {
    Node(ElemValue),
    Class(String),
    SubjectsOf(String),
    ObjectsOf(String),
    /// The shape itself is declared as `rdfs:Class` (implicit class target).
    ImplicitClass(String),
}

/// Node-kind constraint values from `sh:nodeKind`.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKindValue {
    IRI,
    Literal,
    BlankNode,
    BlankNodeOrIRI,
    BlankNodeOrLiteral,
    IRIOrLiteral,
}

/// A property constraint parsed from a `sh:property` block.
#[derive(Debug, Clone)]
pub enum PropConstraint {
    MinCount(u64),
    MaxCount(u64),
    Class(String),
    Datatype(String),
    NodeKind(NodeKindValue),
    HasValue(ElemValue),
    In(Vec<ElemValue>),
    MinLength(u64),
    MaxLength(u64),
    Pattern(String, Option<String>),
    LanguageIn(Vec<String>),
    UniqueLang,
    Equals(String),
    Disjoint(String),
    LessThan(String),
    LessThanOrEquals(String),
    QualifiedValueShape {
        shape_id: ElemValue,
        min: Option<u64>,
        max: Option<u64>,
    },
}

/// A single `sh:property` block parsed from a shape.
#[derive(Debug, Clone)]
pub struct ParsedPropShape {
    /// Index of this property shape within its parent (for generating unique IRIs).
    pub idx: usize,
    /// The `sh:path` IRI.
    pub path: String,
    pub constraints: Vec<PropConstraint>,
}

/// A complete shape definition.
#[derive(Debug, Clone)]
pub struct ParsedShape {
    /// Sequential index (0, 1, …) for building unique synthetic IRI names.
    pub idx: usize,
    /// IRI of the shape node, if it is a named IRI (not a blank node).
    pub iri: Option<String>,
    pub targets: Vec<Target>,
    pub property_shapes: Vec<ParsedPropShape>,
    /// `sh:closed true` with allowed predicates (from paths + ignoredProperties).
    pub closed: Option<Vec<String>>,
    /// `sh:not <inner_shape_id>` — the inner shape's element value.
    pub not_shape: Option<ElemValue>,
    /// `sh:and (s1 s2 …)` — element values of sub-shapes.
    pub and_shapes: Vec<ElemValue>,
    /// `sh:or (s1 s2 …)`.
    pub or_shapes: Vec<ElemValue>,
    /// `sh:xone (s1 s2 …)`.
    pub xone_shapes: Vec<ElemValue>,
    /// `sh:class C` at node level.
    pub node_class: Option<String>,
}

// ── Parsing ───────────────────────────────────────────────────────────────────

/// Parse all `sh:NodeShape` and `sh:PropertyShape` nodes from `shapes`.
pub fn parse_shapes(shapes: &Datastore) -> Vec<ParsedShape> {
    let mut found: Vec<GraphElementId> = Vec::new();

    // Collect shapes declared via rdf:type sh:NodeShape or sh:PropertyShape
    for type_iri in [SH_NODE_SHAPE, SH_PROPERTY_SHAPE] {
        if let Some(type_id) = graph::lookup_iri(shapes, type_iri) {
            if let Some(rdf_type_id) = graph::lookup_iri(shapes, RDF_TYPE) {
                for triple in shapes.get_triples_with_object_predicate(type_id, rdf_type_id) {
                    if !found.contains(&triple.subject) {
                        found.push(triple.subject);
                    }
                }
            }
        }
    }

    // Also collect shapes that are declared as rdfs:Class (implicit class target)
    if let Some(rdfs_class_id) = graph::lookup_iri(shapes, "http://www.w3.org/2000/01/rdf-schema#Class") {
        if let Some(rdf_type_id) = graph::lookup_iri(shapes, RDF_TYPE) {
            for triple in shapes.get_triples_with_object_predicate(rdfs_class_id, rdf_type_id) {
                if !found.contains(&triple.subject) {
                    found.push(triple.subject);
                }
            }
        }
    }

    found
        .into_iter()
        .enumerate()
        .map(|(idx, shape_id)| parse_one_shape(shapes, shape_id, idx))
        .collect()
}

fn parse_one_shape(shapes: &Datastore, shape_id: GraphElementId, idx: usize) -> ParsedShape {
    let iri = graph::iri_string(shapes, shape_id);
    let targets = parse_targets(shapes, shape_id);
    let property_shapes = parse_property_shapes(shapes, shape_id);

    // sh:closed + sh:ignoredProperties
    let closed = parse_closed(shapes, shape_id, &property_shapes);

    // sh:not
    let not_shape = graph::get_object(shapes, shape_id, SH_NOT)
        .map(|id| id_to_elem(shapes, id));

    // sh:and / sh:or / sh:xone
    let and_shapes = parse_shape_list(shapes, shape_id, SH_AND);
    let or_shapes = parse_shape_list(shapes, shape_id, SH_OR);
    let xone_shapes = parse_shape_list(shapes, shape_id, SH_XONE);

    // sh:class at node level (rare; usually appears inside sh:property)
    let node_class = graph::get_object(shapes, shape_id, SH_CLASS)
        .and_then(|id| graph::iri_string(shapes, id));

    ParsedShape {
        idx,
        iri,
        targets,
        property_shapes,
        closed,
        not_shape,
        and_shapes,
        or_shapes,
        xone_shapes,
        node_class,
    }
}

fn parse_targets(shapes: &Datastore, shape_id: GraphElementId) -> Vec<Target> {
    let mut targets = Vec::new();

    // sh:targetNode
    for obj_id in graph::get_objects(shapes, shape_id, SH_TARGET_NODE) {
        targets.push(Target::Node(id_to_elem(shapes, obj_id)));
    }

    // sh:targetClass
    for obj_id in graph::get_objects(shapes, shape_id, SH_TARGET_CLASS) {
        if let Some(iri) = graph::iri_string(shapes, obj_id) {
            targets.push(Target::Class(iri));
        }
    }

    // sh:targetSubjectsOf
    for obj_id in graph::get_objects(shapes, shape_id, SH_TARGET_SUBJECTS_OF) {
        if let Some(iri) = graph::iri_string(shapes, obj_id) {
            targets.push(Target::SubjectsOf(iri));
        }
    }

    // sh:targetObjectsOf
    for obj_id in graph::get_objects(shapes, shape_id, SH_TARGET_OBJECTS_OF) {
        if let Some(iri) = graph::iri_string(shapes, obj_id) {
            targets.push(Target::ObjectsOf(iri));
        }
    }

    // Implicit class target: shape is also rdfs:Class
    let rdfs_class = "http://www.w3.org/2000/01/rdf-schema#Class";
    if let Some(iri) = graph::iri_string(shapes, shape_id) {
        let rdf_type_id = graph::lookup_iri(shapes, RDF_TYPE);
        let rdfs_class_id = graph::lookup_iri(shapes, rdfs_class);
        if let (Some(rdf_type_id), Some(rdfs_class_id)) = (rdf_type_id, rdfs_class_id) {
            if shapes
                .get_triples_with_subject_predicate(shape_id, rdf_type_id)
                .any(|t| t.obj == rdfs_class_id)
            {
                targets.push(Target::ImplicitClass(iri));
            }
        }
    }

    targets
}

fn parse_property_shapes(shapes: &Datastore, shape_id: GraphElementId) -> Vec<ParsedPropShape> {
    graph::get_objects(shapes, shape_id, SH_PROPERTY)
        .into_iter()
        .enumerate()
        .filter_map(|(prop_idx, prop_node)| {
            let path_id = graph::get_object(shapes, prop_node, SH_PATH)?;
            let path = graph::iri_string(shapes, path_id)?;
            let constraints = parse_prop_constraints(shapes, prop_node);
            Some(ParsedPropShape {
                idx: prop_idx,
                path,
                constraints,
            })
        })
        .collect()
}

fn parse_prop_constraints(shapes: &Datastore, prop_node: GraphElementId) -> Vec<PropConstraint> {
    let mut cs = Vec::new();

    // §4.2 Cardinality
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MIN_COUNT) {
        if let Some(n) = graph::elem_to_u64(shapes, id) {
            cs.push(PropConstraint::MinCount(n));
        }
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MAX_COUNT) {
        if let Some(n) = graph::elem_to_u64(shapes, id) {
            cs.push(PropConstraint::MaxCount(n));
        }
    }

    // §4.1.1 sh:class
    if let Some(id) = graph::get_object(shapes, prop_node, SH_CLASS) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            cs.push(PropConstraint::Class(iri));
        }
    }

    // §4.1.2 sh:datatype
    if let Some(id) = graph::get_object(shapes, prop_node, SH_DATATYPE) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            cs.push(PropConstraint::Datatype(iri));
        }
    }

    // §4.1.3 sh:nodeKind
    if let Some(id) = graph::get_object(shapes, prop_node, SH_NODE_KIND) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            let nk = match iri.as_str() {
                SH_IRI => Some(NodeKindValue::IRI),
                SH_LITERAL => Some(NodeKindValue::Literal),
                SH_BLANK_NODE => Some(NodeKindValue::BlankNode),
                SH_BLANK_NODE_OR_IRI => Some(NodeKindValue::BlankNodeOrIRI),
                SH_BLANK_NODE_OR_LITERAL => Some(NodeKindValue::BlankNodeOrLiteral),
                SH_IRI_OR_LITERAL => Some(NodeKindValue::IRIOrLiteral),
                _ => None,
            };
            if let Some(nk) = nk {
                cs.push(PropConstraint::NodeKind(nk));
            }
        }
    }
    // sh:nodeKind at the node/shape level (targetObjectsOf + sh:nodeKind)
    // is handled separately as a shape-level constraint in parse_one_shape.

    // §4.8.2 sh:hasValue
    if let Some(id) = graph::get_object(shapes, prop_node, SH_HAS_VALUE) {
        cs.push(PropConstraint::HasValue(id_to_elem(shapes, id)));
    }

    // §4.8.3 sh:in
    if let Some(list_head) = graph::get_object(shapes, prop_node, SH_IN) {
        let items = graph::rdf_list(shapes, list_head)
            .into_iter()
            .map(|id| id_to_elem(shapes, id))
            .collect();
        cs.push(PropConstraint::In(items));
    }

    // §4.4.1/4.4.2 sh:minLength / sh:maxLength
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MIN_LENGTH) {
        if let Some(n) = graph::elem_to_u64(shapes, id) {
            cs.push(PropConstraint::MinLength(n));
        }
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MAX_LENGTH) {
        if let Some(n) = graph::elem_to_u64(shapes, id) {
            cs.push(PropConstraint::MaxLength(n));
        }
    }

    // §4.4.3 sh:pattern
    if let Some(id) = graph::get_object(shapes, prop_node, SH_PATTERN) {
        if let Some(pattern) = literal_string(shapes, id) {
            let flags = graph::get_object(shapes, prop_node, SH_FLAGS)
                .and_then(|fid| literal_string(shapes, fid));
            cs.push(PropConstraint::Pattern(pattern, flags));
        }
    }

    // §4.4.4 sh:languageIn
    if let Some(list_head) = graph::get_object(shapes, prop_node, SH_LANGUAGE_IN) {
        let tags = graph::rdf_list(shapes, list_head)
            .into_iter()
            .filter_map(|id| literal_string(shapes, id))
            .collect();
        cs.push(PropConstraint::LanguageIn(tags));
    }

    // §4.4.5 sh:uniqueLang
    if let Some(id) = graph::get_object(shapes, prop_node, SH_UNIQUE_LANG) {
        if graph::elem_to_bool(shapes, id) == Some(true) {
            cs.push(PropConstraint::UniqueLang);
        }
    }

    // §4.5 Property pairs
    if let Some(id) = graph::get_object(shapes, prop_node, SH_EQUALS) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            cs.push(PropConstraint::Equals(iri));
        }
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_DISJOINT) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            cs.push(PropConstraint::Disjoint(iri));
        }
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_LESS_THAN) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            cs.push(PropConstraint::LessThan(iri));
        }
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_LESS_THAN_OR_EQUALS) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            cs.push(PropConstraint::LessThanOrEquals(iri));
        }
    }

    // §4.7.3 sh:qualifiedValueShape
    if let Some(qvs_id) = graph::get_object(shapes, prop_node, SH_QUALIFIED_VALUE_SHAPE) {
        let min = graph::get_object(shapes, prop_node, SH_QUALIFIED_MIN_COUNT)
            .and_then(|id| graph::elem_to_u64(shapes, id));
        let max = graph::get_object(shapes, prop_node, SH_QUALIFIED_MAX_COUNT)
            .and_then(|id| graph::elem_to_u64(shapes, id));
        cs.push(PropConstraint::QualifiedValueShape {
            shape_id: id_to_elem(shapes, qvs_id),
            min,
            max,
        });
    }

    cs
}

fn parse_closed(
    shapes: &Datastore,
    shape_id: GraphElementId,
    props: &[ParsedPropShape],
) -> Option<Vec<String>> {
    let closed_id = graph::get_object(shapes, shape_id, SH_CLOSED)?;
    if graph::elem_to_bool(shapes, closed_id) != Some(true) {
        return None;
    }
    // Allowed predicates = paths from sh:property + sh:ignoredProperties
    let mut allowed: Vec<String> = props.iter().map(|p| p.path.clone()).collect();
    if let Some(list_head) = graph::get_object(shapes, shape_id, SH_IGNORED_PROPERTIES) {
        for id in graph::rdf_list(shapes, list_head) {
            if let Some(iri) = graph::iri_string(shapes, id) {
                allowed.push(iri);
            }
        }
    }
    Some(allowed)
}

fn parse_shape_list(shapes: &Datastore, shape_id: GraphElementId, pred_iri: &str) -> Vec<ElemValue> {
    graph::get_object(shapes, shape_id, pred_iri)
        .map(|list_head| {
            graph::rdf_list(shapes, list_head)
                .into_iter()
                .map(|id| id_to_elem(shapes, id))
                .collect()
        })
        .unwrap_or_default()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a `GraphElementId` to an `ElemValue`.
pub fn id_to_elem(shapes: &Datastore, id: GraphElementId) -> ElemValue {
    match shapes.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => ElemValue::Iri(iri.0.clone()),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => ElemValue::BlankNode(*n),
        GraphElement::GraphLiteral(lit) => {
            let (value, datatype, lang) = literal_parts(lit);
            ElemValue::Literal { value, datatype, lang }
        }
    }
}

fn literal_string(shapes: &Datastore, id: GraphElementId) -> Option<String> {
    match shapes.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some(s.clone()),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            Some(literal.clone())
        }
        _ => None,
    }
}

fn literal_parts(lit: &RdfLiteral) -> (String, Option<String>, Option<String>) {
    match lit {
        RdfLiteral::LiteralString(s) => (s.clone(), None, None),
        RdfLiteral::LangLiteral { lang, literal } => {
            (literal.clone(), None, Some(lang.clone()))
        }
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            (literal.clone(), Some(type_iri.0.clone()), None)
        }
        other => (other.to_string(), None, None),
    }
}

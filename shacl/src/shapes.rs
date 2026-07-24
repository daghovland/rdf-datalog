/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Parse a SHACL shapes `Datastore` into `Vec<ParsedShape>`.
//!
//! Every IRI from the shapes store is stored as a plain `String`; no shapes-store
//! `GraphElementId`s leak out (they would be meaningless in the data store).
//!
//! Inner shapes for `sh:not` / `sh:and` / `sh:or` are stored as
//! `InnerShapeRef { shapes_id, … }` so the translator can look up their constraints
//! directly in the shapes `Datastore`.

use crate::graph;
use crate::vocab::*;
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, RdfResource};
use ingress::RDF_TYPE;

// ── Public types ──────────────────────────────────────────────────────────────

/// A value from a shape constraint — an IRI, blank node, or literal.
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
    /// Shape node is also `rdfs:Class` → implicit class target.
    ImplicitClass(String),
}

/// Node-kind values from `sh:nodeKind`.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKindValue {
    IRI,
    Literal,
    BlankNode,
    BlankNodeOrIRI,
    BlankNodeOrLiteral,
    IRIOrLiteral,
}

impl NodeKindValue {
    pub fn from_iri(iri: &str) -> Option<Self> {
        use crate::vocab::*;
        match iri {
            SH_IRI => Some(Self::IRI),
            SH_LITERAL => Some(Self::Literal),
            SH_BLANK_NODE => Some(Self::BlankNode),
            SH_BLANK_NODE_OR_IRI => Some(Self::BlankNodeOrIRI),
            SH_BLANK_NODE_OR_LITERAL => Some(Self::BlankNodeOrLiteral),
            SH_IRI_OR_LITERAL => Some(Self::IRIOrLiteral),
            _ => None,
        }
    }
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
    MinInclusive(ElemValue),
    MaxInclusive(ElemValue),
    MinExclusive(ElemValue),
    MaxExclusive(ElemValue),
    LessThan(String),
    LessThanOrEquals(String),
    NodeShape(GraphElementId),
    QualifiedValueShape {
        shapes_id: GraphElementId,
        min: Option<u64>,
        max: Option<u64>,
    },
}

/// A parsed `sh:property` block.
#[derive(Debug, Clone)]
pub struct ParsedPropShape {
    /// Position within the parent shape (used for unique helper-IRI names).
    pub idx: usize,
    /// `sh:path` IRI.
    pub path: String,
    pub constraints: Vec<PropConstraint>,
}

/// A reference to an inner shape node in the shapes store.
///
/// Used for `sh:not`, `sh:and`, `sh:or`, `sh:xone`.  The `shapes_id` lets
/// the translator query the shapes `Datastore` for the inner shape's constraints.
#[derive(Debug, Clone)]
pub struct InnerShapeRef {
    /// ID of the shape node in the **shapes** Datastore.
    pub shapes_id: GraphElementId,
}

/// A fully parsed shape definition.
#[derive(Debug, Clone)]
pub struct ParsedShape {
    /// Sequential index across all shapes (for unique synthetic IRI names).
    pub idx: usize,
    /// IRI of the shape if it is a named node.
    pub iri: Option<String>,
    pub targets: Vec<Target>,
    pub property_shapes: Vec<ParsedPropShape>,
    /// `sh:closed true` with the list of allowed predicate IRIs.
    pub closed: Option<Vec<String>>,
    /// `sh:not <inner>`.
    pub not_inner: Option<InnerShapeRef>,
    /// `sh:and (s1 s2 …)`.
    pub and_inners: Vec<InnerShapeRef>,
    /// `sh:or (s1 s2 …)`.
    pub or_inners: Vec<InnerShapeRef>,
    /// `sh:xone (s1 s2 …)`.
    pub xone_inners: Vec<InnerShapeRef>,
    /// Value constraints declared directly on the shape node itself (no `sh:path`),
    /// e.g. `ex:S a sh:NodeShape ; sh:targetNode ex:n ; sh:datatype xsd:integer .`
    /// These apply to each focus node directly, rather than to path-traversed
    /// values. Only populated when the shape has no `sh:path` (see `parse_one_shape`);
    /// `sh:nodeKind` is excluded here since it is already handled by the dedicated
    /// `node_kind` field below. See [#260](https://github.com/daghovland/rdf-datalog/issues/260).
    pub node_constraints: Vec<PropConstraint>,
    /// `sh:nodeKind NK` at the node level.
    pub node_kind: Option<NodeKindValue>,
    /// `sh:severity` on this shape, defaulting to `Severity::Violation` when unset.
    pub severity: crate::Severity,
}

// ── Top-level entry point ─────────────────────────────────────────────────────

/// Parse all `sh:NodeShape` and `sh:PropertyShape` nodes from `shapes`.
pub fn parse_shapes(shapes: &Datastore) -> Vec<ParsedShape> {
    let mut found: Vec<GraphElementId> = Vec::new();

    let rdf_type_id = graph::lookup_iri(shapes, RDF_TYPE);
    let rdfs_class_iri = "http://www.w3.org/2000/01/rdf-schema#Class";

    for type_iri in [SH_NODE_SHAPE, SH_PROPERTY_SHAPE, rdfs_class_iri] {
        if let (Some(rdf_type_id), Some(type_id)) =
            (rdf_type_id, graph::lookup_iri(shapes, type_iri))
        {
            for t in shapes.get_triples_with_object_predicate(type_id, rdf_type_id) {
                if !found.contains(&t.subject) {
                    found.push(t.subject);
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

// ── Shape parsing ─────────────────────────────────────────────────────────────

fn parse_one_shape(shapes: &Datastore, shape_id: GraphElementId, idx: usize) -> ParsedShape {
    let iri = graph::iri_string(shapes, shape_id);
    let targets = parse_targets(shapes, shape_id, &iri);
    let mut property_shapes = parse_property_shapes(shapes, shape_id);

    // A sh:PropertyShape may have sh:path + constraints directly on the shape node
    // (rather than inside a sh:property block). Detect and handle this case.
    let has_direct_path = graph::get_object(shapes, shape_id, SH_PATH).is_some();
    if let Some(path_id) = graph::get_object(shapes, shape_id, SH_PATH)
        && let Some(path_iri) = graph::iri_string(shapes, path_id)
    {
        let direct_constraints = parse_prop_constraints(shapes, shape_id);
        if !direct_constraints.is_empty() {
            let next_idx = property_shapes.len();
            property_shapes.push(ParsedPropShape {
                idx: next_idx,
                path: path_iri,
                constraints: direct_constraints,
            });
        }
    }
    let closed = parse_closed(shapes, shape_id, &property_shapes);

    // Node-level (pathless) value constraints, e.g. `sh:datatype`/`sh:in`/`sh:class`
    // directly on the shape node with no `sh:path`. These apply to the focus node
    // itself. Only parsed when there is no `sh:path` on this shape node — a shape
    // node that also declares `sh:path` is itself a property shape whose direct
    // constraints (parsed above) apply to path-traversed values, not the focus
    // node. `sh:nodeKind` is filtered out to avoid double-counting against the
    // dedicated `node_kind` field/mechanism below. See #260.
    let node_constraints: Vec<PropConstraint> = if has_direct_path {
        Vec::new()
    } else {
        parse_prop_constraints(shapes, shape_id)
            .into_iter()
            .filter(|c| !matches!(c, PropConstraint::NodeKind(_)))
            .collect()
    };

    let not_inner =
        graph::get_object(shapes, shape_id, SH_NOT).map(|id| InnerShapeRef { shapes_id: id });

    let and_inners = shape_list_refs(shapes, shape_id, SH_AND);
    let or_inners = shape_list_refs(shapes, shape_id, SH_OR);
    let xone_inners = shape_list_refs(shapes, shape_id, SH_XONE);

    let node_kind = graph::get_object(shapes, shape_id, SH_NODE_KIND)
        .and_then(|id| graph::iri_string(shapes, id))
        .and_then(|iri| parse_node_kind(&iri));

    let severity = graph::get_object(shapes, shape_id, SH_SEVERITY)
        .and_then(|id| graph::iri_string(shapes, id))
        .and_then(|iri| crate::Severity::from_iri(&iri))
        .unwrap_or_default();

    ParsedShape {
        idx,
        iri,
        targets,
        property_shapes,
        closed,
        not_inner,
        and_inners,
        or_inners,
        xone_inners,
        node_constraints,
        node_kind,
        severity,
    }
}

fn parse_targets(
    shapes: &Datastore,
    shape_id: GraphElementId,
    shape_iri: &Option<String>,
) -> Vec<Target> {
    let mut targets = Vec::new();
    let rdf_type_id = graph::lookup_iri(shapes, RDF_TYPE);
    let rdfs_class_iri = "http://www.w3.org/2000/01/rdf-schema#Class";

    for id in graph::get_objects(shapes, shape_id, SH_TARGET_NODE) {
        targets.push(Target::Node(id_to_elem(shapes, id)));
    }
    for id in graph::get_objects(shapes, shape_id, SH_TARGET_CLASS) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            targets.push(Target::Class(iri));
        }
    }
    for id in graph::get_objects(shapes, shape_id, SH_TARGET_SUBJECTS_OF) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            targets.push(Target::SubjectsOf(iri));
        }
    }
    for id in graph::get_objects(shapes, shape_id, SH_TARGET_OBJECTS_OF) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            targets.push(Target::ObjectsOf(iri));
        }
    }

    // Implicit class target: shape also declared as rdfs:Class
    if let (Some(iri), Some(rdf_type_id), Some(rdfs_class_id)) = (
        shape_iri,
        rdf_type_id,
        graph::lookup_iri(shapes, rdfs_class_iri),
    ) && shapes
        .get_triples_with_subject_predicate(shape_id, rdf_type_id)
        .any(|t| t.obj == rdfs_class_id)
    {
        targets.push(Target::ImplicitClass(iri.clone()));
    }

    targets
}

fn parse_property_shapes(shapes: &Datastore, shape_id: GraphElementId) -> Vec<ParsedPropShape> {
    graph::get_objects(shapes, shape_id, SH_PROPERTY)
        .into_iter()
        .enumerate()
        .filter_map(|(idx, prop_node)| {
            let path_id = graph::get_object(shapes, prop_node, SH_PATH)?;
            let path = graph::iri_string(shapes, path_id)?;
            Some(ParsedPropShape {
                idx,
                path,
                constraints: parse_prop_constraints(shapes, prop_node),
            })
        })
        .collect()
}

pub fn parse_prop_constraints(
    shapes: &Datastore,
    prop_node: GraphElementId,
) -> Vec<PropConstraint> {
    let mut cs = Vec::new();

    if let Some(id) = graph::get_object(shapes, prop_node, SH_MIN_COUNT)
        && let Some(n) = graph::elem_to_u64(shapes, id)
    {
        cs.push(PropConstraint::MinCount(n));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MAX_COUNT)
        && let Some(n) = graph::elem_to_u64(shapes, id)
    {
        cs.push(PropConstraint::MaxCount(n));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_CLASS)
        && let Some(iri) = graph::iri_string(shapes, id)
    {
        cs.push(PropConstraint::Class(iri));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_DATATYPE)
        && let Some(iri) = graph::iri_string(shapes, id)
    {
        cs.push(PropConstraint::Datatype(iri));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_NODE_KIND)
        && let Some(iri) = graph::iri_string(shapes, id)
        && let Some(nk) = parse_node_kind(&iri)
    {
        cs.push(PropConstraint::NodeKind(nk));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_HAS_VALUE) {
        cs.push(PropConstraint::HasValue(id_to_elem(shapes, id)));
    }
    if let Some(head) = graph::get_object(shapes, prop_node, SH_IN) {
        let items = graph::rdf_list(shapes, head)
            .into_iter()
            .map(|id| id_to_elem(shapes, id))
            .collect();
        cs.push(PropConstraint::In(items));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MIN_LENGTH)
        && let Some(n) = graph::elem_to_u64(shapes, id)
    {
        cs.push(PropConstraint::MinLength(n));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MAX_LENGTH)
        && let Some(n) = graph::elem_to_u64(shapes, id)
    {
        cs.push(PropConstraint::MaxLength(n));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_PATTERN)
        && let Some(pat) = literal_string(shapes, id)
    {
        let flags = graph::get_object(shapes, prop_node, SH_FLAGS)
            .and_then(|fid| literal_string(shapes, fid));
        cs.push(PropConstraint::Pattern(pat, flags));
    }
    if let Some(head) = graph::get_object(shapes, prop_node, SH_LANGUAGE_IN) {
        let tags = graph::rdf_list(shapes, head)
            .into_iter()
            .filter_map(|id| literal_string(shapes, id))
            .collect();
        cs.push(PropConstraint::LanguageIn(tags));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_UNIQUE_LANG)
        && graph::elem_to_bool(shapes, id) == Some(true)
    {
        cs.push(PropConstraint::UniqueLang);
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_EQUALS)
        && let Some(iri) = graph::iri_string(shapes, id)
    {
        cs.push(PropConstraint::Equals(iri));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_DISJOINT)
        && let Some(iri) = graph::iri_string(shapes, id)
    {
        cs.push(PropConstraint::Disjoint(iri));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_LESS_THAN)
        && let Some(iri) = graph::iri_string(shapes, id)
    {
        cs.push(PropConstraint::LessThan(iri));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_LESS_THAN_OR_EQUALS)
        && let Some(iri) = graph::iri_string(shapes, id)
    {
        cs.push(PropConstraint::LessThanOrEquals(iri));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MIN_INCLUSIVE) {
        cs.push(PropConstraint::MinInclusive(id_to_elem(shapes, id)));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MAX_INCLUSIVE) {
        cs.push(PropConstraint::MaxInclusive(id_to_elem(shapes, id)));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MIN_EXCLUSIVE) {
        cs.push(PropConstraint::MinExclusive(id_to_elem(shapes, id)));
    }
    if let Some(id) = graph::get_object(shapes, prop_node, SH_MAX_EXCLUSIVE) {
        cs.push(PropConstraint::MaxExclusive(id_to_elem(shapes, id)));
    }
    if let Some(inner_id) = graph::get_object(shapes, prop_node, SH_NODE) {
        cs.push(PropConstraint::NodeShape(inner_id));
    }
    if let Some(qvs_id) = graph::get_object(shapes, prop_node, SH_QUALIFIED_VALUE_SHAPE) {
        let min = graph::get_object(shapes, prop_node, SH_QUALIFIED_MIN_COUNT)
            .and_then(|id| graph::elem_to_u64(shapes, id));
        let max = graph::get_object(shapes, prop_node, SH_QUALIFIED_MAX_COUNT)
            .and_then(|id| graph::elem_to_u64(shapes, id));
        cs.push(PropConstraint::QualifiedValueShape {
            shapes_id: qvs_id,
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
    let id = graph::get_object(shapes, shape_id, SH_CLOSED)?;
    if graph::elem_to_bool(shapes, id) != Some(true) {
        return None;
    }
    let mut allowed: Vec<String> = props.iter().map(|p| p.path.clone()).collect();
    if let Some(head) = graph::get_object(shapes, shape_id, SH_IGNORED_PROPERTIES) {
        for id in graph::rdf_list(shapes, head) {
            if let Some(iri) = graph::iri_string(shapes, id) {
                allowed.push(iri);
            }
        }
    }
    Some(allowed)
}

fn shape_list_refs(
    shapes: &Datastore,
    shape_id: GraphElementId,
    pred_iri: &str,
) -> Vec<InnerShapeRef> {
    graph::get_object(shapes, shape_id, pred_iri)
        .map(|head| {
            graph::rdf_list(shapes, head)
                .into_iter()
                .map(|id| InnerShapeRef { shapes_id: id })
                .collect()
        })
        .unwrap_or_default()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a shapes-store `GraphElementId` to an `ElemValue`.
pub fn id_to_elem(shapes: &Datastore, id: GraphElementId) -> ElemValue {
    match shapes.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => ElemValue::Iri(iri.0.clone()),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => ElemValue::BlankNode(*n),
        GraphElement::GraphLiteral(lit) => {
            let (value, datatype, lang) = literal_parts(lit);
            ElemValue::Literal {
                value,
                datatype,
                lang,
            }
        }
        // Triple terms cannot appear as SHACL values; treat as blank node placeholder (#143).
        GraphElement::TripleTerm(k) => ElemValue::BlankNode(k.subject),
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
        RdfLiteral::LangLiteral { lang, literal } => (literal.clone(), None, Some(lang.clone())),
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            (literal.clone(), Some(type_iri.0.clone()), None)
        }
        other => (other.to_string(), None, None),
    }
}

fn parse_node_kind(iri: &str) -> Option<NodeKindValue> {
    match iri {
        SH_IRI => Some(NodeKindValue::IRI),
        SH_LITERAL => Some(NodeKindValue::Literal),
        SH_BLANK_NODE => Some(NodeKindValue::BlankNode),
        SH_BLANK_NODE_OR_IRI => Some(NodeKindValue::BlankNodeOrIRI),
        SH_BLANK_NODE_OR_LITERAL => Some(NodeKindValue::BlankNodeOrLiteral),
        SH_IRI_OR_LITERAL => Some(NodeKindValue::IRIOrLiteral),
        _ => None,
    }
}

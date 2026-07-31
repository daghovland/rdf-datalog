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

impl PropConstraint {
    /// The `sh:sourceConstraintComponent` IRI for this constraint's kind, per
    /// the W3C SHACL spec's constraint-component table:
    /// <https://www.w3.org/TR/shacl/#core-components>. Used to populate
    /// `ValidationResult::source_constraint`. See
    /// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
    pub fn component_iri(&self) -> &'static str {
        use crate::vocab::*;
        match self {
            PropConstraint::MinCount(_) => CC_MIN_COUNT,
            PropConstraint::MaxCount(_) => CC_MAX_COUNT,
            PropConstraint::Class(_) => CC_CLASS,
            PropConstraint::Datatype(_) => CC_DATATYPE,
            PropConstraint::NodeKind(_) => CC_NODE_KIND,
            PropConstraint::HasValue(_) => CC_HAS_VALUE,
            PropConstraint::In(_) => CC_IN,
            PropConstraint::MinLength(_) => CC_MIN_LENGTH,
            PropConstraint::MaxLength(_) => CC_MAX_LENGTH,
            PropConstraint::Pattern(_, _) => CC_PATTERN,
            PropConstraint::LanguageIn(_) => CC_LANGUAGE_IN,
            PropConstraint::UniqueLang => CC_UNIQUE_LANG,
            PropConstraint::Equals(_) => CC_EQUALS,
            PropConstraint::Disjoint(_) => CC_DISJOINT,
            PropConstraint::MinInclusive(_) => CC_MIN_INCLUSIVE,
            PropConstraint::MaxInclusive(_) => CC_MAX_INCLUSIVE,
            PropConstraint::MinExclusive(_) => CC_MIN_EXCLUSIVE,
            PropConstraint::MaxExclusive(_) => CC_MAX_EXCLUSIVE,
            PropConstraint::LessThan(_) => CC_LESS_THAN,
            PropConstraint::LessThanOrEquals(_) => CC_LESS_THAN_OR_EQUALS,
            PropConstraint::NodeShape(_) => CC_NODE,
            // sh:qualifiedMinCount/sh:qualifiedMaxCount are two independent
            // SHACL constraint components sharing one `PropConstraint`
            // variant here. When a property shape declares only one bound,
            // there is no ambiguity. When BOTH are declared (an interval),
            // this static method has no access to the runtime qualifying
            // count needed to say which bound actually failed for a given
            // violation, so it picks min as an arbitrary representative —
            // this is NOT what the real evaluator reports, though: see
            // `evaluate::eval_qualified_value`, which checks each bound
            // independently at evaluation time and reports the correct,
            // specific component per violation (never calling this method
            // for that variant). See #264.
            PropConstraint::QualifiedValueShape { min, max, .. } => {
                if min.is_some() {
                    CC_QUALIFIED_MIN_COUNT
                } else if max.is_some() {
                    CC_QUALIFIED_MAX_COUNT
                } else {
                    CC_QUALIFIED_MIN_COUNT
                }
            }
        }
    }
}

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
        /// `sh:qualifiedValueShapesDisjoint true` — when set, value nodes
        /// that also conform to a *sibling* qualified value shape (another
        /// `sh:property` block on the same parent node shape, sharing this
        /// one's `sh:path`, that also declares `sh:qualifiedValueShape`) are
        /// excluded from this shape's qualifying count. See
        /// [#311](https://github.com/daghovland/rdf-datalog/issues/311) and
        /// <https://www.w3.org/TR/shacl/#QualifiedValueShapeConstraintComponent>.
        disjoint: bool,
    },
}

/// A parsed `sh:property` block.
#[derive(Debug, Clone)]
pub struct ParsedPropShape {
    /// Position within the parent shape (used for unique helper-IRI names).
    pub idx: usize,
    /// ID of this property shape's own node (the object of `sh:property`) in
    /// the **shapes** `Datastore` — an IRI if named, a blank node otherwise
    /// (real SHACL property shapes are commonly named, not always blank
    /// nodes). This is the shape SHACL's `sh:sourceShape` should point to for
    /// a violation produced by this property shape's constraints — NOT the
    /// parent node shape, which previously stood in for it unconditionally
    /// because this field didn't exist. See
    /// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
    pub shapes_id: GraphElementId,
    /// `sh:path` IRI.
    pub path: String,
    pub constraints: Vec<PropConstraint>,
    /// `sh:deactivated true` on this property shape itself (as opposed to the
    /// parent node shape). Per SHACL §3, a deactivated shape produces no
    /// results from any of its constraints. See
    /// [#262](https://github.com/daghovland/rdf-datalog/issues/262).
    pub deactivated: bool,
    /// `sh:not <inner>` declared directly on this property shape (applies to
    /// each path-traversed value, not the focus node — contrast with
    /// `ParsedShape::not_inner`, which applies to the focus node). See
    /// [#311](https://github.com/daghovland/rdf-datalog/issues/311).
    pub not_inner: Option<InnerShapeRef>,
    /// `sh:and (s1 s2 …)` declared directly on this property shape.
    pub and_inners: Vec<InnerShapeRef>,
    /// `sh:or (s1 s2 …)` declared directly on this property shape.
    pub or_inners: Vec<InnerShapeRef>,
    /// `sh:xone (s1 s2 …)` declared directly on this property shape.
    pub xone_inners: Vec<InnerShapeRef>,
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
    /// ID of this shape node in the **shapes** `Datastore` it was parsed from.
    /// Used as the root of the static shape-reference cycle check (see
    /// [`find_shape_reference_cycle`]) — a top-level shape's `shapes_id` is
    /// where evaluation actually enters the "does shape S hold" recursion.
    pub shapes_id: GraphElementId,
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
    /// `sh:message` on this shape, surfaced verbatim on every `ValidationResult`
    /// it produces. See [#264](https://github.com/daghovland/rdf-datalog/issues/264).
    pub message: Option<String>,
    /// `sh:deactivated true` on this shape. Per SHACL §3, a deactivated shape
    /// must produce no validation results at all, from any of its
    /// constraints — every place a shape is processed must check this flag
    /// and skip constraint generation/evaluation entirely when set. See
    /// [#262](https://github.com/daghovland/rdf-datalog/issues/262).
    pub deactivated: bool,
}

/// Return `true` if the shape-graph node `shape_id` carries `sh:deactivated true`.
///
/// A lightweight standalone check (rather than a full `parse_one_shape` call)
/// used wherever only the deactivated flag of a shape reference is needed
/// before deciding whether to process it further (e.g. an inner shape inside
/// `sh:and`). See [#262](https://github.com/daghovland/rdf-datalog/issues/262).
pub(crate) fn is_deactivated(shapes: &Datastore, shape_id: GraphElementId) -> bool {
    graph::get_object(shapes, shape_id, SH_DEACTIVATED)
        .and_then(|id| graph::elem_to_bool(shapes, id))
        .unwrap_or(false)
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

pub(crate) fn parse_one_shape(
    shapes: &Datastore,
    shape_id: GraphElementId,
    idx: usize,
) -> ParsedShape {
    let deactivated = is_deactivated(shapes, shape_id);
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
        let direct_not_inner =
            graph::get_object(shapes, shape_id, SH_NOT).map(|id| InnerShapeRef { shapes_id: id });
        let direct_and_inners = shape_list_refs(shapes, shape_id, SH_AND);
        let direct_or_inners = shape_list_refs(shapes, shape_id, SH_OR);
        let direct_xone_inners = shape_list_refs(shapes, shape_id, SH_XONE);
        if !direct_constraints.is_empty()
            || direct_not_inner.is_some()
            || !direct_and_inners.is_empty()
            || !direct_or_inners.is_empty()
            || !direct_xone_inners.is_empty()
        {
            let next_idx = property_shapes.len();
            property_shapes.push(ParsedPropShape {
                idx: next_idx,
                shapes_id: shape_id,
                path: path_iri,
                constraints: direct_constraints,
                deactivated,
                not_inner: direct_not_inner,
                and_inners: direct_and_inners,
                or_inners: direct_or_inners,
                xone_inners: direct_xone_inners,
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

    let message =
        graph::get_object(shapes, shape_id, SH_MESSAGE).and_then(|id| literal_string(shapes, id));

    ParsedShape {
        idx,
        shapes_id: shape_id,
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
        message,
        deactivated,
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
                shapes_id: prop_node,
                path,
                constraints: parse_prop_constraints(shapes, prop_node),
                deactivated: is_deactivated(shapes, prop_node),
                not_inner: graph::get_object(shapes, prop_node, SH_NOT)
                    .map(|id| InnerShapeRef { shapes_id: id }),
                and_inners: shape_list_refs(shapes, prop_node, SH_AND),
                or_inners: shape_list_refs(shapes, prop_node, SH_OR),
                xone_inners: shape_list_refs(shapes, prop_node, SH_XONE),
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
        let disjoint = graph::get_object(shapes, prop_node, SH_QUALIFIED_VALUE_SHAPES_DISJOINT)
            .and_then(|id| graph::elem_to_bool(shapes, id))
            .unwrap_or(false);
        cs.push(PropConstraint::QualifiedValueShape {
            shapes_id: qvs_id,
            min,
            max,
            disjoint,
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

// ── Static shape-reference cycle check ────────────────────────────────────────
//
// The "shape S references shape T" graph (via sh:not/sh:and/sh:or/sh:xone/
// sh:node/sh:qualifiedValueShape) is fixed once the shapes graph is parsed —
// entirely independent of what data is later validated against it. Rather than
// guarding every recursive per-node conformance check at evaluation time (cost
// proportional to data size), a cycle in this graph is detected exactly once,
// statically, before any data validation begins. See
// [#278](https://github.com/daghovland/rdf-datalog/issues/278).

/// DFS visitation state for [`find_shape_reference_cycle`]'s white/gray/black
/// marking. Nodes with no entry in the map are implicitly white (unvisited).
#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    /// On the current DFS path (gray) — re-entering this node is a cycle.
    InProgress,
    /// Fully explored (black) — already known cycle-free from here.
    Done,
}

/// Every other shape node that `shape_id` references directly, across all the
/// constructs that recurse into `shape_conforms_for_node` at evaluation time
/// (`evaluate.rs`): `sh:not`, `sh:and`, `sh:or`, `sh:xone`, and the
/// `sh:node`/`sh:qualifiedValueShape` property constraints (both on
/// `sh:property` blocks and on pathless node-level constraints).
fn shape_references(shapes_store: &Datastore, shape_id: GraphElementId) -> Vec<GraphElementId> {
    // idx is irrelevant here — only used for synthetic violation-IRI naming
    // elsewhere, never for graph structure.
    let parsed = parse_one_shape(shapes_store, shape_id, 0);
    let mut refs = Vec::new();

    if let Some(inner) = &parsed.not_inner {
        refs.push(inner.shapes_id);
    }
    refs.extend(parsed.and_inners.iter().map(|r| r.shapes_id));
    refs.extend(parsed.or_inners.iter().map(|r| r.shapes_id));
    refs.extend(parsed.xone_inners.iter().map(|r| r.shapes_id));

    let constraint_refs = |cs: &[PropConstraint]| -> Vec<GraphElementId> {
        cs.iter()
            .filter_map(|c| match c {
                PropConstraint::NodeShape(id) => Some(*id),
                PropConstraint::QualifiedValueShape { shapes_id, .. } => Some(*shapes_id),
                _ => None,
            })
            .collect()
    };
    for prop in &parsed.property_shapes {
        refs.extend(constraint_refs(&prop.constraints));
        // sh:not/sh:and/sh:or/sh:xone declared directly inside a sh:property
        // block also reference other shapes (applied to path-traversed
        // values rather than the focus node — see `ParsedPropShape`'s field
        // docs and `evaluate.rs`'s per-property combinator handling), and
        // must be included here for the same reason the node-shape-scoped
        // ones above are: a cycle reachable only through one of these would
        // otherwise blow the stack in `shape_conforms_for_node`, since that
        // function has no runtime cycle guard by design (#278). See #311.
        if let Some(inner) = &prop.not_inner {
            refs.push(inner.shapes_id);
        }
        refs.extend(prop.and_inners.iter().map(|r| r.shapes_id));
        refs.extend(prop.or_inners.iter().map(|r| r.shapes_id));
        refs.extend(prop.xone_inners.iter().map(|r| r.shapes_id));
    }
    refs.extend(constraint_refs(&parsed.node_constraints));

    refs
}

/// Depth-first search from `id`, extending `path` (the current DFS stack) and
/// updating the shared `state` map. Returns the cycle (as a sequence of
/// shapes-store `GraphElementId`s, first element repeated as the last) the
/// first time a node already `InProgress` on the current path is re-entered.
fn dfs_find_cycle(
    shapes_store: &Datastore,
    id: GraphElementId,
    state: &mut std::collections::HashMap<GraphElementId, VisitState>,
    path: &mut Vec<GraphElementId>,
) -> Option<Vec<GraphElementId>> {
    match state.get(&id) {
        Some(VisitState::Done) => return None,
        Some(VisitState::InProgress) => {
            let start = path.iter().position(|&x| x == id).unwrap_or(0);
            let mut cycle = path[start..].to_vec();
            cycle.push(id);
            return Some(cycle);
        }
        None => {}
    }

    state.insert(id, VisitState::InProgress);
    path.push(id);

    for next in shape_references(shapes_store, id) {
        if let Some(cycle) = dfs_find_cycle(shapes_store, next, state, path) {
            return Some(cycle);
        }
    }

    path.pop();
    state.insert(id, VisitState::Done);
    None
}

/// Search the whole shapes graph, once, for a cycle in the shape-reference
/// graph reachable from any top-level parsed shape. Returns the cycle (a
/// sequence of shapes-store `GraphElementId`s) if one exists.
///
/// Roots are exactly `parsed`'s top-level shapes because evaluation only ever
/// *enters* the recursive "does shape S hold for node N" check
/// (`shape_conforms_for_node` in `evaluate.rs`) from one of them — any cycle
/// that could be hit at runtime is therefore reachable from a root here.
pub fn find_shape_reference_cycle(
    shapes_store: &Datastore,
    parsed: &[ParsedShape],
) -> Option<Vec<GraphElementId>> {
    let mut state = std::collections::HashMap::new();
    for shape in parsed {
        let mut path = Vec::new();
        if let Some(cycle) = dfs_find_cycle(shapes_store, shape.shapes_id, &mut state, &mut path) {
            return Some(cycle);
        }
    }
    None
}

/// Render a cycle (as returned by [`find_shape_reference_cycle`]) as a clear
/// diagnostic message naming each shape involved (IRI, or `_:bN` for blank
/// nodes) — used by [`crate::validate`] to reject a provably cyclic shapes
/// graph up front rather than picking an arbitrary runtime answer. SHACL Core
/// leaves recursive shape-reference semantics undefined, so refusing to
/// validate at all against such a shapes graph is more spec-honest than
/// silently choosing a behavior. See
/// [#278](https://github.com/daghovland/rdf-datalog/issues/278).
pub fn describe_shape_cycle(shapes_store: &Datastore, cycle: &[GraphElementId]) -> String {
    let names: Vec<String> = cycle
        .iter()
        .map(|&id| graph::element_display(shapes_store, id))
        .collect();
    format!(
        "shapes graph contains a cycle of shape references, which SHACL Core \
         leaves undefined; refusing to validate: {}",
        names.join(" -> ")
    )
}

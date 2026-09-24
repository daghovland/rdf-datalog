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
    /// SHACL-AF §5 `sh:target [ a sh:SPARQLTarget ; sh:select "..." ]` — the
    /// query's own `?this`/`$this` projection *is* the target-node list (no
    /// external pre-binding, unlike `sh:sparql` constraints). See
    /// [#54](https://github.com/daghovland/rdf-datalog/issues/54).
    Sparql(SparqlQuery),
}

/// A `sh:select`/`sh:ask` query string plus its `sh:prefixes` declarations,
/// shared by `sh:sparql` constraints (§6) and `sh:target`
/// `sh:SPARQLTarget`s (§5).
#[derive(Debug, Clone)]
pub struct SparqlQuery {
    pub query: String,
    pub prefixes: Vec<(String, String)>,
}

/// A SHACL-AF §6.1 `sh:sparql [ a sh:SPARQLConstraint ; ... ]` custom
/// constraint: a SPARQL ASK or SELECT query with `$this` pre-bound to the
/// focus node.
///
/// Spec: <https://www.w3.org/TR/shacl-af/#SPARQLConstraintComponent>. See
/// [#54](https://github.com/daghovland/rdf-datalog/issues/54).
#[derive(Debug, Clone)]
pub struct SparqlConstraint {
    pub query: SparqlQuery,
    pub is_ask: bool,
    pub message: Option<String>,
    /// `sh:severity` declared directly on the `sh:SPARQLConstraint` node,
    /// overriding the parent shape's severity for violations it produces
    /// (mirrors `ParsedPropShape::severity`, #312).
    pub severity: Option<crate::Severity>,
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
    /// Parsed `sh:path` property-path expression. See
    /// [`crate::path::ShPath`] and
    /// [#307](https://github.com/daghovland/rdf-datalog/issues/307).
    pub path: crate::path::ShPath,
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
    /// `sh:severity` declared directly on this property shape, overriding
    /// the parent node shape's severity for violations it produces. `None`
    /// means "not declared here" — the parent shape's severity applies. See
    /// [#312](https://github.com/daghovland/rdf-datalog/issues/312).
    pub severity: Option<crate::Severity>,
    /// `sh:message` declared directly on this property shape, overriding the
    /// parent node shape's message for violations it produces. `None` means
    /// "not declared here" — the parent shape's message applies. See
    /// [#403](https://github.com/daghovland/rdf-datalog/issues/403).
    pub message: Option<String>,
    /// W3C SHACL spec §6 `sh:ConstraintComponent` invocations declared on
    /// this property shape (i.e. the property shape has values for one or
    /// more registered components' parameters). See
    /// [#519](https://github.com/daghovland/rdf-datalog/issues/519).
    pub component_invocations: Vec<ComponentInvocation>,
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
    /// SHACL-AF §6.1 `sh:sparql` custom constraints declared directly on this
    /// shape node. See [#54](https://github.com/daghovland/rdf-datalog/issues/54).
    pub sparql_constraints: Vec<SparqlConstraint>,
    /// W3C SHACL spec §6 `sh:ConstraintComponent` invocations declared
    /// directly on this shape node (node-shape scope — see
    /// [`ParsedPropShape::component_invocations`] for the property-shape
    /// counterpart). Empty when this shape node itself carries `sh:path`
    /// (i.e. it is really a property shape — see `attach_component_invocations`).
    /// See [#519](https://github.com/daghovland/rdf-datalog/issues/519).
    pub node_component_invocations: Vec<ComponentInvocation>,
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
///
/// Per SHACL spec §3.1, a node is a shape whenever it either carries an
/// explicit `rdf:type sh:NodeShape`/`sh:PropertyShape`/`rdfs:Class`
/// declaration, *or* is the subject of one of the target-declaring
/// predicates (`sh:targetNode`/`sh:targetClass`/`sh:targetSubjectsOf`/
/// `sh:targetObjectsOf`) — a shape does not need to be typed explicitly to
/// be recognised, and a target declaration alone is sufficient. Missing this
/// second case meant a shape like `ex:S sh:targetNode ex:x ; sh:nodeKind
/// sh:BlankNode .` (no `rdf:type` triple at all) was silently skipped
/// entirely. See [#312](https://github.com/daghovland/rdf-datalog/issues/312).
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

    for target_pred in [
        SH_TARGET_NODE,
        SH_TARGET_CLASS,
        SH_TARGET_SUBJECTS_OF,
        SH_TARGET_OBJECTS_OF,
    ] {
        if let Some(pred_id) = graph::lookup_iri(shapes, target_pred) {
            for t in shapes.get_triples_with_predicate(pred_id) {
                if !found.contains(&t.subject) {
                    found.push(t.subject);
                }
            }
        }
    }

    let components = parse_constraint_components(shapes);
    found
        .into_iter()
        .enumerate()
        .map(|(idx, shape_id)| {
            let mut parsed = parse_one_shape(shapes, shape_id, idx);
            attach_component_invocations(shapes, &mut parsed, &components);
            parsed
        })
        .collect()
}

/// Attach W3C SHACL spec §6 `sh:ConstraintComponent` invocations to `parsed`
/// (its own node-shape scope, plus each of its property shapes) — a
/// post-processing pass over an already-parsed [`ParsedShape`], rather than
/// threading `components` through `parse_one_shape`/`parse_property_shapes`
/// themselves, so `evaluate::shape_conforms_for_node`'s ad hoc re-parse of an
/// inner shape (`sh:not`/`sh:and`/`sh:or`/`sh:node`/`sh:xone` references) is
/// untouched — custom constraint components are deliberately out of scope
/// there, mirroring the existing scope limit on §5.1 `sh:sparql` constraints
/// (never evaluated for inner shapes either). See
/// [#519](https://github.com/daghovland/rdf-datalog/issues/519).
fn attach_component_invocations(
    shapes: &Datastore,
    parsed: &mut ParsedShape,
    components: &[ConstraintComponentDef],
) {
    // A shape node that itself carries sh:path is really a property shape
    // (already folded into `property_shapes` by `parse_one_shape` — see its
    // `has_direct_path` handling); its own node-level scope contributes no
    // constraints, so no node-shape-scoped invocations either.
    let has_direct_path = graph::get_object(shapes, parsed.shapes_id, SH_PATH).is_some();
    parsed.node_component_invocations = if has_direct_path {
        Vec::new()
    } else {
        find_component_invocations(shapes, parsed.shapes_id, components)
    };
    for prop in &mut parsed.property_shapes {
        prop.component_invocations = find_component_invocations(shapes, prop.shapes_id, components);
    }
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
        && let Some(path) = crate::path::parse_path(shapes, path_id)
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
                path,
                constraints: direct_constraints,
                deactivated,
                not_inner: direct_not_inner,
                and_inners: direct_and_inners,
                or_inners: direct_or_inners,
                xone_inners: direct_xone_inners,
                severity: graph::get_object(shapes, shape_id, SH_SEVERITY)
                    .and_then(|id| graph::iri_string(shapes, id))
                    .and_then(|iri| crate::Severity::from_iri(&iri)),
                message: graph::get_object(shapes, shape_id, SH_MESSAGE)
                    .and_then(|id| literal_string(shapes, id)),
                component_invocations: Vec::new(),
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

    let sparql_constraints = parse_sparql_constraints(shapes, shape_id);

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
        sparql_constraints,
        node_component_invocations: Vec::new(),
    }
}

// ── §5–6 SHACL-AF: SPARQL-based targets/constraints ────────────────────────────

/// Collect `sh:prefixes` declarations for a `sh:SPARQLConstraint`/`sh:SPARQLTarget`
/// node: every `(prefix, namespace)` pair reachable via
/// `node sh:prefixes ?ont . ?ont sh:declare [ sh:prefix "p" ; sh:namespace "ns" ] .`
///
/// `sh:namespace`'s value is typically a `"..."^^xsd:anyURI` typed literal (not a
/// bare IRI node), so this reads it as a literal string first and only falls back
/// to treating it as an IRI resource.
fn parse_sparql_prefixes(shapes: &Datastore, node: GraphElementId) -> Vec<(String, String)> {
    let mut prefixes = Vec::new();
    for ont in graph::get_objects(shapes, node, SH_PREFIXES) {
        for decl in graph::get_objects(shapes, ont, SH_DECLARE) {
            let Some(prefix_id) = graph::get_object(shapes, decl, SH_PREFIX_NAME) else {
                continue;
            };
            let Some(ns_id) = graph::get_object(shapes, decl, SH_NAMESPACE) else {
                continue;
            };
            let Some(prefix) = literal_string(shapes, prefix_id) else {
                continue;
            };
            let namespace =
                literal_string(shapes, ns_id).or_else(|| graph::iri_string(shapes, ns_id));
            if let Some(namespace) = namespace {
                prefixes.push((prefix, namespace));
            }
        }
    }
    prefixes
}

/// Parse every `sh:sparql [ a sh:SPARQLConstraint ; ... ]` declared directly on
/// `shape_id`. A malformed entry (neither `sh:select` nor `sh:ask` present) is
/// skipped — `crate::sparql_constraints`'s evaluator only ever sees well-formed
/// entries.
fn parse_sparql_constraints(shapes: &Datastore, shape_id: GraphElementId) -> Vec<SparqlConstraint> {
    graph::get_objects(shapes, shape_id, SH_SPARQL)
        .into_iter()
        .filter_map(|node| {
            let (query, is_ask) = if let Some(id) = graph::get_object(shapes, node, SH_SELECT) {
                (literal_string(shapes, id)?, false)
            } else if let Some(id) = graph::get_object(shapes, node, SH_ASK) {
                (literal_string(shapes, id)?, true)
            } else {
                return None;
            };
            let message = graph::get_object(shapes, node, SH_MESSAGE)
                .and_then(|id| literal_string(shapes, id));
            let severity = graph::get_object(shapes, node, SH_SEVERITY)
                .and_then(|id| graph::iri_string(shapes, id))
                .and_then(|iri| crate::Severity::from_iri(&iri));
            let prefixes = parse_sparql_prefixes(shapes, node);
            Some(SparqlConstraint {
                query: SparqlQuery { query, prefixes },
                is_ask,
                message,
                severity,
            })
        })
        .collect()
}

// ── §6: SPARQL-based constraint components ─────────────────────────────────
// Spec: <https://www.w3.org/TR/shacl/#constraints-sparql> (§6). See
// [#519](https://github.com/daghovland/rdf-datalog/issues/519) and
// `docs/plans/SHACL_CUSTOM_CONSTRAINT_COMPONENTS_519_PLAN.md`.

/// One `sh:parameter` declaration of a [`ConstraintComponentDef`].
#[derive(Debug, Clone)]
pub struct ComponentParameter {
    /// The parameter's `sh:path` IRI — also the predicate a shape sets
    /// directly to supply this parameter's value when invoking the
    /// component.
    pub path: String,
    /// The SPARQL variable name a validator query pre-binds this
    /// parameter's value to — the local name of `path` (spec §6.2.1).
    pub var_name: String,
    /// `sh:optional true` on this parameter declaration.
    pub optional: bool,
}

/// A single `sh:validator`/`sh:nodeValidator`/`sh:propertyValidator` value —
/// a SPARQL ASK or SELECT query (spec §6.2.3), plus its own `sh:message`
/// template (may contain `{$paramName}`/`{?paramName}` placeholders, spec
/// §6.2.2's templating syntax).
#[derive(Debug, Clone)]
pub struct ValidatorDef {
    pub query: SparqlQuery,
    pub is_ask: bool,
    pub message: Option<String>,
}

/// A parsed `?c a sh:ConstraintComponent` declaration (spec §6.2).
#[derive(Debug, Clone)]
pub struct ConstraintComponentDef {
    /// ID of the component's own node in the **shapes** `Datastore` —
    /// reported as `sh:sourceConstraintComponent` for any violation it
    /// produces.
    pub component_id: GraphElementId,
    pub parameters: Vec<ComponentParameter>,
    /// Generic `sh:validator` — always ASK-based (spec §6.2.3).
    pub validator: Option<ValidatorDef>,
    /// `sh:nodeValidator` — always SELECT-based, used only for node shapes.
    pub node_validator: Option<ValidatorDef>,
    /// `sh:propertyValidator` — always SELECT-based, used only for property
    /// shapes.
    pub property_validator: Option<ValidatorDef>,
}

/// One shape's invocation of a [`ConstraintComponentDef`] — the component's
/// own id (for `sh:sourceConstraintComponent`), the parameter bindings read
/// from the invoking shape, and the (cloned) validator definitions, so
/// evaluation never needs a second lookup by component id.
#[derive(Debug, Clone)]
pub struct ComponentInvocation {
    pub component_id: GraphElementId,
    /// `(parameter var_name, bound value)`, one entry per parameter the
    /// invoking shape actually has a value for (including optional ones
    /// that happen to be set; a missing optional parameter has no entry).
    pub bindings: Vec<(String, GraphElement)>,
    pub validator: Option<ValidatorDef>,
    pub node_validator: Option<ValidatorDef>,
    pub property_validator: Option<ValidatorDef>,
}

/// The local name of an IRI: the longest `NCName`-like suffix after the last
/// `#` or `/` (spec §6.2.1's "longest NCNAME at the end of the IRI, not
/// immediately preceded by the first colon in the IRI" — simplified to the
/// common case of a `#`/`/`-delimited namespace, which covers every
/// parameter path a real shapes graph declares).
fn local_name(iri: &str) -> String {
    iri.rsplit(['#', '/']).next().unwrap_or(iri).to_string()
}

/// Resolve one `sh:validator`/`sh:nodeValidator`/`sh:propertyValidator`
/// value node to a [`ValidatorDef`]. `None` for a malformed entry (neither
/// `sh:select` nor `sh:ask` present) — silently skipped, mirroring
/// `parse_sparql_constraints`'s existing posture on malformed input.
fn parse_validator_def(shapes: &Datastore, node: GraphElementId) -> Option<ValidatorDef> {
    let (query, is_ask) = if let Some(id) = graph::get_object(shapes, node, SH_SELECT) {
        (literal_string(shapes, id)?, false)
    } else if let Some(id) = graph::get_object(shapes, node, SH_ASK) {
        (literal_string(shapes, id)?, true)
    } else {
        return None;
    };
    let message =
        graph::get_object(shapes, node, SH_MESSAGE).and_then(|id| literal_string(shapes, id));
    let prefixes = parse_sparql_prefixes(shapes, node);
    Some(ValidatorDef {
        query: SparqlQuery { query, prefixes },
        is_ask,
        message,
    })
}

/// Parse every `?c a sh:ConstraintComponent` declaration anywhere in
/// `shapes`. A component with no parameters, or with no validator of any
/// kind, is ill-formed per spec (§6.2.1: "every constraint component has at
/// least one non-optional parameter") and is skipped entirely — mirroring
/// this crate's existing posture of silently skipping malformed shapes-graph
/// input rather than erroring.
fn parse_constraint_components(shapes: &Datastore) -> Vec<ConstraintComponentDef> {
    let Some(rdf_type_id) = graph::lookup_iri(shapes, RDF_TYPE) else {
        return Vec::new();
    };
    let Some(cc_id) = graph::lookup_iri(shapes, SH_CONSTRAINT_COMPONENT) else {
        return Vec::new();
    };
    shapes
        .get_triples_with_object_predicate(cc_id, rdf_type_id)
        .map(|t| t.subject)
        .filter_map(|component_id| {
            let parameters: Vec<ComponentParameter> =
                graph::get_objects(shapes, component_id, SH_PARAMETER)
                    .into_iter()
                    .filter_map(|p| {
                        let path_id = graph::get_object(shapes, p, SH_PATH)?;
                        let path = graph::iri_string(shapes, path_id)?;
                        let optional = graph::get_object(shapes, p, SH_OPTIONAL)
                            .and_then(|id| graph::elem_to_bool(shapes, id))
                            .unwrap_or(false);
                        let var_name = local_name(&path);
                        Some(ComponentParameter {
                            path,
                            var_name,
                            optional,
                        })
                    })
                    .collect();
            if parameters.is_empty() {
                return None;
            }
            let validator = graph::get_object(shapes, component_id, SH_VALIDATOR)
                .and_then(|v| parse_validator_def(shapes, v));
            let node_validator = graph::get_object(shapes, component_id, SH_NODE_VALIDATOR)
                .and_then(|v| parse_validator_def(shapes, v));
            let property_validator = graph::get_object(shapes, component_id, SH_PROPERTY_VALIDATOR)
                .and_then(|v| parse_validator_def(shapes, v));
            if validator.is_none() && node_validator.is_none() && property_validator.is_none() {
                return None;
            }
            Some(ConstraintComponentDef {
                component_id,
                parameters,
                validator,
                node_validator,
                property_validator,
            })
        })
        .collect()
}

/// Which components `shape_id` invokes: for every [`ConstraintComponentDef`]
/// whose non-optional parameters all have a value on `shape_id`, one
/// [`ComponentInvocation`] with that shape's bound parameter values (spec
/// §6.1's informative "uses a constraint component" algorithm). A parameter
/// with more than one value on `shape_id` uses only the first found — see
/// the plan doc's "Multi-valued parameters" non-goal.
fn find_component_invocations(
    shapes: &Datastore,
    shape_id: GraphElementId,
    components: &[ConstraintComponentDef],
) -> Vec<ComponentInvocation> {
    components
        .iter()
        .filter_map(|comp| {
            let mut bindings = Vec::new();
            for param in &comp.parameters {
                match graph::get_object(shapes, shape_id, &param.path) {
                    Some(val_id) => {
                        let elem = shapes.resources.get_graph_element(val_id).clone();
                        bindings.push((param.var_name.clone(), elem));
                    }
                    None if param.optional => {}
                    None => return None,
                }
            }
            Some(ComponentInvocation {
                component_id: comp.component_id,
                bindings,
                validator: comp.validator.clone(),
                node_validator: comp.node_validator.clone(),
                property_validator: comp.property_validator.clone(),
            })
        })
        .collect()
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

    // SHACL-AF §5 sh:target [ a sh:SPARQLTarget ; sh:select "..." ]. A
    // sh:target node without a sh:select is skipped (not a SPARQLTarget this
    // implementation recognises — see #54).
    for id in graph::get_objects(shapes, shape_id, SH_TARGET) {
        if let Some(sel_id) = graph::get_object(shapes, id, SH_SELECT)
            && let Some(query) = literal_string(shapes, sel_id)
        {
            let prefixes = parse_sparql_prefixes(shapes, id);
            targets.push(Target::Sparql(SparqlQuery { query, prefixes }));
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
            let path = crate::path::parse_path(shapes, path_id)?;
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
                severity: graph::get_object(shapes, prop_node, SH_SEVERITY)
                    .and_then(|id| graph::iri_string(shapes, id))
                    .and_then(|iri| crate::Severity::from_iri(&iri)),
                message: graph::get_object(shapes, prop_node, SH_MESSAGE)
                    .and_then(|id| literal_string(shapes, id)),
                component_invocations: Vec::new(),
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
    for id in graph::get_objects(shapes, prop_node, SH_CLASS) {
        if let Some(iri) = graph::iri_string(shapes, id) {
            cs.push(PropConstraint::Class(iri));
        }
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
    // Only simple-predicate property shapes contribute to sh:closed's
    // allowed set — a property shape whose path is a compound expression
    // isn't naming one predicate to allow. See #307.
    let mut allowed: Vec<String> = props
        .iter()
        .filter_map(|p| p.path.as_simple_iri().map(str::to_string))
        .collect();
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

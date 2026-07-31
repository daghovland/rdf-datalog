/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SHACL (Shapes Constraint Language) validation.
//!
//! Spec: <https://www.w3.org/TR/shacl/>
//!
//! # Architecture
//!
//! SHACL Core constraints are translated to stratified Datalog rules (same engine as
//! OWL-RL), then materialised over a **clone** of the data graph.  Violation triples
//! derived by the engine are collected into a `ValidationReport`.
//!
//! `sh:closed` is evaluated separately against the original data graph before Datalog
//! materialisation to avoid synthetic helper predicates being mistaken for real data.
//!
//! See `docs/plans/SHACL_PLAN.md` for the phased implementation roadmap.

pub mod evaluate;
pub mod graph;
pub mod shapes;
pub mod translate;
pub mod vocab;

use dag_rdf::ingress::{DEFAULT_GRAPH_ELEMENT_ID, Triple};
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfResource};
use datalog::evaluate_rules;
use ingress::RDF_TYPE;
use std::collections::HashSet;

// ── Public types ──────────────────────────────────────────────────────────────

/// Severity of a SHACL validation result (`sh:resultSeverity`).
///
/// Spec: <https://www.w3.org/TR/shacl/#severity>. A shape's own `sh:severity`
/// determines the severity of every result it produces; the default when
/// unset is `sh:Violation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Severity {
    #[default]
    Violation,
    Warning,
    Info,
}

impl Severity {
    /// Parse a `sh:severity` object IRI (`sh:Violation`/`sh:Warning`/`sh:Info`).
    /// Returns `None` for any other IRI.
    pub fn from_iri(iri: &str) -> Option<Self> {
        match iri {
            vocab::SH_VIOLATION => Some(Severity::Violation),
            vocab::SH_WARNING => Some(Severity::Warning),
            vocab::SH_INFO => Some(Severity::Info),
            _ => None,
        }
    }

    /// The `sh:` term name (`"sh:Violation"`, …) used when serialising a report.
    pub fn turtle_term(self) -> &'static str {
        match self {
            Severity::Violation => "sh:Violation",
            Severity::Warning => "sh:Warning",
            Severity::Info => "sh:Info",
        }
    }
}

/// Per-violation-predicate metadata, threaded alongside a `Severity` from
/// rule/constraint-generation time (`translate.rs`/`evaluate.rs`) through to
/// `collect_violations`, mirroring the way `Severity` itself is threaded
/// through `shapes_to_rules`/`eval_all`/`pre_compute_violations`. One
/// `ViolMeta` describes every violation triple sharing a given synthetic
/// violation predicate — which is always exactly one shape/path/constraint
/// combination (see `vocab::viol_*`). See
/// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
#[derive(Debug, Clone)]
pub struct ViolMeta {
    pub severity: Severity,
    /// Display form of the actual shape node that produced this violation
    /// (`sh:sourceShape`) — an IRI, or a blank-node label (`_:bN`) if the
    /// shape is anonymous. For a property-shape-scoped violation this is the
    /// **property shape's own node** (the object of `sh:property`, which is
    /// commonly a named IRI in real-world SHACL, not necessarily a blank
    /// node), never the enclosing node shape — see
    /// `shapes::ParsedPropShape::shapes_id`. Always resolvable (every shape
    /// node has a display form, IRI or blank node), so unlike the field this
    /// replaced, never `None`. See
    /// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
    pub source_shape: String,
    /// `sh:path` of the property shape that produced this violation, or
    /// `None` for a node-level (pathless) constraint.
    pub path: Option<String>,
    /// `sh:sourceConstraintComponent` IRI for the constraint that produced
    /// this violation.
    pub component: &'static str,
    /// `sh:message` declared on the producing shape, if any.
    pub message: Option<String>,
}

impl ViolMeta {
    /// `source_shape_id` is the shapes-graph node that actually declares the
    /// constraint responsible for this violation: the property shape's own
    /// node for a property-shape-scoped violation, or `shape.shapes_id`
    /// itself for a node-level constraint. `shape` still supplies
    /// `severity`/`message`, which are shape-level (node-shape) concerns in
    /// this crate today, not (yet) overridable per property shape.
    fn new(
        shapes_store: &Datastore,
        shape: &shapes::ParsedShape,
        source_shape_id: GraphElementId,
        path: Option<&str>,
        component: &'static str,
    ) -> Self {
        ViolMeta {
            severity: shape.severity,
            source_shape: graph::element_display(shapes_store, source_shape_id),
            path: path.map(str::to_string),
            component,
            message: shape.message.clone(),
        }
    }
}

/// A single validation result entry (`sh:ValidationResult`).
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub focus_node: Option<String>,
    pub severity: Severity,
    pub message: Option<String>,
    pub result_path: Option<String>,
    pub source_shape: String,
    pub source_constraint: Option<String>,
    pub value: Option<String>,
}

/// The outcome of validating a data graph against a shapes graph (`sh:ValidationReport`).
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub conforms: bool,
    pub results: Vec<ValidationResult>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Validate `data` against the SHACL shapes in `shapes`.
///
/// The data graph is cloned; the caller's `data` is not mutated.
pub fn validate(data: &Datastore, shapes: &Datastore) -> Result<ValidationReport, String> {
    let parsed = shapes::parse_shapes(shapes);

    // Static cycle check over the shapes graph alone (sh:not/sh:and/sh:or/sh:xone/
    // sh:node/sh:qualifiedValueShape references), done once here rather than
    // guarded per-node at evaluation time — the cycle is a property of the
    // shapes graph, independent of what data gets validated against it. See
    // https://github.com/daghovland/rdf-datalog/issues/278.
    if let Some(cycle) = shapes::find_shape_reference_cycle(shapes, &parsed) {
        return Err(shapes::describe_shape_cycle(shapes, &cycle));
    }

    let mut work = data.clone();

    // Pre-compute violations for constraints that must see only the original data triples
    // (before any Datalog materialisation adds synthetic helper predicates).
    let mut all_viol_preds = pre_compute_violations(&parsed, data, shapes, &mut work);

    // Translate remaining constraints to Datalog rules and materialise.
    let (rules, rule_viols) = translate::shapes_to_rules(&parsed, shapes, &mut work);
    // A SHACL constraint should never compile to a Datalog Contradiction rule
    // (SHACL violations are represented as synthetic marker predicates, not
    // via RuleHead::Contradiction), so this should not fail in practice. See
    // https://github.com/daghovland/rdf-datalog/issues/301.
    evaluate_rules(rules, &mut work)
        .map_err(|e| format!("unexpected contradiction while validating SHACL shapes: {e}"))?;
    all_viol_preds.extend(rule_viols);

    let results = collect_violations(&work, &all_viol_preds);
    Ok(ValidationReport {
        conforms: results.is_empty(),
        results,
    })
}

/// Serialize a `ValidationReport` as a Turtle string (SHACL report graph).
///
/// Spec: <https://www.w3.org/TR/shacl/#validation-report>
pub fn report_to_turtle(report: &ValidationReport) -> String {
    let mut out = String::new();
    out.push_str("@prefix sh: <http://www.w3.org/ns/shacl#> .\n");
    out.push_str("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\n");
    out.push_str("[] a sh:ValidationReport ;\n");
    if report.conforms {
        out.push_str("   sh:conforms true .\n");
    } else {
        out.push_str("   sh:conforms false");
        for result in &report.results {
            out.push_str(" ;\n   sh:result [\n");
            out.push_str("       a sh:ValidationResult ;\n");
            out.push_str("       sh:resultSeverity ");
            out.push_str(result.severity.turtle_term());
            if let Some(focus) = &result.focus_node {
                out.push_str(" ;\n       sh:focusNode ");
                out.push_str(&turtle_term(focus));
            }
            if let Some(path) = &result.result_path {
                out.push_str(" ;\n       sh:resultPath ");
                out.push_str(&turtle_term(path));
            }
            if let Some(val) = &result.value {
                out.push_str(" ;\n       sh:value ");
                out.push_str(&turtle_term(val));
            }
            out.push_str(" ;\n       sh:sourceShape ");
            out.push_str(&turtle_term(&result.source_shape));
            if let Some(component) = &result.source_constraint {
                out.push_str(" ;\n       sh:sourceConstraintComponent ");
                out.push_str(&turtle_term(component));
            }
            if let Some(msg) = &result.message {
                out.push_str(" ;\n       sh:resultMessage ");
                out.push('"');
                out.push_str(&msg.replace('"', "\\\""));
                out.push('"');
            }
            out.push_str("\n   ]");
        }
        out.push_str(" .\n");
    }
    out
}

/// Format a value as a Turtle term: IRI `<…>`, blank node `_:…` (as-is, no
/// quoting — needed since `sh:sourceShape` can now be a blank-node display
/// string, see `graph::element_display`), or string literal `"…"`.
fn turtle_term(s: &str) -> String {
    if s.starts_with('<')
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("urn:")
    {
        let iri = s.trim_start_matches('<').trim_end_matches('>');
        format!("<{iri}>")
    } else if s.starts_with("_:") {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('"', "\\\""))
    }
}

// ── Pre-compute violations ────────────────────────────────────────────────────

/// Evaluate constraints that need the original (un-materialised) data graph.
///
/// Handles `sh:closed` and all Phase 2 value-testing constraints (datatype,
/// nodeKind, range, string, property pair, sh:node, sh:qualifiedValueShape, sh:xone).
/// Returns the violation-predicate IDs added.
fn pre_compute_violations(
    parsed: &[shapes::ParsedShape],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<(GraphElementId, ViolMeta)> {
    let mut viol_preds = Vec::new();
    for shape in parsed {
        // sh:deactivated — a deactivated shape produces no results at all,
        // including from sh:closed. See #262.
        if shape.deactivated {
            continue;
        }
        if let Some(allowed_iris) = &shape.closed {
            let pred = closed_violations(shape, allowed_iris, data, work);
            viol_preds.push((
                pred,
                ViolMeta::new(shapes_store, shape, shape.shapes_id, None, vocab::CC_CLOSED),
            ));
        }
    }
    let phase2_viols = evaluate::eval_all(parsed, data, shapes_store, work);
    viol_preds.extend(phase2_viols);
    viol_preds
}

/// Compute `sh:closed` violations directly from the data graph.
///
/// Each `(focusNode, forbiddenPredicate)` pair that occurs in the data becomes
/// one violation triple.  Because we query `data` (before any Datalog derivation),
/// synthetic helper predicates added to `work` are never seen.
fn closed_violations(
    shape: &shapes::ParsedShape,
    allowed_iris: &[String],
    data: &Datastore,
    work: &mut Datastore,
) -> GraphElementId {
    // IDs of allowed predicates in the DATA store.
    let allowed: HashSet<GraphElementId> = allowed_iris
        .iter()
        .filter_map(|iri| graph::lookup_iri(data, iri))
        .collect();

    let viol_pred = graph::intern_iri(work, &vocab::viol_closed(shape.idx));

    for node_id in data_targets(shape, data) {
        for triple in data.get_triples_with_subject(node_id) {
            if !allowed.contains(&triple.predicate) {
                // node_id and triple.predicate are valid IDs in `work` because
                // `work` is a clone of `data` (same resource list, same IDs).
                work.add_triple(Triple {
                    subject: node_id,
                    predicate: viol_pred,
                    obj: triple.predicate,
                });
            }
        }
    }
    viol_pred
}

// ── Target computation from original data ─────────────────────────────────────

/// Compute the focus nodes for `shape` directly from the `data` store.
fn data_targets(shape: &shapes::ParsedShape, data: &Datastore) -> Vec<GraphElementId> {
    let rdf_type_id = graph::lookup_iri(data, RDF_TYPE);
    let mut nodes: Vec<GraphElementId> = Vec::new();

    for target in &shape.targets {
        match target {
            shapes::Target::Node(elem) => {
                if let Some(id) = lookup_elem(elem, data) {
                    push_unique(&mut nodes, id);
                }
            }
            shapes::Target::Class(class_iri) | shapes::Target::ImplicitClass(class_iri) => {
                if let (Some(rdf_type_id), Some(class_id)) =
                    (rdf_type_id, graph::lookup_iri(data, class_iri))
                {
                    for t in data.get_triples_with_object_predicate(class_id, rdf_type_id) {
                        push_unique(&mut nodes, t.subject);
                    }
                }
            }
            shapes::Target::SubjectsOf(pred_iri) => {
                if let Some(pred_id) = graph::lookup_iri(data, pred_iri) {
                    for t in data.get_triples_with_predicate(pred_id) {
                        push_unique(&mut nodes, t.subject);
                    }
                }
            }
            shapes::Target::ObjectsOf(pred_iri) => {
                if let Some(pred_id) = graph::lookup_iri(data, pred_iri) {
                    for t in data.get_triples_with_predicate(pred_id) {
                        push_unique(&mut nodes, t.obj);
                    }
                }
            }
        }
    }
    nodes
}

fn lookup_elem(elem: &shapes::ElemValue, data: &Datastore) -> Option<GraphElementId> {
    match elem {
        shapes::ElemValue::Iri(iri) => graph::lookup_iri(data, iri),
        shapes::ElemValue::BlankNode(n) => data
            .resources
            .resource_map
            .get(&GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(
                *n,
            )))
            .copied(),
        shapes::ElemValue::Literal { .. } => None,
    }
}

fn push_unique(vec: &mut Vec<GraphElementId>, id: GraphElementId) {
    if !vec.contains(&id) {
        vec.push(id);
    }
}

// ── Violation collection ──────────────────────────────────────────────────────

fn collect_violations(
    work: &Datastore,
    viol_preds: &[(GraphElementId, ViolMeta)],
) -> Vec<ValidationResult> {
    let pred_meta: std::collections::HashMap<GraphElementId, &ViolMeta> =
        viol_preds.iter().map(|(id, meta)| (*id, meta)).collect();
    // Only examine default-graph triples (triple_id = 0).
    work.named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .filter(|q| pred_meta.contains_key(&q.predicate))
        .map(|q| {
            let focus = graph::element_display(work, q.subject);
            let val = {
                let s = graph::element_display(work, q.obj);
                if s == vocab::INT_NIL || s == vocab::INT_TRUE {
                    None
                } else {
                    Some(s)
                }
            };
            let meta = pred_meta.get(&q.predicate).copied();
            ValidationResult {
                focus_node: Some(focus),
                severity: meta.map(|m| m.severity).unwrap_or_default(),
                message: meta.and_then(|m| m.message.clone()),
                result_path: meta.and_then(|m| m.path.clone()),
                source_shape: meta.map(|m| m.source_shape.clone()).unwrap_or_default(),
                source_constraint: meta.map(|m| m.component.to_string()),
                value: val,
            }
        })
        .collect()
}

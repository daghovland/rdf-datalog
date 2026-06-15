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
//! See `SHACL_PLAN.md` for the phased implementation roadmap.

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Violation,
    Warning,
    Info,
}

/// A single validation result entry (`sh:ValidationResult`).
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub focus_node: Option<String>,
    pub severity: Severity,
    pub message: Option<String>,
    pub result_path: Option<String>,
    pub source_shape: Option<String>,
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
    let mut work = data.clone();

    // Pre-compute violations for constraints that must see only the original data triples
    // (before any Datalog materialisation adds synthetic helper predicates).
    let mut all_viol_preds = pre_compute_violations(&parsed, data, shapes, &mut work);

    // Translate remaining constraints to Datalog rules and materialise.
    let (rules, rule_viols) = translate::shapes_to_rules(&parsed, shapes, &mut work);
    evaluate_rules(rules, &mut work);
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
            out.push_str("       sh:resultSeverity sh:Violation");
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

/// Format a value as a Turtle term: IRI `<…>` or string literal `"…"`.
fn turtle_term(s: &str) -> String {
    if s.starts_with('<')
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("urn:")
    {
        let iri = s.trim_start_matches('<').trim_end_matches('>');
        format!("<{iri}>")
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
) -> Vec<GraphElementId> {
    let mut viol_preds = Vec::new();
    for shape in parsed {
        if let Some(allowed_iris) = &shape.closed {
            let pred = closed_violations(shape, allowed_iris, data, work);
            viol_preds.push(pred);
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

fn collect_violations(work: &Datastore, viol_preds: &[GraphElementId]) -> Vec<ValidationResult> {
    let pred_set: HashSet<GraphElementId> = viol_preds.iter().copied().collect();
    // Only examine default-graph triples (triple_id = 0).
    work.named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .filter(|q| pred_set.contains(&q.predicate))
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
            ValidationResult {
                focus_node: Some(focus),
                severity: Severity::Violation,
                message: None,
                result_path: None,
                source_shape: None,
                source_constraint: None,
                value: val,
            }
        })
        .collect()
}

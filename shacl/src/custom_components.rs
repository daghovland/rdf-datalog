/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! W3C SHACL spec §6 — SPARQL-based constraint components
//! (`sh:ConstraintComponent`/`sh:parameter`/`sh:validator`).
//!
//! Spec: <https://www.w3.org/TR/shacl/#constraints-sparql>
//!
//! See `docs/plans/SHACL_CUSTOM_CONSTRAINT_COMPONENTS_519_PLAN.md` for the
//! full design rationale and scope. In short: this is the reusable,
//! declaratively-parameterised counterpart of `sparql_constraints.rs`'s
//! single embedded `sh:sparql` mechanism (spec §5) — a shape "invokes" a
//! registered component by setting its parameters directly as shape
//! properties (`shapes::find_component_invocations`), and this module
//! executes the component's selected validator query (spec §6.2.3/§6.3)
//! against the un-materialised `data` graph, reusing `sparql_constraints`'s
//! SPARQL-execution plumbing (`parse`/`run_select`/`run_ask`/
//! `normalize_dollar_vars`/`ge_display`/`row_value_and_path`, all
//! `pub(crate)`) rather than duplicating it.

use crate::shapes::{ComponentInvocation, ParsedPropShape, ParsedShape, SparqlQuery, ValidatorDef};
use crate::{ValidationResult, graph, path, sparql_constraints};
use dag_rdf::{Datastore, GraphElement, GraphElementId};
use regex::Regex;
use sparql_parser::ast::{Query, QueryComponent};
use std::sync::LazyLock;

/// Matches the literal `$PATH`/`?PATH` token (spec §6.2.3.1) so it can be
/// textually replaced with a `sh:propertyValidator`'s invoking property
/// shape's actual path, in SPARQL property-path surface syntax — a one-time
/// macro expansion, not a pre-bound variable.
static PATH_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[$?]PATH\b").unwrap());

/// Matches a `{$name}`/`{?name}` message-template placeholder (spec
/// §6.2.2's templating syntax, reused verbatim by validator `sh:message`).
static MESSAGE_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[$?]([A-Za-z_][A-Za-z0-9_]*)\}").unwrap());

/// Build the full query text for `query`, with `$PATH` replaced by
/// `path_sub`'s SPARQL surface syntax when present (a `sh:propertyValidator`
/// query), then the usual `PREFIX` header and `$name` -> `?name`
/// normalization `sparql_constraints::build_query_text` also does (kept
/// separate here since this crate's private `build_query_text` isn't
/// `pub(crate)` and this one additionally needs the `$PATH` substitution
/// step *before* normalization turns `$PATH` into `?PATH`).
fn build_validator_query_text(query: &SparqlQuery, path_sub: Option<&path::ShPath>) -> String {
    let mut text = String::new();
    for (prefix, namespace) in &query.prefixes {
        text.push_str(&format!("PREFIX {prefix}: <{namespace}>\n"));
    }
    let body = if let Some(p) = path_sub {
        let syntax = path::to_sparql_path(p);
        PATH_TOKEN
            .replace_all(&query.query, |_: &regex::Captures| syntax.clone())
            .into_owned()
    } else {
        query.query.clone()
    };
    text.push_str(&sparql_constraints::normalize_dollar_vars(&body));
    text
}

/// Prepend a single-row `VALUES (?var1 ?var2 …) { (v1 v2 …) }` block binding
/// every one of `bindings` at once — the multi-variable generalization of
/// `sparql_constraints::inject_this_value` (which only ever binds `$this`).
fn inject_prebound(mut query: Query, bindings: &[(String, GraphElement)]) -> Query {
    let names: Vec<String> = bindings.iter().map(|(n, _)| n.clone()).collect();
    let row: Vec<Option<GraphElement>> = bindings.iter().map(|(_, e)| Some(e.clone())).collect();
    let values = QueryComponent::Values(names, vec![row]);
    match &mut query {
        Query::Select { where_clause, .. } | Query::Ask { where_clause, .. } => {
            where_clause.insert(0, values);
        }
        Query::Construct { .. } | Query::Describe { .. } => {}
    }
    query
}

/// Render a validator's `sh:message` template, substituting every
/// `{$name}`/`{?name}` placeholder with the display form of `name`'s bound
/// value in `bindings` (spec §6.2.2). A placeholder naming an unbound
/// variable is left as-is (defensive; every parameter the component
/// declares is either bound or the invocation wouldn't exist at all — see
/// `shapes::find_component_invocations`).
fn render_message(template: &str, bindings: &[(String, GraphElement)]) -> String {
    MESSAGE_PLACEHOLDER
        .replace_all(template, |caps: &regex::Captures| {
            let name = &caps[1];
            bindings
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, e)| sparql_constraints::ge_display(e))
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

// ── Pre-flight parse-check ──────────────────────────────────────────────────

/// Parse-check every constraint-component validator query actually reachable
/// by `parsed`'s invocations, up front — mirrors #54's pre-flight check for
/// `sh:sparql`/`sh:target` (a malformed query is caught unconditionally,
/// rather than only when the invoking shape happens to have non-empty
/// focus/value nodes at evaluation time). Only the validator that will
/// actually be *selected* per spec §6.2.3 is checked (mirroring
/// `eval_node_scope`/`eval_property_scope`'s own selection order exactly) —
/// an unused sibling validator (e.g. a `sh:nodeValidator` on a component only
/// ever invoked via property shapes) is not required to parse.
pub(crate) fn check_all_syntax(parsed: &[ParsedShape]) -> Result<(), String> {
    for shape in parsed {
        if shape.deactivated {
            continue;
        }
        for inv in &shape.node_component_invocations {
            check_node_invocation_syntax(inv)?;
        }
        for prop in &shape.property_shapes {
            if prop.deactivated {
                continue;
            }
            for inv in &prop.component_invocations {
                check_property_invocation_syntax(inv, &prop.path)?;
            }
        }
    }
    Ok(())
}

fn check_node_invocation_syntax(inv: &ComponentInvocation) -> Result<(), String> {
    if let Some(v) = inv.node_validator.as_ref().or(inv.validator.as_ref()) {
        sparql_constraints::parse(&build_validator_query_text(&v.query, None))?;
    }
    Ok(())
}

fn check_property_invocation_syntax(
    inv: &ComponentInvocation,
    prop_path: &path::ShPath,
) -> Result<(), String> {
    if let Some(v) = &inv.property_validator {
        sparql_constraints::parse(&build_validator_query_text(&v.query, Some(prop_path)))?;
    } else if let Some(v) = &inv.validator {
        sparql_constraints::parse(&build_validator_query_text(&v.query, None))?;
    }
    Ok(())
}

// ── Evaluation ───────────────────────────────────────────────────────────────

/// Evaluate every `sh:ConstraintComponent` invocation on every non-deactivated
/// shape in `parsed` (both node-shape-scoped and property-shape-scoped),
/// against `data` (the original, un-materialised data graph — same posture
/// as `sparql_constraints::eval_all`), returning one `ValidationResult` per
/// failing solution/ASK value node.
///
/// `focus_nodes_of` supplies each shape's focus nodes (shared with the rest
/// of `crate` via `crate::data_targets`).
pub fn eval_all(
    parsed: &[ParsedShape],
    shapes_store: &Datastore,
    data: &Datastore,
    focus_nodes_of: impl Fn(&ParsedShape) -> Result<Vec<GraphElementId>, String>,
    cache: &path::PathCache,
) -> Result<Vec<ValidationResult>, String> {
    let mut results = Vec::new();
    for shape in parsed {
        if shape.deactivated {
            continue;
        }
        let has_any = !shape.node_component_invocations.is_empty()
            || shape
                .property_shapes
                .iter()
                .any(|p| !p.component_invocations.is_empty());
        if !has_any {
            continue;
        }
        let focus_nodes = focus_nodes_of(shape)?;
        if focus_nodes.is_empty() {
            continue;
        }

        for inv in &shape.node_component_invocations {
            eval_node_scope(shape, inv, &focus_nodes, shapes_store, data, &mut results)?;
        }
        for prop in &shape.property_shapes {
            if prop.deactivated {
                continue;
            }
            for inv in &prop.component_invocations {
                eval_property_scope(
                    shape,
                    prop,
                    inv,
                    &focus_nodes,
                    shapes_store,
                    data,
                    cache,
                    &mut results,
                )?;
            }
        }
    }
    Ok(results)
}

/// Node-shape-scoped invocation: `sh:nodeValidator` (SELECT, once per focus
/// node, `$this` pre-bound) if present, else the generic `sh:validator`
/// (ASK, once per focus node with `$value` pre-bound to the focus node
/// itself — a node shape's only "value node" is the focus node, SHACL §3.7).
/// No suitable validator: the invocation is silently ignored (spec §6.2.3).
fn eval_node_scope(
    shape: &ParsedShape,
    inv: &ComponentInvocation,
    focus_nodes: &[GraphElementId],
    shapes_store: &Datastore,
    data: &Datastore,
    results: &mut Vec<ValidationResult>,
) -> Result<(), String> {
    let source_shape = graph::element_display(shapes_store, shape.shapes_id);
    let source_constraint = Some(graph::element_display(shapes_store, inv.component_id));

    if let Some(validator) = &inv.node_validator {
        let base_query =
            sparql_constraints::parse(&build_validator_query_text(&validator.query, None))?;
        for &node_id in focus_nodes {
            let this_elem = data.resources.get_graph_element(node_id).clone();
            let mut bindings = inv.bindings.clone();
            bindings.push(("this".to_string(), this_elem));
            let query = inject_prebound(base_query.clone(), &bindings);
            for row in sparql_constraints::run_select(&query, data)? {
                let (value, result_path) = sparql_constraints::row_value_and_path(&row);
                let message = row
                    .get("message")
                    .map(sparql_constraints::ge_display)
                    .or_else(|| shape.message.clone())
                    .or_else(|| validator_message(validator, &inv.bindings));
                results.push(ValidationResult {
                    focus_node: Some(graph::element_display(data, node_id)),
                    severity: shape.severity.clone(),
                    message,
                    result_path,
                    source_shape: source_shape.clone(),
                    source_constraint: source_constraint.clone(),
                    value,
                });
            }
        }
    } else if let Some(validator) = &inv.validator {
        let base_query =
            sparql_constraints::parse(&build_validator_query_text(&validator.query, None))?;
        for &node_id in focus_nodes {
            let this_elem = data.resources.get_graph_element(node_id).clone();
            let mut bindings = inv.bindings.clone();
            bindings.push(("this".to_string(), this_elem.clone()));
            bindings.push(("value".to_string(), this_elem));
            let query = inject_prebound(base_query.clone(), &bindings);
            if !sparql_constraints::run_ask(&query, data)? {
                let message = shape
                    .message
                    .clone()
                    .or_else(|| validator_message(validator, &inv.bindings));
                results.push(ValidationResult {
                    focus_node: Some(graph::element_display(data, node_id)),
                    severity: shape.severity.clone(),
                    message,
                    result_path: None,
                    source_shape: source_shape.clone(),
                    source_constraint: source_constraint.clone(),
                    value: Some(graph::element_display(data, node_id)),
                });
            }
        }
    }
    Ok(())
}

/// Property-shape-scoped invocation: `sh:propertyValidator` (SELECT, once
/// per focus node with `$PATH` substituted and `$this` pre-bound) if
/// present, else the generic `sh:validator` (ASK, once per path-traversed
/// value node, `$this`/`$value` pre-bound). No suitable validator: ignored.
#[allow(clippy::too_many_arguments)]
fn eval_property_scope(
    shape: &ParsedShape,
    prop: &ParsedPropShape,
    inv: &ComponentInvocation,
    focus_nodes: &[GraphElementId],
    shapes_store: &Datastore,
    data: &Datastore,
    cache: &path::PathCache,
    results: &mut Vec<ValidationResult>,
) -> Result<(), String> {
    let message_base = prop.message.clone().or_else(|| shape.message.clone());
    let severity = prop
        .severity
        .clone()
        .unwrap_or_else(|| shape.severity.clone());
    let source_shape = graph::element_display(shapes_store, prop.shapes_id);
    let source_constraint = Some(graph::element_display(shapes_store, inv.component_id));

    if let Some(validator) = &inv.property_validator {
        let base_query = sparql_constraints::parse(&build_validator_query_text(
            &validator.query,
            Some(&prop.path),
        ))?;
        for &node_id in focus_nodes {
            let this_elem = data.resources.get_graph_element(node_id).clone();
            let mut bindings = inv.bindings.clone();
            bindings.push(("this".to_string(), this_elem));
            let query = inject_prebound(base_query.clone(), &bindings);
            for row in sparql_constraints::run_select(&query, data)? {
                let (value, result_path) = sparql_constraints::row_value_and_path(&row);
                let message = row
                    .get("message")
                    .map(sparql_constraints::ge_display)
                    .or_else(|| message_base.clone())
                    .or_else(|| validator_message(validator, &inv.bindings));
                results.push(ValidationResult {
                    focus_node: Some(graph::element_display(data, node_id)),
                    severity: severity.clone(),
                    message,
                    result_path: result_path.or_else(|| Some(prop.path.clone())),
                    source_shape: source_shape.clone(),
                    source_constraint: source_constraint.clone(),
                    value,
                });
            }
        }
    } else if let Some(validator) = &inv.validator {
        let base_query =
            sparql_constraints::parse(&build_validator_query_text(&validator.query, None))?;
        for &node_id in focus_nodes {
            let this_elem = data.resources.get_graph_element(node_id).clone();
            for value_id in path::values_from(data, node_id, &prop.path, cache) {
                let value_elem = data.resources.get_graph_element(value_id).clone();
                let mut bindings = inv.bindings.clone();
                bindings.push(("this".to_string(), this_elem.clone()));
                bindings.push(("value".to_string(), value_elem));
                let query = inject_prebound(base_query.clone(), &bindings);
                if !sparql_constraints::run_ask(&query, data)? {
                    let message = message_base
                        .clone()
                        .or_else(|| validator_message(validator, &inv.bindings));
                    results.push(ValidationResult {
                        focus_node: Some(graph::element_display(data, node_id)),
                        severity: severity.clone(),
                        message,
                        result_path: Some(prop.path.clone()),
                        source_shape: source_shape.clone(),
                        source_constraint: source_constraint.clone(),
                        value: Some(graph::element_display(data, value_id)),
                    });
                }
            }
        }
    }
    Ok(())
}

fn validator_message(
    validator: &ValidatorDef,
    bindings: &[(String, GraphElement)],
) -> Option<String> {
    validator
        .message
        .as_ref()
        .map(|t| render_message(t, bindings))
}

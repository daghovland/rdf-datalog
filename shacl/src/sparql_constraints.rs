/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SHACL-AF §5–6 — SPARQL-based targets and constraints.
//!
//! Spec: <https://www.w3.org/TR/shacl-af/#sparql-based-constraints>
//!
//! Unlike SHACL Core (translated to stratified Datalog, `translate.rs`), a
//! `sh:sparql`/`sh:target` query can express joins, negation, property paths and
//! aggregates with no general Datalog equivalent, so it is executed directly by
//! `sparql_parser` against the (un-materialised) data graph. See
//! `docs/plans/SHACL_PLAN.md`'s "SHACL-SPARQL (§5–6 of SHACL-AF)" section for the
//! design rationale (in particular why `$this` pre-binding is per-focus-node rather
//! than batched, and why a malformed/failing embedded query is a hard `Err` rather
//! than a silently-skipped constraint).

use crate::graph;
use crate::path::ShPath;
use crate::ValidationResult;
use crate::shapes::{ParsedShape, SparqlConstraint, SparqlQuery};
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfResource};
use ingress::NetworkPolicy;
use regex::Regex;
use sparql_parser::ast::{Query, QueryComponent};
use sparql_parser::execute::{QueryResult, SolutionRow, execute_with_base};
use sparql_parser::{ParserContext, parse_query};
use std::collections::HashMap;
use std::sync::LazyLock;

/// Matches a `$name` SPARQL variable reference (SPARQL 1.1's `VAR2` production,
/// `$` + `VARNAME`) so it can be rewritten to the `?name` form `sparql_parser`
/// actually implements (`VAR1`) — see the module-level doc comment and
/// `docs/plans/SHACL_PLAN.md`'s "`$this`/`$value`/`$path` use the `?` sigil
/// internally" section for why this textual rewrite exists instead of extending
/// the shared parser's variable grammar.
static DOLLAR_VAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap());

/// Rewrite every `$name` in `query` to `?name`.
fn normalize_dollar_vars(query: &str) -> String {
    DOLLAR_VAR.replace_all(query, "?$1").into_owned()
}

/// Build the full query text for `sq`: its `sh:prefixes` PREFIX declarations,
/// then the (dollar-normalized) query body.
fn build_query_text(sq: &SparqlQuery) -> String {
    let mut text = String::new();
    for (prefix, namespace) in &sq.prefixes {
        text.push_str(&format!("PREFIX {prefix}: <{namespace}>\n"));
    }
    text.push_str(&normalize_dollar_vars(&sq.query));
    text
}

/// Parse `query_text` into a `Query` AST, returning a caller-facing error string
/// on failure (never silently dropped — see module docs).
fn parse(query_text: &str) -> Result<Query, String> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    parse_query(query_text, &mut ctx)
        .map(|(_, q)| q)
        .map_err(|e| format!("SPARQL parse error in SHACL-AF embedded query: {e:?}"))
}

/// Prepend a single-row `VALUES ($this) { (<elem>) }` block to `query`'s
/// `where_clause`, pre-binding `$this` to the focus node per SHACL-AF §6.
fn inject_this_value(mut query: Query, elem: GraphElement) -> Query {
    let values = QueryComponent::Values(vec!["this".to_string()], vec![vec![Some(elem)]]);
    match &mut query {
        Query::Select { where_clause, .. } | Query::Ask { where_clause, .. } => {
            where_clause.insert(0, values);
        }
        Query::Construct { .. } | Query::Describe { .. } => {}
    }
    query
}

fn run_select(query: &Query, store: &Datastore) -> Result<Vec<SolutionRow>, String> {
    match execute_with_base(query, store, NetworkPolicy::Deny, None, None)? {
        QueryResult::Select(sel) => Ok(sel.rows),
        _ => Err("expected the embedded query to be a SPARQL SELECT".to_string()),
    }
}

fn run_ask(query: &Query, store: &Datastore) -> Result<bool, String> {
    match execute_with_base(query, store, NetworkPolicy::Deny, None, None)? {
        QueryResult::Ask(b) => Ok(b),
        _ => Err("expected the embedded query to be a SPARQL ASK".to_string()),
    }
}

/// Display a raw `GraphElement` (not necessarily interned in any particular
/// store — e.g. a `SolutionRow` binding) the same way `graph::element_display`
/// renders an interned element.
fn ge_display(e: &GraphElement) -> String {
    match e {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => iri.0.clone(),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => format!("_:b{n}"),
        GraphElement::GraphLiteral(lit) => turtle::format_literal(lit),
        GraphElement::TripleTerm(k) => format!("<<( {} {} {} )>>", k.subject, k.predicate, k.obj),
    }
}

/// Run every `sh:target [ a sh:SPARQLTarget ; sh:select "..." ]` query on
/// `sq` against `store`, returning the bound `?this`/`$this` values as raw
/// `GraphElement`s (caller interns them into whichever store it's building
/// facts against). No `$this` pre-binding: the query's own `this` projection
/// *is* the target-node list, per SHACL-AF §5.
pub(crate) fn eval_sparql_target(sq: &SparqlQuery, store: &Datastore) -> Result<Vec<GraphElement>, String> {
    let query = parse(&build_query_text(sq))?;
    let rows = run_select(&query, store)?;
    Ok(rows
        .into_iter()
        .filter_map(|mut row| row.remove("this"))
        .collect())
}

/// Evaluate every `sh:sparql` constraint on every non-deactivated shape in
/// `parsed`, against `data` (the original, un-materialised data graph — see
/// module docs), returning one `ValidationResult` per failing solution/ASK.
///
/// `focus_nodes_of` supplies each shape's focus nodes (shared with the rest of
/// `crate` via `crate::data_targets`, so SPARQL and Core targets agree).
pub fn eval_all(
    parsed: &[ParsedShape],
    shapes_store: &Datastore,
    data: &Datastore,
    focus_nodes_of: impl Fn(&ParsedShape) -> Vec<GraphElementId>,
) -> Result<Vec<ValidationResult>, String> {
    let mut results = Vec::new();
    for shape in parsed {
        if shape.deactivated || shape.sparql_constraints.is_empty() {
            continue;
        }
        let focus_nodes = focus_nodes_of(shape);
        if focus_nodes.is_empty() {
            continue;
        }
        for constraint in &shape.sparql_constraints {
            let mut r = eval_one_constraint(shape, constraint, &focus_nodes, shapes_store, data)?;
            results.append(&mut r);
        }
    }
    Ok(results)
}

fn eval_one_constraint(
    shape: &ParsedShape,
    constraint: &SparqlConstraint,
    focus_nodes: &[GraphElementId],
    shapes_store: &Datastore,
    data: &Datastore,
) -> Result<Vec<ValidationResult>, String> {
    let query_text = build_query_text(&constraint.query);
    let mut results = Vec::new();

    for &node_id in focus_nodes {
        let this_elem = data.resources.get_graph_element(node_id).clone();
        let base_query = parse(&query_text)?;
        let query = inject_this_value(base_query, this_elem);

        if constraint.is_ask {
            if !run_ask(&query, data)? {
                results.push(make_result(shape, constraint, shapes_store, data, node_id, None, None));
            }
        } else {
            for row in run_select(&query, data)? {
                let value = row.get("value").map(ge_display);
                let path = row.get("path").and_then(|e| match e {
                    GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
                        Some(ShPath::Predicate(iri.0.clone()))
                    }
                    _ => None,
                });
                results.push(make_result(
                    shape,
                    constraint,
                    shapes_store,
                    data,
                    node_id,
                    value,
                    path,
                ));
            }
        }
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
fn make_result(
    shape: &ParsedShape,
    constraint: &SparqlConstraint,
    shapes_store: &Datastore,
    data: &Datastore,
    focus_node_id: GraphElementId,
    value: Option<String>,
    path: Option<ShPath>,
) -> ValidationResult {
    ValidationResult {
        focus_node: Some(graph::element_display(data, focus_node_id)),
        severity: constraint
            .severity
            .clone()
            .unwrap_or_else(|| shape.severity.clone()),
        message: constraint.message.clone().or_else(|| shape.message.clone()),
        result_path: path,
        source_shape: graph::element_display(shapes_store, shape.shapes_id),
        source_constraint: Some(crate::vocab::CC_SPARQL.to_string()),
        value,
    }
}

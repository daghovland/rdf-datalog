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

use crate::ValidationResult;
use crate::graph;
use crate::path::ShPath;
use crate::shapes::{ParsedShape, SparqlConstraint, SparqlQuery};
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfResource};
use ingress::NetworkPolicy;
use regex::Regex;
use sparql_parser::ast::{Expression, GroupCondition, ProjectionElement, Query, QueryComponent};
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

/// Prepend a multi-row `VALUES ($this) { (<elem1>) (<elem2>) ... }` block to
/// `query`'s `where_clause`, pre-binding `$this` to every one of `elems` in a
/// single execution — the batched-fast-path counterpart of
/// `inject_this_value`. Only meaningful for `Query::Select` (callers only use
/// this when [`is_batchable`] has already confirmed the query is a `Select`).
fn inject_this_values(mut query: Query, elems: &[GraphElement]) -> Query {
    let rows = elems.iter().map(|e| vec![Some(e.clone())]).collect();
    let values = QueryComponent::Values(vec!["this".to_string()], rows);
    if let Query::Select { where_clause, .. } = &mut query {
        where_clause.insert(0, values);
    }
    query
}

/// True if `expr` contains an aggregate function call anywhere within it
/// (possibly nested inside arithmetic/function-call subexpressions) — SPARQL
/// 1.1 §18.2.4.3's `AggregateExpr`, reachable from a projection expression,
/// `HAVING`, or `ORDER BY`.
fn expr_has_aggregate(expr: &Expression) -> bool {
    match expr {
        Expression::Aggregate(_) => true,
        Expression::Binary(l, _, r) => expr_has_aggregate(l) || expr_has_aggregate(r),
        Expression::Unary(_, inner) => expr_has_aggregate(inner),
        Expression::FunctionCall(_, args) => args.iter().any(expr_has_aggregate),
        Expression::In(inner, list) | Expression::NotIn(inner, list) => {
            expr_has_aggregate(inner) || list.iter().any(expr_has_aggregate)
        }
        Expression::Variable(_) | Expression::Constant(_) => false,
        // EXISTS/NOT EXISTS wrap a graph pattern, not an aggregate-bearing
        // SELECT clause — a standalone aggregate cannot appear directly
        // inside one (it would need its own SELECT), so nothing to recurse
        // into here.
        Expression::Exists(_) | Expression::NotExists(_) => false,
    }
}

/// True iff `group_by` includes `$this` (the internal `?this` form) as one of
/// its grouping keys — see [`is_batchable`]'s point on `GROUP BY`.
fn grouped_by_this(group_by: &[GroupCondition]) -> bool {
    group_by
        .iter()
        .any(|gc| matches!(&gc.expr, Expression::Variable(v) if v == "this"))
}

/// True iff `query`'s top-level clause is safe to evaluate once for every
/// focus node in a single batched `VALUES ($this) { ... }` execution, rather
/// than once per focus node. See
/// `docs/plans/SHACL_SPARQL_BATCHING_521_PLAN.md`'s "Detecting the safe case"
/// section for the full reasoning; summary:
///
/// - `LIMIT`/`OFFSET` apply once to the whole batched result set rather than
///   once per focus node, so either makes batching unsafe.
/// - An aggregate (in the projection, `HAVING`, or `ORDER BY`) with no
///   `GROUP BY` implicitly aggregates the *entire* result set into one group
///   (SPARQL 1.1 §11.4) — batched, that would mix every focus node's rows
///   together, so this is unsafe.
/// - A non-empty `GROUP BY` that does **not** include `$this` can merge rows
///   from different focus nodes into the same group (even with no aggregate
///   function involved, e.g. a `GROUP BY` used only to deduplicate), so this
///   is unsafe too.
/// - A `GROUP BY` that does include `$this` scopes every group to exactly one
///   focus node's own rows, so it's safe (this is the case the issue calls
///   out as *safe* to batch, unlike the other aggregate case).
/// - `Query::Ask` (and any other non-`Select` variant) is always unsafe here:
///   an `ASK` result has no `$this` binding to split rows by, so batching
///   doesn't apply to `sh:ask` constraints (only `sh:select`, per the issue).
fn is_batchable(query: &Query) -> bool {
    let Query::Select {
        projection,
        having,
        order_by,
        group_by,
        limit,
        offset,
        ..
    } = query
    else {
        return false;
    };
    if limit.is_some() || offset.is_some() {
        return false;
    }
    if !group_by.is_empty() {
        return grouped_by_this(group_by);
    }
    let has_aggregate = projection.iter().any(|p| match p {
        ProjectionElement::Expression(expr, _) => expr_has_aggregate(expr),
        ProjectionElement::Variable(_) | ProjectionElement::Star => false,
    }) || having.iter().any(expr_has_aggregate)
        || order_by.iter().any(|oc| expr_has_aggregate(&oc.expression));
    !has_aggregate
}

/// Ensure `$this` is present in `query`'s projection, appending a bare
/// `?this` column if it's not already covered by `SELECT *` or an existing
/// `this`-named column. The batched path needs `$this` in every output row
/// to attribute it back to the right focus node — see
/// `docs/plans/SHACL_SPARQL_BATCHING_521_PLAN.md`'s "Batched execution
/// design" section for why this is safe to do unconditionally, including
/// under `DISTINCT` and `GROUP BY`.
fn ensure_this_projected(query: &mut Query) {
    if let Query::Select { projection, .. } = query {
        let has_this = projection.iter().any(|p| match p {
            ProjectionElement::Star => true,
            ProjectionElement::Variable(v) => v == "this",
            ProjectionElement::Expression(_, alias) => alias == "this",
        });
        if !has_this {
            projection.push(ProjectionElement::Variable("this".to_string()));
        }
    }
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

/// Parse-check `sq`'s query text without executing it, surfacing a malformed
/// `sh:target`/`sh:sparql` query as a loud `Err` up front (called from
/// `validate()` before any data-dependent work) rather than only at the point
/// a shape happens to have focus nodes — see `data_targets`'s `Target::Sparql`
/// arm and `translate::target_rules`'s `Target::Sparql` arm, both of which
/// warn-and-skip an *execution*-time failure rather than hard-failing (since
/// neither has a `Result` return threaded through this far). This pre-flight
/// check at least catches a typo'd/unsupported query unconditionally, rather
/// than only when the target happens to be non-empty. See
/// [#54](https://github.com/daghovland/rdf-datalog/issues/54).
pub(crate) fn check_query_syntax(sq: &SparqlQuery) -> Result<(), String> {
    parse(&build_query_text(sq)).map(|_| ())
}

/// Run every `sh:target [ a sh:SPARQLTarget ; sh:select "..." ]` query on
/// `sq` against `store`, returning the bound `?this`/`$this` values as raw
/// `GraphElement`s (caller interns them into whichever store it's building
/// facts against). No `$this` pre-binding: the query's own `this` projection
/// *is* the target-node list, per SHACL-AF §5.
pub(crate) fn eval_sparql_target(
    sq: &SparqlQuery,
    store: &Datastore,
) -> Result<Vec<GraphElement>, String> {
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
    let base_query = parse(&query_text)?;

    if !constraint.is_ask && is_batchable(&base_query) {
        return eval_batched_select(
            shape,
            constraint,
            focus_nodes,
            shapes_store,
            data,
            base_query,
        );
    }

    let mut results = Vec::new();
    for &node_id in focus_nodes {
        let this_elem = data.resources.get_graph_element(node_id).clone();
        let query = inject_this_value(base_query.clone(), this_elem);

        if constraint.is_ask {
            if !run_ask(&query, data)? {
                results.push(make_result(
                    shape,
                    constraint,
                    shapes_store,
                    data,
                    node_id,
                    None,
                    None,
                ));
            }
        } else {
            for row in run_select(&query, data)? {
                let (value, path) = row_value_and_path(&row);
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

/// The batched fast path for a `sh:sparql sh:select` constraint whose query
/// has already been confirmed [`is_batchable`]: execute it once with all of
/// `focus_nodes` bound via a multi-row `VALUES`, then re-split the resulting
/// rows by their `$this` binding back into one `ValidationResult` per
/// matching (focus node, row) pair — see
/// `docs/plans/SHACL_SPARQL_BATCHING_521_PLAN.md`'s "Batched execution
/// design" section.
fn eval_batched_select(
    shape: &ParsedShape,
    constraint: &SparqlConstraint,
    focus_nodes: &[GraphElementId],
    shapes_store: &Datastore,
    data: &Datastore,
    mut query: Query,
) -> Result<Vec<ValidationResult>, String> {
    ensure_this_projected(&mut query);

    let mut node_by_elem: HashMap<GraphElement, GraphElementId> =
        HashMap::with_capacity(focus_nodes.len());
    let mut elems: Vec<GraphElement> = Vec::with_capacity(focus_nodes.len());
    for &node_id in focus_nodes {
        let elem = data.resources.get_graph_element(node_id).clone();
        node_by_elem.insert(elem.clone(), node_id);
        elems.push(elem);
    }

    let query = inject_this_values(query, &elems);
    let mut results = Vec::new();
    for row in run_select(&query, data)? {
        let Some(this_val) = row.get("this") else {
            // The query's own body never bound `$this` in this row (e.g. the
            // WHERE clause doesn't reference it at all) — shouldn't happen,
            // since `ensure_this_projected` always adds it to the
            // projection and the injected VALUES always binds it, but skip
            // defensively rather than panic on an unattributable row.
            continue;
        };
        let Some(&node_id) = node_by_elem.get(this_val) else {
            continue;
        };
        let (value, path) = row_value_and_path(&row);
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
    Ok(results)
}

/// Extract the `$value`/`$path` columns a SELECT constraint's result row
/// conventionally carries (SHACL-AF §6.1) — shared by the per-node and
/// batched execution paths.
fn row_value_and_path(row: &SolutionRow) -> (Option<String>, Option<ShPath>) {
    let value = row.get("value").map(ge_display);
    let path = row.get("path").and_then(|e| match e {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => Some(ShPath::Predicate(iri.0.clone())),
        _ => None,
    });
    (value, path)
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

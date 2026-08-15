/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SPARQL query execution against a [`Datastore`].
//!
//! Supports BGP, FILTER (comparison/regex/BOUND), OPTIONAL, UNION, MINUS,
//! DISTINCT, LIMIT, OFFSET, GROUP BY, HAVING, aggregates (COUNT, SUM, AVG,
//! MIN, MAX, SAMPLE, GROUP_CONCAT).
//!
//! Split into submodules by evaluation concern (see issue #465):
//! - [`solutions`] — [`PartialSub`]/[`SolutionRow`] projection & join helpers
//! - [`components`] — top-level query component evaluation (BGP/OPTIONAL/UNION/MINUS/GRAPH)
//! - [`bgp`] — basic graph pattern / triple pattern matching
//! - [`expressions`] — FILTER/BIND expression evaluation, arithmetic
//! - [`functions`] — built-in SPARQL function dispatch (`eval_function_value`/`eval_function_bool`)
//! - [`casts`] — `xsd:*` cast functions
//! - [`paths`] — property path evaluation
//! - [`aggregates`] — GROUP BY / aggregate evaluation

mod aggregates;
mod bgp;
mod casts;
mod components;
mod expressions;
mod functions;
mod paths;
mod solutions;

use crate::ast::{
    Aggregate, BinaryOp, DatasetClause, Expression, GroupCondition, OrderCondition,
    ProjectionElement, PropertyPath, Query, QueryComponent, Term, TriplePattern, UnaryOp,
};
use crate::deadline::Deadline;
use aggregates::{
    elem_has_aggregate, eval_expr_in_group, eval_having_expr, group_by_solutions,
    project_aggregate_row,
};
#[cfg(test)]
use components::pattern_repeats_variable;
use components::{eval_components, eval_components_budgeted};
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, DEFAULT_GRAPH_ELEMENT_ID};
use expressions::{bind_template_term, eval_expression_value_inner};
use functions::{graph_element_to_string, literal_to_f64};
use ingress::{
    IriReference, NetworkPolicy, XSD_BOOLEAN, XSD_DATE, XSD_DATE_TIME, XSD_DECIMAL, XSD_DOUBLE,
    XSD_FLOAT, XSD_INTEGER, XSD_STRING,
};
use num_bigint::BigInt;
use paths::resolve_term_to_gel;
use solutions::{
    project_with_exprs, project_with_exprs_partial, psv_eq, select_solution_budget,
    solution_row_to_partial,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

// Re-exported so `crate::execute::{eval_expr_as_filter, eval_expression_bool_filter,
// eval_expression_value}` (this module's public API, see `lib.rs`) keeps resolving
// unchanged even though the implementations now live in `expressions.rs`.
pub use expressions::{eval_expr_as_filter, eval_expression_bool_filter, eval_expression_value};

/// A single bound solution: variable name → concrete graph element.
pub type SolutionRow = HashMap<String, GraphElement>;

thread_local! {
    /// The query's effective base IRI (`BASE <...>` directive or caller-
    /// supplied default), used by `IRI()`/`URI()` (SPARQL 1.1 §17.4.2.6) to
    /// resolve a *runtime* string argument at evaluation time — as opposed to
    /// `ParserContext::base` (see #217), which only resolves IRIs written
    /// directly in query syntax at parse time. Threaded via a thread-local
    /// rather than an extra parameter on all ~50 evaluator functions
    /// (`eval_expression_value_inner`/`eval_function_value` and everything
    /// that calls them transitively) — see #346.
    static CURRENT_BASE: RefCell<Option<String>> = const { RefCell::new(None) };

    /// Per-solution memoization cache for `BNODE(str)` (SPARQL 1.1 §17.4.2.7):
    /// repeated calls with the *same* simple-literal argument string within a
    /// single query solution must return the *same* blank node; different
    /// solutions get different ones even with the same argument. Cleared (via
    /// `BnodeMemoGuard`) at each solution-row boundary — see
    /// `project_with_exprs_partial`/`eval_bind_expr`.
    static BNODE_MEMO: RefCell<HashMap<String, GraphElement>> = RefCell::new(HashMap::new());
}

/// RAII guard installing `base` as the thread-local effective query base for
/// the lifetime of the guard, restoring whatever was previously installed
/// when dropped.
///
/// Save-and-restore rather than a bare set/clear: `EXISTS`/`NOT EXISTS` and
/// subquery evaluation can recursively re-enter expression evaluation on the
/// same thread while an outer [`execute_with_base`] call's guard is still
/// alive, so a bare clear-on-drop would wipe the outer call's base out from
/// under it.
struct BaseGuard {
    previous: Option<String>,
}

impl BaseGuard {
    fn install(base: Option<&str>) -> Self {
        let previous = CURRENT_BASE.with(|c| c.replace(base.map(|s| s.to_string())));
        BaseGuard { previous }
    }
}

impl Drop for BaseGuard {
    fn drop(&mut self) {
        CURRENT_BASE.with(|c| *c.borrow_mut() = self.previous.take());
    }
}

fn current_base() -> Option<String> {
    CURRENT_BASE.with(|c| c.borrow().clone())
}

/// RAII guard that installs a fresh, empty `BNODE(str)` memoization map for
/// the lifetime of the guard (one solution row's worth of evaluation),
/// restoring the previous map when dropped.
///
/// Save-and-restore (via `mem::take`) rather than a bare `clear()`, for the
/// same re-entrancy reason as [`BaseGuard`]: a projection/BIND expression
/// that contains an `EXISTS`/subquery can recursively evaluate another
/// solution row's projection while the outer row's guard is still alive.
struct BnodeMemoGuard {
    previous: HashMap<String, GraphElement>,
}

impl BnodeMemoGuard {
    fn install() -> Self {
        let previous = BNODE_MEMO.with(|c| std::mem::take(&mut *c.borrow_mut()));
        BnodeMemoGuard { previous }
    }
}

impl Drop for BnodeMemoGuard {
    fn drop(&mut self) {
        BNODE_MEMO.with(|c| *c.borrow_mut() = std::mem::take(&mut self.previous));
    }
}

/// The result of executing a SPARQL SELECT query.
pub struct SelectResult {
    /// Variable names in projection order.
    pub variables: Vec<String>,
    /// Each row maps projected variable names to their bound value.
    pub rows: Vec<SolutionRow>,
}

/// A single resolved (ground) triple from a CONSTRUCT result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedTriple {
    pub subject: GraphElement,
    pub predicate: GraphElement,
    pub object: GraphElement,
}

/// The result of executing a SPARQL query.
pub enum QueryResult {
    Select(SelectResult),
    Ask(bool),
    Construct(Vec<ResolvedTriple>),
    /// Graph of triples describing the requested resources (DESCRIBE).
    Describe(Vec<ResolvedTriple>),
}

/// Execute a parsed SPARQL query against `datastore`.
///
/// Equivalent to [`execute_with_base`] with no effective base IRI — `IRI()`/
/// `URI()` calls on a relative-IRI string argument are left unresolved
/// (verbatim), matching `ParserContext`'s no-base convention (#217).
///
/// `network` controls how `SERVICE` federation clauses are handled:
/// - [`NetworkPolicy::Deny`] — non-SILENT SERVICE returns an error (default, safe).
/// - [`NetworkPolicy::Ignore`] — all SERVICE clauses return empty results silently.
/// - [`NetworkPolicy::Allow`] — not yet implemented; returns a "not yet implemented" error.
pub fn execute(
    query: &Query,
    datastore: &Datastore,
    network: NetworkPolicy,
) -> Result<QueryResult, String> {
    execute_with_base(query, datastore, network, None, None)
}

/// Execute a parsed SPARQL query against `datastore`, with `base` installed
/// as the query's effective base IRI for evaluation-time `IRI()`/`URI()`
/// resolution (SPARQL 1.1 §17.4.2.6).
///
/// Callers that parsed with [`crate::ParserContext`] should pass
/// `ctx.base.as_deref()` here so that a `BASE <...>` directive (or a
/// caller-supplied default base) is honored not just for IRIs written
/// directly in query syntax (parse-time, `ParserContext::base`, #217) but
/// also for a string computed at runtime and passed through `IRI()`/`URI()`
/// (evaluation-time, this function). See #346.
///
/// `timeout`, when `Some`, bounds the wall-clock time this call may spend
/// evaluating the query: the evaluator periodically checks an absolute
/// deadline derived from it at loop-iteration boundaries and returns `Err`
/// once it has elapsed (cooperative cancellation — see
/// [`crate::deadline`] for why this approach was chosen over an async
/// `tokio::time::timeout`). `None` (the common case, and what every
/// pre-existing caller passes) means no timeout is enforced. See
/// <https://github.com/daghovland/rdf-datalog/issues/372>.
pub fn execute_with_base(
    query: &Query,
    datastore: &Datastore,
    network: NetworkPolicy,
    base: Option<&str>,
    timeout: Option<Duration>,
) -> Result<QueryResult, String> {
    let _base_guard = BaseGuard::install(base);
    let deadline = Deadline::from_timeout(timeout);
    execute_inner(query, datastore, network, &deadline)
}

fn execute_inner(
    query: &Query,
    datastore: &Datastore,
    network: NetworkPolicy,
    deadline: &Deadline,
) -> Result<QueryResult, String> {
    let where_clause = match query {
        Query::Select { where_clause, .. } => where_clause.as_slice(),
        Query::Ask { where_clause, .. } => where_clause.as_slice(),
        Query::Construct { where_clause, .. } => where_clause.as_slice(),
        Query::Describe { where_clause, .. } => where_clause.as_slice(),
    };

    // Apply network policy to SERVICE clauses.
    match network {
        NetworkPolicy::Deny => {
            if let Some(endpoint) = first_non_silent_service(where_clause) {
                return Err(format!(
                    "SERVICE <{endpoint:?}> was rejected: remote network access is disabled. \
                     Start the server with --network=allow to enable federated queries. \
                     See https://github.com/daghovland/rdf-datalog/issues/51"
                ));
            }
            // SILENT SERVICE still returns empty — the SPARQL spec mandates this.
        }
        NetworkPolicy::Ignore => {
            // All SERVICE calls return empty results (handled in the QueryComponent::Service
            // match arm below).
        }
        NetworkPolicy::Allow | NetworkPolicy::AllowList(_) => {
            if first_non_silent_service(where_clause).is_some() {
                return Err(
                    "SERVICE federation is not yet implemented even with --network=allow. \
                     Track progress at https://github.com/daghovland/rdf-datalog/issues/51"
                        .to_string(),
                );
            }
        }
    }

    match query {
        Query::Select {
            projection,
            where_clause,
            limit,
            offset,
            distinct,
            group_by,
            having,
            dataset,
            order_by,
        } => {
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let budget =
                select_solution_budget(*distinct, order_by, group_by, projection, *offset, *limit);
            let solutions = eval_components_budgeted(
                where_clause,
                initial,
                datastore,
                dataset_active_graph(dataset, datastore),
                budget,
                deadline,
            )?;

            let aggregate_mode = !group_by.is_empty() || projection.iter().any(elem_has_aggregate);

            let (variables, mut rows) = if aggregate_mode {
                let groups = group_by_solutions(&solutions, group_by, datastore);
                let vars = projection_variables(projection, where_clause, datastore);
                let rows: Vec<SolutionRow> = groups
                    .into_iter()
                    .filter(|g| {
                        having
                            .iter()
                            .all(|expr| eval_having_expr(expr, g, datastore))
                    })
                    .map(|g| project_aggregate_row(projection, &g, datastore))
                    .collect();
                (vars, rows)
            } else {
                let variables = projection_variables(projection, where_clause, datastore);
                let rows: Vec<SolutionRow> = solutions
                    .iter()
                    .map(|sub| project_with_exprs(sub, projection, datastore))
                    .collect();
                (variables, rows)
            };

            // ORDER BY
            //
            // `rows` here holds already-resolved `SolutionRow`s (post
            // projection, so `(expr AS ?alias)` bindings are available as
            // sort keys), while `sort_solutions`/`eval_expression_value_inner`
            // operate on `PartialSub`. Bridge the two representations via
            // `solution_row_to_partial` and resolve back afterwards. See
            // `execute_select_inner` for the equivalent subquery path.
            if !order_by.is_empty() {
                let mut partial_rows: Vec<PartialSub> =
                    rows.iter().map(solution_row_to_partial).collect();
                sort_solutions(&mut partial_rows, order_by, datastore);
                rows = partial_rows
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|(k, v)| (k, v.resolve(datastore)))
                            .collect()
                    })
                    .collect();
            }

            if *distinct {
                let mut seen: std::collections::HashSet<Vec<(String, GraphElement)>> =
                    std::collections::HashSet::new();
                rows.retain(|row| {
                    let mut key: Vec<(String, GraphElement)> =
                        row.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    key.sort_by(|a, b| a.0.cmp(&b.0));
                    seen.insert(key)
                });
            }

            if let Some(off) = offset {
                let off = *off as usize;
                if off < rows.len() {
                    rows = rows[off..].to_vec();
                } else {
                    rows.clear();
                }
            }
            if let Some(lim) = limit {
                rows.truncate(*lim as usize);
            }

            Ok(QueryResult::Select(SelectResult { variables, rows }))
        }
        Query::Ask {
            where_clause,
            dataset,
        } => {
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let solutions = eval_components(
                where_clause,
                initial,
                datastore,
                dataset_active_graph(dataset, datastore),
                deadline,
            )?;
            Ok(QueryResult::Ask(!solutions.is_empty()))
        }
        Query::Describe {
            resources,
            where_clause,
            dataset,
        } => {
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let solutions = eval_components(
                where_clause,
                initial,
                datastore,
                dataset_active_graph(dataset, datastore),
                deadline,
            )?;

            let mut output: HashSet<ResolvedTriple> = HashSet::new();

            for sub in &solutions {
                let candidates: Vec<GraphElement> = if resources.is_empty() {
                    // DESCRIBE *: describe all variables bound in this solution
                    sub.values().map(|v| v.resolve(datastore)).collect()
                } else {
                    resources
                        .iter()
                        .filter_map(|t| resolve_term_to_gel(t, sub, datastore))
                        .collect()
                };

                for gel in candidates {
                    if let Some(&subject_id) = datastore.resources.resource_map.get(&gel) {
                        for quad in datastore.named_graphs.get_quads_with_subject(subject_id) {
                            let s = datastore.resources.get_graph_element(quad.subject).clone();
                            let p = datastore
                                .resources
                                .get_graph_element(quad.predicate)
                                .clone();
                            let o = datastore.resources.get_graph_element(quad.obj).clone();
                            output.insert(ResolvedTriple {
                                subject: s,
                                predicate: p,
                                object: o,
                            });
                        }
                    }
                }
            }

            Ok(QueryResult::Describe(output.into_iter().collect()))
        }
        Query::Construct {
            template,
            where_clause,
            dataset,
        } => {
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let solutions = eval_components(
                where_clause,
                initial,
                datastore,
                dataset_active_graph(dataset, datastore),
                deadline,
            )?;

            let effective_template: Vec<TriplePattern> = if template.is_empty() {
                collect_bgps_from_components(where_clause)
            } else {
                template.clone()
            };

            let mut output: HashSet<ResolvedTriple> = HashSet::new();
            let mut bnode_counter: u32 = 0;

            for sub in &solutions {
                let mut bnode_map: HashMap<u32, u32> = HashMap::new();
                for tp in &effective_template {
                    let s = bind_template_term(
                        &tp.subject,
                        sub,
                        datastore,
                        &mut bnode_map,
                        &mut bnode_counter,
                    );
                    let p = bind_template_term(
                        &tp.predicate,
                        sub,
                        datastore,
                        &mut bnode_map,
                        &mut bnode_counter,
                    );
                    let o = bind_template_term(
                        &tp.object,
                        sub,
                        datastore,
                        &mut bnode_map,
                        &mut bnode_counter,
                    );
                    if let (Some(s), Some(p), Some(o)) = (s, p, o) {
                        let subject_ok = !matches!(s, GraphElement::GraphLiteral(_));
                        let pred_ok =
                            matches!(p, GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(_)));
                        if subject_ok && pred_ok {
                            output.insert(ResolvedTriple {
                                subject: s,
                                predicate: p,
                                object: o,
                            });
                        }
                    }
                }
            }

            Ok(QueryResult::Construct(output.into_iter().collect()))
        }
    }
}

// ── Projection ────────────────────────────────────────────────────────────────

fn projection_variables(
    proj: &[ProjectionElement],
    components: &[QueryComponent],
    _datastore: &Datastore,
) -> Vec<String> {
    // If star, collect all variables from the where clause
    if proj.iter().any(|p| matches!(p, ProjectionElement::Star)) {
        let mut vars: Vec<String> = Vec::new();
        collect_vars_from_components(components, &mut vars);
        vars.sort();
        vars.dedup();
        return vars;
    }
    proj.iter()
        .filter_map(|p| match p {
            ProjectionElement::Variable(v) => Some(v.clone()),
            ProjectionElement::Expression(_, alias) => Some(alias.clone()),
            ProjectionElement::Star => None,
        })
        .collect()
}

fn collect_vars_from_components(components: &[QueryComponent], vars: &mut Vec<String>) {
    for comp in components {
        match comp {
            QueryComponent::BGP(tps) => {
                for tp in tps {
                    collect_vars_from_term(&tp.subject, vars);
                    collect_vars_from_term(&tp.predicate, vars);
                    collect_vars_from_term(&tp.object, vars);
                }
            }
            QueryComponent::PathPattern(subject, _, object) => {
                // Do NOT expose internal variables; only subject and object matter.
                collect_vars_from_term(subject, vars);
                collect_vars_from_term(object, vars);
            }
            QueryComponent::Subquery(inner_query) => {
                // Only the inner query's projected variables are visible.
                if let Query::Select { projection, .. } = inner_query.as_ref() {
                    for elem in projection {
                        match elem {
                            ProjectionElement::Variable(v) => vars.push(v.clone()),
                            ProjectionElement::Expression(_, alias) => vars.push(alias.clone()),
                            ProjectionElement::Star => {}
                        }
                    }
                }
            }
            QueryComponent::Optional(inner)
            | QueryComponent::Minus(inner)
            | QueryComponent::Group(inner) => {
                collect_vars_from_components(inner, vars);
            }
            QueryComponent::Union(left, right) => {
                collect_vars_from_components(left, vars);
                collect_vars_from_components(right, vars);
            }
            QueryComponent::Graph(graph_term, inner) => {
                collect_vars_from_term(graph_term, vars);
                collect_vars_from_components(inner, vars);
            }
            QueryComponent::Bind(_, alias) => {
                vars.push(alias.clone());
            }
            QueryComponent::Filter(_) => {}
            // A `VALUES` block — whether written inline in the group graph
            // pattern, or a trailing post-query/post-subquery `ValuesClause`
            // appended here by `parse_query_body` (see
            // `join_solutions_with_values`) — introduces its variables into
            // scope exactly like any other pattern element, so `SELECT *`
            // must project them too.
            QueryComponent::Values(values_vars, _) => {
                for v in values_vars {
                    if !is_internal_variable(v) {
                        vars.push(v.clone());
                    }
                }
            }
            QueryComponent::Service(_, inner, _) => {
                collect_vars_from_components(inner, vars);
            }
        }
    }
}

fn collect_vars_from_term(term: &Term, vars: &mut Vec<String>) {
    if let Term::Variable(v) = term {
        if !is_internal_variable(v) {
            vars.push(v.clone());
        }
    }
}

fn is_internal_variable(var: &str) -> bool {
    // `__path_*` — fresh variables introduced for property-path midpoints.
    // `__bn_*` — fresh variables standing in for blank nodes introduced by
    // the `[...]`/`[]` property-list shorthand (subject or object position;
    // see `parse_object_term` / `parse_group_graph_pattern_contents` in
    // `lib.rs`). Neither should ever leak into a `SELECT *` projection: they
    // don't appear in the query text, so a user has no name to reference
    // them by. See [#201](https://github.com/daghovland/rdf-datalog/issues/201).
    var.starts_with("__path_") || var.starts_with("__bn_")
}

/// Returns `Some(endpoint_iri)` for the first non-SILENT SERVICE node found,
/// or `None` if the query contains no non-SILENT SERVICE.
fn first_non_silent_service(components: &[QueryComponent]) -> Option<&Term> {
    for comp in components {
        match comp {
            QueryComponent::Service(endpoint, inner, silent) => {
                if !silent {
                    return Some(endpoint);
                }
                if let Some(ep) = first_non_silent_service(inner) {
                    return Some(ep);
                }
            }
            QueryComponent::Optional(inner) | QueryComponent::Minus(inner) => {
                if let Some(ep) = first_non_silent_service(inner) {
                    return Some(ep);
                }
            }
            QueryComponent::Graph(_, inner) => {
                if let Some(ep) = first_non_silent_service(inner) {
                    return Some(ep);
                }
            }
            QueryComponent::Union(left, right) => {
                if let Some(ep) = first_non_silent_service(left) {
                    return Some(ep);
                }
                if let Some(ep) = first_non_silent_service(right) {
                    return Some(ep);
                }
            }
            _ => {}
        }
    }
    None
}

/// A single variable binding during evaluation.
///
/// The common case — a value bound directly from a matched quad — is kept as a
/// cheap interned [`GraphElementId`] (`u32`) so the hot BGP/join path never
/// clones a full [`GraphElement`]. Computed values (from `BIND`, `VALUES`, or
/// aggregates) may not be interned in the store, so they are carried inline as
/// a `GraphElement` instead — this is why a plain
/// `HashMap<String, GraphElementId>` does not work: interning a fresh value
/// would require `&mut Datastore`, but the eval stack only holds `&Datastore`.
///
/// Intentionally does **not** derive `PartialEq`: equality between two bindings
/// must compare their *resolved* [`GraphElement`] values (an `Interned` id and a
/// `Computed` value can denote the same element), which requires the datastore —
/// use [`psv_eq`]. Omitting the derive makes any accidental representation-level
/// `==` or `.contains` a compile error rather than a silent correctness bug.
/// See <https://github.com/daghovland/rdf-datalog/issues/141>.
#[derive(Clone, Debug)]
enum PartialSubValue {
    /// A value that came straight from a quad field — cheap to clone, resolved
    /// back to a [`GraphElement`] via the datastore only when needed.
    Interned(GraphElementId),
    /// A computed value (`BIND`/`VALUES`/aggregate result) that is not
    /// necessarily present in the store, carried inline.
    Computed(GraphElement),
}

impl PartialSubValue {
    /// Resolve to a concrete [`GraphElement`] (cloning). `Interned` ids are
    /// looked up in the store; `Computed` values are returned as-is.
    fn resolve(&self, datastore: &Datastore) -> GraphElement {
        match self {
            PartialSubValue::Interned(id) => datastore.resources.get_graph_element(*id).clone(),
            PartialSubValue::Computed(gel) => gel.clone(),
        }
    }

    /// The interned [`GraphElementId`] this binding denotes, if any. `Interned`
    /// already holds it; a `Computed` value only has one when it happens to be
    /// present in the store. Returns `None` for a computed value that was never
    /// interned. Equivalent to the pre-#141 `resource_map.get(gel).copied()`.
    fn to_id(&self, datastore: &Datastore) -> Option<GraphElementId> {
        match self {
            PartialSubValue::Interned(id) => Some(*id),
            PartialSubValue::Computed(gel) => datastore.resources.resource_map.get(gel).copied(),
        }
    }
}

/// Internal solution mapping: variable → [`PartialSubValue`]. Decoupled from
/// [`SolutionRow`] (the public result type) so the hot path can hold interned
/// ids; see [`PartialSubValue`]. Resolution back to `GraphElement` happens only
/// when producing final query results or evaluating expressions.
type PartialSub = HashMap<String, PartialSubValue>;

#[derive(Clone)]
enum ActiveGraph {
    Fixed(GraphElementId),
    Variable(String),
}

/// Compute the active graph for a query from its dataset clauses.
///
/// A `FROM <g>` clause makes `<g>` the default graph; the first such clause wins.
/// If no `FROM` clauses are present, the default graph is used unchanged.
fn dataset_active_graph(dataset: &[DatasetClause], datastore: &Datastore) -> ActiveGraph {
    for clause in dataset {
        if let DatasetClause::Default(gel) = clause {
            if let Some(&id) = datastore.resources.resource_map.get(gel) {
                return ActiveGraph::Fixed(id);
            }
        }
    }
    ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID)
}

/// Collect all BGP triple patterns from a component list (for CONSTRUCT WHERE short form).
fn collect_bgps_from_components(components: &[QueryComponent]) -> Vec<TriplePattern> {
    let mut out = Vec::new();
    for comp in components {
        match comp {
            QueryComponent::BGP(tps) => out.extend(tps.clone()),
            QueryComponent::Optional(inner)
            | QueryComponent::Minus(inner)
            | QueryComponent::Group(inner) => {
                out.extend(collect_bgps_from_components(inner));
            }
            QueryComponent::Union(left, right) => {
                out.extend(collect_bgps_from_components(left));
                out.extend(collect_bgps_from_components(right));
            }
            QueryComponent::Graph(_, inner) => {
                out.extend(collect_bgps_from_components(inner));
            }
            QueryComponent::PathPattern(_, _, _)
            | QueryComponent::Subquery(_)
            | QueryComponent::Filter(_)
            | QueryComponent::Bind(_, _)
            | QueryComponent::Values(_, _)
            | QueryComponent::Service(_, _, _) => {}
        }
    }
    out
}

// ── Subquery helpers ──────────────────────────────────────────────────────────

/// Merge two partial substitutions: succeed if they agree on shared variables.
fn merge_solutions(
    outer: &PartialSub,
    inner: &PartialSub,
    datastore: &Datastore,
) -> Option<PartialSub> {
    let mut merged = outer.clone();
    for (var, val) in inner {
        match merged.get(var) {
            Some(existing) if !psv_eq(existing, val, datastore) => return None,
            _ => {
                merged.insert(var.clone(), val.clone());
            }
        }
    }
    Some(merged)
}

/// Execute a SELECT subquery, returning projected solution rows.
///
/// Applies ORDER BY, DISTINCT, LIMIT, and OFFSET from the inner query.
fn execute_select_inner(
    query: &Query,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    let Query::Select {
        projection,
        where_clause,
        limit,
        offset,
        distinct,
        group_by,
        having,
        order_by,
        ..
    } = query
    else {
        return Ok(Vec::new());
    };

    let initial: Vec<PartialSub> = vec![HashMap::new()];
    let budget = select_solution_budget(*distinct, order_by, group_by, projection, *offset, *limit);
    let solutions = eval_components_budgeted(
        where_clause,
        initial,
        datastore,
        (*active_graph).clone(),
        budget,
        deadline,
    )?;

    let aggregate_mode = !group_by.is_empty() || projection.iter().any(elem_has_aggregate);

    let mut rows: Vec<PartialSub> = if aggregate_mode {
        let groups = group_by_solutions(&solutions, group_by, datastore);
        groups
            .into_iter()
            .filter(|g| {
                having
                    .iter()
                    .all(|expr| eval_having_expr(expr, g, datastore))
            })
            .map(|g| {
                // One aggregate row is one query solution — see
                // `project_aggregate_row`/`project_with_exprs_partial` for
                // why `BNODE(str)` needs a fresh memo per row. #346.
                let _bnode_guard = BnodeMemoGuard::install();
                // Build a PartialSub from aggregate projections
                let rep = g.first().cloned().unwrap_or_default();
                let mut row = PartialSub::new();
                for elem in projection.iter() {
                    match elem {
                        ProjectionElement::Variable(v) => {
                            if let Some(val) = rep.get(v) {
                                row.insert(v.clone(), val.clone());
                            }
                        }
                        ProjectionElement::Expression(expr, alias) => {
                            if let Some(val) = eval_expr_in_group(expr, &g, &rep, datastore) {
                                row.insert(alias.clone(), PartialSubValue::Computed(val));
                            }
                        }
                        ProjectionElement::Star => {}
                    }
                }
                row
            })
            .collect()
    } else {
        // Evaluate any `(expr AS ?alias)` projection elements (with alias
        // reuse across the subquery's own projection list — see
        // `project_with_exprs_partial`), and project down to just the
        // requested variables (or keep everything for `SELECT *`).
        // See https://github.com/daghovland/rdf-datalog/issues/223.
        solutions
            .into_iter()
            .map(|sub| project_with_exprs_partial(&sub, projection, datastore))
            .collect()
    };

    // ORDER BY
    if !order_by.is_empty() {
        sort_solutions(&mut rows, order_by, datastore);
    }

    // DISTINCT
    if *distinct {
        let mut seen: HashSet<Vec<(String, GraphElement)>> = HashSet::new();
        rows.retain(|row| {
            let mut key: Vec<(String, GraphElement)> = row
                .iter()
                .map(|(k, v)| (k.clone(), v.resolve(datastore)))
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0));
            seen.insert(key)
        });
    }

    // OFFSET
    if let Some(off) = offset {
        let off = *off as usize;
        if off < rows.len() {
            rows = rows[off..].to_vec();
        } else {
            rows.clear();
        }
    }

    // LIMIT
    if let Some(lim) = limit {
        rows.truncate(*lim as usize);
    }

    Ok(rows)
}

/// Sort solution rows by ORDER BY conditions.
fn sort_solutions(rows: &mut [PartialSub], order_by: &[OrderCondition], datastore: &Datastore) {
    rows.sort_by(|a, b| {
        for cond in order_by {
            let av = eval_expression_value_inner(&cond.expression, a, datastore);
            let bv = eval_expression_value_inner(&cond.expression, b, datastore);
            let ord = match (&av, &bv) {
                (Some(l), Some(r)) => compare_graph_elements_total(l, r),
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            };
            if ord != std::cmp::Ordering::Equal {
                return if cond.ascending { ord } else { ord.reverse() };
            }
        }
        std::cmp::Ordering::Equal
    });
}

/// Total ordering for `GraphElement` values (for ORDER BY).
fn compare_graph_elements_total(a: &GraphElement, b: &GraphElement) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        // Numerics first
        (GraphElement::GraphLiteral(al), GraphElement::GraphLiteral(bl)) => {
            if let (Some(af), Some(bf)) = (literal_to_f64(al), literal_to_f64(bl)) {
                return af.partial_cmp(&bf).unwrap_or(Equal);
            }
            // String comparison of the lexical form
            let as_ = graph_element_to_string(a).unwrap_or_default();
            let bs = graph_element_to_string(b).unwrap_or_default();
            as_.cmp(&bs)
        }
        (
            GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(ai)),
            GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(bi)),
        ) => ai.0.cmp(&bi.0),
        _ => {
            let as_ = graph_element_to_string(a).unwrap_or_default();
            let bs = graph_element_to_string(b).unwrap_or_default();
            as_.cmp(&bs)
        }
    }
}

// ── Property path evaluation ──────────────────────────────────────────────────

/// Generate a fresh, globally-unique internal bridge-variable name for
/// property-path `Sequence` evaluation.
///
/// Previously these were named positionally (`__path_seq_{i}`, `i` being the
/// step index *within a single `Sequence` call*). That collides when a
/// `Sequence` is evaluated while nested inside another `Sequence`'s
/// evaluation — e.g. `(p1/p2){1,}/(p3/p4){1,}`, a 2-step outer `Sequence`
/// whose own step-0 bridge variable is (by the old scheme) always named
/// `__path_seq_0`, while `eval_repeat_path`'s `{1,}` desugaring
/// (`inner`/`inner*`) independently builds a *nested* 2-step `Sequence` that
/// also names its own step-0 bridge `__path_seq_0`. Both variables end up
/// sharing one substitution-map entry, so the outer target variable gets
/// silently aliased to an unrelated intermediate node — see
/// <https://github.com/daghovland/rdf-datalog/issues/203> (the W3C `pp04`
/// "Variable length path with loop" scenario). A process-wide counter makes
/// every bridge variable unique regardless of nesting depth.
fn fresh_bridge_var() -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    format!("__path_bridge_{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod limit_budget_tests {
    use super::*;
    use crate::ast::{
        Aggregate, Expression, GroupCondition, OrderCondition, ProjectionElement, Term,
        TriplePattern,
    };

    fn var(name: &str) -> Term {
        Term::Variable(name.to_string())
    }

    fn iri_const(iri: &str) -> Term {
        Term::Constant(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
            IriReference(iri.to_string()),
        )))
    }

    /// A plain `SELECT ... LIMIT n` (no ORDER BY / DISTINCT / GROUP BY /
    /// aggregate) yields a budget of `OFFSET + LIMIT`, the number of rows the
    /// query can ever return.
    #[test]
    fn budget_is_offset_plus_limit_for_plain_select() {
        let proj = vec![ProjectionElement::Variable("s".into())];
        assert_eq!(
            select_solution_budget(false, &[], &[], &proj, None, Some(10)),
            Some(10),
            "LIMIT 10 with no OFFSET budgets 10 rows"
        );
        assert_eq!(
            select_solution_budget(false, &[], &[], &proj, Some(5), Some(10)),
            Some(15),
            "OFFSET 5 LIMIT 10 must fetch 15 rows before slicing"
        );
    }

    /// No LIMIT means the whole solution set is required — an OFFSET alone is
    /// unbounded, so there is no budget.
    #[test]
    fn no_limit_means_no_budget() {
        let proj = vec![ProjectionElement::Variable("s".into())];
        assert_eq!(
            select_solution_budget(false, &[], &[], &proj, None, None),
            None
        );
        assert_eq!(
            select_solution_budget(false, &[], &[], &proj, Some(3), None),
            None,
            "OFFSET without LIMIT is unbounded"
        );
    }

    /// Modifiers that must observe every row disable the short-circuit.
    #[test]
    fn full_set_modifiers_disable_budget() {
        let proj = vec![ProjectionElement::Variable("s".into())];

        assert_eq!(
            select_solution_budget(true, &[], &[], &proj, None, Some(10)),
            None,
            "DISTINCT (conservative first pass) disables the budget"
        );

        let order = vec![OrderCondition {
            expression: Expression::Variable("s".into()),
            ascending: true,
        }];
        assert_eq!(
            select_solution_budget(false, &order, &[], &proj, None, Some(10)),
            None,
            "ORDER BY must sort the whole set"
        );

        let group = vec![GroupCondition {
            expr: Expression::Variable("s".into()),
            alias: None,
        }];
        assert_eq!(
            select_solution_budget(false, &[], &group, &proj, None, Some(10)),
            None,
            "GROUP BY must fold every row"
        );

        let agg_proj = vec![ProjectionElement::Expression(
            Expression::Aggregate(Aggregate::CountStar),
            "c".into(),
        )];
        assert_eq!(
            select_solution_budget(false, &[], &[], &agg_proj, None, Some(10)),
            None,
            "an aggregate projection must fold every row"
        );
    }

    /// The quad-take gate: distinct variable positions can be truncated at the
    /// quad level; a repeated variable cannot (a matched quad may be dropped).
    #[test]
    fn repeated_variable_gate() {
        let distinct = TriplePattern {
            subject: var("s"),
            predicate: var("p"),
            object: var("o"),
        };
        assert!(
            !pattern_repeats_variable(&distinct, &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID)),
            "s/p/o are distinct — quad-take is sound"
        );

        let self_loop = TriplePattern {
            subject: var("x"),
            predicate: iri_const("http://example.org/p"),
            object: var("x"),
        };
        assert!(
            pattern_repeats_variable(&self_loop, &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID)),
            "?x ... ?x repeats a variable — quad-take must be disabled"
        );

        // The graph variable colliding with a pattern variable also counts.
        assert!(
            pattern_repeats_variable(&distinct, &ActiveGraph::Variable("s".into())),
            "graph variable equal to the subject variable is a repeat"
        );
        assert!(
            !pattern_repeats_variable(&distinct, &ActiveGraph::Variable("g".into())),
            "a fresh graph variable is not a repeat"
        );
    }
}

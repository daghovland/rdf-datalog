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

use crate::ast::{
    Aggregate, BinaryOp, DatasetClause, Expression, GroupCondition, OrderCondition,
    ProjectionElement, PropertyPath, Query, QueryComponent, Term, TriplePattern, UnaryOp,
};
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, DEFAULT_GRAPH_ELEMENT_ID};
use ingress::{
    IriReference, NetworkPolicy, XSD_BOOLEAN, XSD_DATE, XSD_DATE_TIME, XSD_DECIMAL, XSD_DOUBLE,
    XSD_FLOAT, XSD_INTEGER, XSD_STRING,
};
use num_bigint::BigInt;
use std::collections::HashMap;
use std::collections::HashSet;

/// A single bound solution: variable name → concrete graph element.
pub type SolutionRow = HashMap<String, GraphElement>;

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
/// `network` controls how `SERVICE` federation clauses are handled:
/// - [`NetworkPolicy::Deny`] — non-SILENT SERVICE returns an error (default, safe).
/// - [`NetworkPolicy::Ignore`] — all SERVICE clauses return empty results silently.
/// - [`NetworkPolicy::Allow`] — not yet implemented; returns a "not yet implemented" error.
pub fn execute(
    query: &Query,
    datastore: &Datastore,
    network: NetworkPolicy,
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
            );

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
            );
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
            );

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
            );

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
            QueryComponent::Optional(inner) | QueryComponent::Minus(inner) => {
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

/// Project a solution row, evaluating any `(expr AS ?alias)` projection elements.
///
/// Thin wrapper around [`project_with_exprs_partial`] that resolves each
/// projected binding to a concrete [`GraphElement`] for the top-level
/// `SELECT` result shape (`SolutionRow`). See that function for the
/// alias-reuse semantics.
fn project_with_exprs(
    sub: &PartialSub,
    projection: &[ProjectionElement],
    datastore: &Datastore,
) -> SolutionRow {
    project_with_exprs_partial(sub, projection, datastore)
        .into_iter()
        .map(|(k, v)| (k, v.resolve(datastore)))
        .collect()
}

/// Project a solution, evaluating any `(expr AS ?alias)` projection elements,
/// keeping the result as a [`PartialSub`] (unresolved bindings) rather than a
/// fully-resolved [`SolutionRow`].
///
/// A later `(expr AS ?alias)` SELECT item may reference an alias bound by an
/// earlier one in the same projection list (e.g.
/// `SELECT (?a + 1 AS ?x) (?x * 2 AS ?y)`) — the W3C project-expression
/// conformance suite's "Reuse a project expression variable in select" case.
/// To support that, expressions are evaluated against a `sub`-derived
/// substitution that accumulates each computed alias as it goes, rather than
/// against the original WHERE-clause bindings alone. `Star`/`Variable`
/// projection elements are unaffected — they always read the original
/// WHERE-clause bindings, never a previously-projected alias.
///
/// Shared by the top-level `SELECT` path ([`project_with_exprs`]) and the
/// non-aggregate subquery projection path (`execute_select_inner`), so the
/// alias-reuse fix from issue 207 (linked below) applies uniformly to both —
/// see issue 223 (linked below) for the subquery-path gap this closes.
/// See <https://github.com/daghovland/rdf-datalog/issues/207> and
/// <https://github.com/daghovland/rdf-datalog/issues/223>.
fn project_with_exprs_partial(
    sub: &PartialSub,
    projection: &[ProjectionElement],
    datastore: &Datastore,
) -> PartialSub {
    let mut row: PartialSub = HashMap::new();
    let mut extended: PartialSub = sub.clone();
    for elem in projection {
        match elem {
            ProjectionElement::Star => {
                for (k, v) in sub {
                    row.insert(k.clone(), v.clone());
                }
            }
            ProjectionElement::Variable(v) => {
                if let Some(val) = sub.get(v) {
                    row.insert(v.clone(), val.clone());
                }
            }
            ProjectionElement::Expression(expr, alias) => {
                if let Some(val) = eval_expression_value_inner(expr, &extended, datastore) {
                    extended.insert(alias.clone(), PartialSubValue::Computed(val.clone()));
                    row.insert(alias.clone(), PartialSubValue::Computed(val));
                }
            }
        }
    }
    row
}

// ── Evaluation ────────────────────────────────────────────────────────────────

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

/// Value-equality between two bindings, reproducing the pre-#141 semantics
/// where `PartialSub` held resolved [`GraphElement`]s compared with `==`.
fn psv_eq(a: &PartialSubValue, b: &PartialSubValue, datastore: &Datastore) -> bool {
    match (a, b) {
        // Interning is injective (`add_resource` dedups), so equal ids denote
        // equal elements — no datastore lookup needed on the hot path.
        (PartialSubValue::Interned(x), PartialSubValue::Interned(y)) => x == y,
        (PartialSubValue::Computed(x), PartialSubValue::Computed(y)) => x == y,
        // Mixed: an interned id and a computed value can still denote the same
        // element, so compare resolved forms.
        _ => a.resolve(datastore) == b.resolve(datastore),
    }
}

/// Whole-solution equality by resolved value, reproducing the pre-#141
/// `HashMap<String, GraphElement>` `==`/`.contains` semantics: same key set and
/// each shared variable's binding resolves to the same [`GraphElement`].
fn partial_subs_equal(a: &PartialSub, b: &PartialSub, datastore: &Datastore) -> bool {
    a.len() == b.len()
        && a.iter().all(|(k, va)| match b.get(k) {
            Some(vb) => psv_eq(va, vb, datastore),
            None => false,
        })
}

/// Wrap a resolved [`SolutionRow`] as a [`PartialSub`]. Used at the public API
/// boundary where callers hand us already-resolved [`GraphElement`] bindings;
/// they are carried as `Computed` (a `Computed` value that is in fact interned
/// is still resolved correctly by [`PartialSubValue::to_id`] / [`psv_eq`]).
fn solution_row_to_partial(row: &SolutionRow) -> PartialSub {
    row.iter()
        .map(|(k, v)| (k.clone(), PartialSubValue::Computed(v.clone())))
        .collect()
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

fn eval_components(
    components: &[QueryComponent],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: ActiveGraph,
) -> Vec<PartialSub> {
    eval_components_budgeted(components, solutions, datastore, active_graph, None)
}

/// Evaluate a component list, optionally short-circuiting once `budget`
/// output solutions exist.
///
/// The budget is the maximum number of solutions the caller will ever
/// consume (`OFFSET + LIMIT` at the top level). It is passed to the **last**
/// component only: the last component's output *is* the final solution set in
/// order, and projection is 1:1 with solutions while `OFFSET`/`LIMIT` are
/// prefix operations, so returning the first `budget` solutions of the last
/// component is byte-identical to producing them all and truncating. Earlier
/// components must be fully materialised (a later component may filter, so we
/// cannot know how many of their rows are needed). Only the BGP arm actually
/// reads the budget; every other arm ignores it and relies on the caller's
/// existing truncation. See issue #165.
///
/// Phase C (#38): before evaluating, a conjunctive group is reordered so a
/// constraining conjunct is scheduled before a `UNION` it shares variables
/// with, letting its bindings flow into the union arms via the existing
/// per-arm threading. Gated by a cheap check so the common path (notably
/// per-row `OPTIONAL`/`MINUS`/`EXISTS` inner evaluations) stays
/// allocation-free and byte-for-byte unchanged. Reordering is
/// result-preserving (bag-join commutes/distributes over bag-union), so it
/// composes safely with the budget above: the budget applies to whichever
/// component ends up physically last *after* reordering, and since only the
/// BGP arm actually honors it, a non-BGP arm landing in last position simply
/// ignores the budget and falls back on the caller's existing truncation — a
/// missed optimisation in that combination, never a correctness issue.
fn eval_components_budgeted(
    components: &[QueryComponent],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: ActiveGraph,
    budget: Option<usize>,
) -> Vec<PartialSub> {
    let ordered: Vec<&QueryComponent> = if crate::component_ordering::should_reorder(components) {
        let already_bound: HashSet<String> = solutions
            .first()
            .map(|sub| sub.keys().cloned().collect())
            .unwrap_or_default();
        // Correctness-critical, unlike `already_bound` above: variables
        // guaranteed bound on *every* incoming row, not just the first one.
        // Hoisting a conjunct across an `OPTIONAL`/`MINUS` barrier (issue
        // #174) must never be permitted based on a variable that's only
        // *conditionally* bound (e.g. bound in one `UNION` arm but not
        // another feeding into this call) — see
        // `component_ordering::order_components` for why an
        // over-approximation here is unsound, not just imprecise.
        let guaranteed_bound: HashSet<String> = {
            let mut rows = solutions.iter();
            match rows.next() {
                None => HashSet::new(),
                Some(first) => {
                    let mut acc: HashSet<String> = first.keys().cloned().collect();
                    for sub in rows {
                        acc.retain(|k| sub.contains_key(k));
                    }
                    acc
                }
            }
        };
        crate::component_ordering::order_components(
            components,
            &already_bound,
            &guaranteed_bound,
            datastore,
        )
    } else {
        components.iter().collect()
    };

    let mut current = solutions;
    let last = ordered.len().saturating_sub(1);
    for (i, comp) in ordered.into_iter().enumerate() {
        let comp_budget = if i == last { budget } else { None };
        current = eval_component(comp, current, datastore, &active_graph, comp_budget);
        if current.is_empty() {
            break;
        }
    }
    current
}

fn eval_component(
    comp: &QueryComponent,
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
) -> Vec<PartialSub> {
    match comp {
        QueryComponent::BGP(tps) => eval_bgp(tps, solutions, datastore, active_graph, budget),

        QueryComponent::PathPattern(subject, path, object) => solutions
            .into_iter()
            .flat_map(|sub| eval_path_pattern(subject, path, object, sub, datastore, active_graph))
            .collect(),

        QueryComponent::Subquery(inner_query) => {
            let inner_rows = execute_select_inner(inner_query, datastore, active_graph);
            solutions
                .into_iter()
                .flat_map(|outer_sub| {
                    inner_rows
                        .iter()
                        .filter_map(|inner_sub| merge_solutions(&outer_sub, inner_sub, datastore))
                        .collect::<Vec<_>>()
                })
                .collect()
        }

        QueryComponent::Filter(expr) => solutions
            .into_iter()
            .filter(|sub| eval_filter(expr, sub, datastore, active_graph))
            .collect(),

        QueryComponent::Optional(inner) => {
            let mut result = Vec::new();
            for sub in solutions {
                let extended =
                    eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
                if extended.is_empty() {
                    result.push(sub);
                } else {
                    result.extend(extended);
                }
            }
            result
        }

        QueryComponent::Union(left, right) => {
            let left_sols =
                eval_components(left, solutions.clone(), datastore, (*active_graph).clone());
            let right_sols = eval_components(right, solutions, datastore, (*active_graph).clone());
            let mut result = left_sols;
            result.extend(right_sols);
            result
        }

        QueryComponent::Minus(inner) => {
            // SPARQL 1.1 §18.3 domain-disjointness escape: a row that shares
            // no variable at all with anything the MINUS body could bind
            // must never be excluded, regardless of the body's content. The
            // previous implementation threaded the outer `sub` into the
            // inner body's evaluation, so every produced solution was a
            // trivial extension of `sub` and therefore always "compatible"
            // and always domain-overlapping (dom always superset of
            // dom(sub)) — the escape hatch never fired (issue #187).
            // `inner_vars` is a static, safe-to-over-approximate set of
            // every variable the body could ever bind; it's only used to
            // short-circuit rows that can never be affected, never to
            // decide an actual exclusion.
            let inner_vars = crate::component_ordering::variables_in_components(inner);

            // Ω2 is evaluated independently of the outer solutions — an
            // unseeded start, i.e. the real right-hand-side semantics — and
            // memoised across outer rows: its result never depends on
            // `sub`, so recomputing it per row (as the old seeded threading
            // did) was pure waste. This also fixes a subtler bug the naive
            // "thread + check domain" approach would still have: seeding
            // `sub` into a body containing `OPTIONAL` makes an
            // already-bound variable look bound in the produced solution
            // even when that specific inner branch never actually bound it
            // (e.g. the W3C `full-minuend`/`part-minuend` negation tests),
            // corrupting the per-row domain. Evaluating unseeded gives each
            // μ2's real domain (its own `.keys()`), so the check below is
            // exact.
            //
            // This trades the old per-row index-narrowing (seeding pushed a
            // bound outer value into the inner BGP lookup) for one
            // evaluation plus an O(outer × inner) anti-join scan, mirroring
            // the nested-loop join the `Subquery` arm above already uses; a
            // hash index keyed on a shared variable would be a reasonable
            // follow-up if this ever shows up as a hot path.
            let mut minus_solutions: Option<Vec<PartialSub>> = None;

            solutions
                .into_iter()
                .filter(|sub| {
                    if !sub.keys().any(|k| inner_vars.contains(k)) {
                        // Domain-disjointness escape: statically impossible
                        // for this row to share a variable with the body.
                        return true;
                    }
                    let minus_sols = minus_solutions.get_or_insert_with(|| {
                        eval_components(
                            inner,
                            vec![HashMap::new()],
                            datastore,
                            (*active_graph).clone(),
                        )
                    });
                    // Exclude `sub` iff some μ2 is compatible with it AND
                    // actually shares a bound variable with it — the
                    // spec's `¬(¬compatible ∨ dom-disjoint)`.
                    !minus_sols.iter().any(|ms| {
                        compatible(sub, ms, datastore) && sub.keys().any(|k| ms.contains_key(k))
                    })
                })
                .collect()
        }

        QueryComponent::Graph(graph_term, inner) => solutions
            .into_iter()
            .flat_map(|sub| {
                let scoped_graph = match graph_term {
                    Term::Constant(gel) => {
                        let Some(&graph_id) = datastore.resources.resource_map.get(gel) else {
                            return Vec::new();
                        };
                        ActiveGraph::Fixed(graph_id)
                    }
                    Term::Variable(var) => {
                        match sub.get(var).and_then(|val| val.to_id(datastore)) {
                            Some(graph_id) => ActiveGraph::Fixed(graph_id),
                            None => ActiveGraph::Variable(var.clone()),
                        }
                    }
                    // A triple term can never name a graph.
                    Term::TripleTerm(_) => return Vec::new(),
                };
                eval_components(inner, vec![sub], datastore, scoped_graph)
            })
            .collect(),

        QueryComponent::Bind(expr, alias) => solutions
            .into_iter()
            .map(|mut sub| {
                // SPARQL 1.1 §18.3 Extend: if evaluating the expression
                // raises an error — e.g. `BIND(?nova AS ?z)` where `?nova`
                // was never bound (W3C `bind04`) — the row is not dropped;
                // `alias` is simply left unbound for that solution. The
                // previous `filter_map` dropped the whole row instead,
                // wrongly turning an "unbound" outcome into "no match". See
                // <https://github.com/daghovland/rdf-datalog/issues/198>.
                if let Some(val) = eval_bind_expr(expr, &sub, datastore) {
                    sub.insert(alias.clone(), PartialSubValue::Computed(val));
                }
                sub
            })
            .collect(),

        QueryComponent::Values(vars, rows) => {
            join_solutions_with_values(solutions, vars, rows, datastore)
        }

        QueryComponent::Service(_, inner, _) => {
            // SERVICE not supported; return empty
            let _ = inner;
            Vec::new()
        }
    }
}

/// Natural join of a solution set against a `VALUES` data block: `vars` names
/// the columns, `rows` is each inline-data row (`None` for `UNDEF`).
///
/// For every existing solution, every VALUES row that doesn't conflict with
/// an already-bound variable produces one output solution (so a solution can
/// multiply into several rows when more than one VALUES row is compatible —
/// see the W3C bindings-suite `values04`/`values05` fixtures, which rely on
/// exactly this to produce more output rows than input solutions). A `None`
/// (`UNDEF`) entry in a row leaves that variable unconstrained by *that row*
/// — it neither introduces a new binding nor conflicts with an existing one
/// — per SPARQL 1.1 §10.2's inline-data-as-join semantics.
///
/// Backs [`QueryComponent::Values`] (evaluated in [`eval_component`]), which
/// is *also* how a trailing post-query / post-subquery `ValuesClause` is
/// represented: `sparql_parser::parse_query_body` appends the parsed
/// `ValuesClause` directly onto the query's (or subquery's) `where_clause`
/// rather than modelling it as a separate post-modifier field. That gets its
/// join-before-`Project` placement (SPARQL 1.1 §18.2.4.3 — a ValuesClause
/// variable can bind/restrict solutions even when it isn't in the SELECT
/// list, but is itself projected out only under `SELECT *`) and its
/// subquery-projection scoping for free from the same machinery that
/// already evaluates an inline `VALUES` block, with no separate code path
/// to keep in sync. See <https://github.com/daghovland/rdf-datalog/issues/200>.
fn join_solutions_with_values(
    solutions: Vec<PartialSub>,
    vars: &[String],
    rows: &[Vec<Option<GraphElement>>],
    datastore: &Datastore,
) -> Vec<PartialSub> {
    let mut result = Vec::new();
    for sub in solutions {
        for row in rows {
            if vars.len() != row.len() {
                continue;
            }
            let mut new_sub = sub.clone();
            let mut ok = true;
            for (var, val_opt) in vars.iter().zip(row.iter()) {
                if let Some(gel) = val_opt {
                    let new_val = PartialSubValue::Computed(gel.clone());
                    match new_sub.get(var) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                            break;
                        }
                        _ => {
                            new_sub.insert(var.clone(), new_val);
                        }
                    }
                } // UNDEF (None) — leave unbound
            }
            if ok {
                result.push(new_sub);
            }
        }
    }
    result
}

/// Two substitutions are compatible if they agree on all shared variables.
fn compatible(a: &PartialSub, b: &PartialSub, datastore: &Datastore) -> bool {
    for (var, val_a) in a {
        if let Some(val_b) = b.get(var) {
            if !psv_eq(val_a, val_b, datastore) {
                return false;
            }
        }
    }
    true
}

// ── LIMIT short-circuit budget (issue #165) ─────────────────────────────────

/// The maximum number of solutions the top-level (or subquery) SELECT will
/// ever consume, i.e. `OFFSET + LIMIT`, or `None` when the full solution set
/// is required.
///
/// Returns `None` — disabling the short-circuit — whenever a solution-set
/// modifier must observe every row: no `LIMIT` at all (an `OFFSET` alone is
/// unbounded), `ORDER BY` (sorts the whole set), `GROUP BY` / aggregates
/// (folds every row), or `DISTINCT` (a conservative first pass; counting
/// distinct rows early is legal but not done here). Because SPARQL leaves row
/// order unspecified without `ORDER BY`, returning the first `OFFSET + LIMIT`
/// solutions is a legal — and here byte-identical — selection.
fn select_solution_budget(
    distinct: bool,
    order_by: &[OrderCondition],
    group_by: &[GroupCondition],
    projection: &[ProjectionElement],
    offset: Option<u64>,
    limit: Option<u64>,
) -> Option<usize> {
    let limit = limit? as usize;
    if distinct || !order_by.is_empty() || !group_by.is_empty() {
        return None;
    }
    if projection.iter().any(elem_has_aggregate) {
        return None;
    }
    let offset = offset.map(|o| o as usize).unwrap_or(0);
    Some(offset.saturating_add(limit))
}

/// True if the same variable name appears in more than one position of the
/// triple pattern (subject/predicate/object plus the graph variable, when the
/// active graph is variable). Such repetition means a matched quad can be
/// dropped by the equality re-check in `eval_triple_pattern_core`, so a
/// quad-level `LIMIT` would under-produce and must not be applied.
fn pattern_repeats_variable(tp: &TriplePattern, active_graph: &ActiveGraph) -> bool {
    let mut names: Vec<&str> = Vec::with_capacity(4);
    for term in [&tp.subject, &tp.predicate, &tp.object] {
        if let Term::Variable(v) = term {
            names.push(v.as_str());
        }
    }
    if let ActiveGraph::Variable(v) = active_graph {
        names.push(v.as_str());
    }
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] {
                return true;
            }
        }
    }
    false
}

// ── BGP evaluation ────────────────────────────────────────────────────────────

fn eval_bgp(
    patterns: &[TriplePattern],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
) -> Vec<PartialSub> {
    let already_bound: HashSet<String> = solutions
        .first()
        .map(|sub| sub.keys().cloned().collect())
        .unwrap_or_default();
    let order = crate::join_ordering::order_patterns(patterns, &already_bound, datastore);

    let mut current = solutions;
    let last = order.len().saturating_sub(1);
    for (pos, &idx) in order.iter().enumerate() {
        let pattern = &patterns[idx];
        // Only the last-executed pattern produces the BGP's final output, so
        // only it may honour the row budget (issue #165). Earlier patterns
        // feed the join and must be fully materialised.
        let pat_budget = if pos == last { budget } else { None };
        current = match pat_budget {
            Some(b) => {
                // Accumulate across input solutions with a shrinking budget so
                // the total never exceeds `b`, preserving the exact prefix the
                // unbudgeted evaluation would have produced.
                let mut acc = Vec::new();
                for sub in current {
                    if acc.len() >= b {
                        break;
                    }
                    let remaining = b - acc.len();
                    acc.extend(eval_triple_pattern(
                        pattern,
                        &sub,
                        datastore,
                        active_graph,
                        Some(remaining),
                    ));
                }
                acc
            }
            None => current
                .into_iter()
                .flat_map(|sub| eval_triple_pattern(pattern, &sub, datastore, active_graph, None))
                .collect(),
        };
        if current.is_empty() {
            break;
        }
    }
    current
}

fn eval_triple_pattern(
    tp: &TriplePattern,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
) -> Vec<PartialSub> {
    // RDF 1.2 triple-term subject: `<<( s p o )>> pred obj`. Resolve the
    // embedded pattern against `reified_triples` first (yielding one or more
    // candidate triple-term `GraphElementId`s plus any bindings for
    // variables inside the embedded pattern), then evaluate the outer
    // pattern against `named_graphs` once per candidate, with the triple
    // term's own id fixed as the subject. See "Named-graph semantics for
    // triple terms" in `docs/plans/RDF12_PLAN.md`. Object-position triple
    // terms and nested triple terms are out of scope for phase R3 (#146);
    // see epic #143.
    if let Term::TripleTerm(inner) = &tp.subject {
        let mut results = Vec::new();
        for (term_id, inner_bindings) in triple_term_candidates(inner, sub, datastore) {
            let mut merged = sub.clone();
            let mut ok = true;
            for (var, val) in inner_bindings {
                match merged.get(&var) {
                    Some(existing) if !psv_eq(existing, &val, datastore) => {
                        ok = false;
                        break;
                    }
                    _ => {
                        merged.insert(var, val);
                    }
                }
            }
            if ok {
                // The triple-term subject path forces the subject and may fan
                // out over multiple candidates, so the quad-take budget is not
                // sound here — pass `None` and let the outer truncation apply.
                results.extend(eval_triple_pattern_core(
                    tp,
                    Some(term_id),
                    &merged,
                    datastore,
                    active_graph,
                    None,
                ));
            }
        }
        return results;
    }

    eval_triple_pattern_core(tp, None, sub, datastore, active_graph, budget)
}

/// Core outer-pattern evaluation shared by plain triple patterns and the
/// triple-term-subject case above. `forced_subject`, when `Some`, overrides
/// whatever `tp.subject` would otherwise resolve to (used when `tp.subject`
/// is a triple term already resolved to its own `GraphElementId`).
fn eval_triple_pattern_core(
    tp: &TriplePattern,
    forced_subject: Option<GraphElementId>,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
) -> Vec<PartialSub> {
    // If any constant in the pattern is absent from the store it can never match.
    for term in [&tp.subject, &tp.predicate, &tp.object] {
        if let Term::Constant(gel) = term {
            if !datastore.resources.resource_map.contains_key(gel) {
                return Vec::new();
            }
        }
    }

    let mut new_solutions = Vec::new();

    let g = match active_graph {
        ActiveGraph::Fixed(id) => Some(*id),
        ActiveGraph::Variable(v) => sub.get(v).and_then(|val| val.to_id(datastore)),
    };
    let s_match = match forced_subject {
        Some(id) => MatchTerm::Bound(id),
        None => resolve_match_term(&tp.subject, sub, datastore),
    };
    let p_match = resolve_match_term(&tp.predicate, sub, datastore);
    let o_match = resolve_match_term(&tp.object, sub, datastore);

    // A `Never` in any position (e.g. an unsupported triple-term shape) means
    // this pattern cannot match anything — bail out before it gets collapsed
    // to `None`, which `quads_matching` reads as "unconstrained wildcard".
    // This is the exact bug class behind #146/#153: see `MatchTerm`.
    if matches!(s_match, MatchTerm::Never)
        || matches!(p_match, MatchTerm::Never)
        || matches!(o_match, MatchTerm::Never)
    {
        return Vec::new();
    }
    let s = s_match.into_query_arg();
    let p = p_match.into_query_arg();
    let o = o_match.into_query_arg();

    // A row budget lets us stop enumerating matches early (issue #165). Pushing
    // the budget down as a *quad* limit (avoiding materialising every match)
    // is only sound when each matched quad yields exactly one solution — i.e.
    // the pattern has no repeated variable across its positions, which would
    // otherwise drop quads via the `ok` check below and under-produce. When
    // that gate fails we still cap the produced solutions, just after a full
    // scan.
    let quad_limit = match budget {
        Some(b) if forced_subject.is_none() && !pattern_repeats_variable(tp, active_graph) => {
            Some(b)
        }
        _ => None,
    };

    for quad in datastore.quads_matching_limited(g, s, p, o, quad_limit) {
        let mut new_sub = sub.clone();
        let mut ok = true;

        // Bind a variable to the interned ID of a matched quad field. Keeping
        // the `GraphElementId` (rather than materialising the `GraphElement`)
        // is the #141 hot-path win: no per-match clone/allocation.
        macro_rules! bind {
            ($term:expr, $id:expr) => {
                if let Term::Variable(v) = $term {
                    let new_val = PartialSubValue::Interned($id);
                    match new_sub.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), new_val);
                        }
                    }
                }
            };
        }

        bind!(&tp.subject, quad.subject);
        bind!(&tp.predicate, quad.predicate);
        bind!(&tp.object, quad.obj);

        if let ActiveGraph::Variable(graph_var) = active_graph {
            let new_val = PartialSubValue::Interned(quad.triple_id);
            match new_sub.get(graph_var) {
                Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                    ok = false;
                }
                _ => {
                    new_sub.insert(graph_var.clone(), new_val);
                }
            }
        }

        if ok {
            new_solutions.push(new_sub);
            // Always-safe backstop: we only ever need `budget` solutions from
            // this call, regardless of whether the quad-take gate applied.
            if let Some(b) = budget {
                if new_solutions.len() >= b {
                    break;
                }
            }
        }
    }
    new_solutions
}

/// Result of resolving a SPARQL [`Term`] against the current solution, for use
/// as one position (subject/predicate/object) of a `Datastore::quads_matching`
/// call.
///
/// This exists to keep two genuinely different outcomes from colliding on the
/// same `None`: a free variable that should match *anything*, versus a term
/// shape this evaluator cannot handle at all and that must therefore match
/// *nothing*. Collapsing both to `Option::None` is exactly what caused a real
/// bug: a triple term (`<<( ... )>>`) in predicate/object position — valid
/// syntax per the parser, but unsupported by the executor (#146) — degraded
/// to `None`, which `quads_matching` reads as "unconstrained", so the pattern
/// silently matched every quad instead of none. See #153 for the review that
/// found this. Every call site must check for `Never` before converting to
/// the `Option<GraphElementId>` shape `quads_matching` expects — there is no
/// implicit/accidental way to skip that check, unlike a bare `Option`.
enum MatchTerm {
    /// Resolves to a concrete, interned resource — constrains this position.
    Bound(GraphElementId),
    /// A genuinely free variable — matches anything in this position.
    Wildcard,
    /// This term shape can never match any quad (unsupported, or a constant
    /// that was never interned) — the whole pattern should short-circuit to
    /// zero results rather than silently drop the constraint.
    Never,
}

impl MatchTerm {
    /// Convert to the `Option<GraphElementId>` shape `quads_matching` expects.
    /// Panics on `Never` — every call site must check `matches!(_, MatchTerm::Never)`
    /// first and return an empty result instead of calling this.
    fn into_query_arg(self) -> Option<GraphElementId> {
        match self {
            MatchTerm::Bound(id) => Some(id),
            MatchTerm::Wildcard => None,
            MatchTerm::Never => {
                unreachable!("caller must check for MatchTerm::Never before converting")
            }
        }
    }
}

fn resolve_match_term(term: &Term, sub: &PartialSub, datastore: &Datastore) -> MatchTerm {
    match term {
        Term::Variable(v) => match sub.get(v) {
            // A binding straight from a quad is by construction interned — use
            // its id directly, no store lookup.
            Some(PartialSubValue::Interned(id)) => MatchTerm::Bound(*id),
            Some(PartialSubValue::Computed(gel)) => match datastore.resources.resource_map.get(gel)
            {
                Some(&id) => MatchTerm::Bound(id),
                // Bound to a computed value (e.g. a BIND arithmetic result)
                // that was never interned — that exact value structurally
                // cannot appear in any quad, so this must be `Never`, not an
                // unconstrained wildcard (#154). Every current call site
                // (the `bind!` macro below, the graph-variable recheck, and
                // `PropertyPath::NegatedSet`'s equivalent logic) happens to
                // re-verify this variable's binding against the matched
                // quad afterwards, so returning `Wildcard` here was already
                // filtered back down to zero rows in practice — this was a
                // latent/defensive-correctness and performance issue (an
                // avoidable unconstrained scan), not an observable
                // query-result bug. Returning `Never` directly avoids the
                // wasted scan and removes the risk entirely for any future
                // call site added without that recheck.
                None => MatchTerm::Never,
            },
            None => MatchTerm::Wildcard,
        },
        Term::Constant(gel) => match datastore.resources.resource_map.get(gel) {
            Some(&id) => MatchTerm::Bound(id),
            None => MatchTerm::Never,
        },
        // Triple terms are only handled specially in subject position (see
        // `eval_triple_pattern`); in predicate/object position — or as a
        // property-path endpoint, see `PropertyPath::NegatedSet` — they are
        // out of scope for phase R3 (#146) and must never match anything.
        Term::TripleTerm(_) => MatchTerm::Never,
    }
}

/// Enumerate candidate RDF 1.2 triple terms matching the embedded pattern
/// `inner` (the contents of `<<( s p o )>>`), looked up against
/// `reified_triples`.
///
/// Returns one `(triple_term_id, bindings)` pair per matching row, where
/// `bindings` holds the `GraphElement` values that free variables inside
/// `inner` must bind to. A fully-ground `inner` (all three positions already
/// resolvable from `sub` or as constants) yields at most one candidate, via
/// an exact structural lookup in `reified_triples` — no scan.
///
/// See "Named-graph semantics for triple terms" in
/// `docs/plans/RDF12_PLAN.md`.
fn triple_term_candidates(
    inner: &TriplePattern,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Vec<(GraphElementId, HashMap<String, PartialSubValue>)> {
    /// One position (subject/predicate/object) of the embedded pattern,
    /// resolved as far as possible against the current solution.
    enum Slot {
        /// Already resolvable to a concrete resource (constant, or a
        /// variable already bound in `sub`).
        Known(GraphElementId),
        /// A variable not yet bound; `reified_triples` will supply its value.
        Free(String),
        /// A constant, or an already-bound variable, whose value was never
        /// interned — this pattern can never match.
        Unmatchable,
    }

    fn resolve_slot(term: &Term, sub: &PartialSub, datastore: &Datastore) -> Slot {
        match term {
            Term::Constant(gel) => match datastore.resources.resource_map.get(gel) {
                Some(&id) => Slot::Known(id),
                None => Slot::Unmatchable,
            },
            Term::Variable(v) => match sub.get(v) {
                Some(val) => match val.to_id(datastore) {
                    Some(id) => Slot::Known(id),
                    None => Slot::Unmatchable,
                },
                None => Slot::Free(v.clone()),
            },
            // Nested triple terms inside an embedded pattern are deferred
            // (#146 / epic #143); not needed by any current test.
            Term::TripleTerm(_) => Slot::Unmatchable,
        }
    }

    let s_slot = resolve_slot(&inner.subject, sub, datastore);
    let p_slot = resolve_slot(&inner.predicate, sub, datastore);
    let o_slot = resolve_slot(&inner.object, sub, datastore);

    if matches!(s_slot, Slot::Unmatchable)
        || matches!(p_slot, Slot::Unmatchable)
        || matches!(o_slot, Slot::Unmatchable)
    {
        return Vec::new();
    }

    let quads: Vec<dag_rdf::Quad> = match (&s_slot, &p_slot, &o_slot) {
        (Slot::Known(s), Slot::Known(p), Slot::Known(o)) => {
            let key = GraphElement::TripleTerm(dag_rdf::TripleTermKey {
                subject: *s,
                predicate: *p,
                obj: *o,
            });
            match datastore.resources.resource_map.get(&key) {
                Some(&id) => vec![dag_rdf::Quad {
                    triple_id: id,
                    subject: *s,
                    predicate: *p,
                    obj: *o,
                }],
                None => Vec::new(),
            }
        }
        (Slot::Known(s), Slot::Known(p), Slot::Free(_)) => datastore
            .reified_triples
            .get_quads_with_subject_predicate(*s, *p)
            .collect(),
        (Slot::Free(_), Slot::Known(p), Slot::Known(o)) => datastore
            .reified_triples
            .get_quads_with_object_predicate(*o, *p)
            .collect(),
        (Slot::Known(s), Slot::Free(_), Slot::Known(o)) => datastore
            .reified_triples
            .get_quads_with_subject_object(*s, *o)
            .collect(),
        (Slot::Free(_), Slot::Known(p), Slot::Free(_)) => datastore
            .reified_triples
            .get_quads_with_predicate(*p)
            .collect(),
        (Slot::Known(s), Slot::Free(_), Slot::Free(_)) => datastore
            .reified_triples
            .get_quads_with_subject(*s)
            .collect(),
        (Slot::Free(_), Slot::Free(_), Slot::Known(o)) => datastore
            .reified_triples
            .get_quads_with_object(*o)
            .collect(),
        (Slot::Free(_), Slot::Free(_), Slot::Free(_)) => {
            datastore.reified_triples.get_all_quads().collect()
        }
        _ => unreachable!("Unmatchable combinations were already filtered out above"),
    };

    let mut out = Vec::new();
    for quad in quads {
        let mut bindings: HashMap<String, PartialSubValue> = HashMap::new();
        let mut ok = true;

        macro_rules! bind_free {
            ($slot:expr, $id:expr) => {
                if let Slot::Free(v) = $slot {
                    let new_val = PartialSubValue::Interned($id);
                    match bindings.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => ok = false,
                        _ => {
                            bindings.insert(v.clone(), new_val);
                        }
                    }
                }
            };
        }

        bind_free!(&s_slot, quad.subject);
        bind_free!(&p_slot, quad.predicate);
        bind_free!(&o_slot, quad.obj);

        if ok {
            out.push((quad.triple_id, bindings));
        }
    }
    out
}

// ── FILTER expression evaluation ──────────────────────────────────────────────

/// Evaluate a SPARQL expression as a boolean filter guard.
///
/// `sub` maps variable names to interned [`GraphElementId`]s — the same type as a
/// Datalog `Substitution`.  Returns `false` if the expression is unbound or errors.
///
/// Uses the default graph as the active graph (appropriate for Datalog rules,
/// which do not operate over named-graph scopes).
///
/// This is the bridge used by `datalog::RuleAtom::FilterAtom` to evaluate
/// SPARQL-style expression guards inside Datalog rule bodies.
pub fn eval_expr_as_filter(
    expr: &Expression,
    sub: &HashMap<String, GraphElementId>,
    datastore: &Datastore,
) -> bool {
    // The Datalog substitution already holds interned ids — carry them through
    // directly as `Interned` bindings (no `GraphElement` materialisation).
    let gel_sub: PartialSub = sub
        .iter()
        .map(|(var, &id)| (var.clone(), PartialSubValue::Interned(id)))
        .collect();
    eval_expression_bool(
        expr,
        &gel_sub,
        datastore,
        &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
    )
    .unwrap_or(false)
}

/// Evaluate a SPARQL expression as a boolean filter against the default graph.
///
/// `sub` maps variable names to their bound `GraphElement` values.  This is
/// the public entry point for downstream crates (e.g. `datalog`, `shacl`) that
/// need to test a SPARQL `Expression` guard without access to the internal
/// `ActiveGraph` type.  EXISTS / NOT EXISTS expressions use the default graph.
///
/// Returns `false` on evaluation error or when the expression is unbound.
/// See: <https://github.com/daghovland/rdf-datalog/issues/60>
pub fn eval_expression_bool_filter(
    expr: &Expression,
    sub: &SolutionRow,
    datastore: &Datastore,
) -> bool {
    eval_expression_bool(
        expr,
        &solution_row_to_partial(sub),
        datastore,
        &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
    )
    .unwrap_or(false)
}

fn eval_filter(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> bool {
    eval_expression_bool(expr, sub, datastore, active_graph).unwrap_or(false)
}

/// Evaluate an expression to a concrete GraphElement value.
///
/// `sub` maps variable names to their bound [`GraphElement`] values.  Constants
/// in the query (e.g. `"SPARQL"` in `regex(?x, "SPARQL")`) are returned directly
/// without touching the datastore.
///
/// Returns `None` when the expression is unbound or evaluation fails (e.g.
/// division by zero, type mismatch).
///
/// This is the public entry point for downstream crates; internally the
/// evaluator threads a `PartialSub` (which may hold interned ids) through
/// `eval_expression_value_inner`.
/// See: <https://github.com/daghovland/rdf-datalog/issues/60>
pub fn eval_expression_value(
    expr: &Expression,
    sub: &SolutionRow,
    datastore: &Datastore,
) -> Option<GraphElement> {
    eval_expression_value_inner(expr, &solution_row_to_partial(sub), datastore)
}

/// Evaluate an expression against an internal [`PartialSub`] solution.
///
/// `sub` maps variable names to their current bindings ([`PartialSubValue`]).
/// Returns `None` when the expression is unbound or evaluation fails.
fn eval_expression_value_inner(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match expr {
        Expression::Variable(v) => sub.get(v).map(|val| val.resolve(datastore)),
        Expression::Constant(gel) => Some(gel.clone()),
        Expression::FunctionCall(name, args) => eval_function_value(name, args, sub, datastore),
        Expression::Binary(
            l,
            op @ (BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div),
            r,
        ) => eval_arithmetic(l, op, r, sub, datastore),
        // Comparison (`=`, `!=`, `<`, `>`, `<=`, `>=`) and logical (`&&`,
        // `||`) operators don't produce a value of their own in
        // `eval_arithmetic` — they're boolean-valued. In a value-producing
        // context (projection, `BIND`) that boolean must still surface as
        // an `xsd:boolean` literal rather than silently evaluating to
        // nothing; delegate to the boolean evaluator (which already
        // normalizes numeric equality/ordering across literal
        // representations, see `values_equal`/`compare_graph_elements`) and
        // wrap the result. `EXISTS`/`NOT EXISTS` don't appear directly under
        // these operators here (they're handled by `eval_expression_bool`
        // itself against the default graph, matching `eval_bind_expr`'s and
        // `eval_expression_bool_filter`'s existing convention for
        // value/BIND contexts that have no `ActiveGraph` in scope).
        // See https://github.com/daghovland/rdf-datalog/issues/207.
        Expression::Binary(
            _,
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge
            | BinaryOp::And
            | BinaryOp::Or,
            _,
        )
        | Expression::Unary(UnaryOp::Not, _) => {
            let b = eval_expression_bool(
                expr,
                sub,
                datastore,
                &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
            )?;
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)))
        }
        Expression::Unary(UnaryOp::Plus, inner) => {
            eval_expression_value_inner(inner, sub, datastore)
        }
        Expression::Unary(UnaryOp::Minus, inner) => {
            arithmetic_negate(eval_expression_value_inner(inner, sub, datastore)?)
        }
        _ => None,
    }
}

/// Negate a numeric literal.
///
/// Uses `classify_numeric`/`numeric_lit_to_element` (rather than matching
/// each `RdfLiteral` variant by hand) so that: (1) a real `TypedLiteral{
/// xsd:decimal, .. }` input stays `xsd:decimal` after negation instead of
/// being promoted to `xsd:double` (the previous fallback for any non-integer
/// `TypedLiteral`), and (2) the result is emitted in the same `TypedLiteral`
/// shape real data uses, so it can join against already-interned data of
/// the same negated value. See <https://github.com/daghovland/rdf-datalog/issues/228>.
fn arithmetic_negate(el: GraphElement) -> Option<GraphElement> {
    let lit = match &el {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let negated = match classify_numeric(lit)? {
        NumericLit::Integer(n) => NumericLit::Integer(-n),
        NumericLit::Decimal(d) => NumericLit::Decimal(-d),
        NumericLit::Float(f) => NumericLit::Float(-f),
        NumericLit::Double(f) => NumericLit::Double(-f),
    };
    Some(numeric_lit_to_element(negated))
}

/// A numeric literal normalised to one of SPARQL/XPath's four numeric type
/// ranks — `xsd:integer` ⊂ `xsd:decimal` ⊂ `xsd:float` ⊂ `xsd:double`
/// (SPARQL 1.1 §17.1, "Operand Data Types") — regardless of which
/// `RdfLiteral` shape it arrived in.
enum NumericLit {
    Integer(BigInt),
    Decimal(rust_decimal::Decimal),
    Float(f64),
    Double(f64),
}

/// Classify a literal's numeric type and value.
///
/// Both the Turtle parser (`turtle::convert_literal`) and the SPARQL
/// numeric-literal parser (`parse_numeric_literal`, deliberately mirroring
/// it) always produce the generic `RdfLiteral::TypedLiteral { type_iri,
/// literal }` shape for numeric data. The canonical `IntegerLiteral` /
/// `DecimalLiteral` / `FloatLiteral` / `DoubleLiteral` variants are only ever
/// produced by aggregates (`SUM`/`COUNT`/`AVG`/etc., which cannot appear
/// inside `BIND` so never hit the join-lookup bug below) — every scalar
/// producer (`eval_arithmetic`, `arithmetic_negate`, `ABS`/`CEIL`/`FLOOR`/
/// `ROUND`, the xsd casts) goes through `numeric_lit_to_element` below to
/// emit the same `TypedLiteral` shape. Recognising only the canonical
/// variants here would mean arithmetic on any real data silently falls
/// through to `xsd:double` promotion, corrupting `1 + 1` into `2.0e0`. See
/// <https://github.com/daghovland/rdf-datalog/issues/207> (and the sibling
/// gap in <https://github.com/daghovland/rdf-datalog/issues/198>).
fn classify_numeric(lit: &RdfLiteral) -> Option<NumericLit> {
    match lit {
        RdfLiteral::IntegerLiteral(n) => Some(NumericLit::Integer(n.clone())),
        RdfLiteral::DecimalLiteral(d) => Some(NumericLit::Decimal(*d)),
        RdfLiteral::FloatLiteral(f) => Some(NumericLit::Float(f.into_inner())),
        RdfLiteral::DoubleLiteral(d) => Some(NumericLit::Double(d.into_inner())),
        RdfLiteral::TypedLiteral { type_iri, literal } => match type_iri.0.as_str() {
            XSD_INTEGER => literal.parse::<BigInt>().ok().map(NumericLit::Integer),
            XSD_DECIMAL => literal
                .parse::<rust_decimal::Decimal>()
                .ok()
                .map(NumericLit::Decimal),
            XSD_FLOAT => literal.parse::<f64>().ok().map(NumericLit::Float),
            XSD_DOUBLE => literal.parse::<f64>().ok().map(NumericLit::Double),
            _ => None,
        },
        _ => None,
    }
}

/// Reconstruct a classified numeric value as the `TypedLiteral { type_iri,
/// literal }` shape real parsed data always uses (see `classify_numeric`'s
/// doc comment above), rather than a producer-specific native `RdfLiteral`
/// variant.
///
/// A single normalization point for every scalar numeric producer
/// (`eval_arithmetic`, `arithmetic_negate`, `ABS`/`CEIL`/`FLOOR`/`ROUND`) so a
/// `BIND`-computed numeric value used in a later triple-pattern join
/// position (e.g. `BIND(ABS(?o) AS ?z) . ?s1 ?p1 ?z`) is looked up in
/// `resource_map` by structural equality (`resolve_match_term`) and actually
/// finds the already-interned resource, regardless of which function
/// produced it. Generalizes the integer-only version of this fix in
/// `eval_arithmetic` (W3C `bind03`,
/// <https://github.com/daghovland/rdf-datalog/issues/198>) to every other
/// producer — see <https://github.com/daghovland/rdf-datalog/issues/228>.
fn numeric_lit_to_element(n: NumericLit) -> GraphElement {
    let (type_iri, literal) = match n {
        NumericLit::Integer(i) => (XSD_INTEGER, i.to_string()),
        NumericLit::Decimal(d) => (XSD_DECIMAL, d.to_string()),
        NumericLit::Float(f) => (XSD_FLOAT, f.to_string()),
        NumericLit::Double(f) => (XSD_DOUBLE, f.to_string()),
    };
    GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(type_iri.to_string()),
        literal,
    })
}

/// Evaluate a binary arithmetic expression (Add/Sub/Mul/Div), applying the
/// SPARQL/XPath numeric type-promotion rules: the result takes the wider of
/// the two operand types (integer < decimal < float < double), so
/// `integer + integer` stays `integer`, `decimal + integer` becomes
/// `decimal`, and only an operand that is genuinely `xsd:float`/`xsd:double`
/// forces promotion to floating point.
/// Returns `None` if operands are not numeric or op is not arithmetic.
fn eval_arithmetic(
    left: &Expression,
    op: &BinaryOp,
    right: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let l = eval_expression_value_inner(left, sub, datastore)?;
    let r = eval_expression_value_inner(right, sub, datastore)?;
    let l_lit = match &l {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let r_lit = match &r {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let ln = classify_numeric(l_lit)?;
    let rn = classify_numeric(r_lit)?;

    // Exact fast path: integer op integer stays integer for Add/Sub/Mul.
    // `Div` is deliberately excluded: SPARQL/XPath's `op:numeric-divide`
    // always promotes an integer/integer division to `xsd:decimal`, even
    // when both operands are integers (e.g. `2/2` is `1.0`, not the
    // integer-division result `1`) — falls through to the decimal path
    // below instead. See W3C `coalesce01` (#205): a prior version used
    // truncating `BigInt` division and emitted `xsd:integer`, which both
    // computed the wrong value for non-exact quotients and used the wrong
    // datatype for exact ones.
    if let (NumericLit::Integer(a), NumericLit::Integer(b)) = (&ln, &rn) {
        if !matches!(op, BinaryOp::Div) {
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                _ => return None,
            };
            // Emit the same `TypedLiteral { type_iri, literal }` shape the
            // Turtle and SPARQL numeric-literal parsers always produce for
            // real data (see `classify_numeric`'s doc comment above), via
            // `numeric_lit_to_element`, rather than the canonical
            // `IntegerLiteral` variant. A `BIND`-computed value used in a
            // later triple-pattern position (e.g. `BIND(?o+1 AS ?z) . ?s1
            // ?p1 ?z`) is looked up in `resource_map` by structural equality
            // (`resolve_match_term`); an `IntegerLiteral` never structurally
            // equals the `TypedLiteral` shape under which the same value was
            // actually interned, so the lookup silently failed and the join
            // produced zero rows regardless of whether the value was
            // genuinely present. See W3C `bind03` and
            // <https://github.com/daghovland/rdf-datalog/issues/198>.
            return Some(numeric_lit_to_element(NumericLit::Integer(result)));
        }
        // `Div`: fall through to the decimal path below (see comment above).
    }

    // A genuinely `xsd:double` operand forces double-precision arithmetic.
    // See #228: the result must use `numeric_lit_to_element`, not the
    // native `DoubleLiteral` variant, for the same join-lookup reason as the
    // integer fast path above.
    if matches!(ln, NumericLit::Double(_)) || matches!(rn, NumericLit::Double(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Double(result)));
    }

    // A genuinely `xsd:float` operand (with no double present) forces
    // float-precision arithmetic. See #228, as above.
    if matches!(ln, NumericLit::Float(_)) || matches!(rn, NumericLit::Float(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Float(result)));
    }

    // Remaining case: an integer/decimal mix with at least one decimal
    // operand — exact decimal arithmetic, result stays decimal. See #228,
    // as above.
    let ad = numeric_lit_to_decimal(&ln)?;
    let bd = numeric_lit_to_decimal(&rn)?;
    let result = match op {
        BinaryOp::Add => ad + bd,
        BinaryOp::Sub => ad - bd,
        BinaryOp::Mul => ad * bd,
        BinaryOp::Div => {
            if bd.is_zero() {
                return None;
            }
            ad / bd
        }
        _ => return None,
    };
    Some(numeric_lit_to_element(NumericLit::Decimal(result)))
}

/// Widen a classified numeric literal to `f64` for float/double arithmetic.
fn numeric_lit_to_f64(n: &NumericLit) -> f64 {
    match n {
        NumericLit::Integer(i) => i.to_string().parse().unwrap_or(f64::NAN),
        NumericLit::Decimal(d) => d.to_string().parse().unwrap_or(f64::NAN),
        NumericLit::Float(f) | NumericLit::Double(f) => *f,
    }
}

/// Widen a classified integer/decimal numeric literal to `Decimal` for exact
/// decimal arithmetic. Not meaningful for `Float`/`Double` — callers only
/// reach this after ruling both out.
fn numeric_lit_to_decimal(n: &NumericLit) -> Option<rust_decimal::Decimal> {
    match n {
        NumericLit::Integer(i) => i.to_string().parse().ok(),
        NumericLit::Decimal(d) => Some(*d),
        NumericLit::Float(_) | NumericLit::Double(_) => None,
    }
}

/// Apply an arithmetic `BinaryOp` to two `f64` operands.
fn apply_f64_op(op: &BinaryOp, a: f64, b: f64) -> Option<f64> {
    Some(match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => {
            if b == 0.0 {
                return None;
            }
            a / b
        }
        _ => return None,
    })
}

/// Shared implementation for the boolean-valued string predicates
/// `STRSTARTS`, `STRENDS`, and `CONTAINS` (SPARQL 1.1 §17.4.3).
///
/// Used by both `eval_function_value` (for `BIND`/projection contexts,
/// wrapped in a `BooleanLiteral`) and `eval_function_bool` (for direct
/// `FILTER` contexts) so the two dispatch paths cannot diverge.
fn eval_string_predicate(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<bool> {
    let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let text = graph_element_to_string(&text_el)?;
    let arg_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let arg = graph_element_to_string(&arg_el)?;
    match name {
        "STRSTARTS" => Some(text.starts_with(arg.as_str())),
        "STRENDS" => Some(text.ends_with(arg.as_str())),
        "CONTAINS" => Some(text.contains(arg.as_str())),
        _ => None,
    }
}

fn eval_function_value(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    // XSD datatype constructor/cast functions (SPARQL 1.1 §17.4.2). Unlike the
    // bare-keyword builtins below, these arrive as the function name's
    // *resolved* IRI: `xsd:integer(...)` is parsed via `parse_prefixed_name`
    // (or `<http://...#integer>(...)` via `parse_iri_ref`), never as the bare
    // word `xsd:integer` (see #186/PR #189), so dispatch matches the full IRI
    // rather than joining the uppercase-keyword match below.
    if matches!(
        name,
        XSD_STRING
            | XSD_BOOLEAN
            | XSD_INTEGER
            | XSD_DECIMAL
            | XSD_DOUBLE
            | XSD_FLOAT
            | XSD_DATE_TIME
    ) {
        let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
        return eval_xsd_cast(name, &el);
    }
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "STRSTARTS" | "STRENDS" | "CONTAINS" => {
            let b = eval_string_predicate(upper.as_str(), args, sub, datastore)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)))
        }
        // STRBEFORE/STRAFTER (SPARQL 1.1 §17.4.3.13/14, `fn:substring-before`/
        // `fn:substring-after`): the result carries arg1's simple/lang/
        // xsd:string tag regardless of whether the separator is found, and
        // the two operands must be "argument compatible" (§17.1) — arg2 may
        // be a simple literal or `xsd:string` (compatible with anything), or
        // must share arg1's exact language tag; any other combination
        // (e.g. arg1 plain, arg2 language-tagged) is an error. A prior
        // implementation always emitted a plain simple literal and never
        // checked compatibility, failing the "datatyping" W3C fixtures (#205).
        "STRBEFORE" => {
            let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let (text, tag1) = literal_str_tag(&text_el)?;
            let sep_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let (sep, tag2) = literal_str_tag(&sep_el)?;
            if !str_args_compatible(&tag1, &tag2) {
                return None;
            }
            // Per the W3C-approved `strbefore01a`/`strafter01a` revision: an
            // empty *separator* still yields arg1's tag (§17.4.3.13's
            // explicit empty-`B` case), but a separator that simply isn't
            // found in the text falls back to an untagged plain empty
          // literal, discarding arg1's tag — a distinct case from "found
            // an empty match". A prior version applied arg1's tag to both.
            if sep.is_empty() {
                return Some(str_tag_to_element(String::new(), tag1));
            }
            match text.find(sep.as_str()) {
                Some(idx) => Some(str_tag_to_element(text[..idx].to_string(), tag1)),
                None => Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    String::new(),
                ))),
            }
        }
        "STRAFTER" => {
            let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let (text, tag1) = literal_str_tag(&text_el)?;
            let sep_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let (sep, tag2) = literal_str_tag(&sep_el)?;
            if !str_args_compatible(&tag1, &tag2) {
                return None;
            }
            // See `STRBEFORE`'s comment above: empty separator preserves
            // arg1's tag (returns arg1 unchanged), but "not found" falls back
            // to an untagged plain empty literal.
            if sep.is_empty() {
                return Some(str_tag_to_element(text.clone(), tag1));
            }
            match text.find(sep.as_str()) {
                Some(idx) => Some(str_tag_to_element(
                    text[idx + sep.len()..].to_string(),
                    tag1,
                )),
                None => Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    String::new(),
                ))),
            }
        }
        "STR" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)))
        }
        "LANG" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            if let GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) = el {
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(lang)))
            } else {
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    String::new(),
                )))
            }
        }
        "STRLEN" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = match &el {
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s.clone(),
                GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
                    literal.clone()
                }
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
                    literal.clone()
                }
                _ => return None,
            };
            let len = s.chars().count();
            Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                type_iri: IriReference(XSD_INTEGER.to_string()),
                literal: len.to_string(),
            }))
        }
        "DATATYPE" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let dt_iri = match &el {
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, .. }) => {
                    type_iri.0.clone()
                }
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(_)) => XSD_STRING.to_string(),
                GraphElement::GraphLiteral(RdfLiteral::LangLiteral { .. }) => {
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString".to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(_)) => {
                    XSD_BOOLEAN.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(_)) => {
                    XSD_INTEGER.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(_)) => {
                    XSD_DECIMAL.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::FloatLiteral(_)) => XSD_FLOAT.to_string(),
                GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(_)) => XSD_DOUBLE.to_string(),
                // `NOW()` produces a native `DateTimeLiteral` (see its own
                // comment below), so `DATATYPE(NOW())` must recognise it too
                // rather than falling through to `None` — otherwise
                // `FILTER(DATATYPE(?n) = xsd:dateTime)` always fails (W3C
                // `now01`, #205).
                GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(_)) => {
                    ingress::XSD_DATE_TIME.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::DateLiteral(_)) => {
                    ingress::XSD_DATE.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::TimeLiteral(_)) => {
                    ingress::XSD_TIME.to_string()
                }
                _ => return None,
            };
            Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
                IriReference(dt_iri),
            )))
        }
        // ── String functions ──────────────────────────────────────────────────
        // UCASE/LCASE/SUBSTR (SPARQL 1.1 §17.4.3.7/8/10) preserve the
        // operand's simple/lang/xsd:string tag on output — a prior version
        // always emitted a plain simple literal, dropping `@lang`/
        // `^^xsd:string`, and failed every W3C fixture using a tagged
        // operand (#205). Falls back to the untagged `graph_element_to_string`
        // path for any other literal shape (numbers, IRIs, etc.) that isn't
        // strictly a string literal per spec but which earlier callers may
        // still rely on.
        "UCASE" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            if let Some((s, tag)) = literal_str_tag(&el) {
                Some(str_tag_to_element(s.to_uppercase(), tag))
            } else {
                let s = graph_element_to_string(&el)?;
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    s.to_uppercase(),
                )))
            }
        }
        "LCASE" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            if let Some((s, tag)) = literal_str_tag(&el) {
                Some(str_tag_to_element(s.to_lowercase(), tag))
            } else {
                let s = graph_element_to_string(&el)?;
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    s.to_lowercase(),
                )))
            }
        }
        // CONCAT (SPARQL 1.1 §17.4.3.9, `fn:concat` with CONCAT's own
        // datatyping addendum): the result is `xsd:string` if every argument
        // is `xsd:string`; a shared language tag if every argument carries
        // that same language tag; otherwise a plain simple literal. Any
        // non-string-literal argument (e.g. an integer) is an error.
        "CONCAT" => {
            let mut result = String::new();
            let mut tags = Vec::with_capacity(args.len());
            for arg in args {
                let el = eval_expression_value_inner(arg, sub, datastore)?;
                let (s, tag) = literal_str_tag(&el)?;
                result.push_str(&s);
                tags.push(tag);
            }
            let out_tag = if !tags.is_empty() && tags.iter().all(|t| *t == StrLitTag::XsdString) {
                StrLitTag::XsdString
            } else if let Some(StrLitTag::Lang(first_lang)) = tags.first() {
                if tags
                    .iter()
                    .all(|t| matches!(t, StrLitTag::Lang(l) if l == first_lang))
                {
                    StrLitTag::Lang(first_lang.clone())
                } else {
                    StrLitTag::Plain
                }
            } else {
                StrLitTag::Plain
            };
            Some(str_tag_to_element(result, out_tag))
        }
        "SUBSTR" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let (text, tag) = literal_str_tag(&el)
                .or_else(|| graph_element_to_string(&el).map(|s| (s, StrLitTag::Plain)))?;
            let s: Vec<char> = text.chars().collect();
            let start_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let start: usize = element_to_usize(&start_el)?.saturating_sub(1);
            let result: String = if let Some(len_expr) = args.get(2) {
                let len_el = eval_expression_value_inner(len_expr, sub, datastore)?;
                let len: usize = element_to_usize(&len_el)?;
                s.iter().skip(start).take(len).collect()
            } else {
                s.iter().skip(start).collect()
            };
            Some(str_tag_to_element(result, tag))
        }
        // ── Term construction ─────────────────────────────────────────────────
        "IRI" | "URI" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let iri_str = graph_element_to_string(&el)?;
            Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
                IriReference(iri_str),
            )))
        }
        // STRDT/STRLANG (SPARQL 1.1 §17.4.3.5/6, `fn:strdt`/`STRLANG`)
        // require their first argument to be a *simple* literal — no
        // language tag, no datatype (not even `xsd:string`) — and error
        // otherwise. A prior implementation accepted any literal (or even an
        // IRI) via `graph_element_to_string`, silently succeeding on
        // already-typed/lang-tagged/non-literal input where the spec
        // mandates an error (W3C `strdt03`/`strlang03`, #205).
        "STRDT" => {
            let lex_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let literal = match lex_el {
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s,
                _ => return None,
            };
            let dt_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let type_iri = match dt_el {
                GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(iri)) => iri,
                _ => return None,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                type_iri,
                literal,
            }))
        }
        "STRLANG" => {
            let lex_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let literal = match lex_el {
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s,
                _ => return None,
            };
            let lang_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let lang = graph_element_to_string(&lang_el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
                lang,
                literal,
            }))
        }
        // ── Type testing ──────────────────────────────────────────────────────
        "ISNUMERIC" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let is_numeric = match &el {
                GraphElement::GraphLiteral(
                    RdfLiteral::IntegerLiteral(_)
                    | RdfLiteral::DecimalLiteral(_)
                    | RdfLiteral::DoubleLiteral(_)
                    | RdfLiteral::FloatLiteral(_),
                ) => true,
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, .. }) => {
                    matches!(
                        type_iri.0.as_str(),
                        XSD_INTEGER | XSD_DECIMAL | XSD_DOUBLE | XSD_FLOAT
                    )
                }
                _ => false,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
                is_numeric,
            )))
        }
        "SAMETERM" => {
            let a = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let b = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
                a == b,
            )))
        }
        // ── Numeric functions ─────────────────────────────────────────────────
        //
        // ABS/CEIL/FLOOR/ROUND all go through `classify_numeric` for their
        // input (rather than matching `RdfLiteral` variants by hand) and
        // `numeric_lit_to_element` for their output. This matters on both
        // ends (#228): `classify_numeric` recognizes a real `TypedLiteral{
        // xsd:decimal/xsd:float/xsd:double, .. }` input for what it actually
        // is instead of falling through to an `xsd:double`-promoting
        // catch-all (the bug `ABS` had — a real `xsd:decimal` input silently
        // became `xsd:double` output), and `numeric_lit_to_element` emits the
        // `TypedLiteral` shape real data uses so the result can join against
        // already-interned data of the same value.
        "ABS" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let lit = match &el {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            };
            let abs = match classify_numeric(lit)? {
                NumericLit::Integer(n) => {
                    NumericLit::Integer(if n < BigInt::from(0) { -n } else { n })
                }
                NumericLit::Decimal(d) => NumericLit::Decimal(d.abs()),
                NumericLit::Float(f) => NumericLit::Float(f.abs()),
                NumericLit::Double(f) => NumericLit::Double(f.abs()),
            };
            Some(numeric_lit_to_element(abs))
        }
        // CEIL/FLOOR/ROUND preserve the operand's numeric type (SPARQL 1.1
        // §17.4.5's `fn:round`/`fn:ceiling`/`fn:floor` semantics): an
        // `xsd:integer` input passes through unchanged, an `xsd:decimal`
        // input stays `xsd:decimal` (rounded to a whole-number *value*, not
        // cast to `xsd:integer` — e.g. `ROUND("-1.6"^^xsd:decimal)` is
        // `"-2"^^xsd:decimal`, not `"-2"^^xsd:integer`), and float/double
        // stay float/double. An earlier version always promoted the result
        // to `xsd:integer` regardless of input type, which failed the W3C
        // `round01`/`ceil01`/`floor01` fixtures on exact-datatype comparison
        // (#205). An already-integer input is passed through exactly via
        // `classify_numeric` rather than round-tripping through `f64`
        // (avoiding precision loss for values outside `f64`'s exact integer
        // range).
        "ROUND" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let lit = match &el {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            };
            match classify_numeric(lit)? {
                NumericLit::Integer(n) => Some(numeric_lit_to_element(NumericLit::Integer(n))),
                NumericLit::Decimal(d) => Some(numeric_lit_to_element(NumericLit::Decimal(
                    d.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero),
                ))),
                NumericLit::Float(f) => Some(numeric_lit_to_element(NumericLit::Float(
                    (f + 0.5 * f.signum()).trunc(),
                ))),
                NumericLit::Double(f) => Some(numeric_lit_to_element(NumericLit::Double(
                    (f + 0.5 * f.signum()).trunc(),
                ))),
            }
        }
        "CEIL" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let lit = match &el {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            };
            match classify_numeric(lit)? {
                NumericLit::Integer(n) => Some(numeric_lit_to_element(NumericLit::Integer(n))),
                NumericLit::Decimal(d) => Some(numeric_lit_to_element(NumericLit::Decimal(
                    d.ceil(),
                ))),
                NumericLit::Float(f) => {
                    Some(numeric_lit_to_element(NumericLit::Float(f.ceil())))
                }
                NumericLit::Double(f) => {
                    Some(numeric_lit_to_element(NumericLit::Double(f.ceil())))
                }
            }
        }
        "FLOOR" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let lit = match &el {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            };
            match classify_numeric(lit)? {
                NumericLit::Integer(n) => Some(numeric_lit_to_element(NumericLit::Integer(n))),
                NumericLit::Decimal(d) => Some(numeric_lit_to_element(NumericLit::Decimal(
                    d.floor(),
                ))),
                NumericLit::Float(f) => {
                    Some(numeric_lit_to_element(NumericLit::Float(f.floor())))
                }
                NumericLit::Double(f) => {
                    Some(numeric_lit_to_element(NumericLit::Double(f.floor())))
                }
            }
        }
        // ── Logic / control ───────────────────────────────────────────────────
        "COALESCE" => args
            .iter()
            .find_map(|arg| eval_expression_value_inner(arg, sub, datastore)),
        "IF" => {
            let cond_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let cond = element_to_bool(&cond_el)?;
            if cond {
                eval_expression_value_inner(args.get(1)?, sub, datastore)
            } else {
                eval_expression_value_inner(args.get(2)?, sub, datastore)
            }
        }
        // ── Blank nodes ───────────────────────────────────────────────────────
        "BNODE" => {
            // BNODE() or BNODE(str): produce a fresh anonymous blank node.
            // The optional string argument (a label hint) is intentionally
            // ignored; we always return a freshly minted ID.
            use std::sync::atomic::{AtomicU32, Ordering};
            static BNODE_COUNTER: AtomicU32 = AtomicU32::new(0);
            let id = BNODE_COUNTER.fetch_add(1, Ordering::Relaxed);
            Some(GraphElement::NodeOrEdge(
                dag_rdf::RdfResource::AnonymousBlankNode(id),
            ))
        }
        // ── String functions (continued) ──────────────────────────────────────
        "ENCODE_FOR_URI" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            let mut out = String::new();
            for byte in s.bytes() {
                match byte {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        out.push(byte as char);
                    }
                    _ => out.push_str(&format!("%{byte:02X}")),
                }
            }
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(out)))
        }
        // REPLACE (SPARQL 1.1 §17.4.3.15, `fn:replace`) requires its subject
        // to be a genuine string literal (errors on e.g. a numeric operand —
        // W3C `replace01`'s `:s7` case) and preserves that operand's
        // simple/lang/xsd:string tag on output (#205).
        "REPLACE" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let (s, tag) = literal_str_tag(&el)?;
            let pat_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let pat = graph_element_to_string(&pat_el)?;
            let rep_el = eval_expression_value_inner(args.get(2)?, sub, datastore)?;
            let rep = graph_element_to_string(&rep_el)?;
            let flags = if let Some(flag_expr) = args.get(3) {
                if let Some(f_el) = eval_expression_value_inner(flag_expr, sub, datastore) {
                    graph_element_to_string(&f_el).unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let pattern = if flags.contains('i') {
                format!("(?i){pat}")
            } else {
                pat
            };
            let re = regex::Regex::new(&pattern).ok()?;
            Some(str_tag_to_element(
                re.replace_all(&s, rep.as_str()).into_owned(),
                tag,
            ))
        }
        // ── Numeric functions (random) ────────────────────────────────────────
        "RAND" => {
            use rand::Rng;
            let v: f64 = rand::thread_rng().gen();
            Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                v.into(),
            )))
        }
        // ── Datetime functions ────────────────────────────────────────────────
        //
        // YEAR/MONTH/DAY/HOURS/MINUTES/SECONDS weren't in #228's enumerated
        // scope, but are the exact same producer/lookup bug: extracting a
        // date/time component and emitting a native `IntegerLiteral`/
        // `DecimalLiteral` instead of `TypedLiteral` via
        // `numeric_lit_to_element` would mean `BIND(YEAR(?d) AS ?z) . ?s :p
        // ?z` fails to join for the same structural-inequality reason ABS
        // did. Fixed here too rather than left to resurface as issue #4 of
        // the same recurring pattern (see #228's "recurring pattern"
        // section). `NOW()`'s native `DateTimeLiteral` is deliberately left
        // as-is: its value is the current instant, which cannot coincide
        // with already-interned data, so the join-lookup bug can't manifest.
        "NOW" => Some(GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(
            chrono::Utc::now(),
        ))),
        "YEAR" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Datelike;
            Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
                dt.year(),
            ))))
        }
        "MONTH" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Datelike;
            Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
                dt.month(),
            ))))
        }
        "DAY" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Datelike;
            Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
                dt.day(),
            ))))
        }
        "HOURS" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime_local(&el)?;
            use chrono::Timelike;
            Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
                dt.hour(),
            ))))
        }
        "MINUTES" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime_local(&el)?;
            use chrono::Timelike;
            Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
                dt.minute(),
            ))))
        }
        "SECONDS" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime_local(&el)?;
            use chrono::Timelike;
            Some(numeric_lit_to_element(NumericLit::Decimal(
                rust_decimal::Decimal::from(dt.second()),
            )))
        }
        "TZ" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let tz_str = extract_tz_string(&el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                tz_str,
            )))
        }
        // `TIMEZONE()` (SPARQL 1.1 §17.4.4, `fn:timezone-from-dateTime`)
        // differs from `TZ()`: it returns an `xsd:dayTimeDuration` value
        // (e.g. `"-PT8H"`, `"PT0S"`) and, per the spec, is an *error* (so the
        // whole expression is unbound) when the operand has no timezone —
        // whereas `TZ()` returns the empty string for that case. Genuinely
        // missing prior to #205 (only `TZ` existed).
        "TIMEZONE" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let tz_str = extract_tz_string(&el)?;
            if tz_str.is_empty() {
                return None;
            }
            let (sign, hh, mm) = if tz_str == "Z" {
                ('+', 0i64, 0i64)
            } else {
                let sign = if tz_str.starts_with('-') { '-' } else { '+' };
                let rest = &tz_str[1..];
                let mut parts = rest.split(':');
                let hh: i64 = parts.next()?.parse().ok()?;
                let mm: i64 = parts.next().unwrap_or("0").parse().ok()?;
                (sign, hh, mm)
            };
            let duration = if hh == 0 && mm == 0 {
                "PT0S".to_string()
            } else if mm == 0 {
                format!("{sign}PT{hh}H")
            } else {
                format!("{sign}PT{hh}H{mm}M")
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                type_iri: IriReference(
                    "http://www.w3.org/2001/XMLSchema#dayTimeDuration".to_string(),
                ),
                literal: duration,
            }))
        }
        // ── Hash functions ────────────────────────────────────────────────────
        "MD5" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            let hash = md5::compute(s.as_bytes());
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                format!("{hash:x}"),
            )))
        }
        "SHA1" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            use sha1::Digest;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                hex::encode(sha1::Sha1::digest(s.as_bytes())),
            )))
        }
        "SHA256" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            use sha2::Digest;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                hex::encode(sha2::Sha256::digest(s.as_bytes())),
            )))
        }
        "SHA384" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            use sha2::Digest;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                hex::encode(sha2::Sha384::digest(s.as_bytes())),
            )))
        }
        "SHA512" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            use sha2::Digest;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                hex::encode(sha2::Sha512::digest(s.as_bytes())),
            )))
        }
        // ── UUID functions ────────────────────────────────────────────────────
        "UUID" => {
            let id = uuid::Uuid::new_v4();
            Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
                IriReference(format!("urn:uuid:{id}")),
            )))
        }
        "STRUUID" => {
            let id = uuid::Uuid::new_v4();
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                id.to_string(),
            )))
        }
        _ => None,
    }
}

/// Parse an `xsd:dateTime` or `xsd:date` lexical form into a `chrono::DateTime<Utc>`.
/// Accepts full RFC 3339 (`xsd:dateTime` with a timezone/`Z`), a timezone-less
/// `xsd:dateTime` lexical form (naive datetime, assumed UTC — RFC 3339 alone
/// requires an offset but the XSD dateTime lexical space does not), and
/// `xsd:date` (`YYYY-MM-DD`, normalized to midnight UTC). Shared by
/// `parse_xsd_datetime` (which additionally falls back to bare `xsd:gYear`
/// for `YEAR`/`MONTH`/`DAY`) and the `xsd:dateTime` cast (`cast_to_xsd_datetime`),
/// which intentionally does NOT get the gYear fallback — a bare year is not a
/// valid cast source per the XPath casting rules (see #194).
fn parse_datetime_or_date_lexical(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    // Full RFC 3339 dateTime (requires a timezone offset or 'Z').
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // xsd:dateTime lexical form without a timezone (naive; assumed UTC).
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(ndt.and_utc());
    }
    // xsd:date (YYYY-MM-DD)
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).map(|ndt| ndt.and_utc());
    }
    None
}

/// Parse an XSD dateTime (or date/gYear) graph element into a `chrono::DateTime<Utc>`.
/// Handles `DateTimeLiteral`, RFC 3339 `xsd:dateTime` strings, `xsd:date` (YYYY-MM-DD),
/// and `xsd:gYear` (YYYY) so that YEAR/MONTH/DAY work on all common date types.
fn parse_xsd_datetime(el: &GraphElement) -> Option<chrono::DateTime<chrono::Utc>> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(dt)) => Some(*dt),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            if let Some(dt) = parse_datetime_or_date_lexical(literal) {
                return Some(dt);
            }
            // xsd:gYear ("YYYY")
            if let Ok(y) = literal.parse::<i32>() {
                return chrono::NaiveDate::from_ymd_opt(y, 1, 1)
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .map(|ndt| ndt.and_utc());
            }
            None
        }
        _ => None,
    }
}

/// Parse an XSD dateTime graph element into a `chrono::DateTime<FixedOffset>`
/// that preserves the *lexical* timezone offset instead of normalising to
/// UTC. SPARQL 1.1 §17.4.4's `HOURS`/`MINUTES`/`SECONDS` (`fn:hours-from-dateTime`
/// etc.) report the time-of-day components as written in the source literal,
/// not shifted to UTC — e.g. `HOURS("2010-12-21T15:38:02-08:00"^^xsd:dateTime)`
/// is `15`, not `23`. `parse_xsd_datetime`'s `with_timezone(&Utc)` conversion
/// is correct for `YEAR`/`MONTH`/`DAY` in every W3C fixture (none of them
/// cross a date boundary under UTC normalisation) but silently breaks HOURS
/// whenever the offset is non-zero (W3C `hours-01`, #205). A native
/// `DateTimeLiteral` (produced only by `NOW()`) has no separate offset to
/// preserve, so it is treated as UTC (offset `+00:00`).
fn parse_xsd_datetime_local(el: &GraphElement) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(dt)) => Some(dt.fixed_offset()),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(literal) {
                return Some(dt);
            }
            // Timezone-less xsd:dateTime lexical form: treat as UTC.
            if let Ok(ndt) =
                chrono::NaiveDateTime::parse_from_str(literal, "%Y-%m-%dT%H:%M:%S%.f")
            {
                return Some(ndt.and_utc().fixed_offset());
            }
            None
        }
        _ => None,
    }
}

/// Extract the timezone string from an XSD dateTime graph element.
/// Returns `"Z"` for UTC, `"+HH:MM"` / `"-HH:MM"` for fixed offsets, and
/// `""` for naive (no-timezone) values.
fn extract_tz_string(el: &GraphElement) -> Option<String> {
    let raw = match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(_)) => return Some("Z".to_string()),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => literal.as_str(),
        _ => return None,
    };
    if raw.ends_with('Z') {
        return Some("Z".to_string());
    }
    // After the 'T' separator the time portion is HH:MM:SS[.frac].
    // A timezone offset ('+'/'-') can only appear after the seconds.
    if let Some(t_pos) = raw.find('T') {
        let after_t = &raw[t_pos + 1..];
        for (i, c) in after_t.char_indices() {
            if i >= 5 && (c == '+' || c == '-') {
                return Some(after_t[i..].to_string());
            }
        }
    }
    Some(String::new())
}

fn eval_expression_bool(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Option<bool> {
    match expr {
        Expression::Binary(left, op, right) => match op {
            BinaryOp::And => {
                let l = eval_expression_bool(left, sub, datastore, active_graph).unwrap_or(false);
                let r = eval_expression_bool(right, sub, datastore, active_graph).unwrap_or(false);
                Some(l && r)
            }
            BinaryOp::Or => {
                let l = eval_expression_bool(left, sub, datastore, active_graph).unwrap_or(false);
                let r = eval_expression_bool(right, sub, datastore, active_graph).unwrap_or(false);
                Some(l || r)
            }
            BinaryOp::Eq => {
                let l = eval_expression_value_inner(left, sub, datastore)?;
                let r = eval_expression_value_inner(right, sub, datastore)?;
                Some(values_equal(&l, &r))
            }
            BinaryOp::Ne => {
                let l = eval_expression_value_inner(left, sub, datastore)?;
                let r = eval_expression_value_inner(right, sub, datastore)?;
                Some(!values_equal(&l, &r))
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                let l = eval_expression_value_inner(left, sub, datastore)?;
                let r = eval_expression_value_inner(right, sub, datastore)?;
                let ord = compare_graph_elements(&l, &r)?;
                Some(match op {
                    BinaryOp::Lt => ord < 0,
                    BinaryOp::Gt => ord > 0,
                    BinaryOp::Le => ord <= 0,
                    BinaryOp::Ge => ord >= 0,
                    _ => unreachable!(),
                })
            }
            _ => None,
        },
        Expression::Unary(UnaryOp::Not, inner) => {
            Some(!eval_expression_bool(inner, sub, datastore, active_graph).unwrap_or(false))
        }
        Expression::In(expr, list) => {
            let val = eval_expression_value_inner(expr, sub, datastore)?;
            Some(list.iter().any(|item| {
                eval_expression_value_inner(item, sub, datastore)
                    .map(|v| values_equal(&v, &val))
                    .unwrap_or(false)
            }))
        }
        Expression::NotIn(expr, list) => {
            let val = eval_expression_value_inner(expr, sub, datastore)?;
            Some(!list.iter().any(|item| {
                eval_expression_value_inner(item, sub, datastore)
                    .map(|v| values_equal(&v, &val))
                    .unwrap_or(false)
            }))
        }
        Expression::FunctionCall(name, args) => eval_function_bool(name, args, sub, datastore),
        Expression::Exists(inner) => {
            let sols =
                eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
            Some(!sols.is_empty())
        }
        Expression::NotExists(inner) => {
            let sols =
                eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
            Some(sols.is_empty())
        }
        _ => {
            let el = eval_expression_value_inner(expr, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => Some(b),
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                    ref type_iri,
                    ref literal,
                }) if type_iri.0 == XSD_BOOLEAN => Some(literal == "true"),
                _ => None,
            }
        }
    }
}

fn eval_function_bool(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<bool> {
    // `xsd:boolean(v)` used directly in a boolean context (e.g.
    // `FILTER(xsd:boolean(?x))`). The other XSD cast targets (integer,
    // decimal, double, float, string) don't produce a boolean value, so — per
    // this codebase's existing (narrow, non-EBV-coercing) boolean-context
    // conventions, see `element_to_bool` — they're intentionally left
    // unhandled here rather than inventing a general effective-boolean-value
    // coercion just for casts.
    if name == XSD_BOOLEAN {
        let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
        let cast = cast_to_xsd_boolean(&el)?;
        return element_to_bool(&cast);
    }
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "STRSTARTS" | "STRENDS" | "CONTAINS" => {
            eval_string_predicate(upper.as_str(), args, sub, datastore)
        }
        "BOUND" => {
            if let Some(Expression::Variable(v)) = args.first() {
                Some(sub.contains_key(v))
            } else {
                None
            }
        }
        "REGEX" => {
            let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let text = graph_element_to_string(&text_el)?;

            let pat_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let pattern = graph_element_to_string(&pat_el)?;

            // Flags (optional 3rd arg)
            let flags = if let Some(flag_expr) = args.get(2) {
                let fel = eval_expression_value_inner(flag_expr, sub, datastore)?;
                graph_element_to_string(&fel).unwrap_or_default()
            } else {
                String::new()
            };

            // SPARQL 1.1 §17.4.3.14: REGEX performs a genuine XPath-style
            // regular-expression match (`fn:matches`), not a substring test.
            // A prior `text.contains(pattern)` implementation silently
            // treated every pattern as a literal substring, so anchors
            // (`^`/`$`), character classes (`[0-9A-F]`), and repetition
          // (`{8}`) never worked — e.g. UUID-shape validation in the W3C
            // `uuid01`/`struuid01` fixtures always failed. See #205.
            let mut pattern_str = pattern.clone();
            let mut inline_flags = String::new();
            for f in flags.chars() {
                match f {
                    'i' => inline_flags.push('i'),
                    's' => inline_flags.push('s'),
                    'm' => inline_flags.push('m'),
                    'x' => inline_flags.push('x'),
                    _ => {}
                }
            }
            if !inline_flags.is_empty() {
                pattern_str = format!("(?{inline_flags}){pattern_str}");
            }
            let re = regex::Regex::new(&pattern_str).ok()?;
            Some(re.is_match(&text))
        }
        "LANGMATCHES" => {
            let lang_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let lang = graph_element_to_string(&lang_el)?.to_lowercase();

            let range_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let range = graph_element_to_string(&range_el)?.to_lowercase();

            Some(if range == "*" {
                !lang.is_empty()
            } else {
                lang == range || lang.starts_with(&format!("{}-", range))
            })
        }
        "ISIRI" | "ISURI" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(_))
            ))
        }
        "ISBLANK" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::AnonymousBlankNode(_))
            ))
        }
        "ISLITERAL" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            Some(matches!(el, dag_rdf::GraphElement::GraphLiteral(_)))
        }
        // Fallback: any function not given a dedicated boolean-context arm
        // above (e.g. `ISNUMERIC`, `SAMETERM`) may still be usable in a
        // boolean position (`FILTER isNumeric(?x)`) if `eval_function_value`
        // computes an `xsd:boolean`-typed result for it. Without this, such
        // functions silently evaluate to `None` in `FILTER`/boolean contexts
        // even though they work fine inside `BIND`/projections, which used to
        // make `FILTER isNumeric(?num)` reject every row (see #205).
        _ => {
            let el = eval_function_value(name, args, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => Some(b),
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                    ref type_iri,
                    ref literal,
                }) if type_iri.0 == XSD_BOOLEAN => Some(literal == "true"),
                _ => None,
            }
        }
    }
}

/// Evaluate an expression for use in `BIND`, returning its `GraphElement` value.
/// Supports variables, constants, arithmetic, and function calls.
fn eval_bind_expr(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    eval_expression_value_inner(expr, sub, datastore)
}

/// Extract an integer from either `IntegerLiteral` or `TypedLiteral(xsd:integer)`.
fn element_to_usize(el: &GraphElement) -> Option<usize> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.to_string().parse().ok(),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_INTEGER =>
        {
            literal.parse().ok()
        }
        _ => None,
    }
}

/// Coerce a boolean from either `BooleanLiteral` or `TypedLiteral(xsd:boolean)`.
fn element_to_bool(el: &GraphElement) -> Option<bool> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => Some(*b),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            Some(literal == "true")
        }
        _ => None,
    }
}

fn graph_element_to_string(el: &GraphElement) -> Option<String> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some(s.clone()),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            Some(literal.clone())
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            Some(literal.clone())
        }
        GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(iri)) => Some(iri.0.clone()),
        _ => None,
    }
}

/// A string-valued literal's "tag": whether it's a simple literal, has a
/// language tag, or is explicitly `xsd:string`-typed. SPARQL 1.1 §17.4.3's
/// string functions (`UCASE`, `LCASE`, `SUBSTR`, `STRBEFORE`, `STRAFTER`,
/// `REPLACE`, `CONCAT`) must propagate this tag from their input(s) to their
/// output rather than always emitting a plain simple literal — losing it
/// caused every W3C string-function fixture that used a language-tagged or
/// `xsd:string`-typed operand to fail on exact-datatype comparison (#205).
#[derive(Clone, PartialEq, Eq)]
enum StrLitTag {
    Plain,
    Lang(String),
    XsdString,
}

/// Extract a string literal's lexical value and `StrLitTag`. Returns `None`
/// for anything that isn't a simple/lang/xsd:string literal (IRIs, numbers,
/// booleans, dates, blank nodes, other typed literals) — per spec, the
/// string functions this feeds are only defined over string-valued operands
/// and must error (propagate `None`) on anything else.
fn literal_str_tag(el: &GraphElement) -> Option<(String, StrLitTag)> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some((s.clone(), StrLitTag::Plain)),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, lang }) => {
            Some((literal.clone(), StrLitTag::Lang(lang.clone())))
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, type_iri })
            if type_iri.0 == XSD_STRING =>
        {
            Some((literal.clone(), StrLitTag::XsdString))
        }
        _ => None,
    }
}

/// Reconstruct a `GraphElement` from a computed string value and the
/// `StrLitTag` it should carry.
fn str_tag_to_element(s: String, tag: StrLitTag) -> GraphElement {
    match tag {
        StrLitTag::Plain => GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)),
        StrLitTag::Lang(lang) => {
            GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, literal: s })
        }
        StrLitTag::XsdString => GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
            type_iri: IriReference(XSD_STRING.to_string()),
            literal: s,
        }),
    }
}

/// SPARQL 1.1 §17.1's "argument compatibility" rule for two string operands
/// (used by `STRBEFORE`/`STRAFTER`'s second argument, and by other
/// string-comparison builtins): compatible if `arg2` has no language tag (is
/// a simple literal or `xsd:string`), or if both share the exact same
/// language tag. Two literals with *different* language tags are not
/// compatible, and the containing function must error (`None`).
fn str_args_compatible(tag1: &StrLitTag, tag2: &StrLitTag) -> bool {
    match tag2 {
        StrLitTag::Plain | StrLitTag::XsdString => true,
        StrLitTag::Lang(l2) => matches!(tag1, StrLitTag::Lang(l1) if l1 == l2),
    }
}

/// Extract a numeric f64 from a literal if it has a numeric datatype.
fn literal_to_f64(lit: &RdfLiteral) -> Option<f64> {
    match lit {
        RdfLiteral::IntegerLiteral(i) => i.to_string().parse().ok(),
        RdfLiteral::DoubleLiteral(d) => Some(d.into_inner()),
        RdfLiteral::DecimalLiteral(d) => Some(d.to_string().parse().ok()?),
        RdfLiteral::FloatLiteral(f) => Some(f.into_inner()),
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            let iri = &type_iri.0;
            if iri == XSD_INTEGER || iri == XSD_DECIMAL || iri == XSD_DOUBLE || iri == XSD_FLOAT {
                literal.parse().ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

// ── XSD datatype constructor/cast functions (SPARQL 1.1 §17.4.2, #190) ────
//
// `xsd:integer(v)`, `xsd:decimal(v)`, `xsd:double(v)`, `xsd:float(v)`,
// `xsd:string(v)`, `xsd:boolean(v)`, `xsd:dateTime(v)` (#194): given an
// appropriately-typed input (numeric literal, boolean, string, or
// matching-lexical-form typed literal), produce a new literal of the target
// datatype. An invalid conversion returns `None` — per this file's
// established convention (see `ABS`/`ROUND`/etc. above), that leaves the
// enclosing expression's result unbound rather than erroring the whole
// query.

/// Dispatch a resolved XSD datatype IRI to its cast implementation.
fn eval_xsd_cast(target_iri: &str, el: &GraphElement) -> Option<GraphElement> {
    match target_iri {
        XSD_STRING => cast_to_xsd_string(el),
        XSD_BOOLEAN => cast_to_xsd_boolean(el),
        XSD_INTEGER => cast_to_xsd_integer(el),
        XSD_DECIMAL => cast_to_xsd_decimal(el),
        XSD_DOUBLE => cast_to_xsd_double(el),
        XSD_FLOAT => cast_to_xsd_float(el),
        XSD_DATE_TIME => cast_to_xsd_datetime(el),
        _ => None,
    }
}

/// Parse an XSD `integer` lexical form (optional sign, digits only) into a `BigInt`.
fn parse_xsd_integer_lexical(s: &str) -> Option<BigInt> {
    let t = s.trim();
    let (sign, digits) = match t.strip_prefix('+') {
        Some(rest) => ("", rest),
        None => match t.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", t),
        },
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    format!("{sign}{digits}").parse::<BigInt>().ok()
}

/// Parse an XSD `boolean` lexical form (`true`/`false`/`1`/`0`) into a `bool`.
fn parse_xsd_boolean_lexical(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Parse an XSD `double`/`float` lexical form, including the special values
/// `INF`/`-INF`/`NaN`, into an `f64`.
fn parse_xsd_double_lexical(s: &str) -> Option<f64> {
    match s.trim() {
        "INF" | "+INF" => Some(f64::INFINITY),
        "-INF" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        other => other.parse::<f64>().ok(),
    }
}

/// Cast to `xsd:string`: the lexical/string value of the source literal.
/// Kept separate from `graph_element_to_string` (shared by `CONCAT`/`STRLEN`/
/// etc.) so this cast's semantics — e.g. rendering native numeric/boolean
/// literals — can't change the behaviour of those unrelated builtins.
fn cast_to_xsd_string(el: &GraphElement) -> Option<GraphElement> {
    let s = match el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s.clone(),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => literal.clone(),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => literal.clone(),
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => b.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d)) => d.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(d)) => d.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::FloatLiteral(f)) => f.to_string(),
        _ => return None,
    };
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)))
}

/// Cast to `xsd:boolean` per XPath casting rules: numeric zero/NaN is
/// `false`, any other numeric value is `true`; strings must match the
/// `xsd:boolean` lexical space (`true`/`false`/`1`/`0`).
fn cast_to_xsd_boolean(el: &GraphElement) -> Option<GraphElement> {
    let b = match el {
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => *b,
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => parse_xsd_boolean_lexical(s)?,
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_xsd_boolean_lexical(literal)?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN || type_iri.0 == XSD_STRING =>
        {
            parse_xsd_boolean_lexical(literal)?
        }
        GraphElement::GraphLiteral(lit) => {
            let f = literal_to_f64(lit)?;
            !f.is_nan() && f != 0.0
        }
        _ => return None,
    };
    // Emit `TypedLiteral{xsd:boolean, "true"/"false"}` — the shape
    // `parse_boolean_literal` produces for real `true`/`false` literals —
    // rather than the native `BooleanLiteral` variant, so a cast result can
    // join against real interned boolean data. See #228.
    Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(XSD_BOOLEAN.to_string()),
        literal: b.to_string(),
    }))
}

/// Cast to `xsd:integer` per XPath fn:integer casting rules: numeric sources
/// truncate toward zero (NOT floor/round — `xsd:integer(-3.7)` is `-3`).
fn cast_to_xsd_integer(el: &GraphElement) -> Option<GraphElement> {
    let n = match el {
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.clone(),
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => BigInt::from(u8::from(*b)),
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => parse_xsd_integer_lexical(s)?,
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_xsd_integer_lexical(literal)?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            BigInt::from(u8::from(parse_xsd_boolean_lexical(literal)?))
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_INTEGER || type_iri.0 == XSD_STRING =>
        {
            parse_xsd_integer_lexical(literal)?
        }
        GraphElement::GraphLiteral(lit) => {
            // xsd:decimal / xsd:double / xsd:float (native or typed): truncate.
            let f = literal_to_f64(lit)?;
            if !f.is_finite() {
                return None;
            }
            BigInt::from(f.trunc() as i64)
        }
        _ => return None,
    };
    // TypedLiteral, not the native IntegerLiteral variant — see #228.
    Some(numeric_lit_to_element(NumericLit::Integer(n)))
}

/// Cast to `xsd:decimal`. Converts via the source's string form (rather than
/// through `f64`) where possible, to avoid binary-float rounding noise in the
/// resulting decimal's lexical form.
fn cast_to_xsd_decimal(el: &GraphElement) -> Option<GraphElement> {
    let d = match el {
        GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d)) => *d,
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.to_string().parse().ok()?,
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => {
            rust_decimal::Decimal::from(u8::from(*b))
        }
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s.trim().parse().ok()?,
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            literal.trim().parse().ok()?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            rust_decimal::Decimal::from(u8::from(parse_xsd_boolean_lexical(literal)?))
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_DECIMAL
                || type_iri.0 == XSD_INTEGER
                || type_iri.0 == XSD_STRING =>
        {
            literal.trim().parse().ok()?
        }
        GraphElement::GraphLiteral(lit) => {
            // xsd:double / xsd:float (native or typed): round-trip through the
            // decimal string form of the f64 to avoid binary-float noise.
            let f = literal_to_f64(lit)?;
            if !f.is_finite() {
                return None;
            }
            f.to_string().parse().ok()?
        }
        _ => return None,
    };
    // TypedLiteral, not the native DecimalLiteral variant — see #228.
    Some(numeric_lit_to_element(NumericLit::Decimal(d)))
}

/// Shared numeric extraction for `xsd:double`/`xsd:float` casts: handles
/// booleans and lexical strings (including `INF`/`NaN`) directly, and
/// delegates to `literal_to_f64` for the plain numeric literal kinds it
/// already covers.
fn extract_f64_for_cast(el: &GraphElement) -> Option<f64> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => {
            Some(if *b { 1.0 } else { 0.0 })
        }
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => parse_xsd_double_lexical(s),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_xsd_double_lexical(literal)
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            parse_xsd_boolean_lexical(literal).map(|b| if b { 1.0 } else { 0.0 })
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            parse_xsd_double_lexical(literal)
        }
        GraphElement::GraphLiteral(lit) => literal_to_f64(lit),
        _ => None,
    }
}

/// Cast to `xsd:double`. Emits `TypedLiteral{xsd:double, ..}`, not the native
/// `DoubleLiteral` variant, so the result can join against real interned
/// `xsd:double` data — see #228.
fn cast_to_xsd_double(el: &GraphElement) -> Option<GraphElement> {
    let f = extract_f64_for_cast(el)?;
    Some(numeric_lit_to_element(NumericLit::Double(f)))
}

/// Cast to `xsd:float`. Emits `TypedLiteral{xsd:float, ..}`, not the native
/// `FloatLiteral` variant — see #228, as above.
fn cast_to_xsd_float(el: &GraphElement) -> Option<GraphElement> {
    let f = extract_f64_for_cast(el)?;
    Some(numeric_lit_to_element(NumericLit::Float(f)))
}

/// Cast to `xsd:dateTime` (#194): a native `DateTimeLiteral` passes through
/// unchanged; a string (or `xsd:dateTime`/`xsd:date`/`xsd:string`-typed
/// literal) is parsed via `parse_datetime_or_date_lexical`, which accepts the
/// full `xsd:dateTime` lexical space (with or without a timezone) and
/// normalizes `xsd:date` (`YYYY-MM-DD`) to midnight UTC. Deliberately does
/// NOT fall back to bare `xsd:gYear` the way `parse_xsd_datetime` (used by
/// `YEAR`/`MONTH`/`DAY`) does — a bare year is not a valid `xsd:dateTime`
/// cast source per the XPath casting rules, so it stays unbound.
///
/// The result is emitted as `TypedLiteral{xsd:dateTime, dt.to_rfc3339()}`,
/// not the native `DateTimeLiteral` variant — the Turtle parser always
/// produces `TypedLiteral` for `xsd:dateTime` data (only `xsd:string`
/// literals get a dedicated variant; see `turtle::convert_literal`), so a
/// native `DateTimeLiteral` cast result could never structurally match
/// already-interned `xsd:dateTime` data in a later triple-pattern join. See
/// #228. Note `chrono`'s `to_rfc3339()` always normalizes the UTC offset to
/// `+00:00` (never `Z`), so real data joined against a cast result must use
/// the same `+00:00` lexical form.
fn cast_to_xsd_datetime(el: &GraphElement) -> Option<GraphElement> {
    let dt = match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(dt)) => *dt,
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => {
            parse_datetime_or_date_lexical(s)?
        }
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_datetime_or_date_lexical(literal)?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_DATE_TIME
                || type_iri.0 == XSD_DATE
                || type_iri.0 == XSD_STRING =>
        {
            parse_datetime_or_date_lexical(literal)?
        }
        _ => return None,
    };
    Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(XSD_DATE_TIME.to_string()),
        literal: dt.to_rfc3339(),
    }))
}

/// Equality for `=`/`!=`/`IN`/`NOT IN` (SPARQL 1.1 §17.3, §17.4.1.9).
///
/// As of #228, every scalar computed-value producer in this module (unary
/// minus, binary arithmetic, `ABS`/`CEIL`/`FLOOR`/`ROUND`, the xsd casts)
/// emits the same `TypedLiteral { type_iri, literal }` shape that literals
/// parsed directly from SPARQL query text or Turtle data use
/// (`parse_numeric_literal`/`parse_boolean_literal` in `lib.rs`,
/// `turtle::convert_literal`) — see `numeric_lit_to_element`. Only
/// aggregates (`SUM`/`COUNT`/`AVG`/etc., which cannot appear inside `BIND`)
/// still produce the native `RdfLiteral` variants (`IntegerLiteral`,
/// `DecimalLiteral`, `DoubleLiteral`, `FloatLiteral`, `BooleanLiteral`). A
/// raw Rust `==` sees these as different enum variants even when they denote
/// the same value, so e.g. `SUM(?x) = 2` could wrongly compare unequal
/// against a `TypedLiteral` (#208).
///
/// This normalizes numeric and boolean literals across both representations,
/// then falls back to plain equality for every other RDF term shape (IRIs,
/// blank nodes, plain strings, language-tagged literals) — where the
/// native/parsed split does not exist and `==` was already correct.
fn values_equal(a: &GraphElement, b: &GraphElement) -> bool {
    if let (GraphElement::GraphLiteral(a_lit), GraphElement::GraphLiteral(b_lit)) = (a, b) {
        if let (Some(af), Some(bf)) = (literal_to_f64(a_lit), literal_to_f64(b_lit)) {
            return af.partial_cmp(&bf) == Some(std::cmp::Ordering::Equal);
        }
    }
    if let (Some(a_bool), Some(b_bool)) = (element_to_bool(a), element_to_bool(b)) {
        return a_bool == b_bool;
    }
    a == b
}

/// Compare graph elements for FILTER relational operators.
/// Returns negative, 0, positive, or None if not comparable.
fn compare_graph_elements(a: &GraphElement, b: &GraphElement) -> Option<i32> {
    use dag_rdf::GraphElement::GraphLiteral;
    use std::cmp::Ordering::*;
    if let (GraphLiteral(a_lit), GraphLiteral(b_lit)) = (a, b) {
        // Try numeric comparison first
        if let (Some(af), Some(bf)) = (literal_to_f64(a_lit), literal_to_f64(b_lit)) {
            return af.partial_cmp(&bf).map(|o| match o {
                Less => -1,
                Equal => 0,
                Greater => 1,
            });
        }
        // String literal comparison
        let a_str = match a_lit {
            RdfLiteral::LiteralString(s) => Some(s.as_str()),
            RdfLiteral::TypedLiteral { literal, .. } => Some(literal.as_str()),
            _ => None,
        };
        let b_str = match b_lit {
            RdfLiteral::LiteralString(s) => Some(s.as_str()),
            RdfLiteral::TypedLiteral { literal, .. } => Some(literal.as_str()),
            _ => None,
        };
        if let (Some(a_s), Some(b_s)) = (a_str, b_str) {
            return Some(match a_s.cmp(b_s) {
                Less => -1,
                Equal => 0,
                Greater => 1,
            });
        }
    }
    None
}

// ── CONSTRUCT helpers ─────────────────────────────────────────────────────────

/// Collect all BGP triple patterns from a component list (for CONSTRUCT WHERE short form).
fn collect_bgps_from_components(components: &[QueryComponent]) -> Vec<TriplePattern> {
    let mut out = Vec::new();
    for comp in components {
        match comp {
            QueryComponent::BGP(tps) => out.extend(tps.clone()),
            QueryComponent::Optional(inner) | QueryComponent::Minus(inner) => {
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
) -> Vec<PartialSub> {
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
        return Vec::new();
    };

    let initial: Vec<PartialSub> = vec![HashMap::new()];
    let budget = select_solution_budget(*distinct, order_by, group_by, projection, *offset, *limit);
    let solutions = eval_components_budgeted(
        where_clause,
        initial,
        datastore,
        (*active_graph).clone(),
        budget,
    );

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

    rows
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

/// Evaluate a property path pattern against the datastore, extending one solution.
/// Zero-hop ("identity") solutions for a path pattern: subject and object
/// must denote the same node. Used by `?` (`ZeroOrOne`) and by the `k == 0`
/// case of bounded repetition (`{0}`, `{0,m}`).
///
/// Note: when both endpoints are unbound variables this currently returns
/// no solutions rather than enumerating `subject = object = x` for every
/// node `x` in the active graph — a pre-existing gap shared with
/// `ZeroOrOne`, tracked (along with the other zero-length-path semantics
/// gaps) in <https://github.com/daghovland/rdf-datalog/issues/203>.
fn zero_hop_solutions(
    subject_term: &Term,
    object_term: &Term,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Vec<PartialSub> {
    let s_gel = resolve_term_to_gel(subject_term, sub, datastore);
    let o_gel = resolve_term_to_gel(object_term, sub, datastore);
    match (s_gel, o_gel) {
        // Both bound: must be equal
        (Some(s), Some(o)) if s == o => vec![sub.clone()],
        // Subject bound, object unbound: bind object = subject
        (Some(s), None) => {
            if let Term::Variable(v) = object_term {
                let mut new_sub = sub.clone();
                new_sub.insert(v.clone(), PartialSubValue::Computed(s));
                vec![new_sub]
            } else {
                Vec::new()
            }
        }
        // Object bound, subject unbound: bind subject = object
        (None, Some(o)) => {
            if let Term::Variable(v) = subject_term {
                let mut new_sub = sub.clone();
                new_sub.insert(v.clone(), PartialSubValue::Computed(o));
                vec![new_sub]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// Evaluate a bounded/unbounded repetition path (`p{n}`, `p{n,m}`, `p{n,}`,
/// `p{,m}`).
///
/// Unlike `ZeroOrMore`/`OneOrMore`, which use arbitrary-length-path
/// (fixed-point/BFS) semantics — one solution per reachable pair, regardless
/// of how many distinct walks connect it — bounded repetition uses ordinary
/// sequence (join) semantics: `p{k}` is evaluated as a `k`-fold sequence of
/// `p`, so distinct walks of the same length produce distinct (duplicate)
/// solutions. This matches the W3C property-path test expectations (e.g.
/// `data-diamond.ttl` has two distinct 2-hop walks from `:a` to `:z`, and
/// `:a :p{2} ?z` is expected to produce two solutions with `?z = :z`, not
/// one) — see
/// <https://github.com/daghovland/rdf-datalog/issues/203>.
///
/// For an unbounded lower-bounded range (`{n,}`, `max == None`), this is
/// evaluated as `p{n}` followed by `p*`: exactly `n` hops (preserving walk
/// multiplicity, and safe on cyclic data since it's a fixed number of
/// joins) followed by zero-or-more further hops (fixed-point reachability,
/// so cycles don't cause non-termination or multiplicity blow-up).
fn eval_repeat_path(
    subject_term: &Term,
    inner: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    range: (usize, Option<usize>),
) -> Vec<PartialSub> {
    let (min, max) = range;
    match max {
        Some(max_n) => {
            if min > max_n {
                return Vec::new();
            }
            let mut results = Vec::new();
            for k in min..=max_n {
                results.extend(eval_exact_repeat(
                    subject_term,
                    inner,
                    object_term,
                    sub.clone(),
                    datastore,
                    active_graph,
                    k,
                ));
            }
            results
        }
        None => {
            // {min,} == inner{min} / inner*
            let mut steps: Vec<PropertyPath> = (0..min).map(|_| inner.clone()).collect();
            steps.push(PropertyPath::ZeroOrMore(Box::new(inner.clone())));
            let seq = PropertyPath::Sequence(steps);
            eval_path_pattern(
                subject_term,
                &seq,
                object_term,
                sub,
                datastore,
                active_graph,
            )
        }
    }
}

/// Evaluate `inner{k}` for an exact, non-negative repeat count `k`.
fn eval_exact_repeat(
    subject_term: &Term,
    inner: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    k: usize,
) -> Vec<PartialSub> {
    if k == 0 {
        zero_hop_solutions(subject_term, object_term, &sub, datastore)
    } else {
        let steps: Vec<PropertyPath> = (0..k).map(|_| inner.clone()).collect();
        let seq = PropertyPath::Sequence(steps);
        eval_path_pattern(
            subject_term,
            &seq,
            object_term,
            sub,
            datastore,
            active_graph,
        )
    }
}

fn eval_path_pattern(
    subject_term: &Term,
    path: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Vec<PartialSub> {
    match path {
        PropertyPath::Iri(gel) => {
            let tp = TriplePattern {
                subject: subject_term.clone(),
                predicate: Term::Constant(gel.clone()),
                object: object_term.clone(),
            };
            eval_triple_pattern(&tp, &sub, datastore, active_graph, None)
        }

        PropertyPath::Sequence(steps) => {
            if steps.is_empty() {
                return vec![sub];
            }
            // Chain: introduce fresh bridge variables for intermediate nodes.
            let mut current_subject = subject_term.clone();
            let mut current_subs = vec![sub];
            let n = steps.len();
            for (i, step) in steps.iter().enumerate() {
                let current_object = if i + 1 == n {
                    object_term.clone()
                } else {
                    Term::Variable(format!("__path_seq_{}", i))
                };
                current_subs = current_subs
                    .into_iter()
                    .flat_map(|s| {
                        eval_path_pattern(
                            &current_subject,
                            step,
                            &current_object,
                            s,
                            datastore,
                            active_graph,
                        )
                    })
                    .collect();
                current_subject = current_object;
            }
            // Remove internal bridge variables from each solution
            current_subs
                .into_iter()
                .map(|mut s| {
                    for i in 0..n - 1 {
                        s.remove(&format!("__path_seq_{}", i));
                    }
                    s
                })
                .collect()
        }

        PropertyPath::Alternative(left, right) => {
            let mut left_subs = eval_path_pattern(
                subject_term,
                left,
                object_term,
                sub.clone(),
                datastore,
                active_graph,
            );
            let right_subs = eval_path_pattern(
                subject_term,
                right,
                object_term,
                sub,
                datastore,
                active_graph,
            );
            left_subs.extend(right_subs);
            left_subs
        }

        PropertyPath::Inverse(inner) => {
            // Swap subject and object
            eval_path_pattern(
                object_term,
                inner,
                subject_term,
                sub,
                datastore,
                active_graph,
            )
        }

        PropertyPath::ZeroOrOne(inner) => {
            // Zero hops: subject == object
            let zero_hop = zero_hop_solutions(subject_term, object_term, &sub, datastore);
            let one_hop = eval_path_pattern(
                subject_term,
                inner,
                object_term,
                sub,
                datastore,
                active_graph,
            );
            // Deduplicate (zero-hop and one-hop may produce the same solution).
            // Compare by resolved value: zero-hop bindings are `Computed` while
            // one-hop bindings from a BGP match are `Interned`, so the same
            // logical solution can appear in two representations (#141).
            let mut result = zero_hop;
            for s in one_hop {
                if !result.iter().any(|r| partial_subs_equal(r, &s, datastore)) {
                    result.push(s);
                }
            }
            result
        }

        PropertyPath::OneOrMore(inner) => transitive_closure(
            subject_term,
            inner,
            object_term,
            sub,
            datastore,
            active_graph,
            false,
        ),

        PropertyPath::ZeroOrMore(inner) => transitive_closure(
            subject_term,
            inner,
            object_term,
            sub,
            datastore,
            active_graph,
            true,
        ),

        PropertyPath::Repeat(inner, min, max) => eval_repeat_path(
            subject_term,
            inner,
            object_term,
            sub,
            datastore,
            active_graph,
            (*min, *max),
        ),

        PropertyPath::NegatedSet(excluded) => {
            let g = match active_graph {
                ActiveGraph::Fixed(id) => Some(*id),
                ActiveGraph::Variable(v) => sub.get(v).and_then(|val| val.to_id(datastore)),
            };
            let s_match = resolve_match_term(subject_term, &sub, datastore);
            let o_match = resolve_match_term(object_term, &sub, datastore);
            // See `MatchTerm`: an unsupported endpoint (e.g. a triple term)
            // or a never-interned constant must not silently degrade to an
            // unconstrained wildcard.
            if matches!(s_match, MatchTerm::Never) || matches!(o_match, MatchTerm::Never) {
                return Vec::new();
            }
            let s = s_match.into_query_arg();
            let o = o_match.into_query_arg();
            let excluded_ids: HashSet<GraphElementId> = excluded
                .iter()
                .filter_map(|gel| datastore.resources.resource_map.get(gel).copied())
                .collect();

            let mut results = Vec::new();
            for quad in datastore.quads_matching(g, s, None, o) {
                if excluded_ids.contains(&quad.predicate) {
                    continue;
                }
                let mut new_sub = sub.clone();
                let mut ok = true;
                if let Term::Variable(v) = subject_term {
                    let new_val = PartialSubValue::Interned(quad.subject);
                    match new_sub.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), new_val);
                        }
                    }
                }
                if let Term::Variable(v) = object_term {
                    let new_val = PartialSubValue::Interned(quad.obj);
                    match new_sub.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), new_val);
                        }
                    }
                }
                if ok {
                    results.push(new_sub);
                }
            }
            results
        }
    }
}

/// Resolve a `Term` to a `GraphElement` using the current solution.
fn resolve_term_to_gel(
    term: &Term,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match term {
        Term::Variable(v) => sub.get(v).map(|val| val.resolve(datastore)),
        Term::Constant(gel) => Some(gel.clone()),
        // Property paths over triple-term endpoints are out of scope for
        // phase R3 (#146); treat as unbound so no path steps match.
        Term::TripleTerm(_) => None,
    }
}

/// Compute transitive closure of `path` from `subject_term` to `object_term`.
///
/// `include_zero` = true for `*` (include starting node), false for `+`.
///
/// Strategy: BFS from the subject if it is bound (forward traversal).
/// If the subject is unbound and the object is bound, reverse BFS using ^path.
fn transitive_closure(
    subject_term: &Term,
    path: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    include_zero: bool,
) -> Vec<PartialSub> {
    let subject_gel = resolve_term_to_gel(subject_term, &sub, datastore);
    let object_gel = resolve_term_to_gel(object_term, &sub, datastore);

    // Enumerate all nodes reachable from each concrete starting point
    // by doing BFS with the inner path as a single-hop traversal.
    // Forward BFS: returns all nodes reachable from start_gel.
    // For `include_zero` (p*): includes start_gel itself.
    // For `!include_zero` (p+): excludes start_gel.
    let reachable_from = |start_gel: GraphElement| -> Vec<GraphElement> {
        let mut visited: HashSet<GraphElement> = HashSet::new();
        let mut queue = vec![start_gel.clone()];
        while let Some(current) = queue.pop() {
            let current_term = Term::Constant(current.clone());
            let next_subs = eval_path_pattern(
                &current_term,
                path,
                &Term::Variable("__tc_next".to_string()),
                sub.clone(),
                datastore,
                active_graph,
            );
            for s in next_subs {
                if let Some(next_val) = s.get("__tc_next") {
                    let next_gel = next_val.resolve(datastore);
                    if visited.insert(next_gel.clone()) {
                        queue.push(next_gel);
                    }
                }
            }
        }
        if include_zero {
            visited.insert(start_gel);
        }
        visited.into_iter().collect()
    };

    match (subject_gel, object_gel) {
        (Some(s_gel), Some(o_gel)) => {
            // Both bound: check if object is reachable from subject
            let reachable = reachable_from(s_gel);
            if reachable.contains(&o_gel) {
                vec![sub]
            } else {
                Vec::new()
            }
        }
        (Some(s_gel), None) => {
            // Subject bound, object unbound: enumerate all reachable nodes
            let reachable = reachable_from(s_gel);
            if let Term::Variable(obj_var) = object_term {
                reachable
                    .into_iter()
                    .map(|gel| {
                        let mut new_sub = sub.clone();
                        new_sub.insert(obj_var.clone(), PartialSubValue::Computed(gel));
                        new_sub
                    })
                    .collect()
            } else {
                Vec::new()
            }
        }
        (None, Some(o_gel)) => {
            // Object bound, subject unbound: BFS backwards using inverse path.
            // `visited` collects nodes that can reach o_gel in ≥1 hops.
            // For `include_zero` (p*) also include o_gel itself (0 hops).
            let inverse_path = PropertyPath::Inverse(Box::new(path.clone()));
            let reachable = {
                let mut visited: HashSet<GraphElement> = HashSet::new();
                let mut queue = vec![o_gel.clone()];
                while let Some(current) = queue.pop() {
                    let current_term = Term::Constant(current.clone());
                    let next_subs = eval_path_pattern(
                        &current_term,
                        &inverse_path,
                        &Term::Variable("__tc_prev".to_string()),
                        sub.clone(),
                        datastore,
                        active_graph,
                    );
                    for s in next_subs {
                        if let Some(prev_val) = s.get("__tc_prev") {
                            let prev_gel = prev_val.resolve(datastore);
                            if visited.insert(prev_gel.clone()) {
                                queue.push(prev_gel);
                            }
                        }
                    }
                }
                if include_zero {
                    visited.insert(o_gel);
                }
                visited
            };
            if let Term::Variable(subj_var) = subject_term {
                reachable
                    .into_iter()
                    .map(|gel| {
                        let mut new_sub = sub.clone();
                        new_sub.insert(subj_var.clone(), PartialSubValue::Computed(gel));
                        new_sub
                    })
                    .collect()
            } else {
                Vec::new()
            }
        }
        (None, None) => {
            // Both unbound: enumerate all nodes reachable from any node in graph.
            // For each subject node, find all objects reachable from it.
            // This is expensive; for now use the bound-subject BFS for each node.
            let all_subjects: Vec<GraphElement> = {
                let g = match active_graph {
                    ActiveGraph::Fixed(id) => Some(*id),
                    ActiveGraph::Variable(v) => sub.get(v).and_then(|val| val.to_id(datastore)),
                };
                datastore
                    .quads_matching(g, None, None, None)
                    .into_iter()
                    .flat_map(|q| {
                        [
                            datastore.resources.get_graph_element(q.subject).clone(),
                            datastore.resources.get_graph_element(q.obj).clone(),
                        ]
                    })
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect()
            };
            let (subj_var, obj_var) = match (subject_term, object_term) {
                (Term::Variable(s), Term::Variable(o)) => (s, o),
                _ => return Vec::new(),
            };
            let mut results = Vec::new();
            for s_gel in all_subjects {
                let reachable = reachable_from(s_gel.clone());
                for o_gel in reachable {
                    let mut new_sub = sub.clone();
                    new_sub.insert(subj_var.clone(), PartialSubValue::Computed(s_gel.clone()));
                    new_sub.insert(obj_var.clone(), PartialSubValue::Computed(o_gel));
                    if !results
                        .iter()
                        .any(|r| partial_subs_equal(r, &new_sub, datastore))
                    {
                        results.push(new_sub);
                    }
                }
            }
            results
        }
    }
}

// ── Aggregate helpers ─────────────────────────────────────────────────────────

/// True if a projection element contains an aggregate expression.
fn elem_has_aggregate(elem: &ProjectionElement) -> bool {
    match elem {
        ProjectionElement::Expression(expr, _) => expr_has_aggregate(expr),
        _ => false,
    }
}

fn expr_has_aggregate(expr: &Expression) -> bool {
    match expr {
        Expression::Aggregate(_) => true,
        Expression::Binary(l, _, r) => expr_has_aggregate(l) || expr_has_aggregate(r),
        Expression::Unary(_, inner) => expr_has_aggregate(inner),
        Expression::FunctionCall(_, args) => args.iter().any(expr_has_aggregate),
        _ => false,
    }
}

/// Partition solutions into groups keyed by GROUP BY expressions.
///
/// When `group_by` is empty all solutions fall into one implicit group.
///
/// A `GroupCondition` written `(expr AS ?var)` additionally binds the
/// computed grouping key to `?var` in every solution of the resulting group
/// (see [`bind_group_aliases`]), so it is available to the projection,
/// `HAVING`, and `ORDER BY` like any other bound variable — this is what lets
/// `GROUP BY (COALESCE(?w, ...) AS ?X)` project `?X` (W3C SPARQL 1.1
/// `grouping` suite `Group-4`,
/// <https://github.com/daghovland/rdf-datalog/issues/206>).
fn group_by_solutions(
    solutions: &[PartialSub],
    group_by: &[GroupCondition],
    datastore: &Datastore,
) -> Vec<Vec<PartialSub>> {
    if group_by.is_empty() {
        return vec![solutions.to_vec()];
    }
    // Special case per SPARQL 1.1 §11.4.1 (see the "agg empty group" / "Aggregate
    // over empty group resulting in a row with unbound variables" W3C test,
    // <http://answers.semanticweb.com/questions/17410/>, tracked in
    // <https://github.com/daghovland/rdf-datalog/issues/202>): when the WHERE
    // clause produces zero solutions, an explicit GROUP BY still yields exactly
    // one (empty) group rather than zero groups. Every GROUP BY key and
    // aggregate is then evaluated over that empty group, leaving them (and any
    // GROUP BY alias) unbound in the single output row.
    if solutions.is_empty() {
        return vec![vec![]];
    }
    let mut map: Vec<(Vec<Option<GraphElement>>, Vec<PartialSub>)> = Vec::new();
    'outer: for sub in solutions {
        let key: Vec<Option<GraphElement>> = group_by
            .iter()
            .map(|gc| eval_expression_value_inner(&gc.expr, sub, datastore))
            .collect();
        let bound_sub = bind_group_aliases(sub, group_by, &key);
        for (k, group) in &mut map {
            if *k == key {
                group.push(bound_sub);
                continue 'outer;
            }
        }
        map.push((key, vec![bound_sub]));
    }
    map.into_iter().map(|(_, g)| g).collect()
}

/// Bind each aliased `GroupCondition`'s computed key value to its alias
/// variable in `sub`. Conditions with no `AS var` (the common case) leave
/// `sub` untouched. An unbound key component (e.g. the grouping expression
/// errored for this solution) leaves the alias unbound too, rather than
/// binding it to some placeholder value.
fn bind_group_aliases(
    sub: &PartialSub,
    group_by: &[GroupCondition],
    key: &[Option<GraphElement>],
) -> PartialSub {
    if group_by.iter().all(|gc| gc.alias.is_none()) {
        return sub.clone();
    }
    let mut sub = sub.clone();
    for (gc, val) in group_by.iter().zip(key.iter()) {
        if let Some(alias) = &gc.alias {
            match val {
                Some(v) => {
                    sub.insert(alias.clone(), PartialSubValue::Computed(v.clone()));
                }
                None => {
                    sub.remove(alias);
                }
            }
        }
    }
    sub
}

/// Build the output row for one group in an aggregate query.
fn project_aggregate_row(
    projection: &[ProjectionElement],
    group: &[PartialSub],
    datastore: &Datastore,
) -> SolutionRow {
    let rep = group.first().cloned().unwrap_or_default();
    let mut row = SolutionRow::new();
    for elem in projection {
        match elem {
            ProjectionElement::Variable(v) => {
                if let Some(val) = rep.get(v) {
                    row.insert(v.clone(), val.resolve(datastore));
                }
            }
            ProjectionElement::Expression(expr, alias) => {
                if let Some(val) = eval_expr_in_group(expr, group, &rep, datastore) {
                    row.insert(alias.clone(), val);
                }
            }
            ProjectionElement::Star => {}
        }
    }
    row
}

/// Evaluate an expression in the context of a group (for SELECT and HAVING).
///
/// Aggregate sub-expressions are computed over the full group; non-aggregate
/// sub-expressions use the representative solution `rep`.
fn eval_expr_in_group(
    expr: &Expression,
    group: &[PartialSub],
    rep: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match expr {
        Expression::Aggregate(agg) => eval_aggregate_value(agg, group, datastore),
        Expression::Binary(l, op, r) => {
            // Arithmetic in HAVING (e.g. SUM(?x) > 5): eval both sides in group context
            let lv = eval_expr_in_group(l, group, rep, datastore)?;
            let rv = eval_expr_in_group(r, group, rep, datastore)?;
            // Reuse the arithmetic helper by creating single-element "groups" (for pure values)
            eval_binary_value(&lv, op, &rv)
        }
        _ => eval_expression_value_inner(expr, rep, datastore),
    }
}

/// Evaluate a binary operation between two already-resolved values, e.g. an
/// arithmetic expression combining two aggregate results in a SELECT/HAVING
/// clause (`(MIN(?p) + MAX(?p)) / 2`).
///
/// Applies the same SPARQL/XPath numeric type-promotion rules as
/// `eval_arithmetic` (integer < decimal < float < double, result takes the
/// widest operand type) via the shared `classify_numeric`/
/// `numeric_lit_to_element` machinery, rather than the previous
/// integer-fast-path-or-else-`f64` split, which silently forced every
/// non-integer/integer combination (including a plain integer/decimal mix)
/// to `xsd:double` and lost `xsd:decimal` precision/typing. One deliberate
/// divergence from `eval_arithmetic`: `Div` between two integers here
/// produces an exact `xsd:decimal` (per SPARQL/XPath `op:numeric-divide`,
/// which never returns integer), rather than `eval_arithmetic`'s truncating
/// integer division — this only affects aggregate-expression arithmetic
/// (matching the W3C `aggregates` suite's `agg-err-01` expectation), not the
/// general BIND/FILTER arithmetic path. See
/// <https://github.com/daghovland/rdf-datalog/issues/202>.
fn eval_binary_value(l: &GraphElement, op: &BinaryOp, r: &GraphElement) -> Option<GraphElement> {
    let l_lit = match l {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let r_lit = match r {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let ln = classify_numeric(l_lit)?;
    let rn = classify_numeric(r_lit)?;

    if matches!(ln, NumericLit::Double(_)) || matches!(rn, NumericLit::Double(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Double(result)));
    }
    if matches!(ln, NumericLit::Float(_)) || matches!(rn, NumericLit::Float(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Float(result)));
    }
    if let (NumericLit::Integer(a), NumericLit::Integer(b)) = (&ln, &rn) {
        return match op {
            BinaryOp::Add => Some(numeric_lit_to_element(NumericLit::Integer(a + b))),
            BinaryOp::Sub => Some(numeric_lit_to_element(NumericLit::Integer(a - b))),
            BinaryOp::Mul => Some(numeric_lit_to_element(NumericLit::Integer(a * b))),
            BinaryOp::Div => {
                if b == &BigInt::from(0) {
                    return None;
                }
                let ad = numeric_lit_to_decimal(&ln)?;
                let bd = numeric_lit_to_decimal(&rn)?;
                Some(numeric_lit_to_element(NumericLit::Decimal(ad / bd)))
            }
            _ => None,
        };
    }
    // Remaining case: an integer/decimal mix with at least one decimal
    // operand — exact decimal arithmetic, result stays decimal.
    let ad = numeric_lit_to_decimal(&ln)?;
    let bd = numeric_lit_to_decimal(&rn)?;
    let result = match op {
        BinaryOp::Add => ad + bd,
        BinaryOp::Sub => ad - bd,
        BinaryOp::Mul => ad * bd,
        BinaryOp::Div => {
            if bd.is_zero() {
                return None;
            }
            ad / bd
        }
        _ => return None,
    };
    Some(numeric_lit_to_element(NumericLit::Decimal(result)))
}

/// Evaluate a HAVING expression as a boolean, with aggregates computed over the group.
fn eval_having_expr(expr: &Expression, group: &[PartialSub], datastore: &Datastore) -> bool {
    let rep = group.first().cloned().unwrap_or_default();
    eval_having_bool(expr, group, &rep, datastore).unwrap_or(false)
}

fn eval_having_bool(
    expr: &Expression,
    group: &[PartialSub],
    rep: &PartialSub,
    datastore: &Datastore,
) -> Option<bool> {
    match expr {
        Expression::Binary(left, op, right) => match op {
            BinaryOp::And => {
                let l = eval_having_bool(left, group, rep, datastore).unwrap_or(false);
                let r = eval_having_bool(right, group, rep, datastore).unwrap_or(false);
                Some(l && r)
            }
            BinaryOp::Or => {
                let l = eval_having_bool(left, group, rep, datastore).unwrap_or(false);
                let r = eval_having_bool(right, group, rep, datastore).unwrap_or(false);
                Some(l || r)
            }
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge => {
                let l = eval_expr_in_group(left, group, rep, datastore)?;
                let r = eval_expr_in_group(right, group, rep, datastore)?;
                let ord = compare_graph_elements(&l, &r)?;
                Some(match op {
                    BinaryOp::Eq => ord == 0,
                    BinaryOp::Ne => ord != 0,
                    BinaryOp::Lt => ord < 0,
                    BinaryOp::Gt => ord > 0,
                    BinaryOp::Le => ord <= 0,
                    BinaryOp::Ge => ord >= 0,
                    _ => unreachable!(),
                })
            }
            _ => None,
        },
        Expression::Unary(UnaryOp::Not, inner) => {
            Some(!eval_having_bool(inner, group, rep, datastore).unwrap_or(false))
        }
        _ => eval_expression_bool(
            expr,
            rep,
            datastore,
            &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
        ),
    }
}

/// Compute an aggregate function over a group of solutions.
fn eval_aggregate_value(
    agg: &Aggregate,
    group: &[PartialSub],
    datastore: &Datastore,
) -> Option<GraphElement> {
    match agg {
        Aggregate::CountStar => Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
            BigInt::from(group.len()),
        ))),

        Aggregate::Count(expr, distinct) => {
            let mut values: Vec<GraphElement> = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore))
                .collect();
            if *distinct {
                let set: HashSet<_> = values.drain(..).collect();
                values.extend(set);
            }
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(values.len()),
            )))
        }

        Aggregate::Sum(expr, distinct) => {
            let mut values: Vec<GraphElement> = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore))
                .collect();
            if *distinct {
                let set: HashSet<_> = values.drain(..).collect();
                values.extend(set);
            }
            sum_values(&values)
        }

        Aggregate::Avg(expr, distinct) => {
            let mut values: Vec<GraphElement> = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore))
                .collect();
            if *distinct {
                let set: HashSet<_> = values.drain(..).collect();
                values.extend(set);
            }
            if values.is_empty() {
                return None;
            }
            let sum = sum_values(&values)?;
            let sum_lit = match &sum {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            };
            let sum_n = classify_numeric(sum_lit)?;
            let count = values.len();
            // Divide, preserving the same integer < decimal < float < double
            // type-promotion `sum_values` already applied to the sum: a
            // `double`/`float` sum stays floating-point, otherwise the
            // division is exact `xsd:decimal` (never plain `xsd:integer` —
            // SPARQL/XPath `op:numeric-divide` never returns integer,
            // matching AVG/AVG-with-GROUP-BY's expected `xsd:decimal` results
            // rather than the previous unconditional `xsd:double`). See
            // <https://github.com/daghovland/rdf-datalog/issues/202>.
            match sum_n {
                NumericLit::Double(f) => {
                    Some(numeric_lit_to_element(NumericLit::Double(f / count as f64)))
                }
                NumericLit::Float(f) => {
                    Some(numeric_lit_to_element(NumericLit::Float(f / count as f64)))
                }
                _ => {
                    let sum_d = numeric_lit_to_decimal(&sum_n)?;
                    let count_d = rust_decimal::Decimal::from(count);
                    Some(numeric_lit_to_element(NumericLit::Decimal(sum_d / count_d)))
                }
            }
        }

        // MIN/MAX use the `<` operator's comparison semantics
        // (`compare_graph_elements`, which returns `None` for operand pairs
        // with no defined ordering — e.g. a numeric literal against a blank
        // node), not `ORDER BY`'s total extended ordering
        // (`compare_graph_elements_total`). Per SPARQL 1.1, if `<` is
        // undefined for any pair of values in the group, the aggregate itself
        // errors and produces no binding, rather than silently falling back
        // to one of the two operands as the previous `reduce`-with-`_ => b`
        // fallback did. See the W3C `aggregates` suite's "Error in AVG"
        // (`agg-err-01`, mixed numeric-literal/blank-node group under `:y`)
        // and <https://github.com/daghovland/rdf-datalog/issues/202>.
        Aggregate::Min(expr, _) => {
            let mut values = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore));
            let mut current = values.next()?;
            for v in values {
                match compare_graph_elements(&current, &v) {
                    Some(ord) => {
                        if ord > 0 {
                            current = v;
                        }
                    }
                    None => return None,
                }
            }
            Some(current)
        }

        Aggregate::Max(expr, _) => {
            let mut values = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore));
            let mut current = values.next()?;
            for v in values {
                match compare_graph_elements(&current, &v) {
                    Some(ord) => {
                        if ord < 0 {
                            current = v;
                        }
                    }
                    None => return None,
                }
            }
            Some(current)
        }

        Aggregate::Sample(expr, _) => group
            .iter()
            .find_map(|sub| eval_expression_value_inner(expr, sub, datastore)),

        Aggregate::GroupConcat(expr, sep, distinct) => {
            let mut parts: Vec<String> = group
                .iter()
                .filter_map(|sub| {
                    let el = eval_expression_value_inner(expr, sub, datastore)?;
                    graph_element_to_string(&el)
                })
                .collect();
            if *distinct {
                let set: HashSet<_> = parts.drain(..).collect();
                parts.extend(set);
                parts.sort();
            }
            let result = parts.join(sep);
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                result,
            )))
        }
    }
}

/// Sum a list of numeric `GraphElement` values, applying the same
/// SPARQL/XPath numeric type-promotion rules `eval_arithmetic` uses:
/// `xsd:integer` stays exact if every value is an integer, an integer/decimal
/// mix stays exact `xsd:decimal`, and only a genuinely `xsd:double`/
/// `xsd:float` value forces floating-point.
///
/// Uses `classify_numeric` (rather than matching only the native
/// `IntegerLiteral` variant, as a prior version of this function did) so a
/// `TypedLiteral{xsd:integer, ..}` input — which is what every numeric BIND
/// function/cast now produces (#228) as well as what real parsed data always
/// uses — is recognized as an integer instead of silently falling through to
/// the floating-point path (e.g. `SUM(xsd:integer(...))` wrongly summing to
/// an `xsd:double`).
fn sum_values(values: &[GraphElement]) -> Option<GraphElement> {
    if values.is_empty() {
        return Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(0))));
    }
    let mut classified = Vec::with_capacity(values.len());
    for v in values {
        let lit = match v {
            GraphElement::GraphLiteral(lit) => lit,
            _ => return None,
        };
        classified.push(classify_numeric(lit)?);
    }
    if classified
        .iter()
        .any(|n| matches!(n, NumericLit::Double(_)))
    {
        let sum: f64 = classified.iter().map(numeric_lit_to_f64).sum();
        return Some(numeric_lit_to_element(NumericLit::Double(sum)));
    }
    if classified.iter().any(|n| matches!(n, NumericLit::Float(_))) {
        let sum: f64 = classified.iter().map(numeric_lit_to_f64).sum();
        return Some(numeric_lit_to_element(NumericLit::Float(sum)));
    }
    if classified
        .iter()
        .all(|n| matches!(n, NumericLit::Integer(_)))
    {
        let mut int_sum = BigInt::from(0);
        for n in &classified {
            if let NumericLit::Integer(i) = n {
                int_sum += i;
            }
        }
        return Some(numeric_lit_to_element(NumericLit::Integer(int_sum)));
    }
    // Remaining case: an integer/decimal mix with at least one decimal value.
    let mut dec_sum = rust_decimal::Decimal::from(0);
    for n in &classified {
        dec_sum += numeric_lit_to_decimal(n)?;
    }
    Some(numeric_lit_to_element(NumericLit::Decimal(dec_sum)))
}

/// Resolve a template term to a concrete `GraphElement`, remapping blank nodes per solution.
///
/// Returns `None` if the term is an unbound variable (triple is silently skipped).
fn bind_template_term(
    term: &Term,
    sub: &PartialSub,
    datastore: &Datastore,
    bnode_map: &mut HashMap<u32, u32>,
    bnode_counter: &mut u32,
) -> Option<GraphElement> {
    match term {
        Term::Variable(v) => sub.get(v).map(|val| val.resolve(datastore)),
        Term::Constant(gel) => {
            if let GraphElement::NodeOrEdge(dag_rdf::RdfResource::AnonymousBlankNode(orig_id)) = gel
            {
                // Each solution gets a fresh blank node for each distinct label.
                let fresh_id = bnode_map.entry(*orig_id).or_insert_with(|| {
                    let id = *bnode_counter;
                    *bnode_counter += 1;
                    id
                });
                Some(GraphElement::NodeOrEdge(
                    dag_rdf::RdfResource::AnonymousBlankNode(*fresh_id),
                ))
            } else {
                Some(gel.clone())
            }
        }
        // CONSTRUCT templates containing a triple term are out of scope for
        // phase R3 (#146); skip the triple rather than emit something wrong.
        Term::TripleTerm(_) => None,
    }
}

#[cfg(test)]
mod resolve_match_term_tests {
    use super::*;
    use num_bigint::BigInt;

    /// #154: a variable bound (e.g. via `BIND`) to a computed value that was
    /// never interned into the datastore must resolve to `MatchTerm::Never`,
    /// not `MatchTerm::Wildcard` — that exact value structurally cannot
    /// appear in any stored quad, so treating the position as unconstrained
    /// would (absent the defensive recheck every current call site happens
    /// to perform, see the comment on the `Term::Variable` arm above) wrongly
    /// let the pattern match every quad in that position instead of none.
    ///
    /// This is a white-box unit test on the private `resolve_match_term`
    /// function directly, rather than an end-to-end query test, precisely
    /// because every current caller's own downstream equality recheck
    /// already masks the difference in final query results — see the
    /// black-box regression test
    /// `test_sparql_bind_computed_value_not_interned_matches_nothing` in
    /// `tests/sparql12_suite.rs`, which passes both before and after this
    /// fix and therefore cannot discriminate red from green on its own.
    #[test]
    fn variable_bound_to_never_interned_value_resolves_to_never() {
        let ds = Datastore::new(10);

        // A computed value that was never added to `ds` at all, e.g. the
        // result of `BIND(?x + 1000000 AS ?y)` where `1000001` never
        // otherwise appears as a term in the store.
        let computed =
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(1_000_001_i64)));
        let mut sub: PartialSub = HashMap::new();
        sub.insert("y".to_string(), PartialSubValue::Computed(computed));

        let term = Term::Variable("y".to_string());
        let result = resolve_match_term(&term, &sub, &ds);

        assert!(
            matches!(result, MatchTerm::Never),
            "a variable bound to a value never interned into the store must \
             resolve to MatchTerm::Never (it can never match any real quad), \
             not MatchTerm::Wildcard"
        );
    }

    /// Sanity check: a genuinely unbound variable is still a `Wildcard`,
    /// distinguishing it from the bound-but-never-interned case above.
    #[test]
    fn unbound_variable_resolves_to_wildcard() {
        let ds = Datastore::new(10);
        let sub: PartialSub = HashMap::new();
        let term = Term::Variable("z".to_string());

        let result = resolve_match_term(&term, &sub, &ds);

        assert!(
            matches!(result, MatchTerm::Wildcard),
            "a genuinely unbound variable must still resolve to MatchTerm::Wildcard"
        );
    }

    /// Sanity check: a variable bound to a value that *is* interned resolves
    /// to `Bound` with the corresponding id.
    #[test]
    fn variable_bound_to_interned_value_resolves_to_bound() {
        let mut ds = Datastore::new(10);
        let resource = GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(42)));
        let id = ds.add_resource(resource.clone());

        let mut sub: PartialSub = HashMap::new();
        sub.insert("y".to_string(), PartialSubValue::Interned(id));
        let term = Term::Variable("y".to_string());

        let result = resolve_match_term(&term, &sub, &ds);

        assert!(
            matches!(result, MatchTerm::Bound(bound_id) if bound_id == id),
            "a variable bound to an interned value must resolve to MatchTerm::Bound(its id)"
        );
    }
}

#[cfg(test)]
mod partial_sub_value_tests {
    use super::*;
    use num_bigint::BigInt;

    /// #141: the whole reason [`PartialSubValue`] deliberately omits a derived
    /// `PartialEq` is that representation-level equality is wrong — an
    /// `Interned(id)` binding (from a triple-pattern match) and a
    /// `Computed(gel)` binding (from `BIND`/`VALUES`) can denote the *same*
    /// element. [`psv_eq`] must compare by resolved value, so a cross-variant
    /// pair pointing at one interned element compares equal, while distinct
    /// elements do not.
    #[test]
    fn psv_eq_compares_cross_variant_by_resolved_value() {
        let mut ds = Datastore::new(10);
        let resource = GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(42)));
        let id = ds.add_resource(resource.clone());
        let other = GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(43)));

        let interned = PartialSubValue::Interned(id);
        let computed_same = PartialSubValue::Computed(resource);
        let computed_other = PartialSubValue::Computed(other);

        assert!(
            psv_eq(&interned, &computed_same, &ds),
            "an Interned id and a Computed value denoting the same element must be equal"
        );
        assert!(
            psv_eq(&computed_same, &interned, &ds),
            "psv_eq must be symmetric across variants"
        );
        assert!(
            !psv_eq(&interned, &computed_other, &ds),
            "bindings denoting different elements must not be equal"
        );
    }

    /// #141: [`PartialSubValue::to_id`] must reproduce the pre-refactor
    /// `resource_map.get(gel)` lookup — an `Interned` binding already carries
    /// its id; a `Computed` binding yields an id only when that value happens
    /// to be interned, and `None` for a computed value (e.g. a `BIND`
    /// arithmetic result) that was never added to the store.
    #[test]
    fn to_id_resolves_interned_and_present_computed_but_not_absent() {
        let mut ds = Datastore::new(10);
        let resource = GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(7)));
        let id = ds.add_resource(resource.clone());

        assert_eq!(
            PartialSubValue::Interned(id).to_id(&ds),
            Some(id),
            "an Interned binding must return its own id"
        );
        assert_eq!(
            PartialSubValue::Computed(resource).to_id(&ds),
            Some(id),
            "a Computed value that is interned must return the matching id"
        );

        let never_interned =
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(1_000_001_i64)));
        assert_eq!(
            PartialSubValue::Computed(never_interned).to_id(&ds),
            None,
            "a Computed value never added to the store has no id"
        );
    }
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

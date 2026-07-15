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
    Aggregate, BinaryOp, DatasetClause, Expression, OrderCondition, ProjectionElement,
    PropertyPath, Query, QueryComponent, Term, TriplePattern, UnaryOp,
};
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, DEFAULT_GRAPH_ELEMENT_ID};
use ingress::{
    IriReference, NetworkPolicy, XSD_BOOLEAN, XSD_DECIMAL, XSD_DOUBLE, XSD_FLOAT, XSD_INTEGER,
    XSD_STRING,
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
            order_by: _,
        } => {
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let solutions = eval_components(
                where_clause,
                initial,
                datastore,
                dataset_active_graph(dataset, datastore),
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
                    sub.values().cloned().collect()
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
            QueryComponent::Filter(_) | QueryComponent::Values(_, _) => {}
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
    var.starts_with("__path_")
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
fn project_with_exprs(
    sub: &PartialSub,
    projection: &[ProjectionElement],
    datastore: &Datastore,
) -> SolutionRow {
    let mut row: SolutionRow = HashMap::new();
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
                if let Some(val) = eval_expression_value(expr, sub, datastore) {
                    row.insert(alias.clone(), val);
                }
            }
        }
    }
    row
}

// ── Evaluation ────────────────────────────────────────────────────────────────

/// Internal solution mapping: variable → concrete graph element.
///
/// Uses `GraphElement` values directly (not interned IDs) so that computed
/// values from `BIND` expressions can be stored without requiring a mutable
/// reference to the datastore.
type PartialSub = SolutionRow;

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
    let mut current = solutions;
    for comp in components {
        current = eval_component(comp, current, datastore, &active_graph);
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
) -> Vec<PartialSub> {
    match comp {
        QueryComponent::BGP(tps) => eval_bgp(tps, solutions, datastore, active_graph),

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
                        .filter_map(|inner_sub| merge_solutions(&outer_sub, inner_sub))
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

        QueryComponent::Minus(inner) => solutions
            .into_iter()
            .filter(|sub| {
                let minus_sols =
                    eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
                minus_sols.is_empty() || minus_sols.iter().all(|ms| !compatible(sub, ms))
            })
            .collect(),

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
                        if let Some(gel) = sub.get(var) {
                            if let Some(&graph_id) = datastore.resources.resource_map.get(gel) {
                                ActiveGraph::Fixed(graph_id)
                            } else {
                                ActiveGraph::Variable(var.clone())
                            }
                        } else {
                            ActiveGraph::Variable(var.clone())
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
            .filter_map(|mut sub| {
                let val = eval_bind_expr(expr, &sub, datastore)?;
                sub.insert(alias.clone(), val);
                Some(sub)
            })
            .collect(),

        QueryComponent::Values(vars, rows) => {
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
                            match new_sub.get(var) {
                                Some(existing) if existing != gel => {
                                    ok = false;
                                    break;
                                }
                                _ => {
                                    new_sub.insert(var.clone(), gel.clone());
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

        QueryComponent::Service(_, inner, _) => {
            // SERVICE not supported; return empty
            let _ = inner;
            Vec::new()
        }
    }
}

/// Two substitutions are compatible if they agree on all shared variables.
fn compatible(a: &PartialSub, b: &PartialSub) -> bool {
    for (var, gel_a) in a {
        if let Some(gel_b) = b.get(var) {
            if gel_a != gel_b {
                return false;
            }
        }
    }
    true
}

// ── BGP evaluation ────────────────────────────────────────────────────────────

fn eval_bgp(
    patterns: &[TriplePattern],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Vec<PartialSub> {
    let already_bound: HashSet<String> = solutions
        .first()
        .map(|sub| sub.keys().cloned().collect())
        .unwrap_or_default();
    let order = crate::join_ordering::order_patterns(patterns, &already_bound, datastore);

    let mut current = solutions;
    for &idx in &order {
        let pattern = &patterns[idx];
        current = current
            .into_iter()
            .flat_map(|sub| eval_triple_pattern(pattern, &sub, datastore, active_graph))
            .collect();
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
            for (var, gel) in inner_bindings {
                match merged.get(&var) {
                    Some(existing) if existing != &gel => {
                        ok = false;
                        break;
                    }
                    _ => {
                        merged.insert(var, gel);
                    }
                }
            }
            if ok {
                results.extend(eval_triple_pattern_core(
                    tp,
                    Some(term_id),
                    &merged,
                    datastore,
                    active_graph,
                ));
            }
        }
        return results;
    }

    eval_triple_pattern_core(tp, None, sub, datastore, active_graph)
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
        ActiveGraph::Variable(v) => sub
            .get(v)
            .and_then(|gel| datastore.resources.resource_map.get(gel))
            .copied(),
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

    for quad in datastore.quads_matching(g, s, p, o) {
        let mut new_sub = sub.clone();
        let mut ok = true;

        // Bind a variable to the GraphElement resolved from a quad-field ID.
        macro_rules! bind {
            ($term:expr, $id:expr) => {
                if let Term::Variable(v) = $term {
                    let gel = datastore.resources.get_graph_element($id).clone();
                    match new_sub.get(v) {
                        Some(existing) if existing != &gel => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), gel);
                        }
                    }
                }
            };
        }

        bind!(&tp.subject, quad.subject);
        bind!(&tp.predicate, quad.predicate);
        bind!(&tp.object, quad.obj);

        if let ActiveGraph::Variable(graph_var) = active_graph {
            let gel = datastore
                .resources
                .get_graph_element(quad.triple_id)
                .clone();
            match new_sub.get(graph_var) {
                Some(existing) if existing != &gel => {
                    ok = false;
                }
                _ => {
                    new_sub.insert(graph_var.clone(), gel);
                }
            }
        }

        if ok {
            new_solutions.push(new_sub);
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
            Some(gel) => match datastore.resources.resource_map.get(gel) {
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
) -> Vec<(GraphElementId, HashMap<String, GraphElement>)> {
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
                Some(gel) => match datastore.resources.resource_map.get(gel) {
                    Some(&id) => Slot::Known(id),
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
        let mut bindings: HashMap<String, GraphElement> = HashMap::new();
        let mut ok = true;

        macro_rules! bind_free {
            ($slot:expr, $id:expr) => {
                if let Slot::Free(v) = $slot {
                    let gel = datastore.resources.get_graph_element($id).clone();
                    match bindings.get(v) {
                        Some(existing) if existing != &gel => ok = false,
                        _ => {
                            bindings.insert(v.clone(), gel);
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
    // Convert the Datalog ID-based substitution to the GraphElement-based
    // substitution expected by the SPARQL evaluator.
    let gel_sub: PartialSub = sub
        .iter()
        .map(|(var, &id)| {
            (
                var.clone(),
                datastore.resources.get_graph_element(id).clone(),
            )
        })
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
        sub,
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
/// `sub` maps variable names to their current bindings.  Constants in the
/// query (e.g. `"SPARQL"` in `regex(?x, "SPARQL")`) are returned directly
/// without touching the datastore.
///
/// Returns `None` when the expression is unbound or evaluation fails (e.g.
/// division by zero, type mismatch).
/// See: <https://github.com/daghovland/rdf-datalog/issues/60>
pub fn eval_expression_value(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match expr {
        Expression::Variable(v) => sub.get(v).cloned(),
        Expression::Constant(gel) => Some(gel.clone()),
        Expression::FunctionCall(name, args) => eval_function_value(name, args, sub, datastore),
        Expression::Binary(l, op, r) => eval_arithmetic(l, op, r, sub, datastore),
        Expression::Unary(UnaryOp::Plus, inner) => eval_expression_value(inner, sub, datastore),
        Expression::Unary(UnaryOp::Minus, inner) => {
            arithmetic_negate(eval_expression_value(inner, sub, datastore)?)
        }
        _ => None,
    }
}

/// Negate a numeric literal.
fn arithmetic_negate(el: GraphElement) -> Option<GraphElement> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => {
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(-n)))
        }
        GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d)) => {
            Some(GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(-d)))
        }
        GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(d)) => Some(
            GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral((-d.into_inner()).into())),
        ),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
            ref type_iri,
            ref literal,
        }) => {
            if type_iri.0 == XSD_INTEGER {
                let n: i64 = literal.parse().ok()?;
                Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                    BigInt::from(-n),
                )))
            } else {
                let f = literal.parse::<f64>().ok()?;
                Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                    (-f).into(),
                )))
            }
        }
        _ => None,
    }
}

/// Evaluate a binary arithmetic expression (Add/Sub/Mul/Div).
/// Returns `None` if operands are not numeric or op is not arithmetic.
fn eval_arithmetic(
    left: &Expression,
    op: &BinaryOp,
    right: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let l = eval_expression_value(left, sub, datastore)?;
    let r = eval_expression_value(right, sub, datastore)?;
    match (&l, &r) {
        (
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(a)),
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(b)),
        ) => {
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => {
                    if b == &BigInt::from(0) {
                        return None;
                    }
                    a / b
                }
                _ => return None,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                result,
            )))
        }
        _ => {
            // Promote to f64 for mixed / floating-point arithmetic
            let af = literal_to_f64(match &l {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            })?;
            let bf = literal_to_f64(match &r {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            })?;
            let result = match op {
                BinaryOp::Add => af + bf,
                BinaryOp::Sub => af - bf,
                BinaryOp::Mul => af * bf,
                BinaryOp::Div => {
                    if bf == 0.0 {
                        return None;
                    }
                    af / bf
                }
                _ => return None,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                result.into(),
            )))
        }
    }
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
    let text_el = eval_expression_value(args.first()?, sub, datastore)?;
    let text = graph_element_to_string(&text_el)?;
    let arg_el = eval_expression_value(args.get(1)?, sub, datastore)?;
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
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "STRSTARTS" | "STRENDS" | "CONTAINS" => {
            let b = eval_string_predicate(upper.as_str(), args, sub, datastore)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)))
        }
        "STRBEFORE" => {
            let text_el = eval_expression_value(args.first()?, sub, datastore)?;
            let text = graph_element_to_string(&text_el)?;
            let sep_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let sep = graph_element_to_string(&sep_el)?;
            let result = if sep.is_empty() {
                String::new()
            } else {
                match text.find(sep.as_str()) {
                    Some(idx) => text[..idx].to_string(),
                    None => String::new(),
                }
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                result,
            )))
        }
        "STRAFTER" => {
            let text_el = eval_expression_value(args.first()?, sub, datastore)?;
            let text = graph_element_to_string(&text_el)?;
            let sep_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let sep = graph_element_to_string(&sep_el)?;
            let result = if sep.is_empty() {
                text.clone()
            } else {
                match text.find(sep.as_str()) {
                    Some(idx) => text[idx + sep.len()..].to_string(),
                    None => String::new(),
                }
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                result,
            )))
        }
        "STR" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)))
        }
        "LANG" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            if let GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) = el {
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(lang)))
            } else {
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    String::new(),
                )))
            }
        }
        "STRLEN" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
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
            let el = eval_expression_value(args.first()?, sub, datastore)?;
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
                _ => return None,
            };
            Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
                IriReference(dt_iri),
            )))
        }
        // ── String functions ──────────────────────────────────────────────────
        "UCASE" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                s.to_uppercase(),
            )))
        }
        "LCASE" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                s.to_lowercase(),
            )))
        }
        "CONCAT" => {
            let mut result = String::new();
            for arg in args {
                let el = eval_expression_value(arg, sub, datastore)?;
                result.push_str(&graph_element_to_string(&el)?);
            }
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                result,
            )))
        }
        "SUBSTR" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s: Vec<char> = graph_element_to_string(&el)?.chars().collect();
            let start_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let start: usize = element_to_usize(&start_el)?.saturating_sub(1);
            let result: String = if let Some(len_expr) = args.get(2) {
                let len_el = eval_expression_value(len_expr, sub, datastore)?;
                let len: usize = element_to_usize(&len_el)?;
                s.iter().skip(start).take(len).collect()
            } else {
                s.iter().skip(start).collect()
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                result,
            )))
        }
        // ── Term construction ─────────────────────────────────────────────────
        "IRI" | "URI" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let iri_str = graph_element_to_string(&el)?;
            Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
                IriReference(iri_str),
            )))
        }
        "STRDT" => {
            let lex_el = eval_expression_value(args.first()?, sub, datastore)?;
            let literal = graph_element_to_string(&lex_el)?;
            let dt_el = eval_expression_value(args.get(1)?, sub, datastore)?;
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
            let lex_el = eval_expression_value(args.first()?, sub, datastore)?;
            let literal = graph_element_to_string(&lex_el)?;
            let lang_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let lang = graph_element_to_string(&lang_el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
                lang,
                literal,
            }))
        }
        // ── Type testing ──────────────────────────────────────────────────────
        "ISNUMERIC" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
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
            let a = eval_expression_value(args.first()?, sub, datastore)?;
            let b = eval_expression_value(args.get(1)?, sub, datastore)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
                a == b,
            )))
        }
        // ── Numeric functions ─────────────────────────────────────────────────
        "ABS" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => {
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                        if n < BigInt::from(0) { -n } else { n },
                    )))
                }
                GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d)) => Some(
                    GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d.abs())),
                ),
                GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(d)) => {
                    Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                        d.into_inner().abs().into(),
                    )))
                }
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                    ref type_iri,
                    ref literal,
                }) if type_iri.0 == XSD_INTEGER => {
                    let n: i64 = literal.parse().ok()?;
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                        BigInt::from(n.abs()),
                    )))
                }
                GraphElement::GraphLiteral(lit) => {
                    let f = literal_to_f64(&lit)?;
                    Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                        f.abs().into(),
                    )))
                }
                _ => None,
            }
        }
        "ROUND" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => {
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)))
                }
                GraphElement::GraphLiteral(lit) => {
                    let f = literal_to_f64(&lit)?;
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                        BigInt::from((f + 0.5).floor() as i64),
                    )))
                }
                _ => None,
            }
        }
        "CEIL" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => {
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)))
                }
                GraphElement::GraphLiteral(lit) => {
                    let f = literal_to_f64(&lit)?;
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                        BigInt::from(f.ceil() as i64),
                    )))
                }
                _ => None,
            }
        }
        "FLOOR" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => {
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)))
                }
                GraphElement::GraphLiteral(lit) => {
                    let f = literal_to_f64(&lit)?;
                    Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                        BigInt::from(f.floor() as i64),
                    )))
                }
                _ => None,
            }
        }
        // ── Logic / control ───────────────────────────────────────────────────
        "COALESCE" => args
            .iter()
            .find_map(|arg| eval_expression_value(arg, sub, datastore)),
        "IF" => {
            let cond_el = eval_expression_value(args.first()?, sub, datastore)?;
            let cond = element_to_bool(&cond_el)?;
            if cond {
                eval_expression_value(args.get(1)?, sub, datastore)
            } else {
                eval_expression_value(args.get(2)?, sub, datastore)
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
            let el = eval_expression_value(args.first()?, sub, datastore)?;
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
        "REPLACE" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            let pat_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let pat = graph_element_to_string(&pat_el)?;
            let rep_el = eval_expression_value(args.get(2)?, sub, datastore)?;
            let rep = graph_element_to_string(&rep_el)?;
            let flags = if let Some(flag_expr) = args.get(3) {
                if let Some(f_el) = eval_expression_value(flag_expr, sub, datastore) {
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
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                re.replace_all(&s, rep.as_str()).into_owned(),
            )))
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
        "NOW" => Some(GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(
            chrono::Utc::now(),
        ))),
        "YEAR" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Datelike;
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(dt.year()),
            )))
        }
        "MONTH" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Datelike;
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(dt.month()),
            )))
        }
        "DAY" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Datelike;
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(dt.day()),
            )))
        }
        "HOURS" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Timelike;
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(dt.hour()),
            )))
        }
        "MINUTES" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Timelike;
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(dt.minute()),
            )))
        }
        "SECONDS" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let dt = parse_xsd_datetime(&el)?;
            use chrono::Timelike;
            Some(GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(
                rust_decimal::Decimal::from(dt.second()),
            )))
        }
        "TZ" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let tz_str = extract_tz_string(&el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                tz_str,
            )))
        }
        // ── Hash functions ────────────────────────────────────────────────────
        "MD5" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            let hash = md5::compute(s.as_bytes());
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                format!("{hash:x}"),
            )))
        }
        "SHA1" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            use sha1::Digest;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                hex::encode(sha1::Sha1::digest(s.as_bytes())),
            )))
        }
        "SHA256" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            use sha2::Digest;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                hex::encode(sha2::Sha256::digest(s.as_bytes())),
            )))
        }
        "SHA384" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            use sha2::Digest;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                hex::encode(sha2::Sha384::digest(s.as_bytes())),
            )))
        }
        "SHA512" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
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

/// Parse an XSD dateTime (or date/gYear) graph element into a `chrono::DateTime<Utc>`.
/// Handles `DateTimeLiteral`, RFC 3339 `xsd:dateTime` strings, `xsd:date` (YYYY-MM-DD),
/// and `xsd:gYear` (YYYY) so that YEAR/MONTH/DAY work on all common date types.
fn parse_xsd_datetime(el: &GraphElement) -> Option<chrono::DateTime<chrono::Utc>> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(dt)) => Some(*dt),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            // Full RFC 3339 dateTime
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(literal.as_str()) {
                return Some(dt.with_timezone(&chrono::Utc));
            }
            // xsd:date (YYYY-MM-DD)
            if let Ok(d) = chrono::NaiveDate::parse_from_str(literal.as_str(), "%Y-%m-%d") {
                return d.and_hms_opt(0, 0, 0).map(|ndt| ndt.and_utc());
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
                let l = eval_expression_value(left, sub, datastore)?;
                let r = eval_expression_value(right, sub, datastore)?;
                Some(l == r)
            }
            BinaryOp::Ne => {
                let l = eval_expression_value(left, sub, datastore)?;
                let r = eval_expression_value(right, sub, datastore)?;
                Some(l != r)
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                let l = eval_expression_value(left, sub, datastore)?;
                let r = eval_expression_value(right, sub, datastore)?;
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
            let val = eval_expression_value(expr, sub, datastore)?;
            Some(list.iter().any(|item| {
                eval_expression_value(item, sub, datastore)
                    .map(|v| v == val)
                    .unwrap_or(false)
            }))
        }
        Expression::NotIn(expr, list) => {
            let val = eval_expression_value(expr, sub, datastore)?;
            Some(!list.iter().any(|item| {
                eval_expression_value(item, sub, datastore)
                    .map(|v| v == val)
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
            let el = eval_expression_value(expr, sub, datastore)?;
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
            let text_el = eval_expression_value(args.first()?, sub, datastore)?;
            let text = graph_element_to_string(&text_el)?;

            let pat_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let pattern = graph_element_to_string(&pat_el)?;

            // Flags (optional 3rd arg)
            let flags = if let Some(flag_expr) = args.get(2) {
                let fel = eval_expression_value(flag_expr, sub, datastore)?;
                graph_element_to_string(&fel).unwrap_or_default()
            } else {
                String::new()
            };

            let case_insensitive = flags.contains('i');
            let matches = if case_insensitive {
                text.to_lowercase().contains(&pattern.to_lowercase())
            } else {
                text.contains(pattern.as_str())
            };
            Some(matches)
        }
        "LANGMATCHES" => {
            let lang_el = eval_expression_value(args.first()?, sub, datastore)?;
            let lang = graph_element_to_string(&lang_el)?.to_lowercase();

            let range_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let range = graph_element_to_string(&range_el)?.to_lowercase();

            Some(if range == "*" {
                !lang.is_empty()
            } else {
                lang == range || lang.starts_with(&format!("{}-", range))
            })
        }
        "ISIRI" | "ISURI" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(_))
            ))
        }
        "ISBLANK" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::AnonymousBlankNode(_))
            ))
        }
        "ISLITERAL" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            Some(matches!(el, dag_rdf::GraphElement::GraphLiteral(_)))
        }
        _ => None,
    }
}

/// Evaluate an expression for use in `BIND`, returning its `GraphElement` value.
/// Supports variables, constants, arithmetic, and function calls.
fn eval_bind_expr(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    eval_expression_value(expr, sub, datastore)
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
fn merge_solutions(outer: &PartialSub, inner: &PartialSub) -> Option<PartialSub> {
    let mut merged = outer.clone();
    for (var, val) in inner {
        match merged.get(var) {
            Some(existing) if existing != val => return None,
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
    let solutions = eval_components(where_clause, initial, datastore, (*active_graph).clone());

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
                                row.insert(alias.clone(), val);
                            }
                        }
                        ProjectionElement::Star => {}
                    }
                }
                row
            })
            .collect()
    } else {
        // Build projected variables list (without expanding SELECT *)
        let vars: Vec<String> = projection
            .iter()
            .filter_map(|p| match p {
                ProjectionElement::Variable(v) => Some(v.clone()),
                ProjectionElement::Expression(_, alias) => Some(alias.clone()),
                ProjectionElement::Star => None,
            })
            .collect();
        let proj_vars: Option<Vec<String>> = if projection
            .iter()
            .any(|p| matches!(p, ProjectionElement::Star))
        {
            None // keep all vars for SELECT *
        } else {
            Some(vars)
        };
        solutions
            .into_iter()
            .map(|sub| {
                if let Some(ref pvars) = proj_vars {
                    pvars
                        .iter()
                        .filter_map(|v| sub.get(v).map(|val| (v.clone(), val.clone())))
                        .collect()
                } else {
                    sub
                }
            })
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
            let mut key: Vec<(String, GraphElement)> =
                row.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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
            let av = eval_expression_value(&cond.expression, a, datastore);
            let bv = eval_expression_value(&cond.expression, b, datastore);
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
            eval_triple_pattern(&tp, &sub, datastore, active_graph)
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
            let zero_hop = {
                let s_gel = resolve_term_to_gel(subject_term, &sub, datastore);
                let o_gel = resolve_term_to_gel(object_term, &sub, datastore);
                match (s_gel, o_gel) {
                    // Both bound: must be equal
                    (Some(s), Some(o)) if s == o => vec![sub.clone()],
                    // Subject bound, object unbound: bind object = subject
                    (Some(s), None) => {
                        if let Term::Variable(v) = object_term {
                            let mut new_sub = sub.clone();
                            new_sub.insert(v.clone(), s);
                            vec![new_sub]
                        } else {
                            Vec::new()
                        }
                    }
                    // Object bound, subject unbound: bind subject = object
                    (None, Some(o)) => {
                        if let Term::Variable(v) = subject_term {
                            let mut new_sub = sub.clone();
                            new_sub.insert(v.clone(), o);
                            vec![new_sub]
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                }
            };
            let one_hop = eval_path_pattern(
                subject_term,
                inner,
                object_term,
                sub,
                datastore,
                active_graph,
            );
            // Deduplicate (zero-hop and one-hop may produce the same solution)
            let mut result = zero_hop;
            for s in one_hop {
                if !result.contains(&s) {
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

        PropertyPath::NegatedSet(excluded) => {
            let g = match active_graph {
                ActiveGraph::Fixed(id) => Some(*id),
                ActiveGraph::Variable(v) => sub
                    .get(v)
                    .and_then(|gel| datastore.resources.resource_map.get(gel))
                    .copied(),
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
                let pred_gel = datastore
                    .resources
                    .get_graph_element(quad.predicate)
                    .clone();
                let pred_id = quad.predicate;
                if excluded_ids.contains(&pred_id) {
                    continue;
                }
                let mut new_sub = sub.clone();
                let mut ok = true;
                if let Term::Variable(v) = subject_term {
                    let gel = datastore.resources.get_graph_element(quad.subject).clone();
                    match new_sub.get(v) {
                        Some(existing) if existing != &gel => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), gel);
                        }
                    }
                }
                if let Term::Variable(v) = object_term {
                    let gel = datastore.resources.get_graph_element(quad.obj).clone();
                    match new_sub.get(v) {
                        Some(existing) if existing != &gel => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), gel);
                        }
                    }
                }
                // Suppress unused pred_gel warning
                let _ = pred_gel;
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
    _datastore: &Datastore,
) -> Option<GraphElement> {
    match term {
        Term::Variable(v) => sub.get(v).cloned(),
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
                if let Some(next_gel) = s.get("__tc_next") {
                    if visited.insert(next_gel.clone()) {
                        queue.push(next_gel.clone());
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
                        new_sub.insert(obj_var.clone(), gel);
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
                        if let Some(prev_gel) = s.get("__tc_prev") {
                            if visited.insert(prev_gel.clone()) {
                                queue.push(prev_gel.clone());
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
                        new_sub.insert(subj_var.clone(), gel);
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
                    ActiveGraph::Variable(v) => sub
                        .get(v)
                        .and_then(|gel| datastore.resources.resource_map.get(gel))
                        .copied(),
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
                    new_sub.insert(subj_var.clone(), s_gel.clone());
                    new_sub.insert(obj_var.clone(), o_gel);
                    if !results.contains(&new_sub) {
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
fn group_by_solutions(
    solutions: &[PartialSub],
    group_by: &[Expression],
    datastore: &Datastore,
) -> Vec<Vec<PartialSub>> {
    if group_by.is_empty() {
        return vec![solutions.to_vec()];
    }
    let mut map: Vec<(Vec<Option<GraphElement>>, Vec<PartialSub>)> = Vec::new();
    'outer: for sub in solutions {
        let key: Vec<Option<GraphElement>> = group_by
            .iter()
            .map(|expr| eval_expression_value(expr, sub, datastore))
            .collect();
        for (k, group) in &mut map {
            if *k == key {
                group.push(sub.clone());
                continue 'outer;
            }
        }
        map.push((key, vec![sub.clone()]));
    }
    map.into_iter().map(|(_, g)| g).collect()
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
                    row.insert(v.clone(), val.clone());
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
        _ => eval_expression_value(expr, rep, datastore),
    }
}

/// Evaluate a binary operation between two already-resolved values.
fn eval_binary_value(l: &GraphElement, op: &BinaryOp, r: &GraphElement) -> Option<GraphElement> {
    match (l, r) {
        (
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(a)),
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(b)),
        ) => {
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => {
                    if b == &BigInt::from(0) {
                        return None;
                    }
                    a / b
                }
                _ => return None,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                result,
            )))
        }
        _ => {
            let af = literal_to_f64(match l {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            })?;
            let bf = literal_to_f64(match r {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            })?;
            let result = match op {
                BinaryOp::Add => af + bf,
                BinaryOp::Sub => af - bf,
                BinaryOp::Mul => af * bf,
                BinaryOp::Div => {
                    if bf == 0.0 {
                        return None;
                    }
                    af / bf
                }
                _ => return None,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                result.into(),
            )))
        }
    }
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
                .filter_map(|sub| eval_expression_value(expr, sub, datastore))
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
                .filter_map(|sub| eval_expression_value(expr, sub, datastore))
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
                .filter_map(|sub| eval_expression_value(expr, sub, datastore))
                .collect();
            if *distinct {
                let set: HashSet<_> = values.drain(..).collect();
                values.extend(set);
            }
            if values.is_empty() {
                return None;
            }
            let sum = sum_values(&values)?;
            let count = values.len() as f64;
            let sum_f = literal_to_f64(match &sum {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            })?;
            Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                (sum_f / count).into(),
            )))
        }

        Aggregate::Min(expr, _) => group
            .iter()
            .filter_map(|sub| eval_expression_value(expr, sub, datastore))
            .reduce(|a, b| match compare_graph_elements(&a, &b) {
                Some(ord) if ord <= 0 => a,
                _ => b,
            }),

        Aggregate::Max(expr, _) => group
            .iter()
            .filter_map(|sub| eval_expression_value(expr, sub, datastore))
            .reduce(|a, b| match compare_graph_elements(&a, &b) {
                Some(ord) if ord >= 0 => a,
                _ => b,
            }),

        Aggregate::Sample(expr, _) => group
            .iter()
            .find_map(|sub| eval_expression_value(expr, sub, datastore)),

        Aggregate::GroupConcat(expr, sep, distinct) => {
            let mut parts: Vec<String> = group
                .iter()
                .filter_map(|sub| {
                    let el = eval_expression_value(expr, sub, datastore)?;
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

/// Sum a list of numeric `GraphElement` values, returning an `IntegerLiteral` if
/// all inputs are integers or a `DoubleLiteral` for mixed/floating-point inputs.
fn sum_values(values: &[GraphElement]) -> Option<GraphElement> {
    if values.is_empty() {
        return Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
            BigInt::from(0),
        )));
    }
    // Try integer-only sum first
    let mut int_sum = BigInt::from(0);
    let mut all_int = true;
    for v in values {
        match v {
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => int_sum += n,
            _ => {
                all_int = false;
                break;
            }
        }
    }
    if all_int {
        return Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
            int_sum,
        )));
    }
    // Fall back to f64 sum
    let mut f_sum = 0.0f64;
    for v in values {
        match v {
            GraphElement::GraphLiteral(lit) => f_sum += literal_to_f64(lit)?,
            _ => return None,
        }
    }
    Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
        f_sum.into(),
    )))
}

/// Resolve a template term to a concrete `GraphElement`, remapping blank nodes per solution.
///
/// Returns `None` if the term is an unbound variable (triple is silently skipped).
fn bind_template_term(
    term: &Term,
    sub: &PartialSub,
    _datastore: &Datastore,
    bnode_map: &mut HashMap<u32, u32>,
    bnode_counter: &mut u32,
) -> Option<GraphElement> {
    match term {
        Term::Variable(v) => sub.get(v).cloned(),
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
        sub.insert("y".to_string(), computed);

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
        sub.insert("y".to_string(), resource);
        let term = Term::Variable("y".to_string());

        let result = resolve_match_term(&term, &sub, &ds);

        assert!(
            matches!(result, MatchTerm::Bound(bound_id) if bound_id == id),
            "a variable bound to an interned value must resolve to MatchTerm::Bound(its id)"
        );
    }
}

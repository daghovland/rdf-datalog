/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::bgp::eval_bgp;
use super::expressions::{eval_bind_expr, eval_filter};
use super::paths::eval_path_pattern;
use super::solutions::{compatible, join_solutions_with_values};
use super::*;

pub(crate) fn eval_components(
    components: &[QueryComponent],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: ActiveGraph,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    eval_components_budgeted(
        components,
        solutions,
        datastore,
        active_graph,
        None,
        deadline,
    )
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
pub(crate) fn eval_components_budgeted(
    components: &[QueryComponent],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: ActiveGraph,
    budget: Option<usize>,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    // SPARQL 1.1 §18.2.2.8: every `FILTER` in a `GroupGraphPatternSub`
    // applies after ALL of that same scope's other elements have been
    // joined, regardless of the `FILTER`'s textual position among them (W3C
    // `bind08` — a `FILTER` written before a `BIND` it depends on must still
    // see that `BIND`'s result). Stable-partition `components` into
    // non-filters and filters, preserving each partition's relative order;
    // the non-filters are reordered/evaluated exactly as before, and the
    // filters are appended at the end so they always run last, over the
    // fully joined result of this scope. This does NOT reach into nested
    // scopes (`OPTIONAL`/`MINUS`/`UNION`/`Group`/`GRAPH`/`SERVICE` bodies are
    // evaluated recursively via their own call to this same function, so
    // their own filters are deferred only to the end of *their own* scope).
    let non_filters: Vec<QueryComponent> = components
        .iter()
        .filter(|c| !matches!(c, QueryComponent::Filter(_)))
        .cloned()
        .collect();
    let filters: Vec<&QueryComponent> = components
        .iter()
        .filter(|c| matches!(c, QueryComponent::Filter(_)))
        .collect();

    let mut ordered: Vec<&QueryComponent> =
        if crate::component_ordering::should_reorder(&non_filters) {
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
                &non_filters,
                &already_bound,
                &guaranteed_bound,
                datastore,
            )
        } else {
            non_filters.iter().collect()
        };
    ordered.extend(filters);

    let mut current = solutions;
    let last = ordered.len().saturating_sub(1);
    for (i, comp) in ordered.into_iter().enumerate() {
        deadline.check()?;
        let comp_budget = if i == last { budget } else { None };
        current = eval_component(
            comp,
            current,
            datastore,
            &active_graph,
            comp_budget,
            deadline,
        )?;
        if current.is_empty() {
            break;
        }
    }
    Ok(current)
}

/// Evaluate `inner` as its own independent scope (SPARQL 1.1 §18.2.2.8) —
/// starting from a single empty solution, never seeded with `outer_solutions`
/// — and then natural-join the result back against `outer_solutions` (a
/// compatibility-checked merge per outer row, mirroring the `Subquery` arm's
/// nested-loop join above).
///
/// This is the correct semantics for both `UNION` arms and a bare nested
/// `{ ... }` group: a `BIND`/`FILTER` positioned *inside* `inner` must not be
/// able to see a variable that is only bound *outside* it (W3C `bind07`/
/// `bind10`), which a "thread the outer solutions straight into `inner`'s
/// evaluation" approach would incorrectly allow. Since SPARQL join is
/// commutative/associative, this produces byte-identical results to the old
/// threaded approach whenever `inner` contains no such cross-scope
/// visibility trap — the difference is only observable (and only matters)
/// exactly in those trap cases. See issue #198.
pub(crate) fn eval_independent_then_join(
    inner: &[QueryComponent],
    outer_solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    let inner_sols = eval_components(
        inner,
        vec![HashMap::new()],
        datastore,
        active_graph.clone(),
        deadline,
    )?;
    let mut result = Vec::new();
    for outer_sub in outer_solutions {
        deadline.check()?;
        result.extend(
            inner_sols
                .iter()
                .filter_map(|inner_sub| merge_solutions(&outer_sub, inner_sub, datastore)),
        );
    }
    Ok(result)
}

pub(crate) fn eval_component(
    comp: &QueryComponent,
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    match comp {
        QueryComponent::BGP(tps) => {
            eval_bgp(tps, solutions, datastore, active_graph, budget, deadline)
        }

        QueryComponent::PathPattern(subject, path, object) => {
            let mut result = Vec::new();
            for sub in solutions {
                deadline.check()?;
                result.extend(eval_path_pattern(
                    subject,
                    path,
                    object,
                    sub,
                    datastore,
                    active_graph,
                    deadline,
                )?);
            }
            Ok(result)
        }

        QueryComponent::Subquery(inner_query) => {
            let inner_rows = execute_select_inner(inner_query, datastore, active_graph, deadline)?;
            let mut result = Vec::new();
            for outer_sub in solutions {
                deadline.check()?;
                result.extend(
                    inner_rows
                        .iter()
                        .filter_map(|inner_sub| merge_solutions(&outer_sub, inner_sub, datastore)),
                );
            }
            Ok(result)
        }

        QueryComponent::Filter(expr) => Ok(solutions
            .into_iter()
            .filter(|sub| eval_filter(expr, sub, datastore, active_graph))
            .collect()),

        QueryComponent::Optional(inner) => {
            let mut result = Vec::new();
            for sub in solutions {
                deadline.check()?;
                let extended = eval_components(
                    inner,
                    vec![sub.clone()],
                    datastore,
                    (*active_graph).clone(),
                    deadline,
                )?;
                if extended.is_empty() {
                    result.push(sub);
                } else {
                    result.extend(extended);
                }
            }
            Ok(result)
        }

        QueryComponent::Union(left, right) => {
            let left_sols = eval_independent_then_join(
                left,
                solutions.clone(),
                datastore,
                active_graph,
                deadline,
            )?;
            let right_sols =
                eval_independent_then_join(right, solutions, datastore, active_graph, deadline)?;
            let mut result = left_sols;
            result.extend(right_sols);
            Ok(result)
        }

        // A bare nested `{ ... }` group: its own scope (SPARQL 1.1
        // §18.2.2.8), evaluated independently of the outer solutions and
        // then joined back in, exactly like a `UNION` arm — see
        // `eval_independent_then_join`. Unlike `OPTIONAL`, a non-matching
        // inner solution drops the outer row entirely (this is a mandatory
        // join, not a left join). See issue #198.
        QueryComponent::Group(inner) => {
            eval_independent_then_join(inner, solutions, datastore, active_graph, deadline)
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

            let mut result = Vec::new();
            for sub in solutions {
                deadline.check()?;
                if !sub.keys().any(|k| inner_vars.contains(k)) {
                    // Domain-disjointness escape: statically impossible
                    // for this row to share a variable with the body.
                    result.push(sub);
                    continue;
                }
                let minus_sols = match &minus_solutions {
                    Some(sols) => sols,
                    None => {
                        let sols = eval_components(
                            inner,
                            vec![HashMap::new()],
                            datastore,
                            (*active_graph).clone(),
                            deadline,
                        )?;
                        minus_solutions.insert(sols)
                    }
                };
                // Exclude `sub` iff some μ2 is compatible with it AND
                // actually shares a bound variable with it — the
                // spec's `¬(¬compatible ∨ dom-disjoint)`.
                let excluded = minus_sols.iter().any(|ms| {
                    compatible(&sub, ms, datastore) && sub.keys().any(|k| ms.contains_key(k))
                });
                if !excluded {
                    result.push(sub);
                }
            }
            Ok(result)
        }

        QueryComponent::Graph(graph_term, inner) => {
            let mut result = Vec::new();
            for sub in solutions {
                deadline.check()?;
                let scoped_graph = match graph_term {
                    Term::Constant(gel) => {
                        let Some(&graph_id) = datastore.resources.resource_map.get(gel) else {
                            continue;
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
                    Term::TripleTerm(_) => continue,
                };
                result.extend(eval_components(
                    inner,
                    vec![sub],
                    datastore,
                    scoped_graph,
                    deadline,
                )?);
            }
            Ok(result)
        }

        QueryComponent::Bind(expr, alias) => Ok(solutions
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
            .collect()),

        QueryComponent::Values(vars, rows) => {
            Ok(join_solutions_with_values(solutions, vars, rows, datastore))
        }

        QueryComponent::Service(_, inner, _) => {
            // SERVICE not supported; return empty
            let _ = inner;
            Ok(Vec::new())
        }
    }
}

/// True if the same variable name appears in more than one position of the
/// triple pattern (subject/predicate/object plus the graph variable, when the
/// active graph is variable). Such repetition means a matched quad can be
/// dropped by the equality re-check in `eval_triple_pattern_core`, so a
/// quad-level `LIMIT` would under-produce and must not be applied.
pub(crate) fn pattern_repeats_variable(tp: &TriplePattern, active_graph: &ActiveGraph) -> bool {
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

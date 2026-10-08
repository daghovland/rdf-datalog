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
use crate::profile;

/// `profile::ProfileNode::kind` string for `comp`, mirroring
/// `explain::PlanNode`'s `"kind"` JSON tag exactly (one vocabulary, not
/// two) — see `crate::profile`'s module doc.
fn profile_kind(comp: &QueryComponent) -> &'static str {
    match comp {
        QueryComponent::BGP(_) => "BGP",
        QueryComponent::PathPattern(_, _, _) => "PathPattern",
        QueryComponent::Subquery(_) => "Subquery",
        QueryComponent::Optional(_) => "Optional",
        QueryComponent::Union(_, _) => "Union",
        QueryComponent::Filter(_) => "Filter",
        QueryComponent::Bind(_, _) => "Bind",
        QueryComponent::Values(_, _) => "Values",
        QueryComponent::Minus(_) => "Minus",
        QueryComponent::Graph(_, _) => "Graph",
        QueryComponent::Group(_) => "Group",
        QueryComponent::Service(_, _, _) => "Service",
    }
}

/// True for `QueryComponent` kinds whose `eval_component` arm may leave a
/// nested scope's children pending (BGP's own pattern loop, or a recursive
/// `eval_components`/`eval_independent_then_join` call) for
/// `eval_components_budgeted`'s generic wrapper to consume as this node's
/// `children`. False for leaf kinds, whose pending slot is explicitly
/// discarded instead — see `crate::profile`'s module doc.
///
/// `Filter` and `PathPattern` were leaves before #697: `Filter`'s own arm
/// now merges per-row `EXISTS`/`NOT EXISTS` nodes (`eval_filter` ->
/// `eval_expression_bool`'s `Exists`/`NotExists` arms in
/// `expressions.rs`), and `PathPattern`'s now merges per-row property-path
/// internals (`eval_path_pattern`'s composite arms in `paths.rs`) —  both
/// via the same `Vec<PartialSub>`-per-row + merge discipline `Optional`/
/// `Graph` already use below.
fn profile_has_children(comp: &QueryComponent) -> bool {
    matches!(
        comp,
        QueryComponent::BGP(_)
            | QueryComponent::PathPattern(_, _, _)
            | QueryComponent::Subquery(_)
            | QueryComponent::Optional(_)
            | QueryComponent::Union(_, _)
            | QueryComponent::Minus(_)
            | QueryComponent::Graph(_, _)
            | QueryComponent::Group(_)
            | QueryComponent::Filter(_)
    )
}

/// Rendered label for `comp`'s profile node, mirroring
/// `explain::explain_component`'s `detail` text for the same
/// `QueryComponent` variants (one rendering, not two that could drift).
fn profile_label(comp: &QueryComponent) -> Option<String> {
    match comp {
        QueryComponent::PathPattern(subject, path, object) => Some(format!(
            "{} {:?} {}",
            crate::explain::render_term(subject),
            path,
            crate::explain::render_term(object)
        )),
        QueryComponent::Filter(expr) => Some(format!("{expr:?}")),
        QueryComponent::Bind(expr, alias) => Some(format!("{expr:?} AS ?{alias}")),
        QueryComponent::Values(vars, rows) => Some(format!(
            "VALUES ({}) — {} row(s)",
            vars.join(" "),
            rows.len()
        )),
        QueryComponent::Graph(graph_term, _) => Some(crate::explain::render_term(graph_term)),
        QueryComponent::Service(endpoint, _, silent) => Some(format!(
            "{}{}",
            crate::explain::render_term(endpoint),
            if *silent { " SILENT" } else { "" }
        )),
        QueryComponent::BGP(_)
        | QueryComponent::Subquery(_)
        | QueryComponent::Optional(_)
        | QueryComponent::Union(_, _)
        | QueryComponent::Minus(_)
        | QueryComponent::Group(_) => None,
    }
}

pub(crate) fn eval_components(
    components: &[QueryComponent],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: ActiveGraph,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, ExecError> {
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
) -> Result<Vec<PartialSub>, ExecError> {
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

    // Opens a scope whose children — one `ProfileNode` per sibling
    // component in `ordered`, pushed below via `profile::finish` — become
    // this call's result (the list `eval_components`/`eval_component`'s
    // container arms, or the top-level caller, attach as a node's
    // `children` or as the whole query's root profile). `Drop` (not a
    // manual pop) is what lets a partial component list survive a
    // `deadline.check()?` early return. See `crate::profile`'s module doc.
    let _scope = profile::enter_scope();

    let mut current = solutions;
    let last = ordered.len().saturating_sub(1);
    for (i, comp) in ordered.into_iter().enumerate() {
        deadline.check()?;
        let comp_budget = if i == last { budget } else { None };
        // Per-component timing granularity: one `Instant` pair per
        // `eval_component` call (not per row) — see `crate::profile`'s
        // module doc.
        if profile::is_active() {
            let rows_in = current.len();
            let t0 = profile::now();
            current = eval_component(
                comp,
                current,
                datastore,
                &active_graph,
                comp_budget,
                deadline,
            )?;
            profile::finish(
                profile_kind(comp),
                profile_label(comp),
                rows_in,
                current.len(),
                t0.elapsed(),
                profile_has_children(comp),
            );
        } else {
            current = eval_component(
                comp,
                current,
                datastore,
                &active_graph,
                comp_budget,
                deadline,
            )?;
        }
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
) -> Result<Vec<PartialSub>, ExecError> {
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
) -> Result<Vec<PartialSub>, ExecError> {
    let ctx = EvalCtx::new(datastore, active_graph, deadline);
    match comp {
        QueryComponent::BGP(tps) => eval_bgp(tps, solutions, budget, ctx),

        QueryComponent::PathPattern(subject, path, object) => {
            // `eval_path_pattern` is called once per outer row (like
            // `Optional`'s body). Its composite `PropertyPath` arms
            // (`Sequence`, `Alternative`, ...) leave their own step-level
            // breakdown pending — see `paths.rs` and `crate::profile`'s
            // module doc, "`EXISTS`/`NOT EXISTS` and property-path
            // step-level timing" (#697) — so each row's pending children
            // must be taken and merged immediately, then re-set once at
            // the end for this call's own `profile::finish` to consume.
            let profiling = profile::is_active();
            let mut merged_children: Vec<profile::ProfileNode> = Vec::new();
            let mut result = Vec::new();
            for sub in solutions {
                deadline.check()?;
                let extended = eval_path_pattern(subject, path, object, sub, ctx)?;
                if profiling {
                    profile::merge_children(&mut merged_children, profile::take_pending_children());
                }
                result.extend(extended);
            }
            if profiling {
                profile::set_pending_children(merged_children);
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

        QueryComponent::Filter(expr) => {
            // `eval_filter` may evaluate an `EXISTS`/`NOT EXISTS`
            // sub-expression once per row being filtered (see
            // `expressions.rs`'s `eval_expression_bool`) — the same
            // per-row-body shape `Optional`'s inner evaluation has, so the
            // same take-and-merge discipline applies: each row's pending
            // children (if any) must be taken immediately, before the
            // next row's `eval_filter` call can overwrite the single
            // slot. See `crate::profile`'s module doc (#697). For a
            // `FILTER` with no `EXISTS`/`NOT EXISTS` at all, every row
            // leaves pending empty, so `merged_children` stays empty and
            // this is unobservable in the profile.
            let profiling = profile::is_active();
            let mut merged_children: Vec<profile::ProfileNode> = Vec::new();
            let mut result = Vec::new();
            for sub in solutions {
                let keep = eval_filter(expr, &sub, datastore, active_graph);
                if profiling {
                    profile::merge_children(&mut merged_children, profile::take_pending_children());
                }
                if keep {
                    result.push(sub);
                }
            }
            if profiling {
                profile::set_pending_children(merged_children);
            }
            Ok(result)
        }

        QueryComponent::Optional(inner) => {
            let mut result = Vec::new();
            // `inner` is evaluated once per outer row, so (when profiling)
            // each row's `eval_components` call sets the single
            // `pending_children` slot independently — must be taken and
            // merged immediately after each call, before the next row's
            // call overwrites it, then re-set once at the end so
            // `eval_components_budgeted`'s generic wrapper picks up the
            // *merged* (not just-the-last-row's) children. See
            // `crate::profile`'s module doc, "single-slot discipline".
            let mut merged_children: Vec<profile::ProfileNode> = Vec::new();
            for sub in solutions {
                deadline.check()?;
                let extended = eval_components(
                    inner,
                    vec![sub.clone()],
                    datastore,
                    (*active_graph).clone(),
                    deadline,
                )?;
                if profile::is_active() {
                    profile::merge_children(&mut merged_children, profile::take_pending_children());
                }
                if extended.is_empty() {
                    result.push(sub);
                } else {
                    result.extend(extended);
                }
            }
            if profile::is_active() {
                profile::set_pending_children(merged_children);
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
            // Must take the left arm's pending children before issuing the
            // right arm's call, which would otherwise overwrite the
            // single-slot value before anyone read it — see
            // `crate::profile`'s module doc, "single-slot discipline".
            let left_children = profile::is_active().then(profile::take_pending_children);
            let right_sols =
                eval_independent_then_join(right, solutions, datastore, active_graph, deadline)?;
            let right_children = profile::is_active().then(profile::take_pending_children);
            if let (Some(lc), Some(rc)) = (left_children, right_children) {
                profile::set_pending_children(vec![
                    profile::ProfileNode::wrapper("UnionLeft", lc),
                    profile::ProfileNode::wrapper("UnionRight", rc),
                ]);
            }
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
            // Like `Optional` above: `inner` is evaluated once per outer
            // row, so each row's pending children must be taken and merged
            // immediately, then re-set once at the end. See
            // `crate::profile`'s module doc, "single-slot discipline".
            let mut merged_children: Vec<profile::ProfileNode> = Vec::new();
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
                let row_result =
                    eval_components(inner, vec![sub], datastore, scoped_graph, deadline)?;
                if profile::is_active() {
                    profile::merge_children(&mut merged_children, profile::take_pending_children());
                }
                result.extend(row_result);
            }
            if profile::is_active() {
                profile::set_pending_children(merged_children);
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

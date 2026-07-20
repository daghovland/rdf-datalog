/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Component-level join reordering:
//! - semi-join pushdown across `UNION` branches (Phase C of
//!   `docs/plans/JOIN_REORDERING_PLAN.md`, issue
//!   [#38](https://github.com/daghovland/rdf-datalog/issues/38));
//! - hoisting a later, independent conjunct across an `OPTIONAL`/`MINUS`
//!   barrier (Phase C-2, issue
//!   [#174](https://github.com/daghovland/rdf-datalog/issues/174)).
//!
//! `eval_components` runs a group graph pattern as a left-to-right join
//! pipeline. When a `UNION` appears *before* a conjunct that constrains its
//! variables, the union is evaluated with empty outer solutions, so both arms
//! become full scans and the whole union is materialised before the downstream
//! pattern can filter it — the `union-constraint-large-join` blow-up.
//!
//! The executor already threads outer solutions into `UNION` arms (see
//! `eval_component`), so the fix is purely to schedule the constraining
//! conjunct *before* the union; its bindings then flow into the arms through
//! that existing path. Reordering components **within a maximal conjunctive
//! run** is result-preserving — bag-join is commutative/associative and
//! distributes over bag-union — so this can only change performance, never the
//! result multiset. Correctness therefore rests solely on *never reordering
//! across a barrier*.
//!
//! ## Hard vs. soft barriers (issue #174)
//!
//! `FILTER`/`BIND`/`SERVICE` are **hard barriers**: nothing is ever reordered
//! across them, in either direction (`FILTER`/`BIND` need their own referenced
//! variables already bound to evaluate at all — a different ordering
//! constraint than left-join semantics; `SERVICE` is unsupported and out of
//! scope here regardless).
//!
//! `OPTIONAL`/`MINUS` are **soft barriers**: they still never move themselves,
//! but a later, *independent* conjunct in the same group may be hoisted to run
//! *before* one. This is the standard outer-join-pushdown rewrite:
//!
//! ```text
//! (Ω1 OPT Ω2) ⋈ Ω3  =  (Ω1 ⋈ Ω3) OPT Ω2   iff   vars(Ω3) ∩ vars(Ω2) ⊆ vars(Ω1)
//! ```
//!
//! i.e. the later conjunct Ω3 must not touch any variable bound *exclusively*
//! inside the barrier's body (`vars(Ω2) \ vars(Ω1)`, the barrier's
//! *internal-only* variables). Violating this is a real bug, not a style
//! nit — see the module tests for the counter-example (a variable left
//! unbound by a non-matching `OPTIONAL` must remain free for a later pattern
//! to bind independently; hoisting that pattern would instead make it compete
//! with the `OPTIONAL` for the same binding).
//!
//! `MINUS` gets the identical condition applied to its own body's variables.
//! This is *not* copied blindly from `OPTIONAL` — SPARQL's `MINUS` has a
//! domain-disjointness quirk (a row sharing no variable at all with a `MINUS`
//! solution is never excluded) that in principle differs from a plain
//! anti-join. However, this codebase's actual `QueryComponent::Minus`
//! evaluation (see `eval_component` in `execute.rs`) threads the outer `sub`
//! into the inner body's evaluation and only ever *extends* it, so for any
//! well-formed body every produced inner solution already agrees with `sub`
//! on every variable they share — `compatible()` in `execute.rs` is
//! therefore effectively always `true` given any non-empty inner result, and
//! the domain-disjointness escape hatch is not actually exercised; the
//! current behaviour is equivalent to `FILTER NOT EXISTS { inner }`. That is
//! a pre-existing spec-compliance gap independent of reordering (not fixed
//! here — tracked separately as issue
//! [#187](https://github.com/daghovland/rdf-datalog/issues/187)). Crucially,
//! the `MINUS` reordering safety condition below does *not* rely on that
//! observation at all: hoisting only ever adds bindings for variables the
//! barrier's body never references, so it can't change whether the body's
//! evaluation is satisfiable either way — this holds for today's
//! `NOT EXISTS`-like behaviour and would hold equally for a spec-correct
//! domain-aware `MINUS`, so the condition stays sound regardless of whether
//! #187 is ever fixed.
//!
//! ## Why the internal-only-variable set must be conservative
//!
//! The set of variables "already bound" before a barrier must be an
//! *under*-approximation of what's truly guaranteed on every row (i.e. safe
//! to err small), never an over-approximation. Overstating it would understate
//! the barrier's internal-only variables and could permit an unsafe hoist.
//! Concretely: a `UNION` immediately before an `OPTIONAL`, where only one arm
//! binds a variable the `OPTIONAL` body also uses — e.g.
//! `{ ?s :pa ?x } UNION { ?s :pb ?a } OPTIONAL { ?a :q ?opt } ?a :r ?z`. Here
//! `?a` is *conditionally* bound (only by the second arm), so it must **not**
//! count as bound going into the `OPTIONAL`, or the trailing `?a :r ?z` would
//! wrongly look eligible to hoist. `must_bind_vars` below computes this
//! conservative "guaranteed on every row" set (a `UNION`'s contribution is the
//! *intersection* of its arms', not the union of them, unlike the
//! purely-heuristic `bound` set used for connectedness/cardinality scoring
//! elsewhere in this module, which is allowed to over-approximate because it
//! only affects performance, never correctness).

use crate::ast::{Expression, Query, QueryComponent, Term, TriplePattern};
use dag_rdf::Datastore;
use std::collections::HashSet;

/// Cheap gate used by `eval_components` before doing any work: reordering is
/// only ever beneficial for a group of at least two components that contains a
/// `UNION`, `OPTIONAL`, or `MINUS`. Keeping this check in the caller lets the
/// common path (a single-component group, e.g. most per-row `OPTIONAL`/
/// `MINUS`/`EXISTS` inner evaluations) stay completely allocation-free.
pub(crate) fn should_reorder(components: &[QueryComponent]) -> bool {
    components.len() >= 2
        && components.iter().any(|c| {
            matches!(
                c,
                QueryComponent::Union(_, _)
                    | QueryComponent::Optional(_)
                    | QueryComponent::Minus(_)
            )
        })
}

/// Return a reordered view of `components` for evaluation.
///
/// The sequence is split into segments by **hard** barriers
/// (`FILTER`/`BIND`/`SERVICE`), which keep their position and are never
/// crossed. Within a segment, maximal runs of conjunctive, reorderable
/// components are greedily ordered by `(connectedness desc, estimated
/// cardinality asc)` given the variables bound so far — but only when the run
/// actually contains a `UNION`; otherwise it is emitted unchanged. In
/// addition, whenever an `OPTIONAL`/`MINUS` (a **soft** barrier) is
/// encountered, any component in the immediately following run that is
/// independent of the barrier's *internal-only* variables (see module docs)
/// is hoisted to run before it.
///
/// `already_bound` seeds the heuristic connectedness/cardinality scoring
/// (over-approximation is fine — it only affects performance).
/// `guaranteed_bound` seeds the correctness-critical "definitely bound on
/// every row" set used for the `OPTIONAL`/`MINUS` hoisting decision, and must
/// be a true under-approximation — see module docs.
pub(crate) fn order_components<'a>(
    components: &'a [QueryComponent],
    already_bound: &HashSet<String>,
    guaranteed_bound: &HashSet<String>,
    datastore: &Datastore,
) -> Vec<&'a QueryComponent> {
    // Precompute, for each index, the set of variables guaranteed bound by
    // every component strictly before it, in the *original* textual order.
    // Using the original order (rather than tracking this incrementally as
    // hoisting decisions are made) is a deliberate simplification: it is
    // always a valid under-approximation (hoisting can only add *more*
    // guaranteed bindings than this table credits), so it can only be overly
    // conservative, never unsafe. See module docs for why conservatism here
    // is required, not just tidy.
    let mut guaranteed_before: Vec<HashSet<String>> = Vec::with_capacity(components.len());
    let mut guaranteed_acc = guaranteed_bound.clone();
    for comp in components {
        guaranteed_before.push(guaranteed_acc.clone());
        guaranteed_acc.extend(must_bind_vars(comp));
    }

    let mut out: Vec<&QueryComponent> = Vec::with_capacity(components.len());
    let mut bound: HashSet<String> = already_bound.clone();
    let mut pending_run: Vec<&QueryComponent> = Vec::new();

    let mut i = 0;
    while i < components.len() {
        let comp = &components[i];
        if is_hard_barrier(comp) {
            flush_run(&mut pending_run, &mut bound, datastore, &mut out);
            add_component_vars(comp, &mut bound);
            out.push(comp);
            i += 1;
        } else if is_soft_barrier(comp) {
            // Everything scheduled so far (including the run just before
            // this barrier) is now committed to running before it; flush it
            // first so `bound` reflects that when we compute the barrier's
            // internal-only variables below (used for the heuristic score of
            // hoisted candidates, not for the safety check itself, which uses
            // `guaranteed_before`).
            flush_run(&mut pending_run, &mut bound, datastore, &mut out);

            let internal_only = barrier_internal_only_vars(comp, &guaranteed_before[i]);

            // Look ahead into the immediately following contiguous run of
            // reorderable components (stopping at the next barrier of either
            // kind, or the end) for hoist candidates independent of
            // `internal_only`.
            let mut j = i + 1;
            let mut hoisted: Vec<&QueryComponent> = Vec::new();
            let mut leftover: Vec<usize> = Vec::new();
            while j < components.len() && !is_barrier(&components[j]) {
                let c = &components[j];
                let mut cvars = HashSet::new();
                collect_component_vars(c, &mut cvars);
                if cvars.is_disjoint(&internal_only) {
                    hoisted.push(c);
                } else {
                    leftover.push(j);
                }
                j += 1;
            }

            if !hoisted.is_empty() {
                flush_run(&mut hoisted, &mut bound, datastore, &mut out);
            }

            add_component_vars(comp, &mut bound);
            out.push(comp);

            for k in leftover {
                pending_run.push(&components[k]);
            }
            i = j;
        } else {
            pending_run.push(comp);
            i += 1;
        }
    }
    flush_run(&mut pending_run, &mut bound, datastore, &mut out);
    out
}

/// Emit `run` into `out` in evaluation order, updating `bound` with every
/// component's variables. Runs without a `UNION` (or shorter than two
/// components) are emitted unchanged; otherwise they are greedily ordered.
fn flush_run<'a>(
    run: &mut Vec<&'a QueryComponent>,
    bound: &mut HashSet<String>,
    datastore: &Datastore,
    out: &mut Vec<&'a QueryComponent>,
) {
    if run.is_empty() {
        return;
    }

    let worth_reordering =
        run.len() >= 2 && run.iter().any(|c| matches!(c, QueryComponent::Union(_, _)));

    if !worth_reordering {
        for comp in run.drain(..) {
            add_component_vars(comp, bound);
            out.push(comp);
        }
        return;
    }

    let mut remaining: Vec<&QueryComponent> = std::mem::take(run);
    while !remaining.is_empty() {
        // Greedy pick: highest connectedness first, then smallest estimated
        // cardinality. `pick_best` keeps the earliest candidate on a tie so a
        // genuinely indifferent choice preserves the original order.
        let best = pick_best(&remaining, bound, datastore);
        let comp = remaining.remove(best);
        add_component_vars(comp, bound);
        out.push(comp);
    }
}

/// Index of the best component to schedule next, breaking ties toward the
/// earliest candidate to keep reordering stable.
fn pick_best(
    remaining: &[&QueryComponent],
    bound: &HashSet<String>,
    datastore: &Datastore,
) -> usize {
    let key = |c: &QueryComponent| -> (usize, std::cmp::Reverse<usize>) {
        (
            connectedness(c, bound),
            std::cmp::Reverse(estimate_cardinality(c, datastore)),
        )
    };
    let mut best = 0;
    let mut best_key = key(remaining[0]);
    for (i, c) in remaining.iter().enumerate().skip(1) {
        let k = key(c);
        if k > best_key {
            best = i;
            best_key = k;
        }
    }
    best
}

/// Hard barriers: never reordered across, in either direction. `FILTER`/
/// `BIND` need their own referenced variables already bound to evaluate at
/// all (a different ordering constraint from left-join semantics), and
/// `SERVICE` is unsupported and out of scope for hoisting regardless (issue
/// #174 explicitly excludes all three).
fn is_hard_barrier(comp: &QueryComponent) -> bool {
    matches!(
        comp,
        QueryComponent::Filter(_) | QueryComponent::Bind(_, _) | QueryComponent::Service(_, _, _)
    )
}

/// Soft barriers: `OPTIONAL`/`MINUS` never move themselves (they are
/// non-commutative anti/left joins), but a later independent conjunct may be
/// hoisted *before* one — see module docs for the correctness condition.
fn is_soft_barrier(comp: &QueryComponent) -> bool {
    matches!(comp, QueryComponent::Optional(_) | QueryComponent::Minus(_))
}

/// Any barrier, hard or soft — used to bound the lookahead window for
/// `OPTIONAL`/`MINUS` hoisting (a candidate is only considered from the
/// immediately following run, stopping at the next barrier of either kind).
fn is_barrier(comp: &QueryComponent) -> bool {
    is_hard_barrier(comp) || is_soft_barrier(comp)
}

/// The variables an `OPTIONAL`/`MINUS` barrier's body references that are
/// *not* already guaranteed bound going into it — the set a later conjunct
/// must avoid touching to be safely hoistable before the barrier (see module
/// docs for the correctness condition and why `guaranteed_before` must be a
/// conservative under-approximation).
fn barrier_internal_only_vars(
    comp: &QueryComponent,
    guaranteed_before: &HashSet<String>,
) -> HashSet<String> {
    let mut inner_vars = HashSet::new();
    collect_component_vars(comp, &mut inner_vars);
    inner_vars.difference(guaranteed_before).cloned().collect()
}

/// Variables guaranteed to be bound on *every* solution row that survives
/// evaluating `comp`, given that its own inputs are already supplied. This
/// must be a conservative under-approximation: when a component's binding
/// behaviour is conditional (a `UNION` arm binding a variable the other
/// doesn't, `VALUES` rows with `UNDEF`, `OPTIONAL`/`MINUS` never guaranteeing
/// anything new, subqueries, `SERVICE`), returning too few variables here is
/// always safe (it only makes a barrier's internal-only-variable set look
/// larger than necessary, missing an optimisation); returning too many would
/// make it look smaller and could permit an unsafe hoist. See module docs.
fn must_bind_vars(comp: &QueryComponent) -> HashSet<String> {
    match comp {
        QueryComponent::BGP(patterns) => {
            let mut vars = HashSet::new();
            for tp in patterns {
                collect_pattern_vars(tp, &mut vars);
            }
            vars
        }
        QueryComponent::PathPattern(subject, _, object) => {
            let mut vars = HashSet::new();
            collect_term_vars(subject, &mut vars);
            collect_term_vars(object, &mut vars);
            vars
        }
        QueryComponent::Union(left, right) => {
            let l = must_bind_sequence(left);
            let r = must_bind_sequence(right);
            l.intersection(&r).cloned().collect()
        }
        QueryComponent::Graph(term, inner) => {
            let mut vars = must_bind_sequence(inner);
            if let Term::Variable(v) = term {
                vars.insert(v.clone());
            }
            vars
        }
        QueryComponent::Values(names, rows) => names
            .iter()
            .enumerate()
            .filter(|(idx, _)| {
                rows.iter()
                    .all(|row| matches!(row.get(*idx), Some(Some(_))))
            })
            .map(|(_, n)| n.clone())
            .collect(),
        // `Bind` drops the row entirely when the expression fails to
        // evaluate (see `eval_component`'s `Bind` arm in `execute.rs`), so
        // every *surviving* row does have the alias bound.
        QueryComponent::Bind(_, alias) => {
            let mut vars = HashSet::new();
            vars.insert(alias.clone());
            vars
        }
        // `Optional`/`Minus` never guarantee a new binding for what follows
        // (a non-matching `OPTIONAL` leaves its variables unbound; `Minus`
        // never binds anything into the outer solution at all). `Filter`
        // never binds. `Service` and `Subquery` are conservatively credited
        // with nothing, rather than spend the complexity to compute their
        // guaranteed set precisely.
        QueryComponent::Optional(_)
        | QueryComponent::Minus(_)
        | QueryComponent::Filter(_)
        | QueryComponent::Service(_, _, _)
        | QueryComponent::Subquery(_) => HashSet::new(),
    }
}

/// `must_bind_vars`, folded over a component sequence: since every component
/// in a conjunctive pipeline must succeed for a row to survive, the
/// guaranteed-bound set for the whole sequence is the union of each
/// component's own contribution.
fn must_bind_sequence(components: &[QueryComponent]) -> HashSet<String> {
    let mut vars = HashSet::new();
    for c in components {
        vars.extend(must_bind_vars(c));
    }
    vars
}

/// Number of a component's referenced variables that are already bound — the
/// primary ordering key. A higher count means more of the component's runtime
/// restriction comes from joins already performed.
fn connectedness(comp: &QueryComponent, bound: &HashSet<String>) -> usize {
    let mut vars = HashSet::new();
    collect_component_vars(comp, &mut vars);
    vars.iter().filter(|v| bound.contains(*v)).count()
}

/// Coarse output-size proxy for a conjunctive component, used only as the
/// tie-break after connectedness. `UNION` sums its arms' proxies; a `BGP` uses
/// its most selective pattern's index cardinality; unknown shapes (paths,
/// subqueries) are treated as large so they sort late unless already bound.
fn estimate_cardinality(comp: &QueryComponent, datastore: &Datastore) -> usize {
    match comp {
        QueryComponent::BGP(patterns) => patterns
            .iter()
            .map(|tp| crate::join_ordering::known_cardinality(tp, datastore))
            .min()
            .unwrap_or(usize::MAX),
        QueryComponent::Union(left, right) => {
            estimate_sequence(left, datastore).saturating_add(estimate_sequence(right, datastore))
        }
        QueryComponent::Values(_, rows) => rows.len(),
        QueryComponent::Graph(_, inner) => estimate_sequence(inner, datastore),
        // Unknown / not costed here: paths, subqueries, and any barrier that
        // slipped in (barriers only reach this via a union arm's sequence).
        _ => usize::MAX,
    }
}

/// Estimated size of a component sequence (e.g. one `UNION` arm): the most
/// selective leg, mirroring how an indexed nested-loop join is driven by its
/// cheapest starting pattern.
fn estimate_sequence(components: &[QueryComponent], datastore: &Datastore) -> usize {
    components
        .iter()
        .map(|c| estimate_cardinality(c, datastore))
        .min()
        .unwrap_or(usize::MAX)
}

/// Add every variable a component references to `bound`. Used both to seed
/// connectedness for later components and to advance the bound set after a
/// component is scheduled. Over-approximation (e.g. a variable bound in only
/// one `UNION` arm) is harmless: this feeds the ordering heuristic only, never
/// evaluation.
fn add_component_vars(comp: &QueryComponent, bound: &mut HashSet<String>) {
    collect_component_vars(comp, bound);
}

/// Union of every variable name referenced anywhere within `components`.
///
/// Used by `execute.rs`'s `MINUS` evaluation (issue
/// [#187](https://github.com/daghovland/rdf-datalog/issues/187)) to cheaply
/// detect when a `MINUS` body could not possibly share any variable with an
/// outer row — the SPARQL 1.1 §18.3 domain-disjointness escape. An
/// over-approximation is safe for that use: it can only cause a missed
/// short-circuit (falling through to the exact per-row domain check), never
/// an incorrect exclusion.
pub(crate) fn variables_in_components(components: &[QueryComponent]) -> HashSet<String> {
    let mut vars = HashSet::new();
    for c in components {
        collect_component_vars(c, &mut vars);
    }
    vars
}

/// Collect all variable names appearing anywhere in a component into `vars`.
fn collect_component_vars(comp: &QueryComponent, vars: &mut HashSet<String>) {
    match comp {
        QueryComponent::BGP(patterns) => {
            for tp in patterns {
                collect_pattern_vars(tp, vars);
            }
        }
        QueryComponent::PathPattern(subject, _, object) => {
            collect_term_vars(subject, vars);
            collect_term_vars(object, vars);
        }
        QueryComponent::Union(left, right) => {
            for c in left.iter().chain(right.iter()) {
                collect_component_vars(c, vars);
            }
        }
        QueryComponent::Optional(inner)
        | QueryComponent::Minus(inner)
        | QueryComponent::Service(_, inner, _) => {
            for c in inner {
                collect_component_vars(c, vars);
            }
        }
        QueryComponent::Graph(term, inner) => {
            collect_term_vars(term, vars);
            for c in inner {
                collect_component_vars(c, vars);
            }
        }
        QueryComponent::Values(names, _) => {
            for n in names {
                vars.insert(n.clone());
            }
        }
        QueryComponent::Bind(_, alias) => {
            vars.insert(alias.clone());
        }
        QueryComponent::Filter(expr) => collect_expr_vars(expr, vars),
        QueryComponent::Subquery(query) => collect_query_vars(query, vars),
    }
}

fn collect_pattern_vars(tp: &TriplePattern, vars: &mut HashSet<String>) {
    collect_term_vars(&tp.subject, vars);
    collect_term_vars(&tp.predicate, vars);
    collect_term_vars(&tp.object, vars);
}

fn collect_term_vars(term: &Term, vars: &mut HashSet<String>) {
    match term {
        Term::Variable(v) => {
            vars.insert(v.clone());
        }
        Term::Constant(_) => {}
        Term::TripleTerm(inner) => collect_pattern_vars(inner, vars),
    }
}

/// Collect variables an expression references. Only the variable leaves matter
/// for the heuristic; `EXISTS`/`NOT EXISTS` sub-patterns are descended into so
/// a filter's connectedness reflects them.
fn collect_expr_vars(expr: &Expression, vars: &mut HashSet<String>) {
    match expr {
        Expression::Variable(v) => {
            vars.insert(v.clone());
        }
        Expression::Constant(_) => {}
        Expression::Binary(l, _, r) => {
            collect_expr_vars(l, vars);
            collect_expr_vars(r, vars);
        }
        Expression::Unary(_, e) => collect_expr_vars(e, vars),
        Expression::FunctionCall(_, args) => {
            for a in args {
                collect_expr_vars(a, vars);
            }
        }
        Expression::In(e, list) => {
            collect_expr_vars(e, vars);
            for a in list {
                collect_expr_vars(a, vars);
            }
        }
        Expression::Exists(inner) | Expression::NotExists(inner) => {
            for c in inner {
                collect_component_vars(c, vars);
            }
        }
        // Aggregates and any other leaf shapes contribute no join variables
        // that matter to component ordering.
        _ => {}
    }
}

fn collect_query_vars(query: &Query, vars: &mut HashSet<String>) {
    let where_clause = match query {
        Query::Select { where_clause, .. }
        | Query::Ask { where_clause, .. }
        | Query::Construct { where_clause, .. }
        | Query::Describe { where_clause, .. } => where_clause,
    };
    for c in where_clause {
        collect_component_vars(c, vars);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::{GraphElement, IriReference, Quad, RdfResource};

    fn iri_node(iri: &str) -> GraphElement {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
    }

    fn var(name: &str) -> Term {
        Term::Variable(name.to_string())
    }

    fn iri_const(iri: &str) -> Term {
        Term::Constant(iri_node(iri))
    }

    fn add_quad(ds: &mut Datastore, s: &str, p: &str, o: &str) {
        let subject = ds.add_resource(iri_node(s));
        let predicate = ds.add_resource(iri_node(p));
        let object = ds.add_resource(iri_node(o));
        ds.add_quad(Quad {
            triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
            subject,
            predicate,
            obj: object,
        });
    }

    /// `{ ?s :pa ?o1 }`-style single-pattern BGP on predicate `p`.
    fn bgp_on(subject: &str, p: &str, object: &str) -> QueryComponent {
        QueryComponent::BGP(vec![TriplePattern {
            subject: var(subject),
            predicate: iri_const(p),
            object: var(object),
        }])
    }

    const PA: &str = "http://example.org/pa"; // large union arm predicate
    const PB: &str = "http://example.org/pb"; // large union arm predicate
    const PC: &str = "http://example.org/pc"; // smaller constraining predicate

    /// A UNION-then-constraining-BGP group must be reordered so the BGP is
    /// evaluated first, feeding its `?s` bindings into the union arms.
    #[test]
    fn moves_constraining_bgp_before_union() {
        let mut ds = Datastore::new(1_000);
        // Union arms are large; the constraining predicate is smaller.
        for i in 0..8 {
            add_quad(
                &mut ds,
                &format!("http://example.org/s{i}"),
                PA,
                "http://example.org/oa",
            );
        }
        for i in 0..8 {
            add_quad(
                &mut ds,
                &format!("http://example.org/s{i}"),
                PB,
                "http://example.org/ob",
            );
        }
        for i in 0..2 {
            add_quad(
                &mut ds,
                &format!("http://example.org/s{i}"),
                PC,
                "http://example.org/oc",
            );
        }

        let union = QueryComponent::Union(vec![bgp_on("s", PA, "o1")], vec![bgp_on("s", PB, "o1")]);
        let constraint = bgp_on("s", PC, "o2");
        let components = vec![union, constraint];

        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        assert!(
            matches!(order[0], QueryComponent::BGP(_)),
            "the constraining BGP (smaller cardinality) must be scheduled first"
        );
        assert!(
            matches!(order[1], QueryComponent::Union(_, _)),
            "the UNION must be scheduled after the constraint that feeds it"
        );
    }

    /// A run with no UNION is left exactly as written (Phase A handles
    /// intra-BGP ordering; this pass must not disturb non-union groups).
    #[test]
    fn leaves_union_free_group_untouched() {
        let mut ds = Datastore::new(1_000);
        add_quad(&mut ds, "http://example.org/s", PA, "http://example.org/o");

        // BGP followed by VALUES — both conjunctive, no union.
        let components = vec![
            bgp_on("s", PA, "o"),
            QueryComponent::Values(
                vec!["s".to_string()],
                vec![vec![Some(iri_node("http://example.org/s"))]],
            ),
        ];
        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        assert!(matches!(order[0], QueryComponent::BGP(_)));
        assert!(matches!(order[1], QueryComponent::Values(_, _)));
        assert_eq!(order.len(), 2);
    }

    /// A hard barrier (`FILTER`) must pin ordering absolutely: nothing is
    /// ever reordered across it, in either direction (issue #174 scope —
    /// unlike `OPTIONAL`/`MINUS`, `FILTER`/`BIND`/`SERVICE` stay full stops).
    #[test]
    fn does_not_reorder_across_filter_barrier() {
        let mut ds = Datastore::new(1_000);
        for i in 0..8 {
            add_quad(
                &mut ds,
                &format!("http://example.org/s{i}"),
                PA,
                "http://example.org/oa",
            );
        }
        add_quad(
            &mut ds,
            "http://example.org/s0",
            PC,
            "http://example.org/oc",
        );

        let union = QueryComponent::Union(vec![bgp_on("s", PA, "o1")], vec![bgp_on("s", PB, "o1")]);
        let filter =
            QueryComponent::Filter(Expression::Constant(iri_node("http://example.org/true")));
        let cheap_constraint = bgp_on("s", PC, "o2");
        let components = vec![union, filter, cheap_constraint];

        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        // Hard barrier keeps the original relative order across it: UNION,
        // then FILTER, then the constraint. The cheap constraint is NOT
        // hoisted across a hard barrier.
        assert!(matches!(order[0], QueryComponent::Union(_, _)));
        assert!(matches!(order[1], QueryComponent::Filter(_)));
        assert!(matches!(order[2], QueryComponent::BGP(_)));
    }

    /// Issue #174, positive case: a later conjunct independent of an
    /// `OPTIONAL`'s internal-only variables is hoisted to run before it.
    /// `?s :p1 ?x . OPTIONAL { ?s :p2 ?opt } . ?s :p3 ?z` — the trailing
    /// pattern only touches `?s` (already bound) and a fresh `?z`, never
    /// `?opt` (the `OPTIONAL`'s own internal-only variable), so it is safe to
    /// hoist.
    #[test]
    fn hoists_independent_conjunct_before_optional() {
        let ds = Datastore::new(1_000);
        let bgp1 = bgp_on("s", PA, "x");
        let optional = QueryComponent::Optional(vec![bgp_on("s", PB, "opt")]);
        let trailing = bgp_on("s", PC, "z");
        let components = vec![bgp1, optional, trailing];

        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        assert_eq!(order.len(), 3);
        let QueryComponent::BGP(first) = order[0] else {
            panic!("expected BGP first");
        };
        assert_eq!(first[0].predicate, iri_const(PA), "?s :p1 ?x stays first");
        let QueryComponent::BGP(second) = order[1] else {
            panic!("expected the hoisted trailing BGP second");
        };
        assert_eq!(
            second[0].predicate,
            iri_const(PC),
            "the independent trailing conjunct is hoisted before the OPTIONAL"
        );
        assert!(
            matches!(order[2], QueryComponent::Optional(_)),
            "the OPTIONAL is scheduled last"
        );
    }

    /// Issue #174 counter-example: a later conjunct that touches a variable
    /// bound *exclusively* inside the `OPTIONAL` must NOT be hoisted.
    /// `?s :p1 ?x . OPTIONAL { ?s :p2 ?opt } . ?opt :p3 ?z` — if `:p2`
    /// doesn't match, `?opt` stays unbound and the trailing pattern is free
    /// to bind it independently; hoisting would instead make it compete with
    /// the `OPTIONAL` for the same binding of `?opt` — a different join.
    #[test]
    fn does_not_hoist_conjunct_that_escapes_optional_binding() {
        let ds = Datastore::new(1_000);
        let bgp1 = bgp_on("s", PA, "x");
        let optional = QueryComponent::Optional(vec![bgp_on("s", PB, "opt")]);
        let escaping = bgp_on("opt", PC, "z");
        let components = vec![bgp1, optional.clone(), escaping.clone()];

        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        assert_eq!(order.len(), 3);
        assert!(matches!(order[0], QueryComponent::BGP(_)));
        assert!(
            matches!(order[1], QueryComponent::Optional(_)),
            "the OPTIONAL must stay in place — the trailing pattern touches its internal-only ?opt"
        );
        assert!(matches!(order[2], QueryComponent::BGP(_)));
    }

    /// Regression guard for the trap in a naive "over-approximate what's
    /// bound" implementation: a `UNION` immediately before an `OPTIONAL`
    /// where only ONE arm binds a variable the `OPTIONAL`'s body also uses.
    /// `?a` is only *conditionally* bound (by the second arm alone), so it
    /// must not count as "already bound" going into the `OPTIONAL` — an
    /// over-approximation here would wrongly treat `?opt`, but not `?a`, as
    /// the only internal-only variable and hoist the trailing `?a :p3 ?z`,
    /// which shares `?a` with the `OPTIONAL` body. `must_bind_vars` must
    /// compute a `UNION`'s guaranteed set as the *intersection* of its arms
    /// (here: `{s}` only, since `?a` is arm-2-only) for this to stay safe.
    #[test]
    fn does_not_hoist_across_optional_when_union_only_conditionally_binds() {
        let ds = Datastore::new(1_000);
        let union = QueryComponent::Union(vec![bgp_on("s", PA, "x")], vec![bgp_on("s", PB, "a")]);
        let optional = QueryComponent::Optional(vec![bgp_on("a", PC, "opt")]);
        let trailing = bgp_on("a", "http://example.org/pd", "z");
        let components = vec![union, optional, trailing];

        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        assert_eq!(order.len(), 3);
        assert!(matches!(order[0], QueryComponent::Union(_, _)));
        assert!(
            matches!(order[1], QueryComponent::Optional(_)),
            "the OPTIONAL must stay in place — ?a is only conditionally bound by one UNION arm"
        );
        assert!(matches!(order[2], QueryComponent::BGP(_)));
    }

    /// Issue #174, positive case for `MINUS`: same shape as the `OPTIONAL`
    /// positive test, using the same internal-only-variable condition
    /// (derived independently for `MINUS` in the module docs — this
    /// codebase's actual `Minus` evaluation threads bindings the same way
    /// `OPTIONAL` does, so the same safe condition applies).
    #[test]
    fn hoists_independent_conjunct_before_minus() {
        let ds = Datastore::new(1_000);
        let bgp1 = bgp_on("s", PA, "x");
        let minus = QueryComponent::Minus(vec![bgp_on("s", PB, "m")]);
        let trailing = bgp_on("s", PC, "z");
        let components = vec![bgp1, minus, trailing];

        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        assert_eq!(order.len(), 3);
        let QueryComponent::BGP(first) = order[0] else {
            panic!("expected BGP first");
        };
        assert_eq!(first[0].predicate, iri_const(PA));
        let QueryComponent::BGP(second) = order[1] else {
            panic!("expected the hoisted trailing BGP second");
        };
        assert_eq!(
            second[0].predicate,
            iri_const(PC),
            "the independent trailing conjunct is hoisted before the MINUS"
        );
        assert!(matches!(order[2], QueryComponent::Minus(_)));
    }

    /// `MINUS` counter-example, mirroring the `OPTIONAL` one: a later
    /// conjunct touching a variable used exclusively inside `MINUS`'s body
    /// must not be hoisted — doing so would change which rows the `MINUS`
    /// anti-join test excludes (see module docs on why this codebase's
    /// `Minus` evaluation is sensitive to this despite `MINUS` never binding
    /// anything into the outer solution).
    #[test]
    fn does_not_hoist_conjunct_that_escapes_minus_binding() {
        let ds = Datastore::new(1_000);
        let bgp1 = bgp_on("s", PA, "x");
        let minus = QueryComponent::Minus(vec![bgp_on("s", PB, "m")]);
        let escaping = bgp_on("m", PC, "z");
        let components = vec![bgp1, minus, escaping];

        let order = order_components(&components, &HashSet::new(), &HashSet::new(), &ds);
        assert_eq!(order.len(), 3);
        assert!(matches!(order[0], QueryComponent::BGP(_)));
        assert!(
            matches!(order[1], QueryComponent::Minus(_)),
            "the MINUS must stay in place — the trailing pattern touches its internal-only ?m"
        );
        assert!(matches!(order[2], QueryComponent::BGP(_)));
    }

    /// Connectedness outranks cardinality: when the shared variable is already
    /// bound, the UNION that reuses it is scheduled before an unrelated BGP,
    /// even if that BGP looks cheaper.
    #[test]
    fn connectedness_outranks_cardinality() {
        let mut ds = Datastore::new(1_000);
        for i in 0..8 {
            add_quad(
                &mut ds,
                &format!("http://example.org/s{i}"),
                PA,
                "http://example.org/oa",
            );
        }
        for i in 0..8 {
            add_quad(
                &mut ds,
                &format!("http://example.org/s{i}"),
                PB,
                "http://example.org/ob",
            );
        }
        // A cheap, *disconnected* BGP on a fresh variable `t`.
        add_quad(
            &mut ds,
            "http://example.org/t0",
            PC,
            "http://example.org/oc",
        );

        let union = QueryComponent::Union(vec![bgp_on("s", PA, "o1")], vec![bgp_on("s", PB, "o1")]);
        let disconnected = bgp_on("t", PC, "o2");
        let components = vec![union, disconnected];

        let mut already_bound = HashSet::new();
        already_bound.insert("s".to_string());

        let order = order_components(&components, &already_bound, &already_bound, &ds);
        assert!(
            matches!(order[0], QueryComponent::Union(_, _)),
            "the UNION shares the already-bound `?s` (connectedness 1) and must be scheduled before the cheaper but disconnected BGP"
        );
        assert!(matches!(order[1], QueryComponent::BGP(_)));
    }

    /// `should_reorder` gates out the hot path: single components and
    /// groups without a `UNION`/`OPTIONAL`/`MINUS` short-circuit before any
    /// allocation.
    #[test]
    fn should_reorder_gate() {
        let single = vec![bgp_on("s", PA, "o")];
        assert!(
            !should_reorder(&single),
            "a single component never reorders"
        );

        let no_union = vec![bgp_on("s", PA, "o"), bgp_on("s", PC, "o2")];
        assert!(
            !should_reorder(&no_union),
            "a group with no UNION/OPTIONAL/MINUS never reorders"
        );

        let with_union = vec![
            QueryComponent::Union(vec![bgp_on("s", PA, "o1")], vec![bgp_on("s", PB, "o1")]),
            bgp_on("s", PC, "o2"),
        ];
        assert!(should_reorder(&with_union));

        let with_optional = vec![
            bgp_on("s", PA, "x"),
            QueryComponent::Optional(vec![bgp_on("s", PB, "opt")]),
        ];
        assert!(
            should_reorder(&with_optional),
            "issue #174: an OPTIONAL-containing group must also be considered for reordering"
        );

        let with_minus = vec![
            bgp_on("s", PA, "x"),
            QueryComponent::Minus(vec![bgp_on("s", PB, "m")]),
        ];
        assert!(
            should_reorder(&with_minus),
            "issue #174: a MINUS-containing group must also be considered for reordering"
        );
    }
}

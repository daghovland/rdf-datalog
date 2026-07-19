/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Component-level join reordering — semi-join pushdown across `UNION` branches
//! (Phase C of `docs/plans/JOIN_REORDERING_PLAN.md`, issue
//! [#38](https://github.com/daghovland/rdf-datalog/issues/38)).
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
//! across a barrier* (`OPTIONAL`/`MINUS`/`FILTER`/`BIND`/`SERVICE`), which are
//! order-sensitive.
//!
//! `OPTIONAL` is deliberately **not** optimised: a semi-join cannot reduce the
//! left/outer rows of a left-join without violating its semantics, so there is
//! no safe win there.

use crate::ast::{Expression, Query, QueryComponent, Term, TriplePattern};
use dag_rdf::Datastore;
use std::collections::HashSet;

/// Cheap gate used by `eval_components` before doing any work: reordering is
/// only ever beneficial for a group of at least two components that contains a
/// `UNION`. Keeping this check in the caller lets the common path (including
/// the per-row `OPTIONAL`/`MINUS`/`EXISTS` inner evaluations, which are almost
/// always a single BGP) stay completely allocation-free.
pub(crate) fn should_reorder(components: &[QueryComponent]) -> bool {
    components.len() >= 2
        && components
            .iter()
            .any(|c| matches!(c, QueryComponent::Union(_, _)))
}

/// Return a reordered view of `components` for evaluation.
///
/// The sequence is split into maximal *runs* of conjunctive, reorderable
/// components separated by barriers; barriers keep their original position, and
/// each run is greedily ordered by `(connectedness desc, estimated cardinality
/// asc)` given the variables bound so far. Only runs that actually contain a
/// `UNION` are reordered; every other run is emitted unchanged.
pub(crate) fn order_components<'a>(
    components: &'a [QueryComponent],
    already_bound: &HashSet<String>,
    datastore: &Datastore,
) -> Vec<&'a QueryComponent> {
    let mut out: Vec<&QueryComponent> = Vec::with_capacity(components.len());
    let mut bound: HashSet<String> = already_bound.clone();
    let mut run: Vec<&QueryComponent> = Vec::new();

    for comp in components {
        if is_barrier(comp) {
            flush_run(&mut run, &mut bound, datastore, &mut out);
            add_component_vars(comp, &mut bound);
            out.push(comp);
        } else {
            run.push(comp);
        }
    }
    flush_run(&mut run, &mut bound, datastore, &mut out);
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

/// Components that must not be reordered across. `OPTIONAL`/`MINUS` are
/// non-commutative (anti/left joins), and `FILTER`/`BIND`/`SERVICE` are
/// order-sensitive w.r.t. the variables they read or introduce.
fn is_barrier(comp: &QueryComponent) -> bool {
    matches!(
        comp,
        QueryComponent::Optional(_)
            | QueryComponent::Minus(_)
            | QueryComponent::Filter(_)
            | QueryComponent::Bind(_, _)
            | QueryComponent::Service(_, _, _)
    )
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

        let order = order_components(&components, &HashSet::new(), &ds);
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
        let order = order_components(&components, &HashSet::new(), &ds);
        assert!(matches!(order[0], QueryComponent::BGP(_)));
        assert!(matches!(order[1], QueryComponent::Values(_, _)));
        assert_eq!(order.len(), 2);
    }

    /// A barrier (`OPTIONAL`) must pin ordering: a cheap BGP sitting after the
    /// barrier may not be hoisted in front of a UNION that sits before it.
    #[test]
    fn does_not_reorder_across_optional_barrier() {
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
        let optional = QueryComponent::Optional(vec![bgp_on("s", PB, "o3")]);
        let cheap_constraint = bgp_on("s", PC, "o2");
        let components = vec![union, optional, cheap_constraint];

        let order = order_components(&components, &HashSet::new(), &ds);
        // Barrier keeps the original relative order across it: UNION, then the
        // OPTIONAL, then the constraint. The cheap constraint is NOT hoisted.
        assert!(matches!(order[0], QueryComponent::Union(_, _)));
        assert!(matches!(order[1], QueryComponent::Optional(_)));
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

        let order = order_components(&components, &already_bound, &ds);
        assert!(
            matches!(order[0], QueryComponent::Union(_, _)),
            "the UNION shares the already-bound `?s` (connectedness 1) and must be scheduled before the cheaper but disconnected BGP"
        );
        assert!(matches!(order[1], QueryComponent::BGP(_)));
    }

    /// `should_reorder` gates out the hot path: single components and
    /// union-free groups short-circuit before any allocation.
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
            "a union-free group never reorders"
        );

        let with_union = vec![
            QueryComponent::Union(vec![bgp_on("s", PA, "o1")], vec![bgp_on("s", PB, "o1")]),
            bgp_on("s", PC, "o2"),
        ];
        assert!(should_reorder(&with_union));
    }
}

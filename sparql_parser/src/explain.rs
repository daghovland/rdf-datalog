/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Static EXPLAIN plan for a parsed SPARQL query (issue
//! [#537](https://github.com/daghovland/rdf-datalog/issues/537)).
//!
//! See `docs/plans/EXPLAIN_ENDPOINT_537_PLAN.md` for the full design. In
//! short: this module walks a `Query`'s `WHERE` clause the same way
//! `execute::components::eval_components_budgeted` does — same static
//! reordering (`component_ordering::should_reorder`/`order_components`,
//! `join_ordering::order_patterns`), same recursive structure over
//! `QueryComponent` — but instead of evaluating each component against
//! actual solutions, it produces a [`PlanNode`] describing it. This is
//! *read-only* with respect to execution: `explain_query` never calls into
//! `execute::*` and never touches the datastore's quads beyond the
//! index `.len()` lookups `join_ordering::cardinality_and_index` already
//! performs for real query planning.
//!
//! This module never executes anything, so it only ever reports the total
//! caller-measured wall-clock time, never per-operator timing — actual
//! per-operator (per-component, per-triple-pattern) timing is collected
//! separately, from real execution, by `crate::profile`
//! ([#572](https://github.com/daghovland/rdf-datalog/issues/572)) and
//! surfaced as its own tree, not merged into this module's [`PlanNode`]
//! tree — see `crate::profile`'s module doc for why.
//!
//! ## The `already_bound` set (issue #573)
//!
//! A BGP's (and, as of this issue, an `OPTIONAL`/`GRAPH` body's) reported
//! join order depends on which variables are already bound when it starts
//! evaluating. At the query's top level, and at the start of every
//! independently-evaluated scope (`UNION` arms, bare `Group` bodies,
//! `MINUS`'s RHS — see `execute::components::eval_independent_then_join`,
//! which always starts these from an unseeded `vec![HashMap::new()]`), this
//! set is genuinely `∅` — no approximation needed. But `OPTIONAL` and
//! `GRAPH` bodies are seeded per-row with whatever the outer solution
//! already bound (`eval_component`'s `Optional`/`Graph` arms pass
//! `vec![sub.clone()]`/`vec![sub]`, not an unseeded start), and a later BGP
//! sibling in the *same* component list also sees whatever earlier
//! siblings bound — so reporting `∅` there was a conservative, but
//! sometimes misleading, placeholder.
//!
//! This module instead threads a conservative *static* approximation:
//! `component_ordering::must_bind_vars`/`must_bind_sequence`'s
//! "guaranteed bound on every surviving row" set, accumulated over
//! preceding siblings in the same component list (in their *actual*
//! evaluation order, after `component_ordering`'s own static reordering —
//! not their textual order), and passed down into a nested BGP's
//! `order_patterns` call or an `OPTIONAL`/`GRAPH` body's recursive walk.
//!
//! This deliberately does *not* use `component_ordering::
//! variables_in_components` ("every variable any sibling references"),
//! despite that being the issue's originally suggested approximation: that
//! set is only a sound over-approximation for the specific use it was built
//! for (a cheap pre-filter for `MINUS`'s domain-disjointness check, where
//! over-crediting only costs a missed short-circuit). Reused here, it would
//! over-credit variables that are not actually guaranteed bound on every
//! row — e.g. a `MINUS` body's own variables (`MINUS` never extends the
//! outer solution), an `OPTIONAL` body's variables (unbound on a
//! non-matching row), a `BIND` alias (left unbound when its expression
//! errors, per W3C `bind04`), or a variable only one `UNION` arm binds —
//! which would make the reported order claim a binding that might not
//! actually hold, the opposite of conservative. `must_bind_vars` is exactly
//! the existing "true under-approximation" set `component_ordering::
//! order_components` itself relies on for its `OPTIONAL`/`MINUS`-hoisting
//! correctness check, so reusing it here costs nothing new and keeps the
//! soundness argument identical to that already-reviewed code (see
//! `component_ordering.rs`'s module docs for the full argument). The
//! remaining gap, inherent to any static approximation: a particular
//! runtime row can bind strictly *more* than `must_bind_vars` guarantees
//! (e.g. a non-matching `OPTIONAL` that happens to match for this row), so
//! the reported order can still differ from what a specific row's actual
//! execution would do — it is a closer, still-sound approximation, not an
//! exact reproduction.

use crate::ast::{Query, QueryComponent, Term, TriplePattern};
use crate::component_ordering::must_bind_vars;
use crate::join_ordering::{cardinality_and_index, order_patterns};
use dag_rdf::Datastore;
use std::collections::HashSet;

/// The static plan for a `WHERE` clause (or any nested component list): a
/// sequence of [`PlanNode`]s in the order they would actually be evaluated,
/// after applying the same static reordering
/// `execute::components::eval_components_budgeted` applies.
#[derive(Debug, Clone)]
pub struct ExplainPlan {
    pub nodes: Vec<PlanNode>,
}

/// One node in an [`ExplainPlan`], mirroring one `QueryComponent` variant.
#[derive(Debug, Clone)]
pub enum PlanNode {
    /// A basic graph pattern: `patterns` is already in evaluation order
    /// (the permutation `join_ordering::order_patterns` computed), each
    /// entry's `position` giving its 0-based rank in that order.
    Bgp { patterns: Vec<PatternPlan> },
    /// `subject path object`, rendered as debug text (property paths have
    /// no dedicated pretty-printer; this is a debugging aid, not a
    /// round-trippable syntax).
    PathPattern { detail: String },
    /// `{ SELECT ... }` embedded in a group graph pattern. Its own
    /// `WHERE`-clause plan, computed the same way as the outer query's.
    Subquery { plan: Box<ExplainPlan> },
    /// `OPTIONAL { ... }`. See the module doc for why the inner plan's join
    /// order is only a conservative approximation here.
    Optional { children: Box<ExplainPlan> },
    /// `{ ... } UNION { ... }`. Both arms are independently-evaluated
    /// scopes (start from no bound variables), so their inner plans are
    /// exact, not approximated.
    Union {
        left: Box<ExplainPlan>,
        right: Box<ExplainPlan>,
    },
    /// A `FILTER`, rendered as debug text (see `PathPattern`).
    Filter { detail: String },
    /// A `BIND(expr AS ?var)`, rendered as debug text.
    Bind { detail: String },
    /// A `VALUES` clause; `detail` names the bound variables and row count.
    Values { detail: String },
    /// `MINUS { ... }`. Independently-evaluated, like a `UNION` arm.
    Minus { children: Box<ExplainPlan> },
    /// `GRAPH <g|?g> { ... }`.
    Graph {
        detail: String,
        children: Box<ExplainPlan>,
    },
    /// A bare nested `{ ... }` group. Independently-evaluated, like a
    /// `UNION` arm.
    Group { children: Box<ExplainPlan> },
    /// `SERVICE <endpoint> { ... }` — always returns empty results (SERVICE
    /// is not implemented, see `execute::components`'s `Service` arm), so
    /// there is no meaningful plan to show beyond the endpoint term.
    Service { detail: String },
}

/// One triple pattern's entry in a [`PlanNode::Bgp`]'s evaluation-order
/// list.
#[derive(Debug, Clone)]
pub struct PatternPlan {
    /// 0-based rank in the chosen evaluation order (not the pattern's
    /// position in the original query text).
    pub position: usize,
    /// The pattern rendered as `subject predicate object`, using each
    /// term's `Display`/variable-name form.
    pub pattern: String,
    /// `join_ordering::cardinality_and_index`'s cardinality estimate for
    /// this pattern's constant terms.
    pub estimated_cardinality: usize,
    /// Human-readable label for which `QuadTable` index (or index
    /// combination) the estimate came from.
    pub index_used: &'static str,
}

/// Query-type label used in the EXPLAIN report's `queryType` field.
pub fn query_type_label(query: &Query) -> &'static str {
    match query {
        Query::Select { .. } => "Select",
        Query::Ask { .. } => "Ask",
        Query::Construct { .. } => "Construct",
        Query::Describe { .. } => "Describe",
    }
}

/// Build the static EXPLAIN plan for `query`'s `WHERE` clause against
/// `datastore`. Pure and read-only: never executes the query, never
/// mutates `datastore`.
pub fn explain_query(query: &Query, datastore: &Datastore) -> ExplainPlan {
    let where_clause = match query {
        Query::Select { where_clause, .. } => where_clause.as_slice(),
        Query::Ask { where_clause, .. } => where_clause.as_slice(),
        Query::Construct { where_clause, .. } => where_clause.as_slice(),
        Query::Describe { where_clause, .. } => where_clause.as_slice(),
    };
    explain_components(where_clause, datastore)
}

/// Build the static plan for a `WHERE` clause (or any nested, independently-
/// evaluated component list — `UNION` arms, bare `Group` bodies, `MINUS`'s
/// RHS, a subquery's own `WHERE`): the top of any of these scopes genuinely
/// has `already_bound = ∅` (see the module doc), so this is the public/
/// top-level entry point and seeds that empty set itself.
pub(crate) fn explain_components(
    components: &[QueryComponent],
    datastore: &Datastore,
) -> ExplainPlan {
    explain_components_with_bound(components, &HashSet::new(), datastore)
}

/// Build the static plan for `components`, applying the same
/// [`crate::component_ordering`] static reordering
/// `execute::components::eval_components_budgeted` applies before
/// evaluating a component list, given `inherited` — the set of variables
/// statically guaranteed bound *before this list starts* (see the module
/// doc: `∅` for every independently-evaluated scope, non-empty only when
/// recursing into an `OPTIONAL`/`GRAPH` body).
fn explain_components_with_bound(
    components: &[QueryComponent],
    inherited: &HashSet<String>,
    datastore: &Datastore,
) -> ExplainPlan {
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
            crate::component_ordering::order_components(
                &non_filters,
                inherited,
                inherited,
                datastore,
            )
        } else {
            non_filters.iter().collect()
        };
    ordered.extend(filters);

    // Accumulate `must_bind_vars` over `ordered` in actual evaluation
    // order (not textual order — a component hoisted ahead of an
    // `OPTIONAL`/`MINUS` barrier by `order_components` must contribute its
    // variables to what follows it in *this* order), so a later sibling
    // (or an `OPTIONAL`/`GRAPH` body) is credited with exactly what every
    // earlier sibling here is guaranteed to have bound.
    let mut bound = inherited.clone();
    let mut nodes = Vec::with_capacity(ordered.len());
    for comp in ordered {
        nodes.push(explain_component(comp, &bound, datastore));
        bound.extend(must_bind_vars(comp));
    }
    ExplainPlan { nodes }
}

fn explain_component(
    comp: &QueryComponent,
    bound: &HashSet<String>,
    datastore: &Datastore,
) -> PlanNode {
    match comp {
        QueryComponent::BGP(patterns) => {
            let order = order_patterns(patterns, bound, datastore);
            let plan_patterns = order
                .into_iter()
                .enumerate()
                .map(|(position, idx)| {
                    let tp = &patterns[idx];
                    let (estimated_cardinality, index_used) = cardinality_and_index(tp, datastore);
                    PatternPlan {
                        position,
                        pattern: render_triple_pattern(tp),
                        estimated_cardinality,
                        index_used: index_used.description(),
                    }
                })
                .collect();
            PlanNode::Bgp {
                patterns: plan_patterns,
            }
        }
        QueryComponent::PathPattern(subject, path, object) => PlanNode::PathPattern {
            detail: format!(
                "{} {:?} {}",
                render_term(subject),
                path,
                render_term(object)
            ),
        },
        QueryComponent::Subquery(inner) => {
            let where_clause = match inner.as_ref() {
                Query::Select { where_clause, .. } => where_clause.as_slice(),
                Query::Ask { where_clause, .. } => where_clause.as_slice(),
                Query::Construct { where_clause, .. } => where_clause.as_slice(),
                Query::Describe { where_clause, .. } => where_clause.as_slice(),
            };
            // Independently-evaluated scope (its own `WHERE`, planned like
            // a top-level query) — genuinely `∅`, not `bound`.
            PlanNode::Subquery {
                plan: Box::new(explain_components(where_clause, datastore)),
            }
        }
        // `OPTIONAL` is seeded per-row with the outer solution
        // (`eval_component`'s `Optional` arm: `vec![sub.clone()]`), so
        // `bound` — everything guaranteed bound by earlier siblings in this
        // same list — is a sound (if not always exact) approximation of
        // what flows in. See the module doc.
        QueryComponent::Optional(inner) => PlanNode::Optional {
            children: Box::new(explain_components_with_bound(inner, bound, datastore)),
        },
        // Independently-evaluated scope (`eval_independent_then_join`
        // always starts both arms from an unseeded `vec![HashMap::new()]`).
        QueryComponent::Union(left, right) => PlanNode::Union {
            left: Box::new(explain_components(left, datastore)),
            right: Box::new(explain_components(right, datastore)),
        },
        QueryComponent::Filter(expr) => PlanNode::Filter {
            detail: format!("{expr:?}"),
        },
        QueryComponent::Bind(expr, alias) => PlanNode::Bind {
            detail: format!("{expr:?} AS ?{alias}"),
        },
        QueryComponent::Values(vars, rows) => PlanNode::Values {
            detail: format!("VALUES ({}) — {} row(s)", vars.join(" "), rows.len()),
        },
        // Independently-evaluated scope: `eval_component`'s `Minus` arm
        // always evaluates its body unseeded (`vec![HashMap::new()]`), and
        // never extends the outer solution with it regardless.
        QueryComponent::Minus(inner) => PlanNode::Minus {
            children: Box::new(explain_components(inner, datastore)),
        },
        // `GRAPH` is seeded per-row with the outer solution exactly like
        // `OPTIONAL` (`eval_component`'s `Graph` arm: `vec![sub]`, not an
        // unseeded start) — same `bound` propagation, same rationale. The
        // graph term itself is never added to `bound`: at runtime a
        // variable graph term that isn't already in `sub` becomes
        // `ActiveGraph::Variable`, not a new binding threaded into the
        // inner body.
        QueryComponent::Graph(graph_term, inner) => PlanNode::Graph {
            detail: render_term(graph_term),
            children: Box::new(explain_components_with_bound(inner, bound, datastore)),
        },
        // Independently-evaluated scope, like a `UNION` arm (issue #198).
        QueryComponent::Group(inner) => PlanNode::Group {
            children: Box::new(explain_components(inner, datastore)),
        },
        QueryComponent::Service(endpoint, _inner, silent) => PlanNode::Service {
            detail: format!(
                "{}{}",
                render_term(endpoint),
                if *silent { " SILENT" } else { "" }
            ),
        },
    }
}

/// `pub(crate)` rather than private: reused by `sparql_parser::profile`'s
/// instrumentation (`bgp.rs`'s per-pattern timing, `components.rs`'s
/// per-component labels) so a `Pattern`/`PathPattern`/`Graph`/`Service`
/// profile node's rendered label matches the static plan's wording exactly
/// — one rendering function, not two that could drift apart.
pub(crate) fn render_triple_pattern(tp: &TriplePattern) -> String {
    format!(
        "{} {} {}",
        render_term(&tp.subject),
        render_term(&tp.predicate),
        render_term(&tp.object)
    )
}

pub(crate) fn render_term(term: &Term) -> String {
    match term {
        Term::Variable(v) => format!("?{v}"),
        Term::Constant(gel) => gel.to_string(),
        Term::TripleTerm(inner) => format!("<<( {} )>>", render_triple_pattern(inner)),
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

    fn tp(s: Term, p: Term, o: Term) -> TriplePattern {
        TriplePattern {
            subject: s,
            predicate: p,
            object: o,
        }
    }

    /// Add a quad in the default graph, registering all three resources.
    fn add_default_graph_quad(ds: &mut Datastore, s: &str, p: &str, o: &str) {
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

    const PA: &str = "http://example.org/pa";
    const PB: &str = "http://example.org/pb";
    const PC: &str = "http://example.org/pc";

    /// Pull the `patterns` list out of a single top-level `PlanNode::Bgp`,
    /// panicking with a useful message otherwise.
    fn bgp_patterns(plan: &ExplainPlan) -> &[PatternPlan] {
        assert_eq!(plan.nodes.len(), 1, "expected exactly one node: {plan:?}");
        match &plan.nodes[0] {
            PlanNode::Bgp { patterns } => patterns,
            other => panic!("expected a Bgp node, got {other:?}"),
        }
    }

    /// Two-pattern body used by several tests below: `?y PB ?v` has `?y` as
    /// its only shared variable with an outer binder, and `?w PC ?z` shares
    /// nothing. `PB` is deliberately given a *higher* raw cardinality than
    /// `PC` so that, with `already_bound = ∅`, `PC`'s pattern (cheaper, and
    /// tied on bound_count 0) is scheduled first — but once `?y` is credited
    /// as already bound, `PB`'s pattern gets bound_count 1 (connected) and
    /// must be scheduled first instead. This makes the two orders
    /// (`∅` vs `{y}`) provably different, which is the property the
    /// `already_bound` fix is supposed to change.
    fn asymmetric_body() -> Vec<TriplePattern> {
        vec![
            tp(var("y"), iri_const(PB), var("v")),
            tp(var("w"), iri_const(PC), var("z")),
        ]
    }

    fn populate_asymmetric_body_store(ds: &mut Datastore) {
        // PB: 5 quads (less selective).
        for i in 0..5 {
            add_default_graph_quad(
                ds,
                &format!("http://example.org/yb_{i}"),
                PB,
                &format!("http://example.org/vb_{i}"),
            );
        }
        // PC: 2 quads (more selective) — wins on cardinality alone.
        for i in 0..2 {
            add_default_graph_quad(
                ds,
                &format!("http://example.org/wc_{i}"),
                PC,
                &format!("http://example.org/zc_{i}"),
            );
        }
    }

    /// Sanity check backing every precision test below: confirm
    /// `order_patterns` really does choose different orders for
    /// `asymmetric_body()` depending on whether `y` is already bound.
    /// Without this, a precision test could pass vacuously (both orders
    /// identical) and prove nothing about the fix.
    #[test]
    fn asymmetric_body_order_differs_with_and_without_bound_y() {
        let mut ds = Datastore::new(1_000);
        populate_asymmetric_body_store(&mut ds);
        let body = asymmetric_body();

        let order_empty = order_patterns(&body, &HashSet::new(), &ds);
        let mut bound_y = HashSet::new();
        bound_y.insert("y".to_string());
        let order_with_y = order_patterns(&body, &bound_y, &ds);

        assert_ne!(
            order_empty, order_with_y,
            "fixture must actually be sensitive to already_bound, or the precision tests below are vacuous"
        );
        assert_eq!(
            order_with_y,
            vec![0, 1],
            "PB (connected via bound y) should be scheduled first once y is bound"
        );
    }

    /// Issue #573's precision property: an `OPTIONAL` body preceded by a
    /// sibling BGP that provably binds one of the body's variables must be
    /// reported with that variable credited as already-bound, matching
    /// `order_patterns(body, {y}, ds)` rather than the old `∅` baseline.
    #[test]
    fn optional_body_order_reflects_preceding_sibling_binding() {
        let mut ds = Datastore::new(1_000);
        populate_asymmetric_body_store(&mut ds);

        // Outer BGP guarantees `y` is bound (`PA` is irrelevant to ordering
        // here — any single-quad predicate would do).
        add_default_graph_quad(
            &mut ds,
            "http://example.org/x0",
            PA,
            "http://example.org/y0",
        );
        let outer = QueryComponent::BGP(vec![tp(var("x"), iri_const(PA), var("y"))]);

        let components = vec![
            outer,
            QueryComponent::Optional(vec![QueryComponent::BGP(asymmetric_body())]),
        ];

        let plan = explain_components(&components, &ds);
        assert_eq!(plan.nodes.len(), 2);
        let PlanNode::Optional { children } = &plan.nodes[1] else {
            panic!("expected second node to be Optional: {:?}", plan.nodes[1]);
        };
        let patterns = bgp_patterns(children);

        let mut bound_y = HashSet::new();
        bound_y.insert("y".to_string());
        let expected_order = order_patterns(&asymmetric_body(), &bound_y, &ds);
        let expected_first_pattern = render_triple_pattern(&asymmetric_body()[expected_order[0]]);

        assert_eq!(
            patterns[0].pattern, expected_first_pattern,
            "OPTIONAL body's reported order must match order_patterns computed with the preceding BGP's guaranteed bindings, not ∅: {patterns:?}"
        );
    }

    /// `GRAPH` bodies are seeded with the outer solution at runtime exactly
    /// like `OPTIONAL` (`eval_component`'s `Graph` arm passes `vec![sub]`,
    /// not an unseeded `vec![HashMap::new()]`), so they must receive the
    /// same inherited already-bound set.
    #[test]
    fn graph_body_order_reflects_preceding_sibling_binding() {
        let mut ds = Datastore::new(1_000);
        populate_asymmetric_body_store(&mut ds);
        add_default_graph_quad(
            &mut ds,
            "http://example.org/x0",
            PA,
            "http://example.org/y0",
        );

        let components = vec![
            QueryComponent::BGP(vec![tp(var("x"), iri_const(PA), var("y"))]),
            QueryComponent::Graph(
                Term::Constant(iri_node("http://example.org/g1")),
                vec![QueryComponent::BGP(asymmetric_body())],
            ),
        ];

        let plan = explain_components(&components, &ds);
        let PlanNode::Graph { children, .. } = &plan.nodes[1] else {
            panic!("expected second node to be Graph: {:?}", plan.nodes[1]);
        };
        let patterns = bgp_patterns(children);

        let mut bound_y = HashSet::new();
        bound_y.insert("y".to_string());
        let expected_order = order_patterns(&asymmetric_body(), &bound_y, &ds);
        let expected_first_pattern = render_triple_pattern(&asymmetric_body()[expected_order[0]]);

        assert_eq!(
            patterns[0].pattern, expected_first_pattern,
            "GRAPH body's reported order must reflect the preceding BGP's guaranteed bindings: {patterns:?}"
        );
    }

    /// Independent-scope soundness: a `UNION` arm is evaluated unseeded at
    /// runtime (`eval_independent_then_join` always starts from
    /// `vec![HashMap::new()]`) regardless of what a preceding sibling binds,
    /// so its inner BGP must still be reported with `already_bound = ∅`
    /// even when a preceding BGP sibling guarantees `y`.
    #[test]
    fn union_arm_bgp_not_credited_with_preceding_sibling_binding() {
        let mut ds = Datastore::new(1_000);
        populate_asymmetric_body_store(&mut ds);
        add_default_graph_quad(
            &mut ds,
            "http://example.org/x0",
            PA,
            "http://example.org/y0",
        );

        let components = vec![
            QueryComponent::BGP(vec![tp(var("x"), iri_const(PA), var("y"))]),
            QueryComponent::Union(
                vec![QueryComponent::BGP(asymmetric_body())],
                vec![QueryComponent::BGP(vec![tp(
                    var("q"),
                    iri_const(PA),
                    var("r"),
                )])],
            ),
        ];

        let plan = explain_components(&components, &ds);
        // Find the Union node (order_components may place it before or
        // after the BGP depending on connectedness scoring; locate by kind).
        let union_node = plan
            .nodes
            .iter()
            .find_map(|n| match n {
                PlanNode::Union { left, .. } => Some(left),
                _ => None,
            })
            .expect("plan must contain a Union node");
        let patterns = bgp_patterns(union_node);

        let order_empty = order_patterns(&asymmetric_body(), &HashSet::new(), &ds);
        let expected_first_pattern = render_triple_pattern(&asymmetric_body()[order_empty[0]]);

        assert_eq!(
            patterns[0].pattern, expected_first_pattern,
            "UNION arm's BGP must be reported with already_bound = ∅ (independent scope), not credited with the sibling BGP's binding: {patterns:?}"
        );
    }

    /// Soundness negative: `MINUS` never binds a new variable into the
    /// surviving outer rows (`eval_component`'s `Minus` arm only ever
    /// filters `sub`, never extends it), so a BGP sibling *after* a `MINUS`
    /// must not be credited with variables only the `MINUS` body binds,
    /// even though `MINUS`'s body does share no barrier-crossing issue here.
    #[test]
    fn bgp_after_minus_not_credited_with_minus_body_bindings() {
        let mut ds = Datastore::new(1_000);
        populate_asymmetric_body_store(&mut ds);

        // MINUS body binds `y` (and `v`), but MINUS must never be credited
        // downstream.
        let components = vec![
            QueryComponent::Minus(vec![QueryComponent::BGP(vec![tp(
                var("y"),
                iri_const(PB),
                var("v"),
            )])]),
            QueryComponent::BGP(asymmetric_body()),
        ];

        let plan = explain_components(&components, &ds);
        let bgp_node = plan
            .nodes
            .iter()
            .find_map(|n| match n {
                PlanNode::Bgp { patterns } => Some(patterns),
                _ => None,
            })
            .expect("plan must contain the BGP node");

        let order_empty = order_patterns(&asymmetric_body(), &HashSet::new(), &ds);
        let expected_first_pattern = render_triple_pattern(&asymmetric_body()[order_empty[0]]);

        assert_eq!(
            bgp_node[0].pattern, expected_first_pattern,
            "BGP after MINUS must not be credited with the MINUS body's bindings: {bgp_node:?}"
        );
    }

    /// Soundness negative: a `UNION` only guarantees a variable that *every*
    /// arm binds. `?y` is bound by the left arm only here, so a BGP sibling
    /// after the `UNION` must not be credited with `y`.
    #[test]
    fn bgp_after_union_not_credited_with_single_arm_binding() {
        let mut ds = Datastore::new(1_000);
        populate_asymmetric_body_store(&mut ds);

        let components = vec![
            QueryComponent::Union(
                vec![QueryComponent::BGP(vec![tp(
                    var("y"),
                    iri_const(PB),
                    var("v"),
                )])],
                vec![QueryComponent::BGP(vec![tp(
                    var("q"),
                    iri_const(PC),
                    var("r"),
                )])],
            ),
            QueryComponent::BGP(asymmetric_body()),
        ];

        let plan = explain_components(&components, &ds);
        let bgp_node = plan
            .nodes
            .iter()
            .find_map(|n| match n {
                PlanNode::Bgp { patterns } => Some(patterns),
                _ => None,
            })
            .expect("plan must contain the trailing BGP node");

        let order_empty = order_patterns(&asymmetric_body(), &HashSet::new(), &ds);
        let expected_first_pattern = render_triple_pattern(&asymmetric_body()[order_empty[0]]);

        assert_eq!(
            bgp_node[0].pattern, expected_first_pattern,
            "BGP after UNION must not be credited with a variable only one arm binds: {bgp_node:?}"
        );
    }
}

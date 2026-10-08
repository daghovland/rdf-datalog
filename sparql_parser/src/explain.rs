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
//! One thing this module deliberately does *not* do, filed as a follow-up
//! rather than built here (see the plan doc):
//! - A non-empty, conservatively-computed `already_bound` set when
//!   recursing into a BGP or an `OPTIONAL` body
//!   ([#573](https://github.com/daghovland/rdf-datalog/issues/573)) — every
//!   BGP and `OPTIONAL` body in the walk is scored as if no outer variable
//!   were already bound, which is exactly right for the query's top level
//!   and for every independently-evaluated scope (`UNION` arms, bare
//!   `Group` bodies, `MINUS`'s RHS — see `execute::components::
//!   eval_independent_then_join`), but only a conservative approximation
//!   for an `OPTIONAL` body, whose inner components are in reality seeded
//!   per-row with whatever the outer solution already bound.

use crate::ast::{Query, QueryComponent, Term, TriplePattern};
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

/// Build the static plan for `components`, applying the same
/// [`crate::component_ordering`] static reordering
/// `execute::components::eval_components_budgeted` applies before
/// evaluating a component list. See the module doc for the `∅`
/// already-bound/guaranteed-bound approximation this relies on being exact
/// at every call site in this module.
pub(crate) fn explain_components(
    components: &[QueryComponent],
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

    let empty: HashSet<String> = HashSet::new();
    let mut ordered: Vec<&QueryComponent> =
        if crate::component_ordering::should_reorder(&non_filters) {
            crate::component_ordering::order_components(&non_filters, &empty, &empty, datastore)
        } else {
            non_filters.iter().collect()
        };
    ordered.extend(filters);

    let nodes = ordered
        .into_iter()
        .map(|c| explain_component(c, datastore))
        .collect();
    ExplainPlan { nodes }
}

fn explain_component(comp: &QueryComponent, datastore: &Datastore) -> PlanNode {
    match comp {
        QueryComponent::BGP(patterns) => {
            let order = order_patterns(patterns, &HashSet::new(), datastore);
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
            PlanNode::Subquery {
                plan: Box::new(explain_components(where_clause, datastore)),
            }
        }
        QueryComponent::Optional(inner) => PlanNode::Optional {
            children: Box::new(explain_components(inner, datastore)),
        },
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
        QueryComponent::Minus(inner) => PlanNode::Minus {
            children: Box::new(explain_components(inner, datastore)),
        },
        QueryComponent::Graph(graph_term, inner) => PlanNode::Graph {
            detail: render_term(graph_term),
            children: Box::new(explain_components(inner, datastore)),
        },
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
        assert_eq!(order_with_y, vec![0, 1], "PB (connected via bound y) should be scheduled first once y is bound");
    }

    /// Issue #573's precision property: an `OPTIONAL` body preceded by a
    /// sibling BGP that provably binds one of the body's variables must be
    /// reported with that variable credited as already-bound, matching
    /// `order_patterns(body, {y}, ds)` rather than the old `∅` baseline.
    #[test]
    #[ignore = "issue #573: already_bound set not yet threaded through explain_components"]
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
    #[ignore = "issue #573: already_bound set not yet threaded through explain_components"]
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
    #[ignore = "issue #573: already_bound set not yet threaded through explain_components"]
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
    #[ignore = "issue #573: already_bound set not yet threaded through explain_components"]
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
    #[ignore = "issue #573: already_bound set not yet threaded through explain_components"]
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

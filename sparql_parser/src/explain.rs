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
//! Two things this module deliberately does *not* do, both filed as
//! follow-ups rather than built here (see the plan doc):
//! - Per-operator/per-stage timing
//!   ([#572](https://github.com/daghovland/rdf-datalog/issues/572)) — only
//!   the caller-measured total wall-clock time is meaningful here, since
//!   this module never executes anything.
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

fn render_triple_pattern(tp: &TriplePattern) -> String {
    format!(
        "{} {} {}",
        render_term(&tp.subject),
        render_term(&tp.predicate),
        render_term(&tp.object)
    )
}

fn render_term(term: &Term) -> String {
    match term {
        Term::Variable(v) => format!("?{v}"),
        Term::Constant(gel) => gel.to_string(),
        Term::TripleTerm(inner) => format!("<<( {} )>>", render_triple_pattern(inner)),
    }
}

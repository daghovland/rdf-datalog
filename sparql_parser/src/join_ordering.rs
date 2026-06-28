/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Selectivity-based BGP join reordering (Phase A of
//! `docs/plans/JOIN_REORDERING_PLAN.md`).
//!
//! `order_patterns` chooses an evaluation order for a single BGP's triple
//! patterns so that the most restrictive patterns run first, using only
//! information that's actually known at planning time: real index
//! cardinalities for constant terms, and a structural "will this pattern be
//! restricted by an earlier pattern's bindings" signal for variable terms
//! whose runtime value isn't known yet. See the plan doc for why exact
//! dynamic programming over unknown-value cardinalities was rejected.

use crate::ast::{Term, TriplePattern};
use dag_rdf::Datastore;
use std::collections::HashSet;

/// Return a permutation of `0..patterns.len()` giving the evaluation order
/// `eval_bgp` should use, given the variables already bound by solutions
/// flowing into this BGP.
pub(crate) fn order_patterns(
    patterns: &[TriplePattern],
    already_bound: &HashSet<String>,
    datastore: &Datastore,
) -> Vec<usize> {
    let mut bound: HashSet<String> = already_bound.clone();
    let mut remaining: Vec<usize> = (0..patterns.len()).collect();
    let mut order = Vec::with_capacity(patterns.len());

    while !remaining.is_empty() {
        let best_pos = remaining
            .iter()
            .enumerate()
            .max_by_key(|&(_, &i)| {
                let bc = bound_count(&patterns[i], &bound);
                let cardinality = known_cardinality(&patterns[i], datastore);
                (bc, std::cmp::Reverse(cardinality))
            })
            .map(|(pos, _)| pos)
            .expect("remaining is non-empty inside the loop");

        let best = remaining.remove(best_pos);
        bound.extend(pattern_variables(&patterns[best]).cloned());
        order.push(best);
    }

    order
}

/// Number of {subject, predicate, object} terms that are variables already
/// in `bound`. Constants deliberately do not count: their selectivity is
/// already measured exactly by `known_cardinality`, so counting them here
/// too would double-count them and can outrank a far cheaper pattern (see
/// the worked counterexample in `docs/plans/JOIN_REORDERING_PLAN.md`).
fn bound_count(tp: &TriplePattern, bound: &HashSet<String>) -> usize {
    [&tp.subject, &tp.predicate, &tp.object]
        .into_iter()
        .filter(|t| match t {
            Term::Constant(_) => false,
            Term::Variable(v) => bound.contains(v),
        })
        .count()
}

/// Variables referenced by a triple pattern's subject/predicate/object.
fn pattern_variables(tp: &TriplePattern) -> impl Iterator<Item = &String> {
    [&tp.subject, &tp.predicate, &tp.object]
        .into_iter()
        .filter_map(|t| match t {
            Term::Variable(v) => Some(v),
            Term::Constant(_) => None,
        })
}

/// Resolves a `Term` to its interned `GraphElementId` if it's a constant
/// that's actually registered in the datastore. `None` for a variable;
/// `Some(None)` for a constant that was never interned (can never match).
fn resolve_constant(term: &Term, datastore: &Datastore) -> Option<Option<dag_rdf::GraphElementId>> {
    match term {
        Term::Variable(_) => None,
        Term::Constant(ge) => Some(datastore.resources.resource_map.get(ge).copied()),
    }
}

/// Cardinality estimate using only the pattern's constant terms, via direct
/// `.len()` lookups on `QuadTable`'s public index fields — no allocation,
/// no `.collect()`. Returns 0 if a constant term doesn't resolve to a known
/// resource (the pattern can never match).
fn known_cardinality(tp: &TriplePattern, datastore: &Datastore) -> usize {
    let table = &datastore.named_graphs;

    let subject = resolve_constant(&tp.subject, datastore);
    let predicate = resolve_constant(&tp.predicate, datastore);
    let object = resolve_constant(&tp.object, datastore);

    // Any constant that didn't resolve to a known resource means the
    // pattern can never match.
    if [&subject, &predicate, &object]
        .into_iter()
        .any(|r| matches!(r, Some(None)))
    {
        return 0;
    }

    // Flatten `Option<Option<Id>>` to `Option<Id>`: `None` (variable) or
    // `Some(None)` (unresolved constant, already handled above) both leave
    // the slot unconstrained from here on.
    let subject = subject.flatten();
    let predicate = predicate.flatten();
    let object = object.flatten();

    match (subject, predicate, object) {
        (Some(s), Some(p), Some(o)) => {
            let sp = table
                .subject_predicate_index
                .get(&s)
                .and_then(|m| m.get(&p))
                .map_or(0, |v| v.len());
            let op = table
                .object_predicate_index
                .get(&o)
                .and_then(|m| m.get(&p))
                .map_or(0, |v| v.len());
            sp.min(op)
        }
        (Some(s), Some(p), None) => table
            .subject_predicate_index
            .get(&s)
            .and_then(|m| m.get(&p))
            .map_or(0, |v| v.len()),
        (None, Some(p), Some(o)) => table
            .object_predicate_index
            .get(&o)
            .and_then(|m| m.get(&p))
            .map_or(0, |v| v.len()),
        (None, Some(p), None) => table.predicate_index.get(&p).map_or(0, |v| v.len()),
        (Some(s), None, Some(o)) => {
            let s_sum = table
                .subject_predicate_index
                .get(&s)
                .map_or(0, |m| m.values().map(|v| v.len()).sum());
            let o_sum = table
                .object_predicate_index
                .get(&o)
                .map_or(0, |m| m.values().map(|v| v.len()).sum());
            s_sum.min(o_sum)
        }
        (Some(s), None, None) => table
            .subject_predicate_index
            .get(&s)
            .map_or(0, |m| m.values().map(|v| v.len()).sum()),
        (None, None, Some(o)) => table
            .object_predicate_index
            .get(&o)
            .map_or(0, |m| m.values().map(|v| v.len()).sum()),
        (None, None, None) => table.quad_count,
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

    const P1: &str = "http://example.org/p1"; // deliberately the most selective predicate
    const P2: &str = "http://example.org/p2";
    const P3: &str = "http://example.org/p3";

    #[test]
    fn picks_pattern_with_smallest_predicate_cardinality_first() {
        let mut ds = Datastore::new(1_000);
        // P1: 1 quad (most selective).
        add_default_graph_quad(
            &mut ds,
            "http://example.org/s1",
            P1,
            "http://example.org/o1",
        );
        // P2: 5 quads (least selective).
        for i in 0..5 {
            add_default_graph_quad(
                &mut ds,
                &format!("http://example.org/s2_{i}"),
                P2,
                &format!("http://example.org/o2_{i}"),
            );
        }

        // Deliberately worst order: less selective pattern first.
        let patterns = vec![
            TriplePattern {
                subject: var("x"),
                predicate: iri_const(P2),
                object: var("y"),
            },
            TriplePattern {
                subject: var("x"),
                predicate: iri_const(P1),
                object: var("y"),
            },
        ];

        let order = order_patterns(&patterns, &HashSet::new(), &ds);
        assert_eq!(
            order,
            vec![1, 0],
            "P1 (cardinality 1) must be scheduled before P2 (cardinality 5)"
        );
    }

    #[test]
    fn prefers_connected_pattern_over_cheaper_disconnected_pattern() {
        // This test isolates the connectedness behavior that falls out of
        // the corrected `bound_count` (variables only, not constants): once
        // A is scheduled, B becomes connected (shares `y` with A's object,
        // bound_count 1) while C remains disconnected (no shared variable
        // with anything scheduled, bound_count 0) even though C has a
        // strictly cheaper cardinality (2) than B (5). B must still be
        // picked next, because bound_count ranks ahead of the cardinality
        // tie-break and a disconnected pattern can never have a
        // higher-than-zero bound_count.
        let mut ds = Datastore::new(1_000);
        // A: P1, cardinality 1 — strictly cheapest, so it wins the first (unconstrained) pick.
        add_default_graph_quad(
            &mut ds,
            "http://example.org/sA",
            P1,
            "http://example.org/oA",
        );
        // B: P2, cardinality 5, subject is `y` — shares `y` with A's object once A is scheduled.
        for i in 0..5 {
            add_default_graph_quad(
                &mut ds,
                &format!("http://example.org/sB_{i}"),
                P2,
                &format!("http://example.org/oB_{i}"),
            );
        }
        // C: subject AND predicate both constant, cardinality 2 (cheaper
        // than B's 5, but more expensive than A's 1 so it can't win the
        // first pick either), and shares no variable with A or B — disconnected.
        add_default_graph_quad(
            &mut ds,
            "http://example.org/sC",
            P3,
            "http://example.org/oC_0",
        );
        add_default_graph_quad(
            &mut ds,
            "http://example.org/sC",
            P3,
            "http://example.org/oC_1",
        );

        // Patterns deliberately scrambled: [C, B, A].
        let pattern_c = TriplePattern {
            subject: iri_const("http://example.org/sC"),
            predicate: iri_const(P3),
            object: var("v"),
        };
        let pattern_b = TriplePattern {
            subject: var("y"), // shares `y` with A's object
            predicate: iri_const(P2),
            object: var("z"),
        };
        let pattern_a = TriplePattern {
            subject: var("x"),
            predicate: iri_const(P1),
            object: var("y"),
        };
        let patterns = vec![pattern_c, pattern_b, pattern_a];

        let order = order_patterns(&patterns, &HashSet::new(), &ds);
        assert_eq!(
            order,
            vec![2, 1, 0],
            "A first (cheapest, unconstrained), then B (connected via `y`, bound_count 1 beats C's bound_count 0), then C last"
        );
    }

    #[test]
    fn pattern_with_unresolvable_constant_has_zero_cardinality_and_is_scheduled_first() {
        let mut ds = Datastore::new(1_000);
        // Y: a normal, resolvable, nonzero-cardinality pattern.
        for i in 0..3 {
            add_default_graph_quad(
                &mut ds,
                &format!("http://example.org/sY_{i}"),
                P2,
                &format!("http://example.org/oY_{i}"),
            );
        }
        // X: predicate constant never added to the store — can never match.
        let pattern_x = TriplePattern {
            subject: var("a"),
            predicate: iri_const("http://example.org/never-registered"),
            object: var("b"),
        };
        let pattern_y = TriplePattern {
            subject: var("c"),
            predicate: iri_const(P2),
            object: var("d"),
        };
        let patterns = vec![pattern_y, pattern_x];

        let order = order_patterns(&patterns, &HashSet::new(), &ds);
        assert_eq!(
            order,
            vec![1, 0],
            "the never-matching pattern (cardinality 0) must be scheduled first so eval_bgp short-circuits"
        );
    }

    #[test]
    fn more_bound_variables_outranks_fewer_at_equal_cardinality() {
        // P and Q have the *same* constant predicate, so `known_cardinality`
        // is identical for both — isolating bound_count (now variables-only)
        // as the sole deciding factor, since the cardinality tie-break can't
        // distinguish them. `already_bound` = {x, y}: P reuses both (subject
        // `x` and object `y`), Q reuses only `x` (its object `w` is a fresh,
        // unbound variable).
        let mut ds = Datastore::new(1_000);
        for i in 0..10 {
            add_default_graph_quad(
                &mut ds,
                &format!("http://example.org/s_{i}"),
                P1,
                &format!("http://example.org/o_{i}"),
            );
        }

        // P: subject `x` and object `y` are both already bound -> bound_count 2.
        let pattern_p = TriplePattern {
            subject: var("x"),
            predicate: iri_const(P1),
            object: var("y"),
        };
        // Q: only subject `x` is already bound; `w` is a fresh variable -> bound_count 1.
        let pattern_q = TriplePattern {
            subject: var("x"),
            predicate: iri_const(P1),
            object: var("w"),
        };
        let patterns = vec![pattern_q, pattern_p];

        let mut already_bound = HashSet::new();
        already_bound.insert("x".to_string());
        already_bound.insert("y".to_string());

        let order = order_patterns(&patterns, &already_bound, &ds);
        assert_eq!(
            order,
            vec![1, 0],
            "P (bound_count 2: bound `x` and `y`) must be scheduled before Q (bound_count 1: bound `x` only), even though both have identical cardinality via the shared predicate P1"
        );
    }

    #[test]
    fn known_cardinality_uses_subject_predicate_index_when_both_constant() {
        let mut ds = Datastore::new(1_000);
        // Same subject+predicate, 3 different objects -> subject_predicate
        // index entry has length 3. A different subject with the same
        // predicate must not be counted (proves this isn't just the
        // predicate-only count, which would be 4).
        for i in 0..3 {
            add_default_graph_quad(
                &mut ds,
                "http://example.org/sA",
                P1,
                &format!("http://example.org/o_{i}"),
            );
        }
        add_default_graph_quad(
            &mut ds,
            "http://example.org/sOther",
            P1,
            "http://example.org/oOther",
        );

        let pattern = TriplePattern {
            subject: iri_const("http://example.org/sA"),
            predicate: iri_const(P1),
            object: var("o"),
        };

        assert_eq!(
            known_cardinality(&pattern, &ds),
            3,
            "subject+predicate both constant must use the subject_predicate_index entry (3), not the predicate-only count (4)"
        );
    }
}

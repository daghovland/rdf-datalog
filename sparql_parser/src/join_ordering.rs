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
#[allow(dead_code)]
pub(crate) fn order_patterns(
    patterns: &[TriplePattern],
    already_bound: &HashSet<String>,
    datastore: &Datastore,
) -> Vec<usize> {
    let _ = (patterns, already_bound, datastore);
    unimplemented!(
        "Phase A: selectivity-based BGP reordering — see docs/plans/JOIN_REORDERING_PLAN.md"
    )
}

/// Number of {subject, predicate, object} terms that are either a constant
/// or a variable already in `bound`. Higher means more selective.
#[allow(dead_code)]
fn bound_count(tp: &TriplePattern, bound: &HashSet<String>) -> usize {
    [&tp.subject, &tp.predicate, &tp.object]
        .into_iter()
        .filter(|t| match t {
            Term::Constant(_) => true,
            Term::Variable(v) => bound.contains(v),
        })
        .count()
}

/// Variables referenced by a triple pattern's subject/predicate/object.
#[allow(dead_code)]
fn pattern_variables(tp: &TriplePattern) -> impl Iterator<Item = &String> {
    [&tp.subject, &tp.predicate, &tp.object]
        .into_iter()
        .filter_map(|t| match t {
            Term::Variable(v) => Some(v),
            Term::Constant(_) => None,
        })
}

/// Cardinality estimate using only the pattern's constant terms, via direct
/// `.len()` lookups on `QuadTable`'s public index fields — no allocation,
/// no `.collect()`. Returns 0 if a constant term doesn't resolve to a known
/// resource (the pattern can never match).
#[allow(dead_code)]
fn known_cardinality(tp: &TriplePattern, datastore: &Datastore) -> usize {
    let _ = (tp, datastore);
    unimplemented!(
        "Phase A: constant-term cardinality estimate — see docs/plans/JOIN_REORDERING_PLAN.md"
    )
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
    #[ignore = "Phase A red phase: order_patterns not yet implemented"]
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
    #[ignore = "Phase A red phase: order_patterns not yet implemented"]
    fn prefers_connected_pattern_over_cheaper_disconnected_pattern() {
        // This test isolates the connectedness *filter*, not just bound_count:
        // after A is scheduled, B and C tie on bound_count (2 each — B via a
        // bound variable, C via two constants), but C has the cheaper
        // tie-break cardinality. If bound_count alone decided the order, C
        // would win the tie on cardinality despite sharing no variable with
        // anything scheduled so far. The connectedness rule must restrict
        // candidates to connected ones (here, just B) before applying the
        // cardinality tie-break, so B is picked next regardless of C's lower
        // cardinality.
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
        // C: subject AND predicate both constant (cardinality 1, cheaper than
        // B's 5), but shares no variable with A or B — disconnected.
        add_default_graph_quad(
            &mut ds,
            "http://example.org/sC",
            P3,
            "http://example.org/oC",
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
            "A first (cheapest, unconstrained), then B (connected via `y`, bound_count ties with C but C is disconnected), then C last"
        );
    }

    #[test]
    #[ignore = "Phase A red phase: order_patterns not yet implemented"]
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
    #[ignore = "Phase A red phase: order_patterns not yet implemented"]
    fn more_bound_terms_outranks_fewer_even_with_unknown_cardinality() {
        // Both D and E share variable `x` with `already_bound`, so both pass
        // the connectedness filter — isolating bound_count as the deciding
        // factor (unlike a disconnected-vs-connected setup, where the
        // connectedness filter alone would explain the outcome).
        let mut ds = Datastore::new(1_000);
        for i in 0..10 {
            add_default_graph_quad(
                &mut ds,
                &format!("http://example.org/s_{i}"),
                P1,
                &format!("http://example.org/o_{i}"),
            );
        }

        // D: subject `x` is already bound, predicate constant -> bound_count 2.
        let pattern_d = TriplePattern {
            subject: var("x"),
            predicate: iri_const(P1),
            object: var("z"),
        };
        // E: subject `x` is also already bound (so E is connected too), but
        // predicate is an unbound variable, not a constant -> bound_count 1.
        let pattern_e = TriplePattern {
            subject: var("x"),
            predicate: var("pred"),
            object: var("z"),
        };
        let patterns = vec![pattern_e, pattern_d];

        let mut already_bound = HashSet::new();
        already_bound.insert("x".to_string());

        let order = order_patterns(&patterns, &already_bound, &ds);
        assert_eq!(
            order,
            vec![1, 0],
            "D (bound_count 2: bound `x` + constant predicate) must be scheduled before E (bound_count 1: bound `x` only), even though both are equally connected"
        );
    }

    #[test]
    #[ignore = "Phase A red phase: known_cardinality not yet implemented"]
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

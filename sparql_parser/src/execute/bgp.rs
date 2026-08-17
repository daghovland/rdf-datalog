/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::components::pattern_repeats_variable;
use super::solutions::psv_eq;
use super::*;

pub(crate) fn eval_bgp(
    patterns: &[TriplePattern],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    let already_bound: HashSet<String> = solutions
        .first()
        .map(|sub| sub.keys().cloned().collect())
        .unwrap_or_default();
    let order = crate::join_ordering::order_patterns(patterns, &already_bound, datastore);

    let mut current = solutions;
    let last = order.len().saturating_sub(1);
    for (pos, &idx) in order.iter().enumerate() {
        let pattern = &patterns[idx];
        // Only the last-executed pattern produces the BGP's final output, so
        // only it may honour the row budget (issue #165). Earlier patterns
        // feed the join and must be fully materialised.
        let pat_budget = if pos == last { budget } else { None };
        current = match pat_budget {
            Some(b) => {
                // Accumulate across input solutions with a shrinking budget so
                // the total never exceeds `b`, preserving the exact prefix the
                // unbudgeted evaluation would have produced.
                let mut acc = Vec::new();
                for sub in current {
                    if acc.len() >= b {
                        break;
                    }
                    deadline.check()?;
                    let remaining = b - acc.len();
                    acc.extend(eval_triple_pattern(
                        pattern,
                        &sub,
                        datastore,
                        active_graph,
                        Some(remaining),
                        deadline,
                    )?);
                }
                acc
            }
            None => {
                let mut acc = Vec::new();
                for sub in current {
                    deadline.check()?;
                    acc.extend(eval_triple_pattern(
                        pattern,
                        &sub,
                        datastore,
                        active_graph,
                        None,
                        deadline,
                    )?);
                }
                acc
            }
        };
        if current.is_empty() {
            break;
        }
    }
    Ok(current)
}

pub(crate) fn eval_triple_pattern(
    tp: &TriplePattern,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
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
            deadline.check()?;
            let mut merged = sub.clone();
            let mut ok = true;
            for (var, val) in inner_bindings {
                match merged.get(&var) {
                    Some(existing) if !psv_eq(existing, &val, datastore) => {
                        ok = false;
                        break;
                    }
                    _ => {
                        merged.insert(var, val);
                    }
                }
            }
            if ok {
                // The triple-term subject path forces the subject and may fan
                // out over multiple candidates, so the quad-take budget is not
                // sound here — pass `None` and let the outer truncation apply.
                results.extend(eval_triple_pattern_core(
                    tp,
                    Some(term_id),
                    &merged,
                    datastore,
                    active_graph,
                    None,
                    deadline,
                )?);
            }
        }
        return Ok(results);
    }

    eval_triple_pattern_core(tp, None, sub, datastore, active_graph, budget, deadline)
}

/// Core outer-pattern evaluation shared by plain triple patterns and the
/// triple-term-subject case above. `forced_subject`, when `Some`, overrides
/// whatever `tp.subject` would otherwise resolve to (used when `tp.subject`
/// is a triple term already resolved to its own `GraphElementId`).
pub(crate) fn eval_triple_pattern_core(
    tp: &TriplePattern,
    forced_subject: Option<GraphElementId>,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    budget: Option<usize>,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    // If any constant in the pattern is absent from the store it can never match.
    for term in [&tp.subject, &tp.predicate, &tp.object] {
        if let Term::Constant(gel) = term {
            if !datastore.resources.resource_map.contains_key(gel) {
                return Ok(Vec::new());
            }
        }
    }

    let mut new_solutions = Vec::new();

    let g = match active_graph {
        ActiveGraph::Fixed(id) => Some(*id),
        ActiveGraph::Variable(v) => sub.get(v).and_then(|val| val.to_id(datastore)),
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
        return Ok(Vec::new());
    }
    let s = s_match.into_query_arg();
    let p = p_match.into_query_arg();
    let o = o_match.into_query_arg();

    // A row budget lets us stop enumerating matches early (issue #165). Pushing
    // the budget down as a *quad* limit (avoiding materialising every match)
    // is only sound when each matched quad yields exactly one solution — i.e.
    // the pattern has no repeated variable across its positions, which would
    // otherwise drop quads via the `ok` check below and under-produce. When
    // that gate fails we still cap the produced solutions, just after a full
    // scan.
    let quad_limit = match budget {
        Some(b) if forced_subject.is_none() && !pattern_repeats_variable(tp, active_graph) => {
            Some(b)
        }
        _ => None,
    };

    for quad in datastore.quads_matching_limited(g, s, p, o, quad_limit) {
        // Per-candidate check (issue #372): the loop enumerating matching
        // quads is the actual "matching loop" that can run long on a
        // dense/unselective pattern (e.g. an unbound predicate over a large
        // graph).
        deadline.check()?;
        let mut new_sub = sub.clone();
        let mut ok = true;

        // Bind a variable to the interned ID of a matched quad field. Keeping
        // the `GraphElementId` (rather than materialising the `GraphElement`)
        // is the #141 hot-path win: no per-match clone/allocation.
        macro_rules! bind {
            ($term:expr, $id:expr) => {
                if let Term::Variable(v) = $term {
                    let new_val = PartialSubValue::Interned($id);
                    match new_sub.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), new_val);
                        }
                    }
                }
            };
        }

        bind!(&tp.subject, quad.subject);
        bind!(&tp.predicate, quad.predicate);
        bind!(&tp.object, quad.obj);

        if let ActiveGraph::Variable(graph_var) = active_graph {
            let new_val = PartialSubValue::Interned(quad.triple_id);
            match new_sub.get(graph_var) {
                Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                    ok = false;
                }
                _ => {
                    new_sub.insert(graph_var.clone(), new_val);
                }
            }
        }

        if ok {
            new_solutions.push(new_sub);
            // Always-safe backstop: we only ever need `budget` solutions from
            // this call, regardless of whether the quad-take gate applied.
            if let Some(b) = budget {
                if new_solutions.len() >= b {
                    break;
                }
            }
        }
    }
    Ok(new_solutions)
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
pub(crate) enum MatchTerm {
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
    pub(crate) fn into_query_arg(self) -> Option<GraphElementId> {
        match self {
            MatchTerm::Bound(id) => Some(id),
            MatchTerm::Wildcard => None,
            MatchTerm::Never => {
                unreachable!("caller must check for MatchTerm::Never before converting")
            }
        }
    }
}

pub(crate) fn resolve_match_term(
    term: &Term,
    sub: &PartialSub,
    datastore: &Datastore,
) -> MatchTerm {
    match term {
        Term::Variable(v) => match sub.get(v) {
            // A binding straight from a quad is by construction interned — use
            // its id directly, no store lookup.
            Some(PartialSubValue::Interned(id)) => MatchTerm::Bound(*id),
            Some(PartialSubValue::Computed(gel)) => match datastore.resources.resource_map.get(gel)
            {
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
pub(crate) fn triple_term_candidates(
    inner: &TriplePattern,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Vec<(GraphElementId, HashMap<String, PartialSubValue>)> {
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
                Some(val) => match val.to_id(datastore) {
                    Some(id) => Slot::Known(id),
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
        let mut bindings: HashMap<String, PartialSubValue> = HashMap::new();
        let mut ok = true;

        macro_rules! bind_free {
            ($slot:expr, $id:expr) => {
                if let Slot::Free(v) = $slot {
                    let new_val = PartialSubValue::Interned($id);
                    match bindings.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => ok = false,
                        _ => {
                            bindings.insert(v.clone(), new_val);
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
        sub.insert("y".to_string(), PartialSubValue::Computed(computed));

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
        sub.insert("y".to_string(), PartialSubValue::Interned(id));
        let term = Term::Variable("y".to_string());

        let result = resolve_match_term(&term, &sub, &ds);

        assert!(
            matches!(result, MatchTerm::Bound(bound_id) if bound_id == id),
            "a variable bound to an interned value must resolve to MatchTerm::Bound(its id)"
        );
    }
}

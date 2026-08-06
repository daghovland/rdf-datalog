/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! BF (Backward-Forward) incremental maintenance of a materialised Datalog closure.
//!
//! The two-phase algorithm:
//! 1. **Backward phase** — BFS through the `DerivedFromIndex` reverse graph to compute the
//!    *possibly-deleted* set PD: every derived fact whose derivation chain passes through at
//!    least one deleted base fact.
//! 2. **Forward phase** — remove PD from the closure, then re-run semi-naive materialisation.
//!    Facts in PD that are still derivable from surviving base facts will be re-derived.
//!
//! A tipping-point guard falls back to full re-materialisation when |PD|/|derived| > 25%,
//! avoiding pathological cases where incremental maintenance is more expensive than a rebuild.
//!
//! Related issue: [#109](https://github.com/daghovland/rdf-datalog/issues/109),
//! part of epic [#83](https://github.com/daghovland/rdf-datalog/issues/83).

use crate::reasoner::{DatalogProgram, ReasoningError};
use crate::stratifier::RulePartitioner;
use crate::types::{Derivation, Rule};
use dag_rdf::{Datastore, Quad, QuadTable};
use std::collections::{HashMap, HashSet, VecDeque};

/// Tipping-point: if |PD|/|derived| > this fraction, fall back to full re-materialisation.
const FALLBACK_THRESHOLD: f64 = 0.25;

/// Incremental reasoner implementing the BF algorithm for maintaining a materialised
/// Datalog closure under base-fact insertions and deletions.
///
/// The reasoner is initialised by materialising from scratch (with derivation tracking).
/// Subsequent updates are applied via [`Self::apply_deletions`] and [`Self::apply_insertions`].
pub struct IncrementalReasoner {
    /// One `DatalogProgram` per stratum, in topological stratum order.
    programs: Vec<DatalogProgram>,
}

impl IncrementalReasoner {
    /// Materialise from scratch with derivation tracking enabled.
    ///
    /// Stratifies `rules` and runs semi-naive materialisation over each stratum in order.
    ///
    /// Returns `Err(ReasoningError::Contradiction)` if a genuine, correctly-derived
    /// inconsistency is found — instead of panicking, see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301). `base` may
    /// contain a partially-materialised closure in that case; the caller owns the
    /// store and should discard/reset it rather than reuse a half-built reasoner.
    pub fn new(rules: Vec<Rule>, base: &mut Datastore) -> Result<Self, ReasoningError> {
        let stratifier = RulePartitioner::new(rules);
        let strata = stratifier.order_rules()?;
        let mut programs: Vec<DatalogProgram> =
            strata.into_iter().map(DatalogProgram::new).collect();
        for program in &mut programs {
            program.materialise_seminaive(base)?;
        }
        Ok(IncrementalReasoner { programs })
    }

    /// Apply a batch of base-fact deletions using the BF algorithm.
    ///
    /// Returns the number of derived facts removed from the closure.
    ///
    /// Returns `Err(ReasoningError::Contradiction)` if re-derivation after the
    /// deletion produces a genuine inconsistency (e.g. a negated body atom that
    /// is only satisfied once the deleted fact is gone) — see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301). On error,
    /// `base` and `self` may be left with the delete already applied and a
    /// partially-rebuilt closure; callers should recover via
    /// [`Self::rebuild_from_base`] rather than trust the partial state.
    pub fn apply_deletions(
        &mut self,
        base: &mut Datastore,
        deletes: &[Quad],
    ) -> Result<usize, ReasoningError> {
        if deletes.is_empty() {
            return Ok(0);
        }

        // --- Backward phase ---
        let pd = self.backward_phase(deletes);

        // --- Tipping-point check ---
        let total_derived: usize = self
            .programs
            .iter()
            .map(|p| p.derived_from.iter().count())
            .sum();
        if total_derived > 0 && pd.len() as f64 / total_derived as f64 > FALLBACK_THRESHOLD {
            // PD is large relative to the closure: full rebuild is cheaper.
            return self.full_rematerialise(base, deletes);
        }

        // --- Remove deleted base facts ---
        for q in deletes {
            base.named_graphs.remove_quad(*q);
        }

        // --- Forward phase ---
        self.forward_phase(base, pd, deletes)
    }

    /// Apply a batch of base-fact insertions.
    ///
    /// Inserts the quads into the store and re-runs semi-naive evaluation so that
    /// only quads triggered by the new base facts produce new inferences.
    ///
    /// Returns `Err(ReasoningError::Contradiction)` on a genuine, correctly-derived
    /// inconsistency instead of panicking — see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301). Unlike the
    /// original implementation, **on error `base` and `self` are restored to
    /// exactly their pre-call state** via a cheap undo-log rollback (see
    /// `undo_insertions`) — no full rebuild is performed. Because
    /// [`dag_rdf::QuadTable::add_quad`]/`add_intensional_quad` only ever
    /// *append*, "everything this call added" is exactly the quad-list
    /// suffix appended since entry, and every genuinely-new `derived_from`
    /// entry recorded this call is tracked in a buffer and can be
    /// `unrecord`ed precisely. This makes rollback cost proportional to
    /// *this call's own delta*, not the whole closure — see
    /// [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    /// [`Self::rebuild_from_base`] remains available as a slower fallback
    /// (e.g. if a caller wants to double-check soundness after a rollback).
    pub fn apply_insertions(
        &mut self,
        base: &mut Datastore,
        inserts: &[Quad],
    ) -> Result<(), ReasoningError> {
        let quad_start = base.named_graphs.quad_count;
        for q in inserts {
            base.named_graphs.add_quad(*q);
        }
        // Re-run semi-naive; already-present derived facts are skipped by the dedup
        // check in `add_intensional_quad`, so only genuinely new inferences are added.
        // Track every genuinely new derivation entry per program so a
        // contradiction can be undone exactly, without a full rebuild.
        let mut tracked: Vec<Vec<(Quad, Derivation)>> = Vec::with_capacity(self.programs.len());
        for program in &mut self.programs {
            let mut buf = Vec::new();
            let result = program.materialise_seminaive_tracked(base, &mut buf);
            tracked.push(buf);
            if let Err(e) = result {
                self.undo_insertions(base, quad_start, &tracked);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Undo exactly what [`Self::apply_insertions`] changed during a call
    /// that failed partway through: remove every `(quad, Derivation)` entry
    /// recorded in `tracked` (per program, in the same order as
    /// `self.programs`) from each program's `derived_from` index, then
    /// truncate `base`'s quad table back to `quad_start` — the quad count
    /// captured before any base fact was inserted or any derivation added.
    /// Since insertion only ever appends, this restores the exact pre-call
    /// state at a cost proportional to this call's own delta.
    fn undo_insertions(
        &mut self,
        base: &mut Datastore,
        quad_start: usize,
        tracked: &[Vec<(Quad, Derivation)>],
    ) {
        for (program, buf) in self.programs.iter_mut().zip(tracked.iter()) {
            for (q, d) in buf {
                program.derived_from.unrecord(q, d);
            }
        }
        base.named_graphs.truncate_to(quad_start);
    }

    /// Rebuild the derived closure from scratch using only the base
    /// (extensional) facts currently present in `base`, discarding any
    /// partially-materialised derived facts and derivation records.
    ///
    /// Intended for callers (e.g. `sparql_endpoint`) to restore a consistent
    /// state after [`Self::apply_insertions`]/[`Self::apply_deletions`] returns
    /// `Err(ReasoningError::Contradiction)` partway through materialisation:
    /// undo whatever base-fact change triggered the contradiction (so the
    /// surviving base facts are known-consistent), then call this to rebuild a
    /// sound closure over them. See
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301).
    ///
    /// Returns `Err` if the surviving base facts are *themselves* already
    /// contradictory (should not happen if the invariant "the store was
    /// consistent before the rejected change" holds — callers should treat
    /// this as a serious, non-recoverable-by-rollback error).
    ///
    /// **Precondition — every intensional (derived) quad in `base` must be
    /// re-derivable from `self.programs`.** This rebuild keeps only
    /// `base.named_graphs.extensional_quads()` and re-runs *this reasoner's
    /// own* strata (`self.programs`) over them; every stratum this instance
    /// owns is re-derived correctly (including strata earlier than the one
    /// that contradicted — see `test_rebuild_from_base_preserves_earlier_stratum_derivations`
    /// in this module's tests). But an intensional quad produced by some
    /// *other*, unrelated `evaluate_rules`/`DatalogProgram` call — e.g. an
    /// eager OWL-RL materialisation done once up front and never registered
    /// with this `IncrementalReasoner` — is invisible to this reasoner's
    /// `derived_from` index and will be silently discarded, not re-derived.
    /// Do not call this on a store that mixes this reasoner's incrementally-
    /// maintained closure with derived quads from a separate reasoning pass.
    pub fn rebuild_from_base(&mut self, base: &mut Datastore) -> Result<(), ReasoningError> {
        let base_facts: Vec<Quad> = base.named_graphs.extensional_quads().collect();
        let hint = base_facts.len() as u32;
        base.named_graphs = QuadTable::new(hint);
        for q in base_facts {
            base.named_graphs.add_quad(q);
        }
        for program in &mut self.programs {
            program.derived_from = Default::default();
        }
        for program in &mut self.programs {
            program.materialise_seminaive(base)?;
        }
        Ok(())
    }

    /// Sum of `materialise_calls` across all strata's `DatalogProgram`s.
    ///
    /// Test-only instrumentation proving which rollback path actually ran on
    /// a contradiction: the undo-log fast path (`undo_insertions`/
    /// `undo_deletions`) never invokes materialisation again, while
    /// `rebuild_from_base` always does (once per program) — so a rollback
    /// that leaves this count unchanged from right after the failed call
    /// (no extra calls) demonstrates the fast path ran, not a rebuild. See
    /// [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    #[cfg(test)]
    pub(crate) fn materialise_call_count(&self) -> usize {
        self.programs.iter().map(|p| p.materialise_calls).sum()
    }

    // --- Internal helpers ---

    /// Backward phase: BFS through the reverse derivation graph.
    ///
    /// Starts from the deleted base facts and collects every derived quad whose
    /// derivation chain is broken — the possibly-deleted set PD.
    fn backward_phase(&self, deletes: &[Quad]) -> HashSet<Quad> {
        // Build reverse index: witness_quad → all derived quads that use it as a witness.
        let mut reverse: HashMap<Quad, Vec<Quad>> = HashMap::new();
        for program in &self.programs {
            for (derived, derivations) in program.derived_from.iter() {
                for d in derivations {
                    for &witness in &d.body_witnesses {
                        reverse.entry(witness).or_default().push(*derived);
                    }
                }
            }
        }

        // BFS: propagate deletion upward through derived facts.
        let mut pd: HashSet<Quad> = HashSet::new();
        let mut worklist: VecDeque<Quad> = deletes.iter().copied().collect();
        while let Some(q) = worklist.pop_front() {
            if let Some(dependents) = reverse.get(&q) {
                for &derived in dependents {
                    if pd.insert(derived) {
                        // Propagate: derived facts that depend on this one are also suspect.
                        worklist.push_back(derived);
                    }
                }
            }
        }
        pd
    }

    /// Forward phase: remove PD from the closure, then re-derive surviving facts.
    ///
    /// Returns the number of facts that were permanently removed (not re-derived).
    ///
    /// On a genuine contradiction during re-derivation, `base` and `self` are
    /// restored to exactly their pre-`apply_deletions` state via a cheap
    /// undo-log rollback (see `undo_deletions`) instead of requiring
    /// the caller to call [`Self::rebuild_from_base`] — cost proportional to
    /// this call's own delta (|PD| plus any re-derivations), not the whole
    /// closure. See [#320](https://github.com/daghovland/rdf-datalog/issues/320)
    /// and [`Self::apply_deletions`] for the overall `Err` contract.
    fn forward_phase(
        &mut self,
        base: &mut Datastore,
        pd: HashSet<Quad>,
        deletes: &[Quad],
    ) -> Result<usize, ReasoningError> {
        let removed = pd.len();
        // Snapshot every derivation entry PD removal is about to wipe, per
        // program, so a rollback can restore them exactly (not just
        // re-derive — a partial re-derivation run may not reach the same
        // fixpoint the pre-call state had).
        let mut removed_derivations: Vec<(Quad, usize, Vec<Derivation>)> = Vec::new();
        // Retract PD facts and their derivation records from both the store and the index.
        for q in &pd {
            base.named_graphs.remove_quad(*q);
            for (program_index, program) in self.programs.iter_mut().enumerate() {
                let derivations = program.derived_from.derivations_for(q);
                if !derivations.is_empty() {
                    removed_derivations.push((*q, program_index, derivations.to_vec()));
                }
                program.derived_from.remove(q);
            }
        }

        // Everything appended to the quad table from here on (re-derivations)
        // can be undone by truncating back to this point.
        let redelta_start = base.named_graphs.quad_count;

        // Re-derive: semi-naive will re-add any PD fact that is still provable from the
        // surviving base facts.  Facts that were in PD but are re-derived will be
        // re-inserted by `add_intensional_quad` (dedup ensures no double-counting).
        let mut tracked: Vec<Vec<(Quad, Derivation)>> = Vec::with_capacity(self.programs.len());
        for program in &mut self.programs {
            let mut buf = Vec::new();
            let result = program.materialise_seminaive_tracked(base, &mut buf);
            tracked.push(buf);
            if let Err(e) = result {
                self.undo_deletions(
                    base,
                    redelta_start,
                    &tracked,
                    &pd,
                    deletes,
                    &removed_derivations,
                );
                return Err(e);
            }
        }
        Ok(removed)
    }

    /// Undo exactly what the forward phase of [`Self::apply_deletions`]
    /// changed during a call that failed partway through re-derivation:
    ///
    /// 1. Remove every `(quad, Derivation)` entry recorded in `tracked`
    ///    (the re-derivation attempt) from each program's `derived_from` index.
    /// 2. Truncate the quad table back to `redelta_start`, undoing any quads
    ///    the re-derivation attempt appended.
    /// 3. Re-insert every PD quad (as intensional — they were derived facts)
    ///    and every originally-deleted base fact (as extensional).
    /// 4. Restore each PD quad's exact pre-removal `derived_from` entries
    ///    from `removed_derivations`.
    ///
    /// Cost is proportional to this call's own delta (|PD| plus whatever the
    /// aborted re-derivation attempt added), not the whole closure. See
    /// [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    #[allow(clippy::too_many_arguments)]
    fn undo_deletions(
        &mut self,
        base: &mut Datastore,
        redelta_start: usize,
        tracked: &[Vec<(Quad, Derivation)>],
        pd: &HashSet<Quad>,
        deletes: &[Quad],
        removed_derivations: &[(Quad, usize, Vec<Derivation>)],
    ) {
        for (program, buf) in self.programs.iter_mut().zip(tracked.iter()) {
            for (q, d) in buf {
                program.derived_from.unrecord(q, d);
            }
        }
        base.named_graphs.truncate_to(redelta_start);
        for q in pd {
            base.named_graphs.add_intensional_quad(*q);
        }
        for q in deletes {
            base.named_graphs.add_quad(*q);
        }
        for (q, program_index, derivations) in removed_derivations {
            for d in derivations {
                self.programs[*program_index]
                    .derived_from
                    .record(*q, d.clone());
            }
        }
    }

    /// Full re-materialisation fallback for large deletes.
    ///
    /// Removes the deleted base facts, snapshots surviving base facts, tears down
    /// the derived closure, and rebuilds from scratch.
    ///
    /// See [`Self::apply_deletions`] for the `Err` contract.
    fn full_rematerialise(
        &mut self,
        base: &mut Datastore,
        deletes: &[Quad],
    ) -> Result<usize, ReasoningError> {
        // Remove base facts.
        for q in deletes {
            base.named_graphs.remove_quad(*q);
        }
        // Snapshot only the base (non-derived) facts that survived.
        let base_facts: Vec<Quad> = base.named_graphs.extensional_quads().collect();
        let hint = base_facts.len() as u32;

        // Clear the entire store and reset derivation indexes.
        base.named_graphs = QuadTable::new(hint);
        for q in base_facts {
            base.named_graphs.add_quad(q);
        }

        let before = base.named_graphs.quad_count;
        for program in &mut self.programs {
            program.derived_from = Default::default();
            program.materialise_seminaive(base)?;
        }
        // Return the number of newly derived facts (may differ from the original PD size
        // since some may have been re-derivable, but we report the new derivations added).
        Ok(base.named_graphs.quad_count - before)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RuleAtom, RuleHead};
    use dag_rdf::{DEFAULT_GRAPH_ELEMENT_ID, IriReference, QuadPattern, RdfResource, Term};

    /// Build a Datastore pre-loaded with interned resources a, p, b, c and return
    /// (datastore, g, a, p, b, c).
    fn setup_store() -> (Datastore, u32, u32, u32, u32, u32) {
        let mut ds = Datastore::new(100);
        let g = DEFAULT_GRAPH_ELEMENT_ID;
        let a = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/a".to_string(),
            )));
        let p = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p".to_string(),
            )));
        let b = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/b".to_string(),
            )));
        let c = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/c".to_string(),
            )));
        (ds, g, a, p, b, c)
    }

    /// Build the standard transitivity rule: { ?x p ?y, ?y p ?z } => { ?x p ?z }
    fn transitivity_rule(g: u32, p: u32) -> Rule {
        Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("z".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("y".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("z".to_string()),
                }),
            ],
        }
    }

    /// Deleting the only supporting base fact for a derived quad must remove it.
    ///
    /// Setup: A→B→C with transitivity rule.
    /// After materialisation, A→C is derived.
    /// Deleting A→B must remove A→C (no other path exists).
    #[test]
    fn test_delete_base_fact_removes_derived() {
        let (mut ds, g, a, p, b, c) = setup_store();

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);

        let mut reasoner =
            IncrementalReasoner::new(vec![transitivity_rule(g, p)], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived before deletion"
        );

        // Delete A→B: the only derivation of A→C uses it as a witness.
        reasoner.apply_deletions(&mut ds, &[fact_ab]).unwrap();

        assert!(
            !ds.named_graphs.contains(&fact_ab),
            "deleted base fact A→B should be gone"
        );
        assert!(
            !ds.named_graphs.contains(&derived_ac),
            "derived A→C should be removed after deleting its only support A→B"
        );
        // B→C is not implicated by deleting A→B.
        assert!(
            ds.named_graphs.contains(&fact_bc),
            "unrelated fact B→C should remain"
        );
    }

    /// When a derived fact has two independent derivation paths, deleting the support
    /// for one path must not remove the fact (the second path still validates it).
    ///
    /// Setup: A→B, B→C (derive A→C via transitivity), plus A→p2→C and an alias rule
    /// p2→p.  After materialisation, A→C is derivable via both paths.
    /// Deleting A→B should leave A→C intact (still derivable via A→p2→C → alias rule).
    #[test]
    fn test_delete_base_fact_keeps_multiply_derived() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        // Second, independent path: A →p2→ C
        let fact_ac_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);
        ds.named_graphs.add_quad(fact_ac_p2);

        // Alias rule: { ?x p2 ?z } => { ?x p ?z }
        let alias_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("z".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p2),
                object: Term::Variable("z".to_string()),
            })],
        };

        let mut reasoner =
            IncrementalReasoner::new(vec![transitivity_rule(g, p), alias_rule], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived before deletion"
        );

        // Delete A→B: removes the transitivity path, but alias path survives.
        reasoner.apply_deletions(&mut ds, &[fact_ab]).unwrap();

        assert!(
            !ds.named_graphs.contains(&fact_ab),
            "deleted base fact A→B should be gone"
        );
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should survive: still derivable via A→p2→C + alias rule"
        );
    }

    /// Inserting a new base fact that completes a derivation chain must add the
    /// derived facts produced by that chain.
    ///
    /// Setup: only B→C is in the store initially; no derived facts.
    /// Insert A→B; the transitivity rule should derive A→C.
    #[test]
    fn test_insert_adds_derived() {
        let (mut ds, g, a, p, b, c) = setup_store();

        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_bc);

        let mut reasoner =
            IncrementalReasoner::new(vec![transitivity_rule(g, p)], &mut ds).unwrap();

        // No derived A→C yet (no A→B).
        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            !ds.named_graphs.contains(&derived_ac),
            "A→C should not exist before inserting A→B"
        );

        // Insert A→B: should trigger derivation of A→C.
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        reasoner.apply_insertions(&mut ds, &[fact_ab]).unwrap();

        assert!(
            ds.named_graphs.contains(&fact_ab),
            "inserted base fact A→B should be present"
        );
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived after inserting A→B"
        );
    }

    /// Combined update: delete one base fact and insert another.
    ///
    /// Setup: A→B, B→C, C→D.  Materialise: derives A→C, A→D, B→D.
    /// Delete B→C (removes A→C, A→D, B→D from closure).
    /// Insert A→C directly as a base fact.
    /// After: A→C is present (base), A→D is derived via A→C + C→D, B→D is gone.
    #[test]
    fn test_apply_update_delete_and_insert() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let d = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: d,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);
        ds.named_graphs.add_quad(fact_cd);

        let mut reasoner =
            IncrementalReasoner::new(vec![transitivity_rule(g, p)], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        let derived_ad = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: d,
        };
        let derived_bd = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: d,
        };

        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived initially"
        );
        assert!(
            ds.named_graphs.contains(&derived_ad),
            "A→D should be derived initially"
        );
        assert!(
            ds.named_graphs.contains(&derived_bd),
            "B→D should be derived initially"
        );

        // Step 1: delete B→C.
        reasoner.apply_deletions(&mut ds, &[fact_bc]).unwrap();

        assert!(
            !ds.named_graphs.contains(&fact_bc),
            "deleted B→C should be gone"
        );

        // Step 2: insert A→C as a new base fact.
        let new_base_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        reasoner.apply_insertions(&mut ds, &[new_base_ac]).unwrap();

        // A→C is now present (either re-derived or as a base fact).
        assert!(
            ds.named_graphs.contains(&new_base_ac),
            "A→C should be present after insertion"
        );
        // A→D is re-derivable: A→C + C→D (C→D was never deleted).
        assert!(
            ds.named_graphs.contains(&derived_ad),
            "A→D should be re-derived via A→C + C→D"
        );
        // B→D: only derivable via B→C + C→D or B→?→D chains.
        // B→C was deleted; no other path from B to D.
        assert!(
            !ds.named_graphs.contains(&derived_bd),
            "B→D should remain absent: no surviving path from B to D"
        );
    }

    /// `IncrementalReasoner::new` must return `Err(ReasoningError::Contradiction)`
    /// instead of panicking when the initial materialisation derives a genuine
    /// contradiction. See https://github.com/daghovland/rdf-datalog/issues/301.
    #[test]
    fn test_new_returns_err_on_contradiction() {
        let (mut ds, g, a, p, b, _c) = setup_store();
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        let contradiction_rule = Rule {
            head: RuleHead::Contradiction,
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            })],
        };

        let result = IncrementalReasoner::new(vec![contradiction_rule], &mut ds);
        match result {
            Err(ReasoningError::Contradiction(_)) => {}
            Ok(_) => panic!("expected a Contradiction error, got Ok"),
            Err(other) => panic!("expected a Contradiction error, got {other:?}"),
        }
    }

    /// `apply_insertions` must return `Err(ReasoningError::Contradiction)` (not
    /// panic) when a newly-inserted base fact triggers a contradiction rule, and
    /// `rebuild_from_base` must recover a consistent, usable reasoner/store
    /// afterwards (once the offending insert is retracted).
    /// See https://github.com/daghovland/rdf-datalog/issues/301.
    #[test]
    fn test_apply_insertions_contradiction_then_rebuild_recovers() {
        let (mut ds, g, a, p, b, c) = setup_store();

        // A rule flags a contradiction whenever ?x p ?y AND ?x p2 ?y both hold
        // for the same (x, y) — i.e. a "disjoint properties" style check.
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));
        let contradiction_rule = Rule {
            head: RuleHead::Contradiction,
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p2),
                    object: Term::Variable("y".to_string()),
                }),
            ],
        };

        // Start from a consistent state: only A→p2→B is present, no A→p→B yet.
        let fact_ab_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab_p2);
        // An unrelated fact that should survive the whole ordeal.
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_bc);

        let mut reasoner = IncrementalReasoner::new(vec![contradiction_rule], &mut ds).unwrap();

        // Now insert A→p→B: combined with A→p2→B, this triggers the contradiction.
        let fact_ab_p = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let result = reasoner.apply_insertions(&mut ds, &[fact_ab_p]);
        match result {
            Err(ReasoningError::Contradiction(_)) => {}
            Ok(_) => panic!("expected a Contradiction error, got Ok"),
            Err(other) => panic!("expected a Contradiction error, got {other:?}"),
        }

        // Recover: retract the offending insert and rebuild the closure.
        ds.named_graphs.remove_quad(fact_ab_p);
        reasoner
            .rebuild_from_base(&mut ds)
            .expect("rebuild from the now-consistent base facts must succeed");

        // The store is usable again: the offending fact is gone, the
        // unrelated fact survived, and a subsequent operation still works.
        assert!(
            !ds.named_graphs.contains(&fact_ab_p),
            "offending insert should have been retracted"
        );
        assert!(
            ds.named_graphs.contains(&fact_bc),
            "unrelated fact should survive the contradiction + recovery"
        );

        // A follow-up, non-contradictory insertion still works after recovery.
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p2,
            obj: b,
        };
        reasoner
            .apply_insertions(&mut ds, &[fact_cd])
            .expect("reasoner must remain usable after recovering from a contradiction");
        assert!(ds.named_graphs.contains(&fact_cd));
    }

    /// `rebuild_from_base` re-runs *every* stratum of `self.programs`, in
    /// order, from the surviving extensional facts — so a contradiction that
    /// fires in a later stratum must not lose facts derived by an earlier
    /// stratum: rebuilding re-derives them too.
    ///
    /// (This is specific to the strata *this* `IncrementalReasoner` owns.
    /// Any intensional quad in the store produced by a *different*,
    /// unrelated `evaluate_rules`/`DatalogProgram` call — not part of
    /// `self.programs` — is not tracked by this reasoner's `derived_from`
    /// index and would be discarded by a rebuild. See the precondition
    /// documented on [`IncrementalReasoner::rebuild_from_base`].)
    ///
    /// Setup: stratum 1 copies `?x p ?y` to `?x p3 ?y`. Stratum 2 (depends
    /// negatively on `p3`, so it stratifies after stratum 1) contradicts on
    /// `?x p2 ?y` unless `?x p3 ?y` also holds. Base facts: `a p b` (derives
    /// `a p3 b` in stratum 1). Inserting `c p2 d` — with no `c p3 d` — fires
    /// the stratum-2 contradiction. After retracting the offending insert and
    /// calling `rebuild_from_base`, `a p3 b` (stratum 1's derivation, wholly
    /// unrelated to the contradiction) must still be present.
    #[test]
    fn test_rebuild_from_base_preserves_earlier_stratum_derivations() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));
        let p3 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p3".to_string(),
            )));
        let d = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));

        // Stratum 1: ?x p3 ?y :- ?x p ?y
        let copy_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p3),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            })],
        };
        // Stratum 2 (negatively depends on p3, derived above):
        // Contradiction :- ?x p2 ?y, NOT ?x p3 ?y
        let contradiction_rule = Rule {
            head: RuleHead::Contradiction,
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p2),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::NotPattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p3),
                    object: Term::Variable("y".to_string()),
                }),
            ],
        };

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        let mut reasoner =
            IncrementalReasoner::new(vec![copy_rule, contradiction_rule], &mut ds).unwrap();

        let derived_ab3 = Quad {
            triple_id: g,
            subject: a,
            predicate: p3,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&derived_ab3),
            "stratum 1 should have derived a p3 b at initial materialisation"
        );

        // Insert c p2 d: no c p3 d exists, so stratum 2's contradiction fires.
        let fact_cd_p2 = Quad {
            triple_id: g,
            subject: c,
            predicate: p2,
            obj: d,
        };
        let result = reasoner.apply_insertions(&mut ds, &[fact_cd_p2]);
        match result {
            Err(ReasoningError::Contradiction(_)) => {}
            Ok(_) => panic!("expected a Contradiction error, got Ok"),
            Err(other) => panic!("expected a Contradiction error, got {other:?}"),
        }

        // Recover: retract the offending insert, then rebuild.
        ds.named_graphs.remove_quad(fact_cd_p2);
        reasoner
            .rebuild_from_base(&mut ds)
            .expect("rebuild from the now-consistent base facts must succeed");

        assert!(
            !ds.named_graphs.contains(&fact_cd_p2),
            "offending insert should have been retracted"
        );
        assert!(
            ds.named_graphs.contains(&derived_ab3),
            "stratum 1's derivation (a p3 b) must survive a rebuild triggered \
             by an unrelated stratum-2 contradiction"
        );
    }

    /// `apply_insertions` must roll back a contradiction via the cheap
    /// undo-log fast path (`undo_insertions`), not a full `rebuild_from_base`
    /// — proven behaviourally: after rollback (a) the store is exactly
    /// quad-for-quad identical to before the call, including an unrelated
    /// derived fact from a completely separate derivation chain, and (b)
    /// `materialise_call_count()` only increased by the number of programs
    /// materialisation was actually (re-)attempted for during THIS call —
    /// a full rebuild would additionally re-invoke materialisation for every
    /// program again during rollback, increasing the count further.
    /// See https://github.com/daghovland/rdf-datalog/issues/320.
    #[test]
    fn test_apply_insertions_contradiction_rollback_uses_undo_log() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));
        let d = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));
        let e = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/e".to_string(),
            )));

        // Disjoint-properties contradiction: ?x p ?y AND ?x p2 ?y both hold.
        let contradiction_rule = Rule {
            head: RuleHead::Contradiction,
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p2),
                    object: Term::Variable("y".to_string()),
                }),
            ],
        };

        // Consistent starting state: A p2 B (no A p B yet).
        let fact_ab_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab_p2);
        // A completely unrelated derivation chain: C -p-> D -p-> E, transitively
        // deriving C -p-> E. Untouched by anything that follows.
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: d,
        };
        let fact_de = Quad {
            triple_id: g,
            subject: d,
            predicate: p,
            obj: e,
        };
        ds.named_graphs.add_quad(fact_cd);
        ds.named_graphs.add_quad(fact_de);

        let mut reasoner =
            IncrementalReasoner::new(vec![transitivity_rule(g, p), contradiction_rule], &mut ds)
                .unwrap();

        let derived_ce = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: e,
        };
        assert!(
            ds.named_graphs.contains(&derived_ce),
            "unrelated chain C-D-E should have derived C p E before the insert under test"
        );

        // Snapshot the exact pre-call state.
        let quads_before = ds.named_graphs.quad_list.clone();
        let derivations_ce_before: Vec<Vec<crate::types::Derivation>> = reasoner
            .programs
            .iter()
            .map(|p| p.derived_from.derivations_for(&derived_ce).to_vec())
            .collect();
        let call_count_before = reasoner.materialise_call_count();

        // Insert A p B: combined with A p2 B, this triggers the contradiction.
        let fact_ab_p = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let result = reasoner.apply_insertions(&mut ds, &[fact_ab_p]);
        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );

        // (a) Exact rollback: quad-for-quad identical to before the call.
        assert_eq!(
            ds.named_graphs.quad_list, quads_before,
            "store must be exactly quad-for-quad identical to its pre-call state"
        );
        assert!(
            !ds.named_graphs.contains(&fact_ab_p),
            "the offending insert must not be present after rollback"
        );
        assert!(
            ds.named_graphs.contains(&derived_ce),
            "unrelated derived fact C p E must survive the rollback untouched"
        );
        let derivations_ce_after: Vec<Vec<crate::types::Derivation>> = reasoner
            .programs
            .iter()
            .map(|p| p.derived_from.derivations_for(&derived_ce).to_vec())
            .collect();
        assert_eq!(
            derivations_ce_after, derivations_ce_before,
            "unrelated fact's derivation records must be untouched by the rollback"
        );

        // (b) The undo-log fast path ran, not a full rebuild: materialisation
        // was invoked exactly once per program for this call's own attempt,
        // and never again during rollback (a `rebuild_from_base`-style
        // recovery would invoke it once more per program).
        let attempted_this_call = reasoner.programs.len();
        assert_eq!(
            reasoner.materialise_call_count(),
            call_count_before + attempted_this_call,
            "rollback must not have re-invoked materialisation (no rebuild_from_base)"
        );

        // A follow-up, non-contradictory insertion still works after recovery.
        let fact_ef = Quad {
            triple_id: g,
            subject: e,
            predicate: p,
            obj: c,
        };
        reasoner
            .apply_insertions(&mut ds, &[fact_ef])
            .expect("reasoner must remain usable after the undo-log rollback");
        assert!(ds.named_graphs.contains(&fact_ef));
    }

    /// `apply_deletions` must roll back a re-derivation-triggered contradiction
    /// via the cheap undo-log fast path (`undo_deletions`), not a full
    /// `rebuild_from_base` — proven the same way as the insertions test: exact
    /// quad-for-quad restoration (including an unrelated derived fact from a
    /// separate derivation chain) plus a `materialise_call_count()` that only
    /// grew by this call's own attempted programs.
    ///
    /// This specifically exercises the scenario the issue calls out: BF's
    /// backward phase cannot detect this contradiction (it only traces
    /// positive-witness dependencies, and nothing positively depends on the
    /// deleted quad as a witness) — only the forward phase's re-derivation
    /// pass (a full re-run of `materialise_seminaive`, which re-checks every
    /// `Contradiction` rule against the surviving state) catches it. See
    /// https://github.com/daghovland/rdf-datalog/issues/320.
    #[test]
    fn test_apply_deletions_contradiction_rollback_uses_undo_log() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));
        let p3 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p3".to_string(),
            )));
        let d = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));

        // Stratum 1: ?x p3 ?y :- ?x p ?y
        let copy_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p3),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            })],
        };
        // Stratum 2 (negatively depends on p3): Contradiction :- ?x p2 ?y, NOT ?x p3 ?y
        let contradiction_rule = Rule {
            head: RuleHead::Contradiction,
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p2),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::NotPattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p3),
                    object: Term::Variable("y".to_string()),
                }),
            ],
        };

        // Base facts: A p B (derives A p3 B in stratum 1).
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        // C p2 D, guarded by a base (not derived) C p3 D fact — no
        // contradiction yet, since the NOT p3 condition is unsatisfied.
        let fact_cd_p2 = Quad {
            triple_id: g,
            subject: c,
            predicate: p2,
            obj: d,
        };
        let fact_cd_p3 = Quad {
            triple_id: g,
            subject: c,
            predicate: p3,
            obj: d,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_cd_p2);
        ds.named_graphs.add_quad(fact_cd_p3);

        let mut reasoner =
            IncrementalReasoner::new(vec![copy_rule, contradiction_rule], &mut ds).unwrap();

        let derived_ab3 = Quad {
            triple_id: g,
            subject: a,
            predicate: p3,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&derived_ab3),
            "stratum 1 should have derived A p3 B at initial materialisation"
        );

        // Nothing positively depends on C p3 D as a witness, so BF's backward
        // phase will compute an empty PD for it — the contradiction can only
        // be caught by the forward phase's re-derivation pass.
        let quads_before = ds.named_graphs.quad_list.clone();
        let extensional_before: HashMap<Quad, bool> = quads_before
            .iter()
            .map(|q| (*q, ds.named_graphs.is_extensional(q)))
            .collect();
        let derivations_ab3_before: Vec<Vec<crate::types::Derivation>> = reasoner
            .programs
            .iter()
            .map(|p| p.derived_from.derivations_for(&derived_ab3).to_vec())
            .collect();
        let call_count_before = reasoner.materialise_call_count();

        let result = reasoner.apply_deletions(&mut ds, &[fact_cd_p3]);
        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );

        // (a) Exact rollback — same quads present with the same
        // extensional/intensional status. `apply_deletions`'s rollback
        // re-inserts the deleted base fact via `add_quad` (appending), so
        // insertion order need not exactly match the original list (unlike
        // `apply_insertions`'s `truncate_to`-based rollback, which is
        // order-preserving by construction); set equality plus per-quad
        // extensional/intensional status is the invariant that actually
        // matters for correctness.
        let quads_before_set: HashSet<Quad> = quads_before.iter().copied().collect();
        let quads_after_set: HashSet<Quad> = ds.named_graphs.quad_list.iter().copied().collect();
        assert_eq!(
            quads_after_set, quads_before_set,
            "store must contain exactly the same quads as before the call"
        );
        for (q, was_extensional) in &extensional_before {
            assert_eq!(
                ds.named_graphs.is_extensional(q),
                *was_extensional,
                "extensional/intensional status of {q:?} must be unchanged by the rollback"
            );
        }
        assert!(
            ds.named_graphs.contains(&fact_cd_p3),
            "the base fact targeted for deletion must be restored after rollback"
        );
        assert!(
            ds.named_graphs.contains(&derived_ab3),
            "stratum 1's unrelated derivation (A p3 B) must survive the rollback"
        );
        let derivations_ab3_after: Vec<Vec<crate::types::Derivation>> = reasoner
            .programs
            .iter()
            .map(|p| p.derived_from.derivations_for(&derived_ab3).to_vec())
            .collect();
        assert_eq!(
            derivations_ab3_after, derivations_ab3_before,
            "unrelated fact's derivation records must be untouched by the rollback"
        );

        // (b) The undo-log fast path ran, not a full rebuild.
        let attempted_this_call = reasoner.programs.len();
        assert_eq!(
            reasoner.materialise_call_count(),
            call_count_before + attempted_this_call,
            "rollback must not have re-invoked materialisation (no rebuild_from_base)"
        );

        // A follow-up, non-contradictory deletion still works after recovery.
        reasoner
            .apply_deletions(&mut ds, &[fact_ab])
            .expect("reasoner must remain usable after the undo-log rollback");
        assert!(!ds.named_graphs.contains(&fact_ab));
        assert!(!ds.named_graphs.contains(&derived_ab3));
    }
}

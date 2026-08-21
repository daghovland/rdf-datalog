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
use crate::types::{Derivation, DerivedFromIndex, Rule, RuleHead};
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
    /// Number of times a `full_rematerialise*` fallback has run. Test-only
    /// instrumentation: unlike `materialise_call_count()` (which increases by
    /// the same amount on both the incremental and the fallback path — both
    /// call materialisation exactly once per program — this counter is the
    /// only thing that actually distinguishes "the 25% `FALLBACK_THRESHOLD`
    /// guard tripped" from "the incremental BF path ran". See
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    #[cfg(test)]
    pub(crate) fallback_count: usize,
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
        let mut programs: Vec<DatalogProgram> = strata
            .into_iter()
            .map(DatalogProgram::new)
            .collect::<Result<Vec<_>, _>>()?;
        for program in &mut programs {
            program.materialise_seminaive(base)?;
        }
        Ok(IncrementalReasoner {
            programs,
            #[cfg(test)]
            fallback_count: 0,
        })
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

    /// Apply a batch of TBox (rule/axiom) retractions using the BF algorithm,
    /// seeded from rule removal rather than base-fact removal.
    ///
    /// The ABox-shaped counterpart to [`Self::apply_deletions`]. Typical
    /// caller flow: `owl_ontology::Ontology::remove_axiom` drops the axiom
    /// from the ontology model, `owl2rl2datalog::axiom2datalog` maps it to
    /// the `Rule`s it compiled to, and those rules are passed here to
    /// retract everything they derived (and re-derive anything still
    /// provable via a surviving rule).
    ///
    /// **Precondition: every rule in `rules` must be genuinely dead** — no
    /// longer produced by *any* surviving axiom. `owl2rl2datalog::owl2datalog`
    /// deduplicates its output, so one compiled `Rule` can be shared by
    /// several axioms; passing a still-justified rule disables it with no way
    /// back (the forward phase only re-derives via surviving, enabled rules),
    /// permanently and wrongly losing facts justified by the other axiom. See
    /// `owl2rl2datalog::axiom2datalog`'s doc comment for how a caller
    /// should compute this (the `datalog` crate has no dependency on
    /// `owl2rl2datalog`, so this cannot be a linkable intra-doc reference).
    /// Rules not found in any program (already removed,
    /// or never part of this reasoner) are silently skipped — the call is a
    /// no-op (`Ok(0)`) if none of `rules` match anything.
    ///
    /// Returns the number of derived facts removed from the closure (not
    /// re-derived). Same `Err(ReasoningError::Contradiction)`/rollback
    /// contract as [`Self::apply_deletions`] — on error, `base`/`self` are
    /// restored to their exact pre-call state (including re-enabling any
    /// rule this call disabled) via a cheap undo-log rollback. See
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    pub fn apply_rule_deletions(
        &mut self,
        base: &mut Datastore,
        rules: &[Rule],
    ) -> Result<usize, ReasoningError> {
        if rules.is_empty() {
            return Ok(0);
        }

        // Locate every (program_index, rule_id) occurrence of a targeted
        // rule across all strata, skipping already-disabled indices so
        // repeat calls are idempotent no-ops for already-removed rules.
        let mut targets: Vec<(usize, usize)> = Vec::new();
        for (program_index, program) in self.programs.iter().enumerate() {
            for (rule_id, existing) in program.rules.iter().enumerate() {
                if program.is_rule_disabled(rule_id) {
                    continue;
                }
                if rules.contains(existing) {
                    targets.push((program_index, rule_id));
                }
            }
        }
        if targets.is_empty() {
            return Ok(0);
        }

        // --- Backward phase ---
        let pd = self.backward_phase_from_rule_removal(base, &targets);

        // --- Tipping-point check ---
        let total_derived: usize = self
            .programs
            .iter()
            .map(|p| p.derived_from.iter().count())
            .sum();
        if total_derived > 0 && pd.len() as f64 / total_derived as f64 > FALLBACK_THRESHOLD {
            return self.full_rematerialise_rules(base, &targets);
        }

        // --- Disable the targeted rules so they can't refire ---
        for &(program_index, rule_id) in &targets {
            self.programs[program_index].disable_rule(rule_id);
        }

        // --- Forward phase ---
        self.forward_phase_rules(base, pd, &targets)
    }

    /// Add brand-new rules to an already-constructed reasoner, or re-enable
    /// rules previously retracted via [`Self::apply_rule_deletions`], without
    /// a full [`Self::new`] rebuild. The insertion-side counterpart to
    /// [`Self::apply_rule_deletions`]. See
    /// [`docs/plans/INCREMENTAL_RULE_INSERTION_474_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/INCREMENTAL_RULE_INSERTION_474_PLAN.md)
    /// for the full design rationale; this doc comment summarises the
    /// correctness contract.
    ///
    /// **What this accepts:** a rule in `new_rules` is classified as (a)
    /// already present and enabled — silently a no-op (idempotent), (b)
    /// present but disabled — **reactivated** in place, keeping its original
    /// stratum, or (c) genuinely new — **appended** as one or more brand-new
    /// final strata, sub-stratified among themselves.
    ///
    /// Both (b) and (c) are only accepted when they are **strictly
    /// downstream** of everything already materialised: no rule that is (or
    /// will become) enabled elsewhere in this reasoner may have a body atom —
    /// positive or negative — unifiable with the (re)introduced rule's head.
    /// If any such dependency edge exists, this returns
    /// `Err(ReasoningError::NotStratifiable)` **without mutating anything** —
    /// `self` and `base` are left exactly as they were, no rollback needed.
    ///
    /// This restriction is deliberately a strict "any edge" check rather than
    /// "reject only negative edges" or a transitive reachability search: a
    /// positive edge can chain into a negative one further downstream (rule A
    /// feeds existing rule B positively, B feeds existing rule C, C negates
    /// B's head — a one-hop, positive-only-tolerant check starting from A
    /// would miss the B→C hazard entirely), and rejecting on *any* direct
    /// edge sidesteps needing to search for that transitively: it forbids an
    /// existing rule from consuming this rule's output at all, so there is no
    /// downstream chain left to worry about. This is strictly more
    /// conservative than the true minimum rejection set — some programs that
    /// a full rebuild would happily stratify (by moving an existing rule to a
    /// later stratum) are rejected here instead — but the check stays a
    /// cheap, obviously-correct single pass over rule bodies (never
    /// proportional to `base`'s fact count) instead of a graph search or a
    /// stratum reshuffle. Callers that hit this can always fall back to a
    /// full [`Self::new`] rebuild over the combined rule set.
    ///
    /// **Why an existing rule may never change stratum:** [`Derivation::rule_id`]
    /// is an index into one specific program's `rules`; moving a rule to a
    /// different program would require either physically removing it from
    /// its old program (shifting every later index and corrupting *other*
    /// rules' recorded derivations — exactly the bug `DatalogProgram`'s
    /// `disabled_rules` field exists to avoid) or rebuilding that program from scratch (losing its
    /// `derived_from` index, forcing an O(base facts) re-materialisation —
    /// the very cost this method exists to avoid). So every existing rule's
    /// program assignment is permanently fixed once it is first materialised;
    /// this method's whole design follows from that constraint.
    ///
    /// **Correctness for the accepted class:** every fact newly present in
    /// `base` after a successful call is one that a from-scratch
    /// `IncrementalReasoner::new` over the combined rule set would also
    /// derive, and nothing that from-scratch construction would derive is
    /// missing. Reactivated rules are re-materialised via a full
    /// `materialise_seminaive_tracked` on their own program, then their
    /// output is fed forward through every later program in stratum order
    /// (mirroring [`Self::apply_insertions`]'s cross-stratum delta
    /// accumulation) so a later stratum can still legitimately consume a
    /// reactivated rule's output positively. Freshly-appended strata are
    /// materialised with `base` already containing the full existing
    /// extensional + intensional closure, i.e. "materialize forward from the
    /// new rules only, seeded against everything already derived".
    ///
    /// On any `Err(ReasoningError::Contradiction)` raised while
    /// materialising a reactivated or freshly-appended program, `base` and
    /// `self` are restored to exactly their pre-call state via a cheap
    /// undo-log rollback (mirroring every other `IncrementalReasoner`
    /// mutator's contract) — no [`Self::rebuild_from_base`] is required.
    ///
    /// Returns the number of new quads (derived, or re-derived by a
    /// reactivated rule) added to `base`, mirroring
    /// [`Self::apply_rule_deletions`]'s "count of facts affected" return
    /// shape. Returns `Ok(0)` for an empty `new_rules` slice or when every
    /// requested rule was already present and enabled.
    pub fn apply_rule_insertions(
        &mut self,
        base: &mut Datastore,
        new_rules: &[Rule],
    ) -> Result<usize, ReasoningError> {
        if new_rules.is_empty() {
            return Ok(0);
        }

        // --- Step 0: classify each requested rule ---
        let mut reactivate: Vec<(usize, usize)> = Vec::new();
        let mut fresh_set: HashSet<Rule> = HashSet::new();
        for r in new_rules {
            let mut found_enabled = false;
            let mut found_disabled: Option<(usize, usize)> = None;
            'search: for (pi, program) in self.programs.iter().enumerate() {
                for (rid, existing) in program.rules.iter().enumerate() {
                    if existing == r {
                        if program.is_rule_disabled(rid) {
                            found_disabled = Some((pi, rid));
                        } else {
                            found_enabled = true;
                        }
                        break 'search;
                    }
                }
            }
            if found_enabled {
                continue; // idempotent no-op: already active
            }
            if let Some(target) = found_disabled {
                if !reactivate.contains(&target) {
                    reactivate.push(target);
                }
                continue;
            }
            fresh_set.insert(r.clone());
        }
        let fresh: Vec<Rule> = fresh_set.into_iter().collect();

        if reactivate.is_empty() && fresh.is_empty() {
            return Ok(0);
        }

        // --- Step 1: conservative append-only stratifiability check ---
        // (No mutation has happened yet, so a rejection here needs no rollback.)

        // `existing_active`: every rule that either is currently enabled and
        // not itself a reactivation target, or is a reactivation target
        // (about to become enabled).
        let reactivate_set: HashSet<(usize, usize)> = reactivate.iter().copied().collect();
        let mut existing_active: Vec<Rule> = Vec::new();
        for (pi, program) in self.programs.iter().enumerate() {
            for (rid, r) in program.rules.iter().enumerate() {
                let will_be_active =
                    reactivate_set.contains(&(pi, rid)) || !program.is_rule_disabled(rid);
                if will_be_active {
                    existing_active.push(r.clone());
                }
            }
        }

        // Every rule being (re)introduced this call — fresh insertions and
        // reactivations alike — must not be depended on (positively or
        // negatively) by any other existing-active rule.
        let mut reintroduced: Vec<Rule> = fresh.clone();
        for &(pi, rid) in &reactivate {
            reintroduced.push(self.programs[pi].rules[rid].clone());
        }
        for rule in &reintroduced {
            if let RuleHead::NormalHead(ref head_pattern) = rule.head {
                let others: Vec<Rule> = existing_active
                    .iter()
                    .filter(|r| *r != rule)
                    .cloned()
                    .collect();
                let deps = crate::unification::depending_rules(&others, head_pattern);
                if let Some(edge) = deps.first() {
                    return Err(ReasoningError::NotStratifiable(format!(
                        "cannot incrementally insert/reactivate rule without a full rebuild \
                         (rule: {rule}): an existing, already-materialised rule already depends \
                         on its head predicate (positively or negatively): {}. This incremental \
                         path only accepts rules that are strictly downstream of everything \
                         already materialised — retry via a full IncrementalReasoner::new \
                         rebuild over the combined rule set instead.",
                        edge.get_rule()
                    )));
                }
            }
        }

        // Global stratifiability check (defense in depth): the combined
        // rule set (existing-active + fresh) must be stratifiable at all.
        // Given the check above already forbids any existing→fresh edge,
        // this is not expected to newly fail — but it is cheap
        // (rule-count-sized, not fact-count-sized) and validates the
        // append-only reasoning above against the same stratifier
        // `IncrementalReasoner::new` already trusts.
        let combined: Vec<Rule> = existing_active
            .iter()
            .cloned()
            .chain(fresh.iter().cloned())
            .collect();
        RulePartitioner::new(combined).order_rules()?;

        // Sub-stratify the fresh rules among themselves.
        let fresh_strata: Vec<Vec<Rule>> = if fresh.is_empty() {
            Vec::new()
        } else {
            RulePartitioner::new(fresh.clone()).order_rules()?
        };

        // --- Step 2: mutate, tracking everything for rollback ---

        // Build every fresh-stratum `DatalogProgram` BEFORE any mutation:
        // `DatalogProgram::new` can fail with `Err(UnsafeRule)`, and doing
        // this first keeps that a clean no-mutation `Err` like the
        // stratifiability rejections above.
        let mut fresh_programs: Vec<DatalogProgram> = Vec::with_capacity(fresh_strata.len());
        for stratum in fresh_strata {
            fresh_programs.push(DatalogProgram::new(stratum)?);
        }

        let quad_start = base.named_graphs.quad_count;
        let orig_program_count = self.programs.len();
        let mut reactivated_disabled: Vec<(usize, usize)> = Vec::new();
        let mut tracked: Vec<(usize, Vec<(Quad, Derivation)>)> = Vec::new();

        // Reactivate, then sweep the delta forward through every later
        // program — mirroring `apply_insertions`'s cross-stratum
        // accumulation — so a later stratum can still legitimately consume a
        // reactivated rule's output positively.
        for &(program_index, rule_id) in &reactivate {
            self.programs[program_index].enable_rule(rule_id);
            reactivated_disabled.push((program_index, rule_id));

            let mut buf = Vec::new();
            let result = self.programs[program_index].materialise_seminaive_tracked(base, &mut buf);
            let mut delta_facts: Vec<Quad> = buf.iter().map(|(q, _)| *q).collect();
            tracked.push((program_index, buf));
            if let Err(e) = result {
                self.undo_rule_insertions(
                    base,
                    quad_start,
                    orig_program_count,
                    &tracked,
                    &reactivated_disabled,
                );
                return Err(e);
            }

            for later_index in (program_index + 1)..self.programs.len() {
                if delta_facts.is_empty() {
                    break;
                }
                let mut later_buf = Vec::new();
                let result = self.programs[later_index].materialise_seminaive_tracked_from_facts(
                    base,
                    &mut later_buf,
                    &delta_facts,
                );
                delta_facts.extend(later_buf.iter().map(|(q, _)| *q));
                tracked.push((later_index, later_buf));
                if let Err(e) = result {
                    self.undo_rule_insertions(
                        base,
                        quad_start,
                        orig_program_count,
                        &tracked,
                        &reactivated_disabled,
                    );
                    return Err(e);
                }
            }
        }

        // Append and materialise the freshly-built strata, in order — `base`
        // already contains the full existing closure (including anything
        // the reactivation sweep above just added).
        for program in fresh_programs {
            self.programs.push(program);
            let new_index = self.programs.len() - 1;
            let mut buf = Vec::new();
            let result = self.programs[new_index].materialise_seminaive_tracked(base, &mut buf);
            tracked.push((new_index, buf));
            if let Err(e) = result {
                self.undo_rule_insertions(
                    base,
                    quad_start,
                    orig_program_count,
                    &tracked,
                    &reactivated_disabled,
                );
                return Err(e);
            }
        }

        Ok(base.named_graphs.quad_count - quad_start)
    }

    /// Undo exactly what [`Self::apply_rule_insertions`] changed during a
    /// call that failed partway through materialisation: unrecord every
    /// tracked `(Quad, Derivation)` entry, truncate `base` back to its
    /// pre-call quad count, drop every freshly-appended program (truncating
    /// `self.programs` back to its pre-call length), then re-disable every
    /// rule this call reactivated — in that order, mirroring
    /// [`Self::undo_rule_deletions`].
    fn undo_rule_insertions(
        &mut self,
        base: &mut Datastore,
        quad_start: usize,
        orig_program_count: usize,
        tracked: &[(usize, Vec<(Quad, Derivation)>)],
        reactivated_disabled: &[(usize, usize)],
    ) {
        for (program_index, buf) in tracked {
            for (q, d) in buf {
                self.programs[*program_index].derived_from.unrecord(q, d);
            }
        }
        base.named_graphs.truncate_to(quad_start);
        self.programs.truncate(orig_program_count);
        for &(program_index, rule_id) in reactivated_disabled {
            self.programs[program_index].disable_rule(rule_id);
        }
    }

    /// Apply a batch of base-fact insertions.
    ///
    /// Inserts the quads into the store and re-runs semi-naive evaluation so that
    /// only quads triggered by the new base facts produce new inferences.
    ///
    /// **Seeds semi-naive with a true delta**, not the whole store: each
    /// stratum's first iteration only matches rules against `inserts` (plus
    /// whatever an earlier stratum in this same call derived from them), via
    /// [`DatalogProgram::materialise_seminaive_tracked_from_facts`] — see
    /// that method's doc for why this doesn't need a separate "rotate which
    /// body atom is the delta" step, and [#534](https://github.com/daghovland/rdf-datalog/issues/534)
    /// for the before/after cost. The delta is passed as an **explicit fact
    /// list** rather than a `quad_list` position, specifically because some
    /// callers (`sparql_endpoint`) add `inserts` to `base` themselves before
    /// calling this method (for their no-reasoner-configured code path) — a
    /// position-based seed would then see `quad_start == quad_count` (the
    /// quads already appended) and silently derive nothing. `delta_facts`
    /// accumulates across the per-stratum loop below: each later stratum
    /// sees the original `inserts` *plus* every earlier stratum's
    /// newly-derived output as its own delta, not just its own predecessor's.
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
        //
        // `delta_facts` starts as `inserts` and accumulates each stratum's
        // own newly-derived output, so the next stratum's delta is "the
        // original inserts plus everything derived so far this call" — see
        // this method's doc comment for why the delta must be an explicit
        // fact list rather than a `quad_list` position here.
        let mut tracked: Vec<Vec<(Quad, Derivation)>> = Vec::with_capacity(self.programs.len());
        let mut delta_facts: Vec<Quad> = inserts.to_vec();
        for program in &mut self.programs {
            let mut buf = Vec::new();
            let result =
                program.materialise_seminaive_tracked_from_facts(base, &mut buf, &delta_facts);
            delta_facts.extend(buf.iter().map(|(q, _)| *q));
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
    ///
    /// Any rule previously retracted via [`Self::apply_rule_deletions`]
    /// **stays retracted** across a rebuild: this re-runs
    /// `materialise_seminaive`, which now respects each program's disabled-rule
    /// set, so a rebuild will not resurrect facts that only a retracted rule
    /// could produce. See [#162](https://github.com/daghovland/rdf-datalog/issues/162).
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
        let reverse = self.build_reverse_index();

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

    /// Build the reverse witness index shared by both backward-phase
    /// variants: `witness_quad → all derived quads that use it as a witness`.
    fn build_reverse_index(&self) -> HashMap<Quad, Vec<Quad>> {
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
        reverse
    }

    /// Backward phase for rule (TBox) retraction: seeded from every derived
    /// quad that has at least one [`Derivation`] whose `rule_id` matches one
    /// of `targets` (`(program_index, rule_id)` pairs), rather than from
    /// deleted base facts.
    ///
    /// Unlike [`Self::backward_phase`] (where the deleted base facts
    /// themselves are never added to PD — they're not derived quads at all),
    /// here the seed quads *are* derived facts and belong in PD directly:
    /// they were produced by a rule that no longer exists. From there, the
    /// same BFS propagates through the reverse witness index to catch
    /// anything transitively depending on those facts. See
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    ///
    /// **A quad that is currently extensional (EDB) in `base` is never added
    /// to PD, and the BFS never propagates through it** — even if it also
    /// happens to have a stale derivation record naming a `targets` rule
    /// (this happens whenever two rules mutually re-derive each other's
    /// base facts, e.g. `EquivalentClasses(A, B)`'s two directional rules
    /// each re-deriving the other's asserted instance). An extensional fact
    /// is unconditionally true regardless of its derivation history, so
    /// including it in PD would make [`Self::forward_phase_rules`] delete a
    /// real base fact from the store — permanently, since nothing
    /// re-derives a plain asserted fact. Found alongside the
    /// [`dag_rdf::QuadTable::add_intensional_quad`] EDB-downgrade bug this
    /// retraction path also fixes; see
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    fn backward_phase_from_rule_removal(
        &self,
        base: &Datastore,
        targets: &[(usize, usize)],
    ) -> HashSet<Quad> {
        let reverse = self.build_reverse_index();

        let mut pd: HashSet<Quad> = HashSet::new();
        let mut worklist: VecDeque<Quad> = VecDeque::new();
        for &(program_index, rule_id) in targets {
            let program = &self.programs[program_index];
            for (derived, derivations) in program.derived_from.iter() {
                if derivations.iter().any(|d| d.rule_id == rule_id)
                    && !base.named_graphs.is_extensional(derived)
                    && pd.insert(*derived)
                {
                    worklist.push_back(*derived);
                }
            }
        }
        while let Some(q) = worklist.pop_front() {
            if let Some(dependents) = reverse.get(&q) {
                for &derived in dependents {
                    if !base.named_graphs.is_extensional(&derived) && pd.insert(derived) {
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
        #[cfg(test)]
        {
            self.fallback_count += 1;
        }
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

    /// Forward phase for [`Self::apply_rule_deletions`]: mirrors
    /// [`Self::forward_phase`] (remove PD from the closure, then re-run
    /// semi-naive to re-derive anything still provable), with one addition —
    /// on a `Contradiction` during re-derivation, rollback must also
    /// re-enable the rules this call disabled (fact deletion never touches
    /// `disabled_rules`, so [`Self::undo_deletions`] doesn't need to).
    fn forward_phase_rules(
        &mut self,
        base: &mut Datastore,
        pd: HashSet<Quad>,
        targets: &[(usize, usize)],
    ) -> Result<usize, ReasoningError> {
        let removed = pd.len();
        let mut removed_derivations: Vec<(Quad, usize, Vec<Derivation>)> = Vec::new();
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

        let redelta_start = base.named_graphs.quad_count;

        let mut tracked: Vec<Vec<(Quad, Derivation)>> = Vec::with_capacity(self.programs.len());
        for program in &mut self.programs {
            let mut buf = Vec::new();
            let result = program.materialise_seminaive_tracked(base, &mut buf);
            tracked.push(buf);
            if let Err(e) = result {
                self.undo_rule_deletions(
                    base,
                    redelta_start,
                    &tracked,
                    &pd,
                    targets,
                    &removed_derivations,
                );
                return Err(e);
            }
        }
        Ok(removed)
    }

    /// Undo exactly what [`Self::forward_phase_rules`] changed during a call
    /// that failed partway through re-derivation — same shape as
    /// [`Self::undo_deletions`], plus re-enabling every rule `targets`
    /// disabled so the reasoner's rule set is restored to its exact pre-call
    /// state, not just its derived closure.
    fn undo_rule_deletions(
        &mut self,
        base: &mut Datastore,
        redelta_start: usize,
        tracked: &[Vec<(Quad, Derivation)>],
        pd: &HashSet<Quad>,
        targets: &[(usize, usize)],
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
        for (q, program_index, derivations) in removed_derivations {
            for d in derivations {
                self.programs[*program_index]
                    .derived_from
                    .record(*q, d.clone());
            }
        }
        for &(program_index, rule_id) in targets {
            self.programs[program_index].enable_rule(rule_id);
        }
    }

    /// Full re-materialisation fallback for [`Self::apply_rule_deletions`]
    /// when PD is large relative to the closure — mirrors
    /// [`Self::full_rematerialise`]: disables the targeted rules (index-stable,
    /// consistent with the fast-path representation, so a subsequent call
    /// sees the same "which rules are live" state regardless of which path
    /// was taken), tears down the derived closure, and rebuilds from scratch
    /// over the surviving base facts with the disabled rules excluded.
    ///
    /// Snapshots `base.named_graphs` and every program's `derived_from`
    /// before touching anything, and — on a genuine `Contradiction` raised
    /// while re-materialising — restores both snapshots and re-enables
    /// every rule `targets` disabled, so the exact-pre-call-state rollback
    /// contract [`Self::apply_rule_deletions`]'s doc comment promises holds
    /// on *both* the incremental and the fallback path, not just the
    /// former. Without this, a rule disabled here right before a
    /// contradiction was found would stay disabled forever (the caller sees
    /// `Err` and has no way to know a rule was silently and permanently
    /// dropped) — found while adding rollback test coverage for
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    ///
    /// This is intentionally *stronger* than [`Self::full_rematerialise`]
    /// (the plain-fact-deletion fallback), whose own doc — and
    /// [`Self::apply_deletions`]'s — explicitly documents leaving `base`
    /// and `self` in a partially-rebuilt state on error, requiring the
    /// caller to call [`Self::rebuild_from_base`]: that path never disables
    /// anything irreversible, so a caller re-deriving from the (still
    /// intact) base facts is a safe enough recovery. A disabled rule has no
    /// such recovery route, so this path pays for an explicit snapshot
    /// instead.
    fn full_rematerialise_rules(
        &mut self,
        base: &mut Datastore,
        targets: &[(usize, usize)],
    ) -> Result<usize, ReasoningError> {
        #[cfg(test)]
        {
            self.fallback_count += 1;
        }

        let named_graphs_before = base.named_graphs.clone();
        let derived_from_before: Vec<DerivedFromIndex> = self
            .programs
            .iter()
            .map(|p| p.derived_from.clone())
            .collect();

        for &(program_index, rule_id) in targets {
            self.programs[program_index].disable_rule(rule_id);
        }

        let base_facts: Vec<Quad> = base.named_graphs.extensional_quads().collect();
        let hint = base_facts.len() as u32;

        base.named_graphs = QuadTable::new(hint);
        for q in base_facts {
            base.named_graphs.add_quad(q);
        }

        let before = base.named_graphs.quad_count;
        for program in &mut self.programs {
            program.derived_from = Default::default();
            if let Err(e) = program.materialise_seminaive(base) {
                base.named_graphs = named_graphs_before;
                for (program, saved) in self.programs.iter_mut().zip(derived_from_before) {
                    program.derived_from = saved;
                }
                for &(program_index, rule_id) in targets {
                    self.programs[program_index].enable_rule(rule_id);
                }
                return Err(e);
            }
        }
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

    /// Regression test for [#540](https://github.com/daghovland/rdf-datalog/issues/540):
    /// deleting a base fact must cascade through a derived-depends-on-derived
    /// chain of depth ≥ 3, not just a single hop.
    ///
    /// Setup: a single base fact `p(a,b)` and a strictly linear chain of
    /// single-body-atom rules `p2 :- p`, `p3 :- p2`, `p4 :- p3`. Each derived
    /// quad has exactly one witness (its immediate predecessor in the chain),
    /// so the reverse witness index cannot short-circuit through a
    /// multi-witness derivation the way transitivity's rule can — the only
    /// way `backward_phase` can reach `p4(a,b)` is by walking
    /// `p(a,b) → p2(a,b) → p3(a,b) → p4(a,b)`, three hops deep.
    ///
    /// Deleting `p(a,b)` must retract the entire chain.
    #[test]
    fn test_delete_base_fact_cascades_deep_chain() {
        let (mut ds, g, a, _p, b, _c) = setup_store();
        let p = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/chain_p".to_string(),
            )));
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/chain_p2".to_string(),
            )));
        let p3 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/chain_p3".to_string(),
            )));
        let p4 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/chain_p4".to_string(),
            )));

        let fact_p = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_p);

        // Single-body-atom "chain link" rule: { ?x from ?y } => { ?x to ?y }
        let chain_rule = |from: u32, to: u32| Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(to),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(from),
                object: Term::Variable("y".to_string()),
            })],
        };

        let mut reasoner = IncrementalReasoner::new(
            vec![chain_rule(p, p2), chain_rule(p2, p3), chain_rule(p3, p4)],
            &mut ds,
        )
        .unwrap();

        let fact_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: b,
        };
        let fact_p3 = Quad {
            triple_id: g,
            subject: a,
            predicate: p3,
            obj: b,
        };
        let fact_p4 = Quad {
            triple_id: g,
            subject: a,
            predicate: p4,
            obj: b,
        };
        assert!(ds.named_graphs.contains(&fact_p2), "p2 should be derived");
        assert!(ds.named_graphs.contains(&fact_p3), "p3 should be derived");
        assert!(ds.named_graphs.contains(&fact_p4), "p4 should be derived");

        // Delete the root base fact: the whole chain should cascade away.
        reasoner.apply_deletions(&mut ds, &[fact_p]).unwrap();

        assert!(
            !ds.named_graphs.contains(&fact_p),
            "deleted base fact p should be gone"
        );
        assert!(
            !ds.named_graphs.contains(&fact_p2),
            "p2 should be retracted (depth-1 cascade)"
        );
        assert!(
            !ds.named_graphs.contains(&fact_p3),
            "p3 should be retracted (depth-2 cascade)"
        );
        assert!(
            !ds.named_graphs.contains(&fact_p4),
            "p4 should be retracted (depth-3 cascade)"
        );
    }

    /// Regression test for [#540](https://github.com/daghovland/rdf-datalog/issues/540):
    /// a "deep diamond" — two derivation paths of *different* depths converging
    /// on the same fact. Deleting the support for the *shorter* path must not
    /// remove the fact, because `forward_phase`'s full re-derivation still
    /// finds it via the surviving longer path.
    ///
    /// Setup: `final(a,b)` is derivable two ways:
    /// - short path (depth 1): `short(a,b)` directly implies `final(a,b)`.
    /// - long path (depth 4): `longbase(a,b) → long1 → long2 → long3 → final`.
    ///
    /// Deleting `short(a,b)` puts `final(a,b)` in PD (it has a derivation
    /// witnessing off `short(a,b)`), but the long chain's intermediate facts
    /// are untouched, so `forward_phase` re-derives `final(a,b)` via
    /// `long3(a,b)`.
    #[test]
    fn test_delete_base_fact_keeps_deep_diamond_via_longer_path() {
        let (mut ds, g, a, _p, b, _c) = setup_store();
        let short = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/short".to_string(),
            )));
        let longbase = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/longbase".to_string(),
            )));
        let long1 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/long1".to_string(),
            )));
        let long2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/long2".to_string(),
            )));
        let long3 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/long3".to_string(),
            )));
        let final_pred = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/final".to_string(),
            )));

        let fact_short = Quad {
            triple_id: g,
            subject: a,
            predicate: short,
            obj: b,
        };
        let fact_longbase = Quad {
            triple_id: g,
            subject: a,
            predicate: longbase,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_short);
        ds.named_graphs.add_quad(fact_longbase);

        // Single-body-atom "chain link" rule: { ?x from ?y } => { ?x to ?y }
        let chain_rule = |from: u32, to: u32| Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(to),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(from),
                object: Term::Variable("y".to_string()),
            })],
        };

        let mut reasoner = IncrementalReasoner::new(
            vec![
                chain_rule(short, final_pred), // depth-1 path
                chain_rule(longbase, long1),   // depth-4 path, link 1
                chain_rule(long1, long2),      // link 2
                chain_rule(long2, long3),      // link 3
                chain_rule(long3, final_pred), // link 4: converges on `final`
            ],
            &mut ds,
        )
        .unwrap();

        let fact_final = Quad {
            triple_id: g,
            subject: a,
            predicate: final_pred,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&fact_final),
            "final should be derived before deletion"
        );

        // Delete the short path's only support: the long path should keep `final` alive.
        reasoner.apply_deletions(&mut ds, &[fact_short]).unwrap();

        assert!(
            !ds.named_graphs.contains(&fact_short),
            "deleted base fact short should be gone"
        );
        assert!(
            ds.named_graphs.contains(&fact_final),
            "final should survive: still derivable via the longer surviving path"
        );
        // The long chain's intermediate facts are untouched by this deletion.
        let fact_long3 = Quad {
            triple_id: g,
            subject: a,
            predicate: long3,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&fact_long3),
            "long3 (unrelated to the deleted short path) should remain"
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

    // ── TBox (rule/axiom) retraction — #162 ─────────────────────────────────

    /// Removing the only rule that derived a fact must remove that fact,
    /// via the incremental BF path (not the `FALLBACK_THRESHOLD` fallback —
    /// exercised separately below).
    ///
    /// Setup: A→B→C with transitivity rule (derives A→C). Also present: an
    /// unrelated alias rule (p2→p) with several independent facts, purely to
    /// inflate the total derived-fact count so removing the one
    /// transitivity-derived fact stays comfortably under the 25%
    /// `FALLBACK_THRESHOLD` and exercises the incremental path — asserted via
    /// `fallback_count` staying unchanged. Removing the transitivity rule
    /// must remove A→C, but leave the base facts and the unrelated alias
    /// closure untouched.
    #[test]
    fn test_remove_rule_removes_uniquely_derived_facts() {
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
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);

        // Four independent alias-rule facts, unrelated to the transitivity
        // chain above, to inflate the closure's total derived-fact count.
        let mut alias_sources = Vec::new();
        for i in 0..4 {
            let s = ds
                .resources
                .add_node_resource(RdfResource::Iri(IriReference(format!(
                    "http://example.org/alias_s{i}"
                ))));
            let o = ds
                .resources
                .add_node_resource(RdfResource::Iri(IriReference(format!(
                    "http://example.org/alias_o{i}"
                ))));
            ds.named_graphs.add_quad(Quad {
                triple_id: g,
                subject: s,
                predicate: p2,
                obj: o,
            });
            alias_sources.push((s, o));
        }

        let transitivity = transitivity_rule(g, p);
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
            IncrementalReasoner::new(vec![transitivity.clone(), alias_rule], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived before rule removal"
        );
        for &(s, o) in &alias_sources {
            assert!(ds.named_graphs.contains(&Quad {
                triple_id: g,
                subject: s,
                predicate: p,
                obj: o,
            }));
        }

        let fallback_count_before = reasoner.fallback_count;
        let removed = reasoner
            .apply_rule_deletions(&mut ds, &[transitivity])
            .expect("removing the transitivity rule must not error");
        assert_eq!(
            removed, 1,
            "exactly the one derived fact A→C should be removed"
        );
        assert_eq!(
            reasoner.fallback_count, fallback_count_before,
            "removing 1 of 5 derived facts (20%) must stay under FALLBACK_THRESHOLD"
        );

        assert!(
            !ds.named_graphs.contains(&derived_ac),
            "A→C should be gone: its only rule was retracted"
        );
        assert!(
            ds.named_graphs.contains(&fact_ab),
            "base fact A→B must survive rule retraction"
        );
        assert!(
            ds.named_graphs.contains(&fact_bc),
            "base fact B→C must survive rule retraction"
        );
        for &(s, o) in &alias_sources {
            assert!(
                ds.named_graphs.contains(&Quad {
                    triple_id: g,
                    subject: s,
                    predicate: p,
                    obj: o,
                }),
                "unrelated alias-derived facts must survive transitivity-rule retraction"
            );
        }

        // The rule must actually be inert now: inserting a fresh base fact
        // that would have triggered it produces no new transitive closure.
        let d = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: d,
        };
        reasoner.apply_insertions(&mut ds, &[fact_cd]).unwrap();
        let derived_bd = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: d,
        };
        assert!(
            !ds.named_graphs.contains(&derived_bd),
            "retracted transitivity rule must not fire again after later insertions"
        );
    }

    /// A fact derivable via two independent rules survives removal of one of
    /// them — mirrors `test_delete_base_fact_keeps_multiply_derived` at the
    /// rule level.
    ///
    /// Setup: A→B, B→C (derive A→C via transitivity), plus A→p2→C and an
    /// alias rule p2→p. Removing the transitivity rule leaves A→C intact,
    /// still derivable via the alias rule.
    #[test]
    fn test_remove_rule_keeps_multiply_derived_fact() {
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
        let fact_ac_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);
        ds.named_graphs.add_quad(fact_ac_p2);

        let transitivity = transitivity_rule(g, p);
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
            IncrementalReasoner::new(vec![transitivity.clone(), alias_rule], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived before rule removal"
        );

        reasoner
            .apply_rule_deletions(&mut ds, &[transitivity])
            .expect("removing the transitivity rule must not error");

        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should survive: still derivable via A→p2→C + the surviving alias rule"
        );
    }

    /// Removing a `Rule` value that was never part of the program is a no-op.
    #[test]
    fn test_remove_rule_no_op_for_unknown_rule() {
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

        let rule = transitivity_rule(g, p);
        let mut reasoner = IncrementalReasoner::new(vec![rule], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(ds.named_graphs.contains(&derived_ac));

        // A rule that was never registered with this reasoner.
        let unrelated_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            })],
        };

        let removed = reasoner
            .apply_rule_deletions(&mut ds, &[unrelated_rule])
            .expect("removing an unknown rule must not error");
        assert_eq!(removed, 0, "no rule matched, so nothing should be removed");
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "closure must be untouched by a no-op rule removal"
        );
    }

    /// Pins the `apply_rule_deletions` precondition: `owl2datalog`-style
    /// deduplication means one compiled `Rule` can be shared by two distinct
    /// source axioms. If a caller correctly computes "genuinely dead rules"
    /// (the rule is still produced by a surviving axiom, so nothing is
    /// genuinely dead), the call must be a no-op and the closure must survive
    /// untouched — mirrors the issue's "or another axiom implying the same
    /// rule" clause.
    #[test]
    fn test_remove_rule_shared_by_two_axioms_survives_if_one_remains() {
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

        // Two distinct "axioms" that happen to compile to the identical
        // `Rule` value (this is exactly what `owl2datalog`'s dedup produces
        // for structurally-identical rules from different axioms) — modelled
        // here directly as a single shared `Rule`, since `Rule` carries no
        // axiom-identity information once compiled.
        let shared_rule = transitivity_rule(g, p);
        let mut reasoner = IncrementalReasoner::new(vec![shared_rule.clone()], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(ds.named_graphs.contains(&derived_ac));

        // Caller correctly computed the "genuinely dead" set as empty
        // (the rule is still justified by the other, surviving axiom) —
        // so it calls apply_rule_deletions with an empty slice.
        let removed = reasoner
            .apply_rule_deletions(&mut ds, &[])
            .expect("empty rule-deletion batch must not error");
        assert_eq!(removed, 0);
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "closure must be fully untouched: the rule is still justified by a surviving axiom"
        );

        // Sanity: the rule is genuinely still live (not disabled) — a
        // fresh insertion still triggers it.
        let d = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: d,
        };
        reasoner.apply_insertions(&mut ds, &[fact_cd]).unwrap();
        let derived_ad = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: d,
        };
        assert!(
            ds.named_graphs.contains(&derived_ad),
            "shared rule must still be active since it was never actually retracted"
        );
    }

    /// A large rule removal (crossing `FALLBACK_THRESHOLD`) falls back to
    /// full re-materialisation rather than the incremental BF path.
    ///
    /// Setup: a chain A→B→C→D→E with transitivity, so removing the
    /// transitivity rule wipes out every pairwise-derived fact (well over
    /// 25% of the closure, since ALL derived facts come from this one rule).
    /// Verifies both the resulting closure (correct) and that the fallback
    /// path actually ran (`fallback_count`), not the incremental path —
    /// `materialise_call_count()` cannot distinguish the two paths (both call
    /// materialisation exactly once per program), so a dedicated counter is
    /// used instead.
    #[test]
    fn test_remove_rule_large_removal_falls_back_to_full_rematerialise() {
        let (mut ds, g, a, p, b, c) = setup_store();
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

        for (s, o) in [(a, b), (b, c), (c, d), (d, e)] {
            ds.named_graphs.add_quad(Quad {
                triple_id: g,
                subject: s,
                predicate: p,
                obj: o,
            });
        }

        let rule = transitivity_rule(g, p);
        let mut reasoner = IncrementalReasoner::new(vec![rule.clone()], &mut ds).unwrap();

        // Every derived pairwise fact comes from this one rule, so PD == 100%
        // of the derived closure — comfortably over FALLBACK_THRESHOLD.
        //
        // Note: `full_rematerialise_rules` (like the pre-existing
        // `full_rematerialise` it mirrors) reports *newly derived facts
        // added* by the rebuild, not "facts removed" — since the sole rule
        // behind this closure is now disabled, the rebuild derives nothing
        // new, so `removed` is legitimately 0 here. The real correctness
        // check is the store contents asserted below, not this count.
        let fallback_count_before = reasoner.fallback_count;
        let removed = reasoner
            .apply_rule_deletions(&mut ds, &[rule])
            .expect("large rule removal must not error");
        assert_eq!(
            removed, 0,
            "the fallback rebuild derives nothing new once the sole rule is disabled"
        );

        assert_eq!(
            reasoner.fallback_count,
            fallback_count_before + 1,
            "removing the sole rule behind the entire closure must trip the fallback path"
        );

        // Correctness: base facts survive, every transitively-derived fact
        // is gone (no rule left to justify any of them).
        for (s, o) in [(a, b), (b, c), (c, d), (d, e)] {
            assert!(ds.named_graphs.contains(&Quad {
                triple_id: g,
                subject: s,
                predicate: p,
                obj: o,
            }));
        }
        for (s, o) in [(a, c), (a, d), (a, e), (b, d), (b, e), (c, e)] {
            assert!(
                !ds.named_graphs.contains(&Quad {
                    triple_id: g,
                    subject: s,
                    predicate: p,
                    obj: o,
                }),
                "transitively-derived fact {s:?}→{o:?} must be gone: its only rule was retracted"
            );
        }
    }

    /// `apply_rule_deletions` must roll back a re-derivation-triggered
    /// contradiction — including **re-enabling** every rule the failed call
    /// disabled — on *both* of its internal paths, not just the cheap
    /// incremental one. This test exercises the **fallback**
    /// (`full_rematerialise_rules`, tripped when PD is a large fraction of
    /// the closure) path specifically; see
    /// `test_apply_rule_deletions_contradiction_rollback_reenables_rule_incremental_path`
    /// for the same property on the incremental (`forward_phase_rules`) path.
    ///
    /// This test caught a genuine bug while it was being written: prior to
    /// the fix, `full_rematerialise_rules` propagated a `Contradiction` via
    /// a bare `?` with no rollback at all — the disabled rule stayed
    /// disabled forever, and `base`'s derived facts stayed wiped — despite
    /// `apply_rule_deletions`'s doc comment explicitly promising "restored
    /// to their exact pre-call state (including re-enabling any rule this
    /// call disabled)" as its `Err` contract. `full_rematerialise_rules` now
    /// snapshots `base.named_graphs` and every program's `derived_from`
    /// before mutating either, and restores both plus re-enables the
    /// targeted rules on error. See
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    ///
    /// Setup mirrors `test_apply_deletions_contradiction_rollback_uses_undo_log`,
    /// except the trigger is retracting the *rule* that derives `p3`, not
    /// deleting a base `p3` fact directly:
    /// - Stratum 1: `copy_rule`, `?x p3 ?y :- ?x p ?y`.
    /// - Stratum 2 (negatively depends on p3):
    ///   `Contradiction :- ?x p2 ?y, NOT ?x p3 ?y`.
    /// - Base facts: `A p B` and `C p D` (both derive their `p3` counterpart
    ///   via `copy_rule`), plus `C p2 D` — consistent initially because
    ///   `copy_rule` derives `C p3 D`, satisfying the `NOT` guard.
    ///
    /// `copy_rule` is the only rule in the program, so retracting it makes
    /// PD == 100% of the derived closure — comfortably over
    /// `FALLBACK_THRESHOLD` — confirmed below via `fallback_count`.
    /// Retracting `copy_rule` removes both `A p3 B` and `C p3 D`, then
    /// re-materialisation finds `C p2 D` with no surviving `C p3 D` and
    /// raises `Contradiction`. The call must report `Err`, and `copy_rule`
    /// must be re-enabled by the rollback, exactly as if the call had never
    /// happened.
    #[test]
    fn test_apply_rule_deletions_contradiction_rollback_reenables_rule_fallback_path() {
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

        // Base facts: A p B and C p D (both derive their p3 counterpart via
        // copy_rule), plus C p2 D (consistent initially: C p3 D is derived).
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: d,
        };
        let fact_cd_p2 = Quad {
            triple_id: g,
            subject: c,
            predicate: p2,
            obj: d,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_cd);
        ds.named_graphs.add_quad(fact_cd_p2);

        let mut reasoner =
            IncrementalReasoner::new(vec![copy_rule.clone(), contradiction_rule], &mut ds).unwrap();

        let derived_ab3 = Quad {
            triple_id: g,
            subject: a,
            predicate: p3,
            obj: b,
        };
        let derived_cd3 = Quad {
            triple_id: g,
            subject: c,
            predicate: p3,
            obj: d,
        };
        assert!(
            ds.named_graphs.contains(&derived_ab3),
            "stratum 1 should have derived A p3 B at initial materialisation"
        );
        assert!(
            ds.named_graphs.contains(&derived_cd3),
            "stratum 1 should have derived C p3 D at initial materialisation"
        );

        // Locate copy_rule's (program_index, rule_id) to check its disabled
        // state directly, the same way apply_rule_deletions itself does.
        let (program_index, rule_id) = reasoner
            .programs
            .iter()
            .enumerate()
            .find_map(|(pi, program)| {
                program
                    .rules
                    .iter()
                    .position(|r| *r == copy_rule)
                    .map(|ri| (pi, ri))
            })
            .expect("copy_rule must be present in some program");
        assert!(
            !reasoner.programs[program_index].is_rule_disabled(rule_id),
            "copy_rule must be enabled before the call under test"
        );

        let quads_before = ds.named_graphs.quad_list.clone();
        let extensional_before: HashMap<Quad, bool> = quads_before
            .iter()
            .map(|q| (*q, ds.named_graphs.is_extensional(q)))
            .collect();
        let fallback_count_before = reasoner.fallback_count;

        let result = reasoner.apply_rule_deletions(&mut ds, std::slice::from_ref(&copy_rule));
        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );
        assert_eq!(
            reasoner.fallback_count,
            fallback_count_before + 1,
            "PD == 100% of the closure must trip the fallback path, not the incremental one"
        );

        // (a) Exact rollback: same quads, same extensional/intensional status.
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
            ds.named_graphs.contains(&derived_ab3),
            "A p3 B must be restored after rollback"
        );
        assert!(
            ds.named_graphs.contains(&derived_cd3),
            "C p3 D must be restored after rollback"
        );

        // (b) The key correctness property this test exists for: the rule
        // disabled mid-call must be re-enabled by the rollback, not left
        // permanently disabled.
        assert!(
            !reasoner.programs[program_index].is_rule_disabled(rule_id),
            "copy_rule must be re-enabled after the rollback — a failed call must not \
             permanently disable a rule it only tentatively touched"
        );

        // Behavioural confirmation: the rule is genuinely live again, not
        // just reporting `false` from a stale bookkeeping bit — a fresh
        // insertion still triggers it.
        let e = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/e".to_string(),
            )));
        let fact_ef = Quad {
            triple_id: g,
            subject: e,
            predicate: p,
            obj: c,
        };
        reasoner
            .apply_insertions(&mut ds, &[fact_ef])
            .expect("reasoner must remain usable after the rollback");
        let derived_ef3 = Quad {
            triple_id: g,
            subject: e,
            predicate: p3,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ef3),
            "copy_rule must actually still fire on new insertions after being re-enabled"
        );
    }

    /// Same property as
    /// `test_apply_rule_deletions_contradiction_rollback_reenables_rule_fallback_path`
    /// — a rule disabled mid-call is re-enabled on `Contradiction` rollback
    /// — but forced down the **incremental** (`forward_phase_rules`/
    /// `undo_rule_deletions`) path instead of the fallback, confirmed via
    /// `fallback_count` staying unchanged. Padded with several unrelated
    /// alias-derived facts (same technique as
    /// `test_remove_rule_removes_uniquely_derived_facts`) so PD (the 2
    /// `copy_rule`-derived facts) stays under `FALLBACK_THRESHOLD` relative
    /// to the padded closure.
    #[test]
    fn test_apply_rule_deletions_contradiction_rollback_reenables_rule_incremental_path() {
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
        let p4 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p4".to_string(),
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
        // Unrelated stratum-1 alias rule, purely to inflate the total
        // derived-fact count so PD/total stays under FALLBACK_THRESHOLD.
        let alias_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p3),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p4),
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

        // Base facts: A p B and C p D (both derive their p3 counterpart via
        // copy_rule), plus C p2 D (consistent initially: C p3 D is derived).
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: d,
        };
        let fact_cd_p2 = Quad {
            triple_id: g,
            subject: c,
            predicate: p2,
            obj: d,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_cd);
        ds.named_graphs.add_quad(fact_cd_p2);

        // Eight independent alias-rule facts, unrelated to copy_rule's
        // derivations, so PD (2 facts) stays comfortably under 25% of the
        // padded total (10 facts).
        let mut alias_sources = Vec::new();
        for i in 0..8 {
            let s = ds
                .resources
                .add_node_resource(RdfResource::Iri(IriReference(format!(
                    "http://example.org/alias_s{i}"
                ))));
            let o = ds
                .resources
                .add_node_resource(RdfResource::Iri(IriReference(format!(
                    "http://example.org/alias_o{i}"
                ))));
            ds.named_graphs.add_quad(Quad {
                triple_id: g,
                subject: s,
                predicate: p4,
                obj: o,
            });
            alias_sources.push((s, o));
        }

        let mut reasoner = IncrementalReasoner::new(
            vec![copy_rule.clone(), alias_rule, contradiction_rule],
            &mut ds,
        )
        .unwrap();

        let derived_ab3 = Quad {
            triple_id: g,
            subject: a,
            predicate: p3,
            obj: b,
        };
        let derived_cd3 = Quad {
            triple_id: g,
            subject: c,
            predicate: p3,
            obj: d,
        };
        assert!(ds.named_graphs.contains(&derived_ab3));
        assert!(ds.named_graphs.contains(&derived_cd3));
        for &(s, o) in &alias_sources {
            assert!(ds.named_graphs.contains(&Quad {
                triple_id: g,
                subject: s,
                predicate: p3,
                obj: o,
            }));
        }

        let (program_index, rule_id) = reasoner
            .programs
            .iter()
            .enumerate()
            .find_map(|(pi, program)| {
                program
                    .rules
                    .iter()
                    .position(|r| *r == copy_rule)
                    .map(|ri| (pi, ri))
            })
            .expect("copy_rule must be present in some program");
        assert!(!reasoner.programs[program_index].is_rule_disabled(rule_id));

        let quads_before = ds.named_graphs.quad_list.clone();
        let fallback_count_before = reasoner.fallback_count;

        let result = reasoner.apply_rule_deletions(&mut ds, std::slice::from_ref(&copy_rule));
        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );
        assert_eq!(
            reasoner.fallback_count, fallback_count_before,
            "PD (2/10 = 20%) must stay under FALLBACK_THRESHOLD and take the incremental path"
        );

        // Exact rollback.
        let quads_before_set: HashSet<Quad> = quads_before.iter().copied().collect();
        let quads_after_set: HashSet<Quad> = ds.named_graphs.quad_list.iter().copied().collect();
        assert_eq!(
            quads_after_set, quads_before_set,
            "store must contain exactly the same quads as before the call"
        );
        assert!(ds.named_graphs.contains(&derived_ab3));
        assert!(ds.named_graphs.contains(&derived_cd3));
        for &(s, o) in &alias_sources {
            assert!(
                ds.named_graphs.contains(&Quad {
                    triple_id: g,
                    subject: s,
                    predicate: p3,
                    obj: o,
                }),
                "unrelated alias-derived facts must be untouched by the rollback"
            );
        }

        // The key correctness property: the rule disabled mid-call must be
        // re-enabled by the rollback.
        assert!(
            !reasoner.programs[program_index].is_rule_disabled(rule_id),
            "copy_rule must be re-enabled after the rollback"
        );
    }

    /// Regression test for two bugs found while adding
    /// `apply_rule_deletions` end-to-end coverage for `EquivalentClasses`
    /// (a positive two-rule cycle: `?x cB ?y :- ?x cA ?y` and its reverse):
    ///
    /// 1. [`dag_rdf::QuadTable::add_intensional_quad`]/`mark_intensional`
    ///    used to unconditionally mark a quad intensional, even when it was
    ///    already present as an extensional (asserted) fact — so once
    ///    `rule_atob` re-derived `y type B` (already a base fact) and
    ///    `rule_btoa` re-derived `x type A` (also already a base fact),
    ///    both base facts permanently lost their EDB status. The next
    ///    `full_rematerialise_rules` rebuild then used
    ///    `extensional_quads()` to reseed the store and silently dropped
    ///    them.
    /// 2. `backward_phase_from_rule_removal` didn't check `is_extensional`
    ///    before adding a quad to PD, so even with (1) fixed, a still-live
    ///    base fact with a *stale* derivation record naming the retracted
    ///    rule would be swept into PD and deleted outright by
    ///    `forward_phase_rules`'s `base.named_graphs.remove_quad` sweep —
    ///    with nothing to re-derive it afterwards, since it was never a
    ///    rule consequence to begin with.
    ///
    /// See [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    #[test]
    fn test_retracting_one_rule_of_a_two_rule_cycle_preserves_base_facts() {
        let (mut ds, g, _a, _p, _b, _c) = setup_store();
        let class_a = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/A".to_string(),
            )));
        let class_b = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/B".to_string(),
            )));
        let rdf_type = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
            )));
        let x = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/x".to_string(),
            )));
        let y = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/y".to_string(),
            )));

        // rule_atob: ?X type B :- ?X type A
        let rule_atob = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("X".to_string()),
                predicate: Term::Resource(rdf_type),
                object: Term::Resource(class_b),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("X".to_string()),
                predicate: Term::Resource(rdf_type),
                object: Term::Resource(class_a),
            })],
        };
        // rule_btoa: ?X type A :- ?X type B
        let rule_btoa = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("X".to_string()),
                predicate: Term::Resource(rdf_type),
                object: Term::Resource(class_a),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("X".to_string()),
                predicate: Term::Resource(rdf_type),
                object: Term::Resource(class_b),
            })],
        };

        let fact_x_a = Quad {
            triple_id: g,
            subject: x,
            predicate: rdf_type,
            obj: class_a,
        };
        let fact_y_b = Quad {
            triple_id: g,
            subject: y,
            predicate: rdf_type,
            obj: class_b,
        };
        ds.named_graphs.add_quad(fact_x_a);
        ds.named_graphs.add_quad(fact_y_b);

        let mut reasoner =
            IncrementalReasoner::new(vec![rule_atob.clone(), rule_btoa.clone()], &mut ds).unwrap();

        let derived_x_b = Quad {
            triple_id: g,
            subject: x,
            predicate: rdf_type,
            obj: class_b,
        };
        let derived_y_a = Quad {
            triple_id: g,
            subject: y,
            predicate: rdf_type,
            obj: class_a,
        };
        assert!(ds.named_graphs.contains(&derived_x_b));
        assert!(ds.named_graphs.contains(&derived_y_a));

        reasoner
            .apply_rule_deletions(&mut ds, &[rule_btoa])
            .unwrap();

        assert!(
            ds.named_graphs.contains(&derived_x_b),
            "x type B must survive: justified independently by rule_atob + base fact x type A"
        );
        assert!(
            !ds.named_graphs.contains(&derived_y_a),
            "y type A must be gone: only justified by the retracted rule_btoa"
        );
        // The two original base facts must survive as base facts, not just
        // survive as *some* quad in the store — pinning bug (1) above.
        assert!(
            ds.named_graphs.is_extensional(&fact_x_a),
            "x type A must remain extensional: it was asserted, never only derived"
        );
        assert!(
            ds.named_graphs.is_extensional(&fact_y_b),
            "y type B must remain extensional: it was asserted, never only derived"
        );
    }

    // The end-to-end `Ontology::remove_axiom` + `axiom2datalog` +
    // `apply_rule_deletions` test lives in `owl2rl2datalog` (which already
    // depends on `datalog`, avoiding a dev-dependency cycle) — see
    // `owl2rl2datalog/src/lib.rs`'s test module, `test_remove_axiom_end_to_end_retracts_derived_type_assertions`.

    // ── Delta-seeding regression tests (#534) ──────────────────────────────
    //
    // `apply_insertions` used to call `materialise_seminaive_tracked`, which
    // always seeds its first semi-naive iteration with `delta_start = 0` —
    // i.e. the *entire* current store, not just the newly-inserted fact(s).
    // The fix seeds with a true delta (`materialise_seminaive_tracked_from`,
    // `initial_delta_start = quad_start`). The tests below are the
    // regression coverage for that change: they must fail against a
    // (hypothetical) "single fixed body-atom position" seeding as described
    // in the issue, and must keep passing against the real fix, which
    // relies on `DatalogProgram`'s existing per-atom `rule_map` indexing to
    // get the rotation for free (see `materialise_seminaive_tracked_from`'s
    // doc comment in `reasoner.rs`).

    /// Two-different-predicate rule, so the two body atoms do NOT share a
    /// `rule_map` wildcard key (unlike transitivity, where both atoms have
    /// the same `(g, *, p, *)` shape and a lookup would find both even under
    /// a broken single-position seed). This is the test that actually pins
    /// the "delta seeded into the wrong body-atom position" trap described
    /// in the issue:
    ///
    ///   uncle(x, z) :- parent(x, y), brother(y, z)
    ///
    /// Pre-load only `parent(a, b)`. Materialise (no derivations yet — no
    /// `brother` facts exist). Then `apply_insertions([brother(b, c)])`: the
    /// newly-inserted fact can *only* bind the rule's SECOND body atom
    /// (`brother`), never the first (`parent`). A seeding scheme that only
    /// tries the delta fact against "the first body atom" would silently
    /// find zero matches and never derive `uncle(a, c)`.
    #[test]
    fn test_apply_insertions_derives_via_non_first_body_atom() {
        let (mut ds, g, a, _p, b, c) = setup_store();
        let parent = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/parent".to_string(),
            )));
        let brother = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/brother".to_string(),
            )));
        let uncle = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/uncle".to_string(),
            )));

        // uncle(x, z) :- parent(x, y), brother(y, z)
        let uncle_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(uncle),
                object: Term::Variable("z".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(parent),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("y".to_string()),
                    predicate: Term::Resource(brother),
                    object: Term::Variable("z".to_string()),
                }),
            ],
        };

        // Only parent(a, b) is present initially; no brother facts yet, so
        // no uncle facts can be derived.
        let fact_parent_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: parent,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_parent_ab);

        let mut reasoner = IncrementalReasoner::new(vec![uncle_rule], &mut ds).unwrap();

        let derived_uncle_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: uncle,
            obj: c,
        };
        assert!(
            !ds.named_graphs.contains(&derived_uncle_ac),
            "uncle(a, c) should not exist before inserting brother(b, c)"
        );

        // Insert brother(b, c): this fact matches only the rule's SECOND
        // body atom. Combined with the pre-existing parent(a, b), it should
        // derive uncle(a, c).
        let fact_brother_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: brother,
            obj: c,
        };
        reasoner
            .apply_insertions(&mut ds, &[fact_brother_bc])
            .unwrap();

        assert!(
            ds.named_graphs.contains(&fact_brother_bc),
            "inserted base fact brother(b, c) should be present"
        );
        assert!(
            ds.named_graphs.contains(&derived_uncle_ac),
            "uncle(a, c) should be derived via delta fact brother(b, c) \
             matching the rule's second body atom, joined against the \
             pre-existing parent(a, b)"
        );
    }

    /// Differential-equivalence check: materialising a full fact set from
    /// scratch must produce exactly the same closure as materialising a
    /// subset from scratch and then `apply_insertions`-ing the remainder —
    /// across a handful of differently-shaped multi-atom rules (transitivity,
    /// a heterogeneous parent/brother/uncle chain, and a subproperty-style
    /// alias). Any under-derivation from delta-seeding a rule the other two
    /// hand-written tests don't happen to cover shows up here as a set
    /// mismatch. See [#534](https://github.com/daghovland/rdf-datalog/issues/534).
    #[test]
    fn test_apply_insertions_matches_full_materialisation() {
        let (mut ds_full, g, a, p, b, c) = setup_store();
        let parent = ds_full
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/parent".to_string(),
            )));
        let brother = ds_full
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/brother".to_string(),
            )));
        let uncle = ds_full
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/uncle".to_string(),
            )));
        let p2 = ds_full
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));
        let d = ds_full
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));

        fn build_rules(
            g: u32,
            p: u32,
            p2: u32,
            parent: u32,
            brother: u32,
            uncle: u32,
        ) -> Vec<Rule> {
            vec![
                // Transitivity: x p z :- x p y, y p z
                transitivity_rule(g, p),
                // Alias: x p z :- x p2 z
                Rule {
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
                },
                // Heterogeneous chain: uncle(x, z) :- parent(x, y), brother(y, z)
                Rule {
                    head: RuleHead::NormalHead(QuadPattern {
                        graph: Term::Resource(g),
                        subject: Term::Variable("x".to_string()),
                        predicate: Term::Resource(uncle),
                        object: Term::Variable("z".to_string()),
                    }),
                    body: vec![
                        RuleAtom::PositivePattern(QuadPattern {
                            graph: Term::Resource(g),
                            subject: Term::Variable("x".to_string()),
                            predicate: Term::Resource(parent),
                            object: Term::Variable("y".to_string()),
                        }),
                        RuleAtom::PositivePattern(QuadPattern {
                            graph: Term::Resource(g),
                            subject: Term::Variable("y".to_string()),
                            predicate: Term::Resource(brother),
                            object: Term::Variable("z".to_string()),
                        }),
                    ],
                },
            ]
        }

        // Full fact set, inserted all at once.
        let all_facts = vec![
            Quad {
                triple_id: g,
                subject: a,
                predicate: p,
                obj: b,
            },
            Quad {
                triple_id: g,
                subject: b,
                predicate: p,
                obj: c,
            },
            Quad {
                triple_id: g,
                subject: c,
                predicate: p,
                obj: d,
            },
            Quad {
                triple_id: g,
                subject: a,
                predicate: p2,
                obj: d,
            },
            Quad {
                triple_id: g,
                subject: a,
                predicate: parent,
                obj: b,
            },
            Quad {
                triple_id: g,
                subject: b,
                predicate: brother,
                obj: c,
            },
            Quad {
                triple_id: g,
                subject: c,
                predicate: parent,
                obj: d,
            },
        ];

        // Path A: materialise everything from scratch in one go.
        for q in &all_facts {
            ds_full.named_graphs.add_quad(*q);
        }
        let rules_a = build_rules(g, p, p2, parent, brother, uncle);
        IncrementalReasoner::new(rules_a, &mut ds_full).unwrap();
        let mut quads_full: Vec<Quad> = ds_full.named_graphs.get_all_quads().collect();
        quads_full.sort_by_key(|q| (q.triple_id, q.subject, q.predicate, q.obj));

        // Path B: materialise a subset from scratch, then insert the rest
        // incrementally via `apply_insertions` (possibly in several batches,
        // to exercise the delta seed across multiple calls).
        let (mut ds_incr, g2, a2, p2_, b2, c2) = setup_store();
        assert_eq!(
            (g, a, p, b, c),
            (g2, a2, p2_, b2, c2),
            "setup_store determinism"
        );
        let parent2 = ds_incr
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/parent".to_string(),
            )));
        let brother2 = ds_incr
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/brother".to_string(),
            )));
        let uncle2 = ds_incr
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/uncle".to_string(),
            )));
        let p2_2 = ds_incr
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));
        let d2 = ds_incr
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));
        assert_eq!(
            (parent, brother, uncle, p2, d),
            (parent2, brother2, uncle2, p2_2, d2)
        );

        let seed_facts = vec![all_facts[0], all_facts[4]]; // a p b, a parent b
        for q in &seed_facts {
            ds_incr.named_graphs.add_quad(*q);
        }
        let rules_b = build_rules(g, p, p2, parent, brother, uncle);
        let mut reasoner = IncrementalReasoner::new(rules_b, &mut ds_incr).unwrap();

        // Insert the rest of the facts in two batches to exercise
        // `apply_insertions` being called more than once.
        let remaining = &all_facts[1..];
        let (batch1, batch2) = remaining.split_at(remaining.len() / 2);
        reasoner.apply_insertions(&mut ds_incr, batch1).unwrap();
        reasoner.apply_insertions(&mut ds_incr, batch2).unwrap();

        let mut quads_incr: Vec<Quad> = ds_incr.named_graphs.get_all_quads().collect();
        quads_incr.sort_by_key(|q| (q.triple_id, q.subject, q.predicate, q.obj));

        assert_eq!(
            quads_full, quads_incr,
            "incremental delta-seeded insertion must derive exactly the same \
             closure as materialising the whole fact set from scratch"
        );
    }

    /// Inserting a quad that is already present in the store is a no-op:
    /// `add_quad` dedups (so `quad_start == quad_count` after the insert
    /// loop), and the delta-seeded first iteration correctly sees an empty
    /// delta and performs zero rule evaluation instead of a full rescan.
    /// The closure must be exactly unchanged.
    #[test]
    fn test_apply_insertions_of_existing_quad_is_noop() {
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

        let mut quads_before: Vec<Quad> = ds.named_graphs.get_all_quads().collect();
        quads_before.sort_by_key(|q| (q.triple_id, q.subject, q.predicate, q.obj));

        // Re-insert an already-present base fact.
        reasoner.apply_insertions(&mut ds, &[fact_ab]).unwrap();

        let mut quads_after: Vec<Quad> = ds.named_graphs.get_all_quads().collect();
        quads_after.sort_by_key(|q| (q.triple_id, q.subject, q.predicate, q.obj));

        assert_eq!(
            quads_before, quads_after,
            "re-inserting an already-present base fact must not change the closure"
        );
    }

    /// Regression for a second correctness trap uncovered while fixing #534:
    /// some `apply_insertions` callers (`sparql_endpoint`'s SPARQL Update and
    /// transaction-commit paths) add the new base fact(s) directly to `base`
    /// themselves *before* calling `apply_insertions` — `add_quad` is
    /// idempotent, so this is harmless for the plain "is the fact present"
    /// question, but it means a **position-based** delta seed (inferring
    /// "what's new" from `base.named_graphs.quad_count` at entry) would see
    /// `quad_start == quad_count` — nothing to process — and silently skip
    /// all rule evaluation, exactly like the original `delta_start = 0` bug
    /// but inverted (empty delta instead of a full-store delta). The fix
    /// seeds `apply_insertions` with the explicit `inserts` fact list
    /// instead of a position, so it works whether or not the caller already
    /// added them.
    ///
    /// This test pre-adds the new fact to `base` exactly as those callers
    /// do, then calls `apply_insertions` with the same fact, and asserts the
    /// derivation still fires.
    #[test]
    fn test_apply_insertions_derives_even_when_caller_preadded_the_fact() {
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

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            !ds.named_graphs.contains(&derived_ac),
            "A->C should not exist before inserting A->B"
        );

        // Simulate a caller (like `sparql_endpoint::sparql_update`) that
        // adds the new base fact to the live store itself before invoking
        // the reasoner.
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        // `apply_insertions` is called with the SAME fact, which is already
        // present in `ds` at this point.
        reasoner.apply_insertions(&mut ds, &[fact_ab]).unwrap();

        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A->C must still be derived even though the caller pre-added A->B \
             to the store before calling apply_insertions"
        );
    }

    // ── apply_rule_insertions tests (#474) ──────────────────────────────

    /// Adding a rule for an entirely disjoint predicate must derive its own
    /// consequences without disturbing anything already in the closure.
    ///
    /// Setup: existing transitivity rule on `p`, materialised over A→B→C
    /// (derives A→C). Insert an unrelated rule deriving `q` facts from `p2`
    /// facts (no shared predicates in either direction). The new rule's
    /// consequence must appear; the transitivity closure must be byte-for-
    /// byte unaffected.
    #[test]
    fn test_apply_rule_insertions_no_interaction() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));
        let q = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q".to_string(),
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
        // Unrelated fact that will feed the new rule.
        let fact_ab_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);
        ds.named_graphs.add_quad(fact_ab_p2);

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
            "A->C should be derived before rule insertion"
        );

        // New rule: { ?x p2 ?y } => { ?x q ?y } -- disjoint from p/transitivity.
        let q_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(q),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p2),
                object: Term::Variable("y".to_string()),
            })],
        };

        let added = reasoner
            .apply_rule_insertions(&mut ds, &[q_rule])
            .expect("disjoint rule insertion must not error");
        assert_eq!(added, 1, "exactly one new q-fact should be derived");

        let derived_ab_q = Quad {
            triple_id: g,
            subject: a,
            predicate: q,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&derived_ab_q),
            "new rule's own consequence must be derived"
        );
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "pre-existing transitivity closure must be unaffected"
        );
    }

    /// A newly inserted rule must be able to consume facts derived by an
    /// *existing* rule (stratum > 0 output), not just raw base (EDB) facts —
    /// this is the "sees everything already derived" guarantee the issue
    /// requires.
    ///
    /// Setup: existing rule derives `mid(x,y)` from a base fact `base(x,y)`.
    /// Insert a new rule deriving `top(x,y)` from `mid(x,y)` (an intensional
    /// predicate the new rule never saw at construction time). `top` must be
    /// derived after insertion.
    #[test]
    fn test_apply_rule_insertions_consumes_existing_derived_fact() {
        let (mut ds, g, a, _p, b, _c) = setup_store();
        let base_pred = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/base_pred".to_string(),
            )));
        let mid_pred = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/mid_pred".to_string(),
            )));
        let top_pred = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/top_pred".to_string(),
            )));

        let fact_base = Quad {
            triple_id: g,
            subject: a,
            predicate: base_pred,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_base);

        // Existing rule: { ?x base_pred ?y } => { ?x mid_pred ?y }
        let mid_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(mid_pred),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(base_pred),
                object: Term::Variable("y".to_string()),
            })],
        };
        let mut reasoner = IncrementalReasoner::new(vec![mid_rule], &mut ds).unwrap();

        let derived_mid = Quad {
            triple_id: g,
            subject: a,
            predicate: mid_pred,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&derived_mid),
            "mid_pred should already be derived (not an EDB fact)"
        );

        // New rule: { ?x mid_pred ?y } => { ?x top_pred ?y } -- consumes the
        // *derived* fact, not the base fact.
        let top_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(top_pred),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(mid_pred),
                object: Term::Variable("y".to_string()),
            })],
        };

        let added = reasoner
            .apply_rule_insertions(&mut ds, &[top_rule])
            .expect("rule consuming existing derived facts must not error");
        assert_eq!(added, 1);

        let derived_top = Quad {
            triple_id: g,
            subject: a,
            predicate: top_pred,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&derived_top),
            "new rule must be able to derive from an existing rule's output, \
             not just raw base facts"
        );
    }

    /// Inserting a rule whose head an *existing* rule already negates must be
    /// rejected, and must not corrupt reasoner state.
    ///
    /// Setup: existing rule `flag(x,y) :- p(x,y), NOT blocked(x,y)`. Insert a
    /// new rule that can produce `blocked` facts. The existing rule's already-
    /// computed `NOT blocked` derivations would become stale if the new rule
    /// were silently appended after it, so this must return
    /// `Err(NotStratifiable)` and leave the reasoner's existing closure and
    /// queries fully intact.
    #[test]
    fn test_apply_rule_insertions_rejects_when_existing_rule_negates_new_predicate() {
        let (mut ds, g, a, p, b, _c) = setup_store();
        let blocked_pred = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/blocked".to_string(),
            )));
        let flag_pred = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/flag".to_string(),
            )));
        let source_pred = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/source".to_string(),
            )));

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        // Existing rule: flag(x,y) :- p(x,y), NOT blocked(x,y)
        let flag_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(flag_pred),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::NotPattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(blocked_pred),
                    object: Term::Variable("y".to_string()),
                }),
            ],
        };
        let mut reasoner = IncrementalReasoner::new(vec![flag_rule], &mut ds).unwrap();

        let derived_flag = Quad {
            triple_id: g,
            subject: a,
            predicate: flag_pred,
            obj: b,
        };
        assert!(
            ds.named_graphs.contains(&derived_flag),
            "flag should be derived: NOT blocked currently succeeds"
        );

        // New rule that could produce `blocked` facts.
        let blocked_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(blocked_pred),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(source_pred),
                object: Term::Variable("y".to_string()),
            })],
        };

        let result = reasoner.apply_rule_insertions(&mut ds, &[blocked_rule]);
        match result {
            Err(ReasoningError::NotStratifiable(_)) => {}
            Ok(_) => panic!("expected Err(NotStratifiable), got Ok"),
            Err(other) => panic!("expected Err(NotStratifiable), got {other:?}"),
        }

        // State must be fully intact: flag is still derived, and inserting a
        // `source` fact still doesn't produce `blocked` (the rule was never
        // wired in).
        assert!(
            ds.named_graphs.contains(&derived_flag),
            "pre-existing derivation must survive the rejected insertion"
        );
        let fact_source = Quad {
            triple_id: g,
            subject: a,
            predicate: source_pred,
            obj: b,
        };
        reasoner.apply_insertions(&mut ds, &[fact_source]).unwrap();
        let would_be_blocked = Quad {
            triple_id: g,
            subject: a,
            predicate: blocked_pred,
            obj: b,
        };
        assert!(
            !ds.named_graphs.contains(&would_be_blocked),
            "rejected rule must never have been wired in"
        );
    }

    /// Mirrors the rejection above but with the direction reversed: an
    /// *existing* rule positively consumes exactly the predicate a new rule's
    /// head would (re)produce. Must also be rejected, even though no negation
    /// is present anywhere -- pins down that the check rejects on *any*
    /// dependency edge, not just negative ones.
    ///
    /// Setup: existing transitivity rule on `p`. Insert a new alias rule
    /// `{ ?x p2 ?z } => { ?x p ?z }` -- its head is `p`, exactly the predicate
    /// the existing transitivity rule's body positively consumes.
    #[test]
    fn test_apply_rule_insertions_rejects_when_existing_rule_positively_consumes_new_predicate() {
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
        assert!(ds.named_graphs.contains(&derived_ac));

        // New rule: { ?x p2 ?z } => { ?x p ?z } -- head `p` is consumed by
        // the existing transitivity rule's body.
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

        let result = reasoner.apply_rule_insertions(&mut ds, &[alias_rule]);
        match result {
            Err(ReasoningError::NotStratifiable(_)) => {}
            Ok(_) => panic!("expected Err(NotStratifiable), got Ok"),
            Err(other) => panic!("expected Err(NotStratifiable), got {other:?}"),
        }
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "pre-existing closure must survive the rejected insertion"
        );
    }

    /// Re-adding a rule previously retracted via `apply_rule_deletions` must
    /// re-derive its facts, and must go through the reactivation path (the
    /// rule's original program/stratum), not append a brand-new stratum.
    #[test]
    fn test_apply_rule_insertions_reactivates_previously_deleted_rule() {
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

        let transitivity = transitivity_rule(g, p);
        let mut reasoner = IncrementalReasoner::new(vec![transitivity.clone()], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(ds.named_graphs.contains(&derived_ac));

        reasoner
            .apply_rule_deletions(&mut ds, std::slice::from_ref(&transitivity))
            .expect("retraction must succeed");
        assert!(
            !ds.named_graphs.contains(&derived_ac),
            "A->C should be gone after retraction"
        );

        let programs_before = reasoner.programs.len();

        let added = reasoner
            .apply_rule_insertions(&mut ds, &[transitivity])
            .expect("reactivating a previously-deleted rule must not error");
        assert_eq!(added, 1, "A->C should be re-derived");

        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A->C must be re-derived after reactivation"
        );
        assert_eq!(
            reasoner.programs.len(),
            programs_before,
            "reactivation must reuse the rule's original stratum, not append a new one"
        );
    }

    /// Regression test: reactivating a rule must go through the *same*
    /// append-only safety check as a fresh insertion, so it can't silently
    /// leave a stale negative derivation behind.
    ///
    /// Setup: stratum 0 `P(x) :- A(x)`; stratum 1 `R(x) :- NOT P(x)`.
    /// Retracting `P :- A` (correctly) makes `R` derivable (NOT P succeeds).
    /// Re-inserting `P :- A` must be rejected -- silently re-enabling it
    /// would leave the now-stale `R` fact behind (over-derivation), since
    /// semi-naive only adds and nothing would retract `R`.
    #[test]
    fn test_apply_rule_insertions_reactivation_rejects_stale_negation() {
        let (mut ds, g, a, _p, _b, _c) = setup_store();
        let pred_a = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/pred_a".to_string(),
            )));
        let pred_p = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/pred_p".to_string(),
            )));
        let pred_r = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/pred_r".to_string(),
            )));
        // A "domain" predicate purely to give `r_rule` a positive body atom
        // to trigger from -- the forward-chaining engine only ever fires a
        // rule off a *positive* body-atom match (see `get_rules_for_fact`),
        // so a rule with only a `NotPattern` body atom and no positive one
        // can never actually fire, regardless of `is_safe_rule`.
        let pred_dom = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/pred_dom".to_string(),
            )));

        let fact_a = Quad {
            triple_id: g,
            subject: a,
            predicate: pred_a,
            obj: a,
        };
        let fact_dom = Quad {
            triple_id: g,
            subject: a,
            predicate: pred_dom,
            obj: a,
        };
        ds.named_graphs.add_quad(fact_a);
        ds.named_graphs.add_quad(fact_dom);

        // Stratum 0: P(x) :- A(x)
        let p_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(pred_p),
                object: Term::Variable("x".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(pred_a),
                object: Term::Variable("x".to_string()),
            })],
        };
        // Stratum 1: R(x) :- dom(x), NOT P(x)
        let r_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(pred_r),
                object: Term::Variable("x".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(pred_dom),
                    object: Term::Variable("x".to_string()),
                }),
                RuleAtom::NotPattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(pred_p),
                    object: Term::Variable("x".to_string()),
                }),
            ],
        };

        let mut reasoner = IncrementalReasoner::new(vec![p_rule.clone(), r_rule], &mut ds).unwrap();

        let derived_p = Quad {
            triple_id: g,
            subject: a,
            predicate: pred_p,
            obj: a,
        };
        let derived_r = Quad {
            triple_id: g,
            subject: a,
            predicate: pred_r,
            obj: a,
        };
        assert!(ds.named_graphs.contains(&derived_p));
        assert!(
            !ds.named_graphs.contains(&derived_r),
            "R should not hold while P does"
        );

        reasoner
            .apply_rule_deletions(&mut ds, std::slice::from_ref(&p_rule))
            .expect("retracting P must succeed");
        assert!(!ds.named_graphs.contains(&derived_p));
        assert!(
            ds.named_graphs.contains(&derived_r),
            "R should now be derived: NOT P succeeds once P is retracted"
        );

        let result = reasoner.apply_rule_insertions(&mut ds, &[p_rule]);
        match result {
            Err(ReasoningError::NotStratifiable(_)) => {}
            Ok(_) => {
                panic!("expected Err(NotStratifiable): reactivating P would leave stale R behind")
            }
            Err(other) => panic!("expected Err(NotStratifiable), got {other:?}"),
        }
        // State must remain exactly as it was before the rejected call.
        assert!(!ds.named_graphs.contains(&derived_p));
        assert!(ds.named_graphs.contains(&derived_r));
    }

    /// `apply_rule_insertions` with an empty slice is a no-op, mirroring
    /// `apply_rule_deletions`'s `rules.is_empty()` short-circuit.
    #[test]
    fn test_apply_rule_insertions_empty_is_noop() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);
        let mut reasoner =
            IncrementalReasoner::new(vec![transitivity_rule(g, p)], &mut ds).unwrap();
        let added = reasoner.apply_rule_insertions(&mut ds, &[]).unwrap();
        assert_eq!(added, 0);
        let _ = c; // suppress unused warning if setup_store's c is unused here
    }

    /// Inserting a rule that already exists and is currently enabled is a
    /// no-op, idempotent across repeat calls.
    #[test]
    fn test_apply_rule_insertions_duplicate_of_enabled_rule_is_noop() {
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

        let transitivity = transitivity_rule(g, p);
        let mut reasoner = IncrementalReasoner::new(vec![transitivity.clone()], &mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(ds.named_graphs.contains(&derived_ac));

        let added = reasoner
            .apply_rule_insertions(&mut ds, std::slice::from_ref(&transitivity))
            .expect("re-inserting an already-enabled rule must not error");
        assert_eq!(
            added, 0,
            "duplicate insertion of an enabled rule is a no-op"
        );

        // Repeat: still idempotent.
        let added_again = reasoner
            .apply_rule_insertions(&mut ds, &[transitivity])
            .unwrap();
        assert_eq!(added_again, 0);
    }

    /// Reproduction attempt for a real-world deletion bug report (project #559/#533
    /// investigation): manual testing against the Dexpi2Imf project's actual
    /// `datalog/noaka_boundary.datalog` ruleset (public at
    /// https://github.com/equinor/Dexpi2Imf/blob/main/datalog/noaka_boundary.datalog)
    /// reportedly shows deleted base facts not properly cascading to delete
    /// inferred facts. This test reconstructs that exact rule shape:
    ///
    /// Stratum 0: `isBoundaryOf[new,pkg] :- isBoundaryOf[node,pkg], hasPart[new,node]`
    ///   (self-recursive transitive closure over an EDB `isBoundaryOf` seed).
    /// Stratum 1: `isInPackage[node,pkg] :- selInternal[node,pkg]`
    ///            `isInPackage[new,pkg] :- isInPackage[node,pkg], hasPart[new,node]`
    ///            `isInPackage[new,pkg] :- isInPackage[node,pkg], adjacentTo[node,new],
    ///                                      NOT isBoundaryOf[node,pkg]`
    ///   (self-recursive, cross-predicate positive dependency on stratum-0
    ///   `isBoundaryOf` via `hasPart`/R2, AND a negative dependency via R1 —
    ///   i.e. `isInPackage` facts can be derived either through a positive
    ///   cross-predicate edge (R2, `hasPart`) or gated by a negated cross-predicate
    ///   edge (R1). Existing regression tests (`test_delete_base_fact_cascades_deep_chain`,
    ///   `test_delete_base_fact_keeps_deep_diamond_via_longer_path`) only cover a single
    ///   self-recursive predicate with no cross-predicate edge at all — this is the
    ///   coverage gap this test targets.
    ///
    /// Oracle: after each deletion, compare `apply_deletions`'s result against a
    /// from-scratch `IncrementalReasoner::new` rebuild over the post-delete base
    /// facts — full-rebuild equivalence, not hand-enumerated expected facts, since
    /// hand-enumerating a transitive+negated program's exact closure is error-prone.
    #[test]
    fn test_delete_base_fact_cross_predicate_cascade_matches_full_rebuild() {
        let (mut ds, g, a, _p, b, _c) = setup_store();
        let mk_pred = |ds: &mut Datastore, name: &str| {
            ds.resources
                .add_node_resource(RdfResource::Iri(IriReference(format!(
                    "http://example.org/{name}"
                ))))
        };
        let has_part = mk_pred(&mut ds, "hasPart");
        let adjacent_to = mk_pred(&mut ds, "adjacentTo");
        let sel_internal = mk_pred(&mut ds, "selInternal");
        let is_boundary_of = mk_pred(&mut ds, "isBoundaryOf");
        let is_in_package = mk_pred(&mut ds, "isInPackage");

        // Nodes: node_a, node_b (= `a`, `b` from setup_store), plus node_c, node_d.
        let node_c = mk_pred(&mut ds, "node_c");
        let node_d = mk_pred(&mut ds, "node_d");
        let pkg = mk_pred(&mut ds, "pkg");

        // R4 (stratum 0): isBoundaryOf[new,pkg] :- isBoundaryOf[node,pkg], hasPart[new,node]
        let r4 = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("new".to_string()),
                predicate: Term::Resource(is_boundary_of),
                object: Term::Variable("pkg".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("node".to_string()),
                    predicate: Term::Resource(is_boundary_of),
                    object: Term::Variable("pkg".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("new".to_string()),
                    predicate: Term::Resource(has_part),
                    object: Term::Variable("node".to_string()),
                }),
            ],
        };
        // R3 (stratum 1): isInPackage[node,pkg] :- selInternal[node,pkg]
        let r3 = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("node".to_string()),
                predicate: Term::Resource(is_in_package),
                object: Term::Variable("pkg".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("node".to_string()),
                predicate: Term::Resource(sel_internal),
                object: Term::Variable("pkg".to_string()),
            })],
        };
        // R2 (stratum 1): isInPackage[new,pkg] :- isInPackage[node,pkg], hasPart[new,node]
        let r2 = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("new".to_string()),
                predicate: Term::Resource(is_in_package),
                object: Term::Variable("pkg".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("node".to_string()),
                    predicate: Term::Resource(is_in_package),
                    object: Term::Variable("pkg".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("new".to_string()),
                    predicate: Term::Resource(has_part),
                    object: Term::Variable("node".to_string()),
                }),
            ],
        };
        // R1 (stratum 1): isInPackage[new,pkg] :- isInPackage[node,pkg],
        //                  adjacentTo[node,new], NOT isBoundaryOf[node,pkg]
        let r1 = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("new".to_string()),
                predicate: Term::Resource(is_in_package),
                object: Term::Variable("pkg".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("node".to_string()),
                    predicate: Term::Resource(is_in_package),
                    object: Term::Variable("pkg".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("node".to_string()),
                    predicate: Term::Resource(adjacent_to),
                    object: Term::Variable("new".to_string()),
                }),
                RuleAtom::NotPattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("node".to_string()),
                    predicate: Term::Resource(is_boundary_of),
                    object: Term::Variable("pkg".to_string()),
                }),
            ],
        };
        let rules = vec![r4, r3, r2, r1];

        // Padding: enough unrelated, independent selInternal/isInPackage facts that
        // the two-fact retraction below stays under FALLBACK_THRESHOLD (25% of total
        // derived facts) and actually exercises the incremental BF path rather than
        // trivially-correct-by-construction full_rematerialise fallback.
        let mut padding_facts = Vec::new();
        for i in 0..20 {
            let pad_node = mk_pred(&mut ds, &format!("pad_node_{i}"));
            let pad_pkg = mk_pred(&mut ds, &format!("pad_pkg_{i}"));
            let f = Quad {
                triple_id: g,
                subject: pad_node,
                predicate: sel_internal,
                obj: pad_pkg,
            };
            ds.named_graphs.add_quad(f);
            padding_facts.push(f);
        }

        // Base facts:
        //   selInternal(a, pkg)     -- seeds isInPackage(a,pkg) via R3
        //   hasPart(d, a)           -- isInPackage(d,pkg) via R2 (independent of negation)
        //   hasPart(c, a)           -- isBoundaryOf(c,pkg) via R4, once isBoundaryOf(a,pkg) exists
        //   adjacentTo(a, b)        -- isInPackage(b,pkg) via R1 IF NOT isBoundaryOf(a,pkg)
        //   isBoundaryOf(a, pkg)    -- EDB fact; currently blocks R1 for node a
        let f_sel_internal = Quad {
            triple_id: g,
            subject: a,
            predicate: sel_internal,
            obj: pkg,
        };
        let f_has_part_da = Quad {
            triple_id: g,
            subject: node_d,
            predicate: has_part,
            obj: a,
        };
        let f_has_part_ca = Quad {
            triple_id: g,
            subject: node_c,
            predicate: has_part,
            obj: a,
        };
        let f_adjacent_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: adjacent_to,
            obj: b,
        };
        let f_is_boundary_a = Quad {
            triple_id: g,
            subject: a,
            predicate: is_boundary_of,
            obj: pkg,
        };
        for f in [
            f_sel_internal,
            f_has_part_da,
            f_has_part_ca,
            f_adjacent_ab,
            f_is_boundary_a,
        ] {
            ds.named_graphs.add_quad(f);
        }

        let mut reasoner = IncrementalReasoner::new(rules.clone(), &mut ds).unwrap();

        let is_in_package_a = Quad {
            triple_id: g,
            subject: a,
            predicate: is_in_package,
            obj: pkg,
        };
        let is_in_package_b = Quad {
            triple_id: g,
            subject: b,
            predicate: is_in_package,
            obj: pkg,
        };
        let is_in_package_d = Quad {
            triple_id: g,
            subject: node_d,
            predicate: is_in_package,
            obj: pkg,
        };
        let is_boundary_c = Quad {
            triple_id: g,
            subject: node_c,
            predicate: is_boundary_of,
            obj: pkg,
        };

        // Sanity-check the initial materialisation matches the hand-derived expectation.
        assert!(
            ds.named_graphs.contains(&is_in_package_a),
            "a should be in package via R3"
        );
        assert!(
            ds.named_graphs.contains(&is_in_package_d),
            "d should be in package via R2 (cross-predicate positive dependency)"
        );
        assert!(
            !ds.named_graphs.contains(&is_in_package_b),
            "b should NOT be in package yet: isBoundaryOf(a,pkg) blocks R1"
        );
        assert!(
            ds.named_graphs.contains(&is_boundary_c),
            "c should be isBoundaryOf via R4 (same-predicate cascade over isBoundaryOf(a,pkg))"
        );

        // ── Control case: delete selInternal(a,pkg) — pure monotonic cross-predicate
        // cascade. isInPackage(a,pkg) loses its only support and must be retracted,
        // which must cascade to isInPackage(d,pkg) (which depended on it via R2).
        // isBoundaryOf facts are untouched (independent derivation chain).
        // Mirror sparql_endpoint's actual call sequence exactly
        // (sparql_endpoint/src/sparql_update.rs's apply_prepared_update_with_options):
        // it physically removes net_deletes from the live store via
        // `store.remove_quad(q)` BEFORE calling `apply_reasoner_delta` /
        // `reasoner.apply_deletions`, rather than leaving the quad present and
        // letting `apply_deletions` do the removal as part of its own algorithm
        // (which is what the rest of this test file's other deletion tests do,
        // and what the doc comment on `apply_deletions` implicitly assumes).
        ds.named_graphs.remove_quad(f_sel_internal);

        let fallback_count_before = reasoner.fallback_count;
        reasoner
            .apply_deletions(&mut ds, &[f_sel_internal])
            .unwrap();
        assert_eq!(
            reasoner.fallback_count, fallback_count_before,
            "this test must exercise the incremental BF path, not full_rematerialise \
             fallback (which would trivially be correct by construction and mask a \
             real incremental-only bug) -- check FALLBACK_THRESHOLD against this \
             fixture's PD/total ratio if this fails"
        );

        // Oracle: fresh rebuild from the base facts alone (minus the deleted one).
        let mut ds_oracle_base = Datastore::new(100);
        ds_oracle_base.resources = ds.resources.clone();
        for f in [f_has_part_da, f_has_part_ca, f_adjacent_ab, f_is_boundary_a] {
            ds_oracle_base.named_graphs.add_quad(f);
        }
        for &f in &padding_facts {
            ds_oracle_base.named_graphs.add_quad(f);
        }
        IncrementalReasoner::new(rules.clone(), &mut ds_oracle_base).unwrap();

        assert_eq!(
            ds.named_graphs.contains(&is_in_package_a),
            ds_oracle_base.named_graphs.contains(&is_in_package_a),
            "control case: isInPackage(a,pkg) retraction must match full-rebuild oracle"
        );
        assert_eq!(
            ds.named_graphs.contains(&is_in_package_d),
            ds_oracle_base.named_graphs.contains(&is_in_package_d),
            "control case: isInPackage(d,pkg) cross-predicate cascade must match full-rebuild oracle"
        );
        assert!(
            !ds.named_graphs.contains(&is_in_package_a),
            "control case: isInPackage(a,pkg) must actually be retracted (not just match a buggy oracle)"
        );
        assert!(
            !ds.named_graphs.contains(&is_in_package_d),
            "control case: isInPackage(d,pkg) must actually be retracted (cross-predicate cascade)"
        );
        assert_eq!(
            ds.named_graphs.contains(&is_boundary_c),
            ds_oracle_base.named_graphs.contains(&is_boundary_c),
            "control case: isBoundaryOf(c,pkg) must be unaffected, matching oracle"
        );
    }
}

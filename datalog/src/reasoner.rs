/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::datalog::{
    apply_substitution_quad, constant_quad_pattern, direct_wildcard_pattern, empty_substitution,
    evaluate, get_matches_for_rule, is_fact, is_safe_rule, wildcard_quad_pattern,
};
use crate::stratifier::RulePartitioner;
use crate::types::{
    Derivation, DerivedFromIndex, PartialRule, QuadWildcard, Rule, RuleAtom, RuleHead,
};
use dag_rdf::Datastore;
use std::collections::HashMap;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Error produced while building or materialising a Datalog program.
///
/// Covers: a genuine, correctly-derived logical contradiction (a rule whose
/// head is [`RuleHead::Contradiction`] had its body satisfied, see
/// [#301](https://github.com/daghovland/rdf-datalog/issues/301)); a program
/// that cannot be stratified; and a rule that is unsafe (head variable not
/// bound in its body). All previously crashed the whole process via
/// `panic!`; see [#363](https://github.com/daghovland/rdf-datalog/issues/363).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReasoningError {
    /// A `RuleHead::Contradiction` rule fired. The `String` describes the
    /// triggering rule (its `Display` output).
    #[error("Contradiction during reasoning: {0}")]
    Contradiction(String),
    /// A negative dependency edge sits on a cycle, so the program cannot be
    /// stratified. The `String` describes a rule on the offending cycle
    /// (its `Display` output). Previously this crashed the whole process via
    /// `panic!`; see [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    #[error(
        "Datalog program has a cycle with negation — not stratifiable! Cycle includes rule: {0}"
    )]
    NotStratifiable(String),
    /// A rule's head references a variable not bound by any body atom. The
    /// `String` describes the unsafe variables and the offending rule (its
    /// `Display` output). Previously this crashed the whole process via
    /// `panic!`; see [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    #[error("Unsafe Datalog rule: {0}")]
    UnsafeRule(String),
}

// ── DatalogProgram ────────────────────────────────────────────────────────────

pub struct DatalogProgram {
    pub rules: Vec<Rule>,
    rule_map: HashMap<QuadWildcard, Vec<PartialRule>>,
    /// Records for each derived quad how it was produced (rule + body witnesses).
    pub derived_from: DerivedFromIndex,
    /// Number of times [`Self::materialise_seminaive`]/
    /// [`Self::materialise_seminaive_tracked`] has run to completion or error.
    /// Exists purely so tests can prove which rollback path
    /// (`IncrementalReasoner`'s undo-log fast path vs. `rebuild_from_base`)
    /// actually ran for a given call — the fast path never re-invokes
    /// materialisation during rollback, `rebuild_from_base` always does. See
    /// [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    pub(crate) materialise_calls: usize,
    /// Indices into `rules` of rules that have been retracted via
    /// [`Self::disable_rule`] (TBox/rule retraction, see
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162)).
    ///
    /// A disabled rule is **not** removed from `rules` — doing so would shift
    /// every later rule's index, silently corrupting existing
    /// [`Derivation::rule_id`](crate::types::Derivation::rule_id) references
    /// recorded in `derived_from` for facts derived by *other* rules. Instead
    /// the rule stays at its original index but is filtered out of
    /// `get_rules_for_fact`/`get_facts`, so it can never fire again while old
    /// derivation records that point at its index remain meaningful (e.g. for
    /// `IncrementalReasoner`'s backward phase to find facts it once produced).
    pub(crate) disabled_rules: std::collections::HashSet<usize>,
}

impl DatalogProgram {
    /// Builds a `DatalogProgram` from `rules`.
    ///
    /// Returns `Err(ReasoningError::UnsafeRule)` if any rule's head
    /// references a variable not bound in its body, instead of panicking —
    /// see [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    pub fn new(rules: Vec<Rule>) -> Result<Self, ReasoningError> {
        for r in &rules {
            is_safe_rule(r)?;
        }
        // Single-pass build: one entry per body atom, using the canonical (exact)
        // wildcard pattern.  Sub-wildcard expansion happens on the FACT side in
        // get_rules_for_fact, so the rule only needs its direct pattern as a key.
        // Using all sub-wildcards here would index every rule under (*, *, *, *),
        // causing every fact to scan every rule — O(facts × rules) = catastrophic.
        let mut rule_map: HashMap<QuadWildcard, Vec<PartialRule>> = HashMap::new();
        for (rule_id, rule) in rules.iter().enumerate() {
            for atom in &rule.body {
                if let RuleAtom::PositivePattern(p) = atom {
                    let wc = direct_wildcard_pattern(p);
                    rule_map.entry(wc).or_default().push(PartialRule {
                        rule: rule.clone(),
                        match_pattern: p.clone(),
                        rule_id,
                    });
                }
            }
        }
        Ok(DatalogProgram {
            rules,
            rule_map,
            derived_from: DerivedFromIndex::new(),
            materialise_calls: 0,
            disabled_rules: std::collections::HashSet::new(),
        })
    }

    /// Retract the rule at `rule_id` (an index into `rules`) so it can no
    /// longer fire. Returns `true` iff it was not already disabled.
    ///
    /// See the `disabled_rules` field doc for why this does not remove the
    /// rule from `rules` outright. Part of [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    pub(crate) fn disable_rule(&mut self, rule_id: usize) -> bool {
        self.disabled_rules.insert(rule_id)
    }

    /// Re-enable a previously-disabled rule (used to roll back a failed
    /// [`crate::IncrementalReasoner::apply_rule_deletions`] call). Returns
    /// `true` iff it was actually disabled beforehand.
    pub(crate) fn enable_rule(&mut self, rule_id: usize) -> bool {
        self.disabled_rules.remove(&rule_id)
    }

    /// True iff the rule at `rule_id` has been retracted via [`Self::disable_rule`].
    pub(crate) fn is_rule_disabled(&self, rule_id: usize) -> bool {
        self.disabled_rules.contains(&rule_id)
    }

    /// Adds a single `rule` to this program.
    ///
    /// Returns `Err(ReasoningError::UnsafeRule)` if `rule`'s head
    /// references a variable not bound in its body, instead of panicking —
    /// see [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    pub fn add_rule(&mut self, rule: Rule) -> Result<(), ReasoningError> {
        is_safe_rule(&rule)?;
        let rule_id = self.rules.len(); // will be the new index after push
        for atom in &rule.body {
            if let RuleAtom::PositivePattern(p) = atom {
                let wc = direct_wildcard_pattern(p);
                self.rule_map.entry(wc).or_default().push(PartialRule {
                    rule: rule.clone(),
                    match_pattern: p.clone(),
                    rule_id,
                });
            }
        }
        self.rules.push(rule);
        Ok(())
    }

    fn get_rules_for_fact(&self, fact: &dag_rdf::Quad) -> Vec<crate::types::PartialRuleMatch> {
        wildcard_quad_pattern(&constant_quad_pattern(fact))
            .iter()
            .filter_map(|wc| self.rule_map.get(wc))
            .flatten()
            .filter(|pr| !self.disabled_rules.contains(&pr.rule_id))
            .flat_map(|pr| get_matches_for_rule(fact, pr))
            .collect()
    }

    fn get_facts(&self) -> Result<Vec<dag_rdf::Quad>, ReasoningError> {
        self.rules
            .iter()
            .enumerate()
            .filter(|(rule_id, r)| is_fact(r) && !self.disabled_rules.contains(rule_id))
            .map(|(_, r)| match &r.head {
                RuleHead::Contradiction => Err(ReasoningError::Contradiction(format!("{r}"))),
                RuleHead::NormalHead(p) => Ok(apply_substitution_quad(&empty_substitution(), p)),
            })
            .collect()
    }

    /// Return the ground facts encoded directly in rules (body-less rules).
    /// Callers that want to drive materialisation manually must seed these before
    /// calling `materialise_one_iteration`.
    ///
    /// Returns `Err(ReasoningError::Contradiction)` if a body-less
    /// `RuleHead::Contradiction` rule is present (an inconsistency asserted
    /// directly, with no body to evaluate) — see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301).
    pub fn materialise_seed_facts(&self) -> Result<Vec<dag_rdf::Quad>, ReasoningError> {
        self.get_facts()
    }

    /// Run one semi-naive iteration over `named_graphs`, starting from `delta_start`.
    ///
    /// Returns `(new_delta_start, inferred_count)` where `inferred_count` is the number
    /// of quads added this iteration.  Returns `None` when the fixpoint is reached
    /// (no new quads were produced in the previous iteration).
    ///
    /// Returns `Err(ReasoningError::Contradiction)` if a `RuleHead::Contradiction`
    /// rule's body is satisfied by the current store — a genuine, correctly-derived
    /// inconsistency. Callers must not treat a partially-applied iteration as
    /// having produced a sound closure; see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301).
    pub fn materialise_one_iteration(
        &mut self,
        datastore: &mut Datastore,
        delta_start: usize,
    ) -> Result<Option<(usize, usize)>, ReasoningError> {
        self.materialise_one_iteration_tracked(datastore, delta_start, None)
    }

    /// Same as [`Self::materialise_one_iteration`], but if `track` is
    /// `Some`, every genuinely new `(derived_quad, Derivation)` entry
    /// recorded this iteration (i.e. every call to
    /// [`DerivedFromIndex::record`] that returned `true`) is also appended
    /// to it. Used by [`Self::materialise_seminaive_tracked`] to build an
    /// undo log for cheap rollback — see
    /// [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    fn materialise_one_iteration_tracked(
        &mut self,
        datastore: &mut Datastore,
        delta_start: usize,
        track: Option<&mut Vec<(dag_rdf::Quad, Derivation)>>,
    ) -> Result<Option<(usize, usize)>, ReasoningError> {
        let delta_end = datastore.named_graphs.quad_count;
        if delta_start >= delta_end {
            return Ok(None); // fixpoint reached
        }

        let delta: Vec<dag_rdf::Quad> =
            datastore.named_graphs.quad_list[delta_start..delta_end].to_vec();
        let new_count = self.materialise_delta_iteration(datastore, &delta, track)?;
        Ok(Some((delta_end, new_count)))
    }

    /// Match every rule body atom that any quad in `delta` can bind, joining
    /// the rule's other body atoms against the full (unrestricted) store,
    /// and add every newly-derived quad to `datastore`. Returns the number
    /// of quads added.
    ///
    /// This is the core of one semi-naive iteration, factored out so it can
    /// be driven two ways:
    /// - by position (`materialise_one_iteration_tracked`): `delta` is a
    ///   contiguous slice of `datastore.named_graphs.quad_list`.
    /// - by an explicit fact list (`materialise_seminaive_tracked_from_facts`):
    ///   `delta` is caller-supplied and does not need to correspond to any
    ///   particular position in `quad_list` — in particular it works whether
    ///   or not `delta`'s quads are already present in `datastore` (each is
    ///   still only matched once, since `get_rules_for_fact` just looks up
    ///   the rule index; whether the quad itself was already extensional
    ///   doesn't change which rules it can trigger). See
    ///   [#534](https://github.com/daghovland/rdf-datalog/issues/534) for why
    ///   this position-independence matters: some callers
    ///   (`sparql_endpoint`) add net-insert quads to the live store
    ///   themselves before invoking the reasoner, so a delta seed that only
    ///   worked by inferring "what's new" from a quad-list position would
    ///   silently see an empty delta and skip derivation entirely.
    fn materialise_delta_iteration(
        &mut self,
        datastore: &mut Datastore,
        delta: &[dag_rdf::Quad],
        mut track: Option<&mut Vec<(dag_rdf::Quad, Derivation)>>,
    ) -> Result<usize, ReasoningError> {
        let quad_count_before = datastore.named_graphs.quad_count;
        for quad in delta {
            for rule_match in self.get_rules_for_fact(quad) {
                // `get_rules_for_fact` only tells us that `quad` matches ONE
                // triggering atom of this rule's body — the other body atoms
                // (e.g. a property-edge atom guarding a conditional
                // contradiction such as `cls-maxc0`, see
                // https://github.com/daghovland/rdf-datalog/issues/298) are
                // NOT yet checked at this point. We must run the full join
                // via `evaluate()` before treating a `Contradiction` head as
                // triggered — panicking here on the bare triggering-atom
                // match would fire the contradiction unconditionally,
                // ignoring the rest of the rule body.
                let head_pattern = match &rule_match.partial_rule.rule.head {
                    RuleHead::Contradiction => {
                        if !evaluate(datastore, &rule_match).is_empty() {
                            return Err(ReasoningError::Contradiction(format!(
                                "{}",
                                rule_match.partial_rule.rule
                            )));
                        }
                        continue;
                    }
                    RuleHead::NormalHead(h) => h.clone(),
                };
                // evaluate() borrows datastore immutably and returns an owned Vec,
                // so the borrow is released before add_intensional_quad() is called.
                let rule = &rule_match.partial_rule.rule;
                let rule_id = rule_match.partial_rule.rule_id;
                let subs = evaluate(datastore, &rule_match);
                for sub in subs {
                    let derived = apply_substitution_quad(&sub, &head_pattern);
                    datastore.named_graphs.add_intensional_quad(derived);
                    // Always record this derivation path.  The BF backward phase
                    // needs all witnesses, not just the first one that created the
                    // fact.  Duplicate (rule_id, witnesses) pairs are suppressed.
                    let body_witnesses: Vec<dag_rdf::Quad> = rule
                        .body
                        .iter()
                        .filter_map(|atom| match atom {
                            RuleAtom::PositivePattern(p) => Some(apply_substitution_quad(&sub, p)),
                            _ => None,
                        })
                        .collect();
                    // record() deduplicates, so no need to check first.
                    let derivation = Derivation {
                        rule_id,
                        body_witnesses,
                    };
                    if let Some(buf) = track.as_deref_mut() {
                        if self.derived_from.record(derived, derivation.clone()) {
                            buf.push((derived, derivation));
                        }
                    } else {
                        self.derived_from.record(derived, derivation); // move — no clone
                    }
                }
            }
        }
        Ok(datastore.named_graphs.quad_count - quad_count_before)
    }

    /// Semi-naive forward-chaining materialisation over the quad store.
    ///
    /// Each iteration evaluates rules only against the *delta* — quads newly
    /// added in the previous iteration — rather than scanning the whole store.
    /// Joins for non-triggering body atoms still use the full indexed store.
    /// This gives O(delta × rules) work per iteration instead of O(store × rules).
    ///
    /// Returns `Err(ReasoningError::Contradiction)` on a genuine, correctly-derived
    /// inconsistency, instead of panicking (see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301)). The store may
    /// already contain some quads derived earlier in the same run when this
    /// happens; callers that need a clean rollback should restore/rebuild from the
    /// base facts (e.g. [`crate::IncrementalReasoner::rebuild_from_base`]) rather
    /// than trust the partially-materialised closure.
    pub fn materialise_seminaive(
        &mut self,
        datastore: &mut Datastore,
    ) -> Result<(), ReasoningError> {
        let mut track = Vec::new();
        self.materialise_seminaive_tracked(datastore, &mut track)
    }

    /// Same as [`Self::materialise_seminaive`], but appends every genuinely
    /// new `(derived_quad, Derivation)` entry produced during THIS call to
    /// `track` (in the order they were recorded). Callers implementing
    /// cheap undo-log rollback (see
    /// [`crate::IncrementalReasoner::apply_insertions`]/
    /// [`crate::IncrementalReasoner::apply_deletions`]) use `track` to know
    /// exactly which `derived_from` entries to remove — via
    /// [`DerivedFromIndex::unrecord`] — on failure, without re-deriving
    /// anything. See [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    pub fn materialise_seminaive_tracked(
        &mut self,
        datastore: &mut Datastore,
        track: &mut Vec<(dag_rdf::Quad, Derivation)>,
    ) -> Result<(), ReasoningError> {
        self.materialise_seminaive_tracked_from(datastore, track, 0)
    }

    /// Same as [`Self::materialise_seminaive_tracked`], but the *first*
    /// semi-naive iteration treats `datastore.named_graphs.quad_list[initial_delta_start..]`
    /// as the seed delta instead of the whole store (`initial_delta_start = 0`).
    ///
    /// This is what makes [`crate::IncrementalReasoner::apply_insertions`]
    /// genuinely incremental: passing `initial_delta_start` = the quad count
    /// captured *before* the newly-inserted base facts were appended means
    /// the first iteration only matches rules against those new facts (and
    /// only joins the *other* body atoms of each rule against the full
    /// store), instead of re-matching every rule against every pre-existing
    /// quad in the store. Every later iteration still starts from the
    /// previous iteration's own output, exactly as in the `_start = 0` case
    /// — this only changes the seed for iteration 1.
    ///
    /// This does **not** require a separate "rotate which body atom is the
    /// delta" step: `get_rules_for_fact` already indexes one [`crate::types::PartialRule`]
    /// entry per body atom (see [`Self::new`]/[`Self::add_rule`]), so for a
    /// delta fact `f` that can bind body atom `Ai` of an n-atom rule, this
    /// naturally evaluates `delta(Ai) ⋈ full(A1..An)` — once per atom `f`
    /// can bind — because [`crate::datalog::evaluate_positive`] joins the
    /// *unrestricted* store for every body atom starting from that one
    /// triggering match. Callers seeding a non-zero `initial_delta_start`
    /// must not skip any newly-added facts: passing anything other than a
    /// quad-count boundary captured before the new facts were appended can
    /// under-derive. See [#534](https://github.com/daghovland/rdf-datalog/issues/534).
    ///
    /// Deletion re-derivation (`IncrementalReasoner::forward_phase`/
    /// `forward_phase_rules`) must keep using `initial_delta_start = 0` (via
    /// [`Self::materialise_seminaive_tracked`]): after removing the
    /// possibly-deleted set, re-derivation needs to re-match rules against
    /// the *surviving* (old) facts, not just anything newly appended.
    pub fn materialise_seminaive_tracked_from(
        &mut self,
        datastore: &mut Datastore,
        track: &mut Vec<(dag_rdf::Quad, Derivation)>,
        initial_delta_start: usize,
    ) -> Result<(), ReasoningError> {
        self.materialise_calls += 1;
        for quad in self.get_facts()? {
            datastore.named_graphs.add_quad(quad);
        }

        let mut delta_start: usize = initial_delta_start;
        loop {
            match self.materialise_one_iteration_tracked(datastore, delta_start, Some(track))? {
                None => break,
                Some((new_start, _)) => delta_start = new_start,
            }
        }
        Ok(())
    }

    /// Same as [`Self::materialise_seminaive_tracked`], but the *first*
    /// semi-naive iteration's delta is the explicit `delta_facts` list
    /// instead of a `quad_list` position range.
    ///
    /// Use this (rather than [`Self::materialise_seminaive_tracked_from`])
    /// when the caller cannot guarantee `delta_facts` haven't already been
    /// appended to `datastore` before this call — [`crate::IncrementalReasoner::apply_insertions`]
    /// uses this because some `sparql_endpoint` call sites add net-insert
    /// quads to the live store themselves (for the no-reasoner-configured
    /// code path) before invoking the reasoner, which would make a
    /// position-based delta seed see an empty delta (quad count already
    /// includes them, so `quad_start == quad_count`) and silently skip
    /// derivation. `delta_facts` are added to `datastore` here if not
    /// already present (idempotent either way, see
    /// [`dag_rdf::QuadTable::add_quad`]), then matched directly — see
    /// [#534](https://github.com/daghovland/rdf-datalog/issues/534).
    ///
    /// After the first iteration, subsequent iterations switch to the
    /// position-based path (their delta — newly-derived facts appended
    /// *during* this call — is unambiguous, since nothing external mutates
    /// `datastore` mid-call).
    pub fn materialise_seminaive_tracked_from_facts(
        &mut self,
        datastore: &mut Datastore,
        track: &mut Vec<(dag_rdf::Quad, Derivation)>,
        delta_facts: &[dag_rdf::Quad],
    ) -> Result<(), ReasoningError> {
        self.materialise_calls += 1;
        for quad in self.get_facts()? {
            datastore.named_graphs.add_quad(quad);
        }
        for q in delta_facts {
            datastore.named_graphs.add_quad(*q);
        }

        // Boundary between `delta_facts` (just processed explicitly below)
        // and whatever this iteration derives — the position-based loop
        // that follows picks up from here.
        let baseline = datastore.named_graphs.quad_count;
        self.materialise_delta_iteration(datastore, delta_facts, Some(track))?;

        let mut delta_start = baseline;
        loop {
            match self.materialise_one_iteration_tracked(datastore, delta_start, Some(track))? {
                None => break,
                Some((new_start, _)) => delta_start = new_start,
            }
        }
        Ok(())
    }

    /// Naive materialisation kept for regression comparison.
    #[allow(dead_code)]
    fn materialise_naive(&self, datastore: &mut Datastore) -> Result<(), ReasoningError> {
        for quad in self.get_facts()? {
            datastore.named_graphs.add_quad(quad);
        }
        let mut changed = true;
        while changed {
            changed = false;
            let quads: Vec<dag_rdf::Quad> = datastore.named_graphs.get_all_quads().collect();
            let mut new_quads: Vec<dag_rdf::Quad> = Vec::new();
            for quad in &quads {
                for rule_match in self.get_rules_for_fact(quad) {
                    // See the matching comment in `materialise_one_iteration`:
                    // the triggering-atom match alone does not prove the full
                    // rule body holds, so a `Contradiction` head must be
                    // re-checked via a full `evaluate()` join first.
                    let head_pattern = match &rule_match.partial_rule.rule.head {
                        RuleHead::Contradiction => {
                            if !evaluate(datastore, &rule_match).is_empty() {
                                return Err(ReasoningError::Contradiction(format!(
                                    "{}",
                                    rule_match.partial_rule.rule
                                )));
                            }
                            continue;
                        }
                        RuleHead::NormalHead(h) => h.clone(),
                    };
                    let subs = evaluate(datastore, &rule_match);
                    for sub in subs {
                        let new_quad = apply_substitution_quad(&sub, &head_pattern);
                        if !datastore.named_graphs.contains(&new_quad) {
                            new_quads.push(new_quad);
                        }
                    }
                }
            }
            for q in new_quads {
                if !datastore.named_graphs.contains(&q) {
                    datastore.named_graphs.add_quad(q);
                    changed = true;
                }
            }
        }
        Ok(())
    }
}

// ── Top-level evaluate ────────────────────────────────────────────────────────

/// Stratify `rules` and materialise each stratum in order over `datastore`.
///
/// Returns `Err(ReasoningError::Contradiction)` if any stratum derives a
/// genuine inconsistency (a `RuleHead::Contradiction` rule body is satisfied),
/// instead of panicking — see
/// [#301](https://github.com/daghovland/rdf-datalog/issues/301). `datastore`
/// may contain a partially-materialised closure in that case.
pub fn evaluate_rules(rules: Vec<Rule>, datastore: &mut Datastore) -> Result<(), ReasoningError> {
    let stratifier = RulePartitioner::new(rules);
    let stratification = stratifier.order_rules()?;
    for partition in stratification {
        let mut program = DatalogProgram::new(partition)?;
        program.materialise_seminaive(datastore)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RuleAtom, RuleHead};
    use dag_rdf::{
        DEFAULT_GRAPH_ELEMENT_ID, Datastore, IriReference, Quad, QuadPattern, RdfResource, Term,
    };

    /// Build a simple transitivity scenario:
    ///   Base facts: (g, a, p, b), (g, b, p, c)
    ///   Rule:       { ?x p ?y, ?y p ?z } => { ?x p ?z }
    /// After materialisation, (g, a, p, c) should be derived, and the base
    /// facts should remain base.
    #[test]
    fn test_reasoner_marks_inferred_as_derived() {
        let mut ds = Datastore::new(100);
        let g = DEFAULT_GRAPH_ELEMENT_ID;

        // Intern resources: a, p, b, c
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

        // Insert base facts directly (not via reasoner)
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

        // Transitivity rule: [?x, p, ?y], [?y, p, ?z] => [?x, p, ?z]
        let rule = Rule {
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
        };

        let mut program = DatalogProgram::new(vec![rule]).unwrap();
        program.materialise_seminaive(&mut ds).unwrap();

        // The derived quad (a, p, c) should exist
        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "transitively inferred quad (a, p, c) should be present"
        );
        assert!(
            !ds.named_graphs.is_extensional(&derived_ac),
            "inferred quad (a, p, c) should be marked derived, not base"
        );

        // Original base facts should still be base
        assert!(
            ds.named_graphs.is_extensional(&fact_ab),
            "original fact (a, p, b) should remain base"
        );
        assert!(
            ds.named_graphs.is_extensional(&fact_bc),
            "original fact (b, p, c) should remain base"
        );
    }

    /// Helper: build a small Datastore with resources a, p, b, c and return
    /// (datastore, g, a, p, b, c) ready for rule tests.
    fn setup_abpc_store() -> (Datastore, u32, u32, u32, u32, u32) {
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

    /// Base quads added directly must have no derivation entry.
    #[test]
    fn test_base_quad_has_no_derivation() {
        let (mut ds, g, a, p, b, _c) = setup_abpc_store();
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        // No rules → program does nothing, index stays empty.
        let mut program = DatalogProgram::new(vec![]).unwrap();
        program.materialise_seminaive(&mut ds).unwrap();

        assert!(
            !program.derived_from.has_derivation(&fact_ab),
            "base quad should have no derivation entry"
        );
    }

    /// Transitively derived quad must have a derivation with correct rule_id and witnesses.
    #[test]
    fn test_derived_quad_has_derivation() {
        let (mut ds, g, a, p, b, c) = setup_abpc_store();
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

        // Transitivity rule: [?x, p, ?y], [?y, p, ?z] => [?x, p, ?z]
        let rule = Rule {
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
        };

        let mut program = DatalogProgram::new(vec![rule]).unwrap();
        program.materialise_seminaive(&mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };

        // The derivation index must record (a, p, c).
        assert!(
            program.derived_from.has_derivation(&derived_ac),
            "derived quad (a, p, c) should have a derivation entry"
        );

        let derivations = program.derived_from.derivations_for(&derived_ac);
        assert_eq!(derivations.len(), 1, "exactly one derivation path expected");

        let deriv = &derivations[0];
        assert_eq!(deriv.rule_id, 0, "should reference rule at index 0");
        // Body witnesses are (a, p, b) then (b, p, c)
        assert_eq!(
            deriv.body_witnesses,
            vec![fact_ab, fact_bc],
            "body witnesses should be the two base facts"
        );
    }

    /// When the same quad is derivable via two paths it should get two entries.
    #[test]
    fn test_multiple_derivation_paths() {
        // Facts: (a, p, b), (b, p, c), (a, p2, c)
        // Rule 1 (transitivity via p): [?x, p, ?y], [?y, p, ?z] => [?x, p, ?z]
        // Rule 2 (alias):              [?x, p2, ?z]              => [?x, p, ?z]
        // Both rules can derive (a, p, c); we want two derivation entries.
        let (mut ds, g, a, p, b, c) = setup_abpc_store();
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

        // Rule 0: transitivity via p
        let rule_transit = Rule {
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
        };
        // Rule 1: alias p2 to p
        let rule_alias = Rule {
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

        let mut program = DatalogProgram::new(vec![rule_transit, rule_alias]).unwrap();
        program.materialise_seminaive(&mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };

        let derivations = program.derived_from.derivations_for(&derived_ac);
        assert!(
            derivations.len() >= 2,
            "expected at least 2 derivation paths for (a, p, c), got {}",
            derivations.len()
        );
    }

    /// A genuine, correctly-derived contradiction must surface as
    /// `Err(ReasoningError::Contradiction)` instead of panicking.
    ///
    /// Setup: fact (a, p, b); rule `{ ?x p ?y } => Contradiction`.
    /// The rule's body is satisfied by the fact, so materialisation must
    /// return an error rather than crash the process.
    /// See https://github.com/daghovland/rdf-datalog/issues/301.
    #[test]
    fn test_contradiction_returns_err_not_panic() {
        let (mut ds, g, a, p, b, _c) = setup_abpc_store();
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

        let mut program = DatalogProgram::new(vec![contradiction_rule]).unwrap();
        let result = program.materialise_seminaive(&mut ds);

        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );
    }

    /// [`evaluate_rules`] (the top-level entry point used throughout the
    /// codebase) must also propagate the contradiction as `Err` rather than
    /// panicking.
    #[test]
    fn test_evaluate_rules_propagates_contradiction() {
        let (mut ds, g, a, p, b, _c) = setup_abpc_store();
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

        let result = evaluate_rules(vec![contradiction_rule], &mut ds);
        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );
    }

    /// `is_safe_rule` must return `Err(ReasoningError::UnsafeRule)` — not
    /// panic — for a rule whose head references a variable not bound in its
    /// body. See [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    #[test]
    fn test_is_safe_rule_rejects_unbound_head_variable() {
        let (_ds, g, _a, p, _b, _c) = setup_abpc_store();

        // Head uses ?y, which never appears in the body.
        let unsafe_rule = Rule {
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
                object: Term::Variable("x".to_string()),
            })],
        };

        let result = crate::datalog::is_safe_rule(&unsafe_rule);
        assert!(
            matches!(result, Err(ReasoningError::UnsafeRule(_))),
            "expected an UnsafeRule error, got {result:?}"
        );

        // DatalogProgram::new must propagate the same error rather than panic.
        let result = DatalogProgram::new(vec![unsafe_rule]);
        match &result {
            Err(ReasoningError::UnsafeRule(_)) => {}
            Ok(_) => panic!("expected DatalogProgram::new to return an UnsafeRule error, got Ok"),
            Err(other) => {
                panic!("expected DatalogProgram::new to return an UnsafeRule error, got {other:?}")
            }
        }
    }

    /// Regression: a normal safe rule (every head variable bound in the
    /// body) still returns `Ok(())`/builds successfully.
    #[test]
    fn test_is_safe_rule_accepts_safe_rule() {
        let (_ds, g, _a, p, _b, _c) = setup_abpc_store();

        let safe_rule = Rule {
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

        assert_eq!(crate::datalog::is_safe_rule(&safe_rule), Ok(()));
        assert!(DatalogProgram::new(vec![safe_rule]).is_ok());
    }

    /// Direct unit test of [`DatalogProgram::materialise_seminaive_tracked_from`]
    /// with an explicit non-zero `initial_delta_start` pointing past
    /// pre-existing facts, pinning that the delta-seeded entry point stays
    /// wired up correctly (a future refactor accidentally reverting to
    /// `initial_delta_start = 0` internally would still pass every other
    /// test here, since 0 is also correct — just slower). See
    /// [#534](https://github.com/daghovland/rdf-datalog/issues/534).
    ///
    /// Setup: fact (a, p, b) is added to the store BEFORE the program is
    /// constructed/tracked. `initial_delta_start` is then set to the quad
    /// count at that point (i.e. this old fact is excluded from delta).
    /// Then a new fact (b, p, c) is appended and materialisation is run from
    /// that `initial_delta_start`. The transitivity rule can only derive
    /// (a, p, c) by joining the NEW fact (b, p, c) — bound as the delta —
    /// against the OLD fact (a, p, b) via the full-store join for the other
    /// body atom. If delta seeding were broken (e.g. ignoring the seed and
    /// re-scanning only what's added going forward without joining against
    /// pre-existing facts), this would fail to derive (a, p, c).
    #[test]
    fn test_materialise_seminaive_tracked_from_respects_delta_start() {
        let (mut ds, g, a, p, b, c) = setup_abpc_store();

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        // Old fact, present before the tracked delta window starts.
        ds.named_graphs.add_quad(fact_ab);
        let initial_delta_start = ds.named_graphs.quad_count;

        // New fact, appended after the delta window boundary.
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_bc);

        let rule = Rule {
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
        };

        let mut program = DatalogProgram::new(vec![rule]).unwrap();
        let mut track = Vec::new();
        program
            .materialise_seminaive_tracked_from(&mut ds, &mut track, initial_delta_start)
            .unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "(a, p, c) must be derived: delta fact (b, p, c) joined against \
             pre-existing (a, p, b) via the full-store join for the other body atom"
        );
        assert!(
            track.iter().any(|(q, _)| *q == derived_ac),
            "the derivation of (a, p, c) must be recorded in the track buffer"
        );
    }
}

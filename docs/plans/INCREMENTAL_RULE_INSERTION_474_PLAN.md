# Plan: genuinely incremental rule addition (#474)

Issue: [#474](https://github.com/daghovland/rdf-datalog/issues/474), follow-up from [#390](https://github.com/daghovland/rdf-datalog/issues/390)
(`docs/plans/RUNTIME_RULESET_ENDPOINT_390_PLAN.md`, finding #2) and sibling of
[#162](https://github.com/daghovland/rdf-datalog/issues/162)/[#455](https://github.com/daghovland/rdf-datalog/issues/455)/[#459](https://github.com/daghovland/rdf-datalog/issues/459)
(`apply_rule_deletions`, the retraction-side counterpart this mirrors).

## 1. What already exists (checked before designing)

- `datalog::IncrementalReasoner` (`datalog/src/incremental.rs`) holds `programs: Vec<DatalogProgram>`,
  one per stratum, in topological order. `apply_insertions`/`apply_deletions` maintain the closure
  under base-fact changes; `apply_rule_deletions` retracts rules via an index-stable
  `disabled_rules: HashSet<usize>` on `DatalogProgram` (never physically removes — see that field's
  doc comment in `reasoner.rs` for why: removal would shift every later rule's index and silently
  corrupt other rules' `Derivation::rule_id` references recorded in `derived_from`).
- `DatalogProgram::add_rule` already exists (`reasoner.rs:145`) and correctly extends `rule_map`,
  but nothing calls it from `IncrementalReasoner` today, and it does nothing about stratification —
  a rule added this way only ever executes within whichever stratum's `DatalogProgram` it's appended
  to, evaluated whenever that program's `materialise_seminaive*` next runs.
- `DatalogProgram::materialise_seminaive_tracked_from_facts` (used by `apply_insertions`) already
  supports seeding semi-naive with an explicit delta-fact list rather than requiring the facts occupy
  a `quad_list` position — this is reusable for reactivating a previously-disabled rule (its own
  matching facts become the delta, everything else in `base` is visible as the join background).
- `RulePartitioner::new` (`stratifier.rs`) dedups the input `Vec<Rule>` via a `HashSet` and returns
  `Vec<Vec<Rule>>` in topological order via Kahn's algorithm over a dependency graph built from
  `unification::depending_rules(rules, head_pattern)` — "which rules have a body atom (positive or
  negative) unifiable with this head pattern". This is the exact primitive the conservative check
  below reuses.

## 2. The hard constraint that shapes the whole design

`Derivation::rule_id` is an index into *one specific* `DatalogProgram::rules`. `self.programs`'
position in the outer `Vec` is that program's stratum number. Moving a rule from one program to
another — which a literal "re-stratify everything and rebuild the partition" would require whenever
new rules shift stratum boundaries — means either:
- physically removing it from its old program's `rules` (shifts every later index, corrupting other
  rules' recorded `Derivation::rule_id`s — the exact bug `disabled_rules` was invented to avoid), or
- rebuilding every program from scratch (an O(existing rule count) rebuild of `rule_map`/`disabled_rules`
  is cheap, but a fresh `DatalogProgram` has an empty `derived_from`, so every fact it previously
  derived needs re-deriving — an O(base facts) `materialise_seminaive`, exactly the full-rebuild cost
  this issue exists to avoid).

So: **an existing (already-materialized) rule may never change which `self.programs` entry it lives
in.** This one invariant is what makes the rest of the design tractable, and is also what limits it:
some ruleset changes that a full rebuild *could* stratify correctly (by moving an old rule to a later
stratum) are conservatively rejected here instead. That trade-off is the "conservative but
structurally sound" approach the issue explicitly sanctions.

## 3. Design

`apply_rule_insertions(&mut self, base: &mut Datastore, new_rules: &[Rule]) -> Result<usize, ReasoningError>`:

**Step 0 — classify each requested rule** by scanning `self.programs` for a value-equal `Rule`
(mirrors `apply_rule_deletions`'s `targets` scan):
- already present and enabled → idempotent no-op, drop from further processing (same idempotency
  contract `apply_rule_deletions` already has for repeat calls).
- present but disabled (i.e. previously removed via `apply_rule_deletions`) → **reactivate**:
  `(program_index, rule_id)` goes in a `reactivate` list. The rule keeps its original stratum — no
  stratification question here at all, it's the same rule going back to exactly where it was.
- not found anywhere → **fresh**: genuinely new rule, dedup via `HashSet<Rule>` (a caller passing the
  same new rule twice, or a rule already present earlier in `new_rules`, is not an error).

If both lists are empty, return `Ok(0)` before touching anything.

**Step 1 — conservative stratifiability check (no mutation yet, so a rejection needs no rollback):**

1. Build `existing_active`: every rule in every program that either (a) is currently enabled and
   *not* being reactivated/is not itself the rule under consideration, or (b) is being reactivated
   this call (`reactivate` treats it as "about to be enabled"). Rules that stay disabled (not
   targeted by this call) are excluded — they cannot fire, so they cannot participate in any
   dependency that matters for soundness.
2. **The append-only soundness check — applies to `fresh` AND `reactivate`d rules alike.** For every
   `fresh` rule's head pattern *and* every `reactivate`d rule's head pattern, call
   `unification::depending_rules(&existing_active_excluding_self, head_pattern)`. If this returns
   *any* edge at all (positive or negative — not just negative; see the note below on why this must
   stay a strict "any edge" check rather than a negative-only one), reject with
   `Err(ReasoningError::NotStratifiable(..))`: some other existing, already-materialized, currently-
   or-about-to-be-enabled rule's body references a predicate this rule would (re)produce. Appending
   (or reactivating) it while leaving everything else exactly where it is would be unsound: if the
   edge is negative, the existing rule's already-computed `NOT`-derivations may be stale once this
   rule's output becomes available (classic stratified-negation ordering violation — this is also
   exactly the hazard reactivation must guard against: re-enabling a rule can make a `NOT`-derivation
   downstream stale in precisely the same way a fresh rule can); if positive, the existing rule
   already reached its fixpoint without seeing this rule's output, so its closure would silently
   under-derive relative to a from-scratch materialization. The only accurate fix in either case is
   placing this rule in an earlier stratum than the dependent existing rule — which this design
   cannot do without the rebuild the issue exists to avoid. Rejecting is conservative: some such
   programs might genuinely restratify to something valid with a full rebuild, but this incremental
   path does not attempt to discover that; it only accepts insertions/reactivations that are strictly
   *downstream* of everything already materialized.

   **Why "any edge", not "negative edge only":** rejecting only on negative edges and allowing
   positive ones through would still be unsound, and — critically — checking only the *direct* (one
   hop) positive edges would not even be self-consistent, because a positive edge can chain into a
   negative one transitively (new/reactivated rule A feeds existing positive-only rule B, which feeds
   an existing rule C that negates B's head — a one-hop scan from A only sees the positive A→B edge
   and misses the real hazard at B→C). Making the check reject on *any* direct edge — positive or
   negative — from an existing rule into this rule's head sidesteps needing a transitive/BFS check
   entirely: it forbids an existing rule from consuming this rule's output at all, so there is no
   downstream chain left to worry about. This is strictly more conservative than the minimum
   necessary rejection set (a purely-positive one-hop chain that never reaches a negation might well
   be stratifiable incrementally in principle) — accepted deliberately to keep the check a cheap,
   obviously-correct single pass instead of a graph search, consistent with "conservative but
   structurally sound" from the issue.
3. **Global stratifiability check (defense in depth):** run `RulePartitioner::new(combined).order_rules()`
   over `existing_active` plus `fresh` plus `reactivate`'s rules. Since step 2 already forbids any
   `existing → fresh` edge, and `reactivate`d rules are literally unchanged existing rules, this call
   is not expected to ever newly fail relative to step 2 — but it's cheap (rule-count-sized, not
   fact-count-sized) and catches any gap in the hand-rolled step-2 reasoning (e.g. a subtlety in how
   `depending_rules` handles multi-atom bodies) directly against the same stratifier `IncrementalReasoner::new`
   already trusts, rather than re-deriving its guarantees by hand. If this fails, propagate the same
   `Err`.
4. Sub-stratify `fresh` alone: `RulePartitioner::new(fresh.clone()).order_rules()` → `Vec<Vec<Rule>>`.
   This can itself fail (fresh rules cyclic through negation *among themselves*) — propagate `Err`.
   (Steps 3 and 4 overlap in what they catch for the fresh-only-cyclic case; kept both because step 3
   is "does the whole thing make sense" and step 4 is "what are the new strata", and the redundancy
   is cheap.)

**Step 2 — mutate, tracking everything for rollback:**

```
quad_start = base.named_graphs.quad_count
reactivated_disabled: Vec<(usize, usize)> = []   // for rollback
appended_program_count: usize = 0                 // for rollback
tracked: Vec<(usize /* program index in self.programs at time of run */, Vec<(Quad, Derivation)>)>
```

1. Build every fresh-stratum `DatalogProgram::new(stratum)` **first, into a local `Vec`, before any
   mutation** — `DatalogProgram::new` calls `is_safe_rule` and can fail with `Err(UnsafeRule)`; doing
   this before enabling any reactivated rule or touching `base` keeps an unsafe-rule rejection a
   clean no-mutation `Err`, exactly like the step-1 stratifiability rejections, instead of forcing a
   rollback for what is really an input-validation failure.
2. For each `(program_index, rule_id)` in `reactivate`: call `self.programs[program_index].enable_rule(rule_id)`,
   record it in `reactivated_disabled` (to re-disable on rollback). Then run
   `materialise_seminaive_tracked(base, &mut buf)` (delta start = 0) on *just that program* to bring
   it back to a full fixpoint — bounded by that one stratum's rule count × current facts, matching the
   cost `rebuild_from_base` already accepts per-program — and push `(program_index, buf)` onto
   `tracked`. **Then continue the delta forward through every later program**, exactly like
   `apply_insertions` already does across all strata: seed `delta_facts` with the quads from `buf`,
   and for each `self.programs[program_index + 1..]` call `materialise_seminaive_tracked_from_facts`
   with the accumulated delta, extending `delta_facts` with each program's own newly-tracked quads
   before moving to the next. Skipping this forward sweep would under-derive: a reactivated rule's
   output can be consumed (positively *or* negatively — the step-1 check already rejects insertions
   where an existing rule negates the reactivated rule's head via a **direct** edge, but a later
   program can still legitimately consume it positively, e.g. a fresh stratum appended by an earlier
   call) by a program at a higher index, and semi-naive only adds — nothing will later revisit that
   program on its own.
3. For each new stratum built in step 2.1 (in order), push it onto `self.programs` (bumping
   `appended_program_count`), then run `materialise_seminaive_tracked(base, &mut buf)` on it — `base`
   already contains the full existing extensional + intensional closure (including anything the
   reactivation forward-sweep in step 2.2 just added), so this is exactly "materialize forward from
   the new rules only, seeded against everything already derived" from the issue's suggested
   conservative approach. Push `(new_program_index, buf)` onto `tracked`.
4. On any `Err` from a `materialise_seminaive_tracked`/`materialise_seminaive_tracked_from_facts`
   call: roll back, in this order —
   1. `unrecord` every `(Quad, Derivation)` in every `tracked` buffer from its program's `derived_from`
      (must happen before the next step, since it needs each program's index to still be valid).
   2. `base.named_graphs.truncate_to(quad_start)`.
   3. truncate `self.programs` back to its pre-call length (drops every appended fresh-stratum program).
   4. re-`disable_rule` every entry in `reactivated_disabled` (last, mirroring `undo_rule_deletions`'s
      existing ordering).
   - return the `Err`. This restores `self` and `base` to exactly their pre-call state, matching the
     rollback contract every other `IncrementalReasoner` mutator already documents.
5. On success: return `Ok(new_quad_count - quad_start)` — total new derived (and reactivated-rule-derived)
   facts added, mirroring `apply_rule_deletions`'s "count of facts affected" return shape.

## 4. Correctness statement (for the doc comment)

`apply_rule_insertions` is **sound and complete** for the sub-class of insertions it accepts: every
fact newly present in `base` after a successful call is one that a from-scratch
`IncrementalReasoner::new(all_rules_including_new, base)` would also derive, and no fact that
from-scratch construction would derive is missing. This holds because the accepted insertions are
exactly those where every `fresh`/reactivated rule's dependencies on existing predicates are
"downstream only" (§3 step 2) — i.e. inserting them can never invalidate a fixpoint already reached
by an existing rule, so re-running only the new/reactivated rules against the full existing closure
reaches the same joint fixpoint a full rebuild would.

It is **conservative**, not complete over all stratifiable combined programs: a `new_rules` batch
that is genuinely stratifiable only via reordering (an existing rule needs to be pushed to a later
stratum than a new one) is rejected with `Err(ReasoningError::NotStratifiable)` even though a full
`IncrementalReasoner::new` rebuild over the combined ruleset would succeed. Callers that hit this can
always fall back to a full rebuild (as `sparql_endpoint`'s current `POST /{dataset}/rules` already
does) — this method is a fast path for the common case (new rules consuming existing predicates,
never producing predicates existing rules already consume), not a universal replacement for it.

## 5. Test plan (TDD, in `datalog/src/incremental.rs`'s existing `#[cfg(test)] mod tests`)

All initially `#[ignore]`d, unignored one at a time during implementation:

1. `test_apply_rule_insertions_no_interaction` — add a rule for an entirely disjoint predicate;
   asserts its own derivation appears and pre-existing derivations are untouched (byte-for-byte
   unaffected — same facts, same count).
2. `test_apply_rule_insertions_consumes_existing_derived_fact` — existing stratum-0 rule derives `P`
   from a base fact (not itself an EDB fact); insert a **new** rule whose body positively matches `P`
   (not the base predicate) to derive `Q`. Asserts `Q` appears — proves the new rule sees intensional
   (derived-by-another-rule) facts, not just the raw base facts, satisfying the issue's explicit
   "interacts with existing derived facts from stratum > 0" requirement.
3. `test_apply_rule_insertions_rejects_when_existing_rule_negates_new_predicate` — existing rule
   `X :- P(x,y), NOT Q(x,y)`; insert a new rule that can produce `Q`. Asserts `Err(NotStratifiable)`,
   and — critically — that the reasoner's pre-call state is fully intact afterwards (a query that
   worked before the rejected call still returns the same results; the new rule was not partially
   wired in).
4. `test_apply_rule_insertions_rejects_when_existing_rule_positively_consumes_new_predicate` — existing
   transitivity rule on `p`; insert a new alias rule `{ ?x p2 ?z } => { ?x p ?z }` (i.e. the new rule's
   head, `p`, is exactly the predicate the *existing* rule's body positively consumes — the direction
   that must be rejected, mirroring `test_delete_base_fact_keeps_multiply_derived`'s alias-rule shape
   but as an insertion). Asserts `Err(NotStratifiable)` even though no negation is involved anywhere —
   this is the test that pins down "any edge, not just negative edges" from §3 step 2, and the one
   that determines whether the follow-up `sparql_endpoint` wiring issue can use this method as-is for
   arbitrary new rules or only for a restricted subclass.
5. `test_apply_rule_insertions_reactivates_previously_deleted_rule` — construct with a rule, call
   `apply_rule_deletions` to retract it (and its facts), then `apply_rule_insertions` with the exact
   same `Rule` value. Asserts the previously-removed facts are re-derived, and that this went through
   the reactivation path (not a duplicate/fresh program) via `programs.len()` staying the same (no new
   stratum appended) as an implementation-detail assertion analogous to `fallback_count` in the
   deletion tests. (Not `materialise_call_count()` — reactivation *does* re-run materialisation on the
   owning program, so that counter changes; `programs.len()` is the signal that actually distinguishes
   "reactivated in place" from "appended as a fresh stratum".)
6. `test_apply_rule_insertions_reactivation_rejects_stale_negation` — regression test for the
   reactivation-specific hazard found during design review: stratum 0 rule `P(x) :- A(x)`; stratum 1
   rule `R(x) :- NOT P(x)`. Retract `P :- A` via `apply_rule_deletions` (which correctly derives `R`
   once `NOT P` starts succeeding); then attempt `apply_rule_insertions([P :- A])`. Must return
   `Err(NotStratifiable)` (the existing `R` rule negates `P`'s head, caught by the same §3 step 2
   check applied to reactivated rules) rather than silently re-enabling `P` and leaving the now-stale
   `R` fact behind (over-derivation) — proving reactivation goes through the identical safety check as
   fresh insertion, not a shortcut that skips it.
7. `test_apply_rule_insertions_empty_is_noop` — `apply_rule_insertions(&mut base, &[])` returns
   `Ok(0)`, mirrors `apply_rule_deletions`'s `rules.is_empty()` short-circuit.
8. `test_apply_rule_insertions_duplicate_of_enabled_rule_is_noop` — inserting a rule that already
   exists and is enabled is a no-op, `Ok(0)`, idempotent on repeat calls.

## 6. Non-goals / deferred

- Re-wiring `sparql_endpoint`'s `POST /{dataset}/rules` (currently a full `IncrementalReasoner::new`
  rebuild, see `docs/plans/RUNTIME_RULESET_ENDPOINT_390_PLAN.md`) to use this new method — tracked as
  a separate follow-up issue, filed unlabeled per repo convention, referencing this issue and its PR.
- Handling the "genuinely needs reordering" case (§4) via an actual rebuild-with-reuse strategy (e.g.
  detecting which specific existing programs would need to move and only rebuilding those) — not
  attempted here; the conservative rejection is the documented, accepted behavior for now.

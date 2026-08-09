# Plan: incremental TBox (axiom/rule) retraction (#162)

Epic/issue: [#162](https://github.com/daghovland/rdf-datalog/issues/162), follow-up from [#83](https://github.com/daghovland/rdf-datalog/issues/83) (ABox incremental maintenance, `datalog::IncrementalReasoner`).

## 1. What the derivation-tracking model already has

Checked `datalog/src/types.rs` and `datalog/src/reasoner.rs` before assuming anything was missing:

- `Derivation` (`datalog/src/types.rs:142`) **already** records `rule_id: usize` — "index into `DatalogProgram::rules` of the rule that produced this fact" — alongside `body_witnesses: Vec<Quad>`. This is populated on every `record()` call in `DatalogProgram::materialise_one_iteration_tracked` (`datalog/src/reasoner.rs:261-268`).
- So rule-provenance tracking is **not missing** — the issue's "check before assuming this data is already there" concern turns out not to apply. No new field needs to be added to `Derivation`.
- What's missing: (a) any way to remove a rule from a `DatalogProgram`/`IncrementalReasoner` without corrupting existing `rule_id` indices, (b) a backward-phase seed that starts from "derivation used this rule_id" instead of "derivation witnessed this quad", (c) `Ontology::remove_axiom`, (d) a public per-axiom axiom→rules mapping to feed the removed rules into (b).

## 2. Key constraint: rule_id stability

`Derivation::rule_id` is a plain index into `DatalogProgram::rules: Vec<Rule>`. If we physically `Vec::remove` a rule, every rule after it shifts down one index, silently corrupting every *other* rule's existing `derived_from` entries (they'd now point at the wrong rule). So rule removal must **not** shift indices.

Fix: add `disabled_rules: HashSet<usize>` to `DatalogProgram`. "Removing" a rule means adding its index to this set — the rule stays in `self.rules` at the same position (so old `Derivation.rule_id` values remain meaningful for backward-phase lookups) but:
- `get_rules_for_fact` filters out `PartialRule`s whose `rule_id` is disabled, so the rule can never fire again.
- `get_facts` (body-less/fact rules) skips disabled indices too.

`disable_rule`/`enable_rule` (the latter needed for rollback on a `Contradiction` during re-derivation) are `pub(crate)` on `DatalogProgram` — internal to the `datalog` crate, called from `incremental.rs`.

## 3. API shape

- `owl_ontology::Ontology::remove_axiom(&mut self, axiom: &Axiom) -> bool` — removes the first `Axiom` value-equal to `axiom` from `self.axioms` (linear scan + `Vec::remove`; `Axiom: PartialEq`). Returns whether anything was removed. By-value equality (not an index/id) matches how `Ontology` is already construct-only/value-oriented, and matches how `IncrementalReasoner::apply_deletions` already takes `&[Quad]` by value rather than an id scheme.
- `owl2rl2datalog`: the existing per-axiom translator `owl_axiom2datalog` (private, `owl2rl2datalog/src/lib.rs:285`) becomes `pub fn axiom2datalog(resources: &mut GraphElementManager, axiom: &Axiom) -> Vec<Rule>` (rename + `pub`, thin wrapper kept private-name-compatible by just making it pub) — this is exactly the "existing per-axiom translation" the issue points at, so removing an axiom maps to removing the same `Rule`s that adding it would have produced, without going through `owl2datalog`'s whole-ontology `sort_unstable()+dedup()` (which reorders and doesn't preserve a stable per-axiom slice).
- `datalog::IncrementalReasoner::apply_rule_deletions(&mut self, base: &mut Datastore, rules: &[Rule]) -> Result<usize, ReasoningError>` — the ABox-shaped counterpart to `apply_deletions`. Takes `Rule` values (not ids): callers get `Rule`s from `axiom2datalog`, so a value-based API avoids inventing a `RuleId` type that would leak `DatalogProgram`'s internal per-stratum indexing across the crate boundary. Internally resolves each `Rule` to every `(program_index, rule_id)` occurrence across strata (a rule can only appear in one stratum in practice, but scanning all is cheap and correct either way), skipping any already-disabled index (idempotent re-calls are a no-op for already-removed rules).

  **Precondition (documented on the method, mirroring the existing precondition block on `rebuild_from_base`): `rules` must be genuinely dead** — i.e. no longer produced by *any* surviving axiom. `owl2rl2datalog::owl2datalog` deduplicates (`rules.sort_unstable(); rules.dedup()`) before returning, so one compiled `Rule` can be shared by several axioms (e.g. two axioms that both compile to the same RDFS-style subsumption rule after normalisation). Passing a still-justified rule here disables it outright with no way back — the forward phase only re-derives via *surviving, enabled* rules, so facts justified by a different axiom sharing that rule are permanently and wrongly lost. The caller (typically whoever wires `Ontology::remove_axiom` + `axiom2datalog` together) is responsible for computing `axiom2datalog(removed_axiom)` minus whatever the *remaining* ontology's `owl2datalog(...)` still produces, and passing only that difference. This mirrors exactly how the issue phrases fact-level survival ("still derivable via another surviving rule **or another axiom implying the same rule**") — the "another axiom implying the same rule" half is a caller-side dedup concern, not something `apply_rule_deletions` can detect from a bare `Rule` value alone (it has no notion of "axiom" at all).

## 4. Backward-phase extension

Existing `backward_phase(deletes: &[Quad])` in `incremental.rs` builds a reverse witness index (`witness_quad -> derived quads that used it`) once, then BFS-propagates from the deleted **base** facts (which are never themselves added to the PD set — they're not derived).

For rule removal the seed is different in kind: it's derived facts themselves (every derived quad with at least one `Derivation` whose `rule_id` matches a removed `(program, rule_id)` pair), and those seed quads *do* belong in PD (they're being retracted, not just propagated through). Plan:
- Extract the reverse-index construction into a shared `build_reverse_index(&self) -> HashMap<Quad, Vec<Quad>>` helper, used by both phases.
- Add `backward_phase_from_rule_removal(&self, targets: &[(usize, usize)]) -> HashSet<Quad>`: seed = every derived quad with a matching `rule_id` in its `derived_from` entries (inserted into PD directly, then BFS-propagated onward exactly like the existing phase).

This deliberately does **not** try to be clever about "does this quad have another surviving derivation, so maybe it doesn't need touching at all" — it mirrors the existing fact-deletion algorithm's philosophy: PD is a conservative "possibly no longer derivable" superset, everything in it is wiped from the store *and* its derivation index, and the forward phase's full semi-naive re-run re-derives anything still provable via a surviving rule/path. This is exactly how `test_delete_base_fact_keeps_multiply_derived` already proves survival at the fact level — the rule-level test mirrors it.

## 5. Forward phase + rollback

Reuses the shape of `forward_phase`/`undo_deletions`, with one addition: rules are disabled **before** the forward/re-derivation pass (so they can't refire during it), and if re-derivation produces a `Contradiction`, rollback must also re-enable them (`undo_deletions` doesn't need this since fact-deletion never touches `disabled_rules`). New `forward_phase_rules`/`undo_rule_deletions` mirror the existing pair with that one extra step. `full_rematerialise_rules` mirrors `full_rematerialise`, disabling the targeted rules up front (index-stable, so consistent with the fast-path representation) before wiping and rebuilding the whole closure — this is the `FALLBACK_THRESHOLD` (25%) guard already used for fact deletion, applied to `pd.len() / total_derived`.

## 5b. Other documentation obligations (advisor review)

- `axiom2datalog(resources: &mut GraphElementManager, ...)` must be called with the **same** `GraphElementManager` that compiled the reasoner's live rule set. Different managers intern IRIs to different numeric IDs, so a `Rule` computed against a fresh/different manager will not equal any rule already in `IncrementalReasoner`'s programs — `apply_rule_deletions` would then find zero targets and silently return `Ok(0)` instead of erroring. Document this sharpest footgun explicitly on `axiom2datalog`'s doc comment.
- `Ontology::remove_axiom` only searches/removes from `self.axioms` — the built-in declarations synthesized by `all_axioms()`/`built_in_declarations()` (owl:Thing, owl:Nothing, top/bottom properties, datatypes, ...) are not stored in `self.axioms` and can never be removed this way; `remove_axiom` returns `false` for one of those. Document on the method.
- `IncrementalReasoner::rebuild_from_base`'s existing doc block gets one line added: since it re-runs `materialise_seminaive`, which now respects each program's `disabled_rules`, a previously-retracted rule **stays** retracted across a rebuild (this is the desired/correct behavior, but it's a behavior change to an existing documented method's contract worth calling out explicitly).

## 6. Test plan (TDD, mirrors `incremental.rs`'s existing module)

All in `datalog/src/incremental.rs`'s `#[cfg(test)] mod tests`, initially `#[ignore]`d:

1. `test_remove_rule_removes_uniquely_derived_facts` — single rule (transitivity), one derivation path; removing the rule removes the derived fact, base facts survive.
2. `test_remove_rule_keeps_multiply_derived_fact` — mirrors `test_delete_base_fact_keeps_multiply_derived`: a fact derivable via two independent rules; removing one rule leaves the fact intact (re-derived via the surviving rule).
3. `test_remove_rule_no_op_for_unknown_rule` — removing a `Rule` value that was never part of the program is a no-op (`Ok(0)`), doesn't panic.
4. `test_remove_rule_shared_by_two_axioms_survives_if_one_remains` — pins the precondition from §3: two distinct source axioms compile (dedup'd) to the *same* `Rule`; the caller correctly computes "genuinely dead rules" as empty (since the rule is still produced by the surviving axiom) and calls `apply_rule_deletions(&[])`, i.e. a no-op — asserting the facts survive untouched. This is the test that makes "another axiom implying the same rule" real rather than assumed, per advisor review.
5. `test_remove_rule_large_removal_falls_back_to_full_rematerialise` — **note:** grepped for an existing dedicated fallback test for fact deletion (`FALLBACK_THRESHOLD`/`full_rematerialise`) and found none — nothing to mirror 1:1, this is a new test. Also note (per advisor review): for rule deletion, PD *includes* the seed (the directly-disabled-rule's own derived facts), unlike fact deletion where PD excludes the deleted base facts themselves — so the PD/total ratio runs systematically higher for rule removal and the fallback trips sooner for a similarly-"large" change. Build the test to deliberately cross 25%. Do **not** use `materialise_call_count()` to distinguish fast vs. fallback path — checked: both `forward_phase_rules` and `full_rematerialise_rules` call materialisation exactly once per program, so the count delta is identical either way (unlike the existing contradiction-rollback tests, which compare a rollback that skips re-materialisation entirely against one that doesn't). Instead add a `#[cfg(test)] fallback_count` counter on `IncrementalReasoner` (bumped once per `full_rematerialise`/`full_rematerialise_rules` call, mirroring `materialise_calls`' existing style) and assert on that, plus assert the resulting closure is correct (survivors present, removed-rule-only facts gone).
6. Reuses `owl_ontology`/`owl2rl2datalog` layer: a smaller integration-style test (in `owl2rl2datalog` or `datalog`, wherever fits without a circular dev-dependency) exercising `Ontology::remove_axiom` + `axiom2datalog` + `apply_rule_deletions` end-to-end for one concrete axiom (e.g. `Dog SubClassOf Animal`).

## 7. Non-goals / deferred

- No SPARQL UPDATE / HTTP wiring for TBox retraction in this PR (issue doesn't ask for it — ABox's `sparql_update.rs` wiring is a separate, already-closed concern from #83). If a follow-up issue for wiring this into an ontology-edit endpoint is wanted, file it unlabeled per repo convention rather than doing it here.
- `apply_rule_deletions`'s rollback restores exact quad/derivation state on a `Contradiction` (cheap undo-log, matching `apply_deletions`'s existing contract) rather than requiring `rebuild_from_base` — this is a natural extension of the existing pattern, not new design.

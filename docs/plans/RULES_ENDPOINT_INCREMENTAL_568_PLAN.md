# Plan: `POST /{dataset}/rules` uses incremental rule insertion/deletion (#568)

Issue: [#568](https://github.com/daghovland/rdf-datalog/issues/568), follow-up from
[#474](https://github.com/daghovland/rdf-datalog/issues/474)
(`docs/plans/INCREMENTAL_RULE_INSERTION_474_PLAN.md`, §6 "non-goals") and
[#390](https://github.com/daghovland/rdf-datalog/issues/390)
(`docs/plans/RUNTIME_RULESET_ENDPOINT_390_PLAN.md`), the endpoint this optimizes.

## 1. What already exists

- `datalog::IncrementalReasoner::apply_rule_insertions` (added in #474, PR #563) — conservative,
  sound-and-complete-for-the-accepted-class insertion of new/reactivated rules without a full
  rebuild. Returns `Err(ReasoningError::NotStratifiable)` (no mutation) when an existing enabled
  rule has any body-atom edge (positive or negative) into the new rule's head.
- `datalog::IncrementalReasoner::apply_rule_deletions` (pre-existing, #162/#455/#459) — retracts
  rules and everything they derived, re-deriving anything still provable via a surviving rule.
- `sparql_endpoint`'s `POST /{dataset}/rules` handler (`sparql_endpoint/src/rules_endpoint.rs`,
  #390) currently *always* does: parse new rules → reset `store.named_graphs` to extensional-only
  → `IncrementalReasoner::new` from scratch. Correct, but O(base facts) on every call regardless
  of how small the ruleset delta is.
- `IncrementalReasoner` had **no accessor** for its current active rule set — needed to diff
  old vs. new rulesets. Added: `IncrementalReasoner::active_rules(&self) -> Vec<Rule>`, which
  walks `self.programs` and clones every rule that is not currently disabled (in program/stratum
  order; order doesn't matter to the diff, which treats both sides as sets).

## 2. Diff algorithm

Given `old_rules = reasoner.active_rules()` and `new_rules` (freshly parsed from the request
body), rules are compared for value equality (`Rule: PartialEq`):

- `added = new_rules - old_rules` (rules in the new set not present in the old set)
- `removed = old_rules - new_rules` (rules in the old set not present in the new set)
- rules present in both are left completely alone (not re-inserted, not re-deleted) — this is
  what makes an unrelated single-rule edit cheap: only the actual delta touches the reasoner.

This is a pure, HashSet-based function (`diff_rulesets`), unit-tested directly with no HTTP or
`Datastore` involved.

## 3. Dispatch / fallback contract

Core logic lives in a standalone function (not the axum handler) so it can be unit-tested without
spinning up a server:

```
fn apply_ruleset_diff(
    reasoner: &mut IncrementalReasoner,
    store: &mut Datastore,
    new_rules: &[Rule],
) -> RulesetUpdateOutcome
```

Sequencing, per the issue's guidance ("diff first, then apply deletions before insertions"):

1. Compute `(added, removed)` via `diff_rulesets(&reasoner.active_rules(), new_rules)`.
2. If both empty: no-op fast path, `rebuilt: false`.
3. Otherwise, attempt the incremental path directly against the live `reasoner`/`store`:
   - if `removed` is non-empty, call `apply_rule_deletions(store, &removed)`
   - if that succeeded and `added` is non-empty, call `apply_rule_insertions(store, &added)`
   - any `Err` from either call aborts the incremental attempt.
4. **On any `Err`** from step 3 — this is deliberately *any* `ReasoningError` variant, not just
   `NotStratifiable`: `NotStratifiable` is the documented common case from #474, but treating every
   incremental-path error as "fall back to full rebuild" is strictly safer and simpler than trying
   to special-case which errors are "recoverable" — the endpoint's only correctness obligation is
   "the resulting reasoner matches the requested ruleset", which the full-rebuild path always
   satisfies regardless of what partial mess an aborted incremental attempt left behind:
   - the full-rebuild fallback extracts `store`'s **extensional-only** quads
     (`extensional_quads()`) — this is unaffected by whatever the aborted incremental attempt did,
     since `apply_rule_deletions`/`apply_rule_insertions` only ever add/remove *derived*
     (intensional) quads, never extensional ones — resets `store.named_graphs` to just those, and
     calls `IncrementalReasoner::new(new_rules.to_vec(), store)` fresh.
   - the entire old `reasoner` object (including whatever partial state the aborted attempt left
     it in) is discarded and replaced, exactly like the endpoint's pre-#568 unconditional
     behavior. `rebuilt: true`.
5. **On success** (step 3 completed without error): `rebuilt: false`. `store`/`reasoner` are left
   exactly as the incremental calls left them; no separate rebuild.
6. The HTTP handler wraps this: reasoner-doesn't-exist-yet (`entry.reasoner` is `None`) and
   empty-`new_rules` ("unload") keep their pre-#568 behavior unconditionally — there is nothing to
   diff against in the first case, and unload is not an "add these rules" request in the second, so
   neither goes through `apply_ruleset_diff` at all.

`RulesetUpdateOutcome { rebuilt: bool }` is purely an internal/test-observability type — the HTTP
response shape (`{"rules_loaded": N}`, status codes) is unchanged; `rebuilt` is not surfaced to
callers, only used by unit tests asserting which path actually ran.

## 4. Mixed add+remove in one request

Handled uniformly by the diff: `removed` and `added` are computed once, deletions are attempted
before insertions (freeing up predicate-dependency edges that a same-request removal might create
room for before the corresponding insertion is attempted), and any failure in either half falls
back to the same full-rebuild path. No separate "mixed" code path — it's the general case that
"pure addition" and "pure removal" are just the two special cases of (one of `added`/`removed`
being empty).

## 5. Test plan (TDD)

All initially `#[ignore]`d in `sparql_endpoint/src/rules_endpoint.rs`'s new `#[cfg(test)] mod
tests` (unit-level, constructing `IncrementalReasoner`/`Datastore` directly — no HTTP needed for
dispatch-path assertions), unignored one at a time during implementation:

1. `test_diff_rulesets_pure_addition` / `test_diff_rulesets_pure_removal` /
   `test_diff_rulesets_mixed` / `test_diff_rulesets_unchanged_is_empty` — pure `diff_rulesets` unit
   tests, no reasoner involved.
2. `test_apply_ruleset_diff_pure_addition_is_incremental` — start from a reasoner with rule A,
   request `[A, B]` where B doesn't interact with A; asserts `rebuilt == false` and that B's
   derived fact is present.
3. `test_apply_ruleset_diff_pure_removal_is_incremental` — start from `[A, B]`, request `[A]`;
   asserts `rebuilt == false` and B's derived facts are gone, A's remain.
4. `test_apply_ruleset_diff_mixed_add_remove_is_incremental` — start from `[A, B]`, request
   `[A, C]` (B removed, C added, no interaction between B and C); asserts `rebuilt == false` and
   correct facts.
5. `test_apply_ruleset_diff_falls_back_on_not_stratifiable` — start from a reasoner with rule `R1: X :- P, NOT Q`;
   request adding a rule that derives `Q` (a same-request addition `apply_rule_insertions` must
   reject per #474 step-2's "any edge" check). Asserts `rebuilt == true` **and** that the resulting
   reasoner still has the fully-correct combined ruleset applied (proving the caller-visible
   contract from the issue: "add these rules" still succeeds via fallback, not surfaced as a hard
   failure).
6. `test_apply_ruleset_diff_unchanged_is_noop` — request the exact same ruleset again; asserts
   `rebuilt == false` and no facts change.

Existing HTTP-level tests in `sparql_endpoint/tests/runtime_ruleset.rs` are left unmodified (they
assert on response shape/observable facts, not on which internal path ran) and must continue to
pass — this is the plan's regression safety net for the parts `apply_ruleset_diff`'s unit tests
don't cover end-to-end (HTTP status codes, content-type validation, dataset isolation, read-only
mode, parse-error-leaves-dataset-untouched).

## 6. Non-goals / deferred

- [#473](https://github.com/daghovland/rdf-datalog/issues/473) (per-ruleset-scoped add/delete) is
  orthogonal — this issue only changes *how* a full-ruleset-replace request is executed
  internally, not the request/response contract.
- Falling back more surgically (e.g. only rebuilding the specific strata that would need
  reordering, rather than a full rebuild) is `IncrementalReasoner`'s own documented non-goal from
  #474 §6, not something this HTTP-layer change attempts to improve on.

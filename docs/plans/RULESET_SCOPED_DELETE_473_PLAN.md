# Per-ruleset-scoped DELETE (#473)

Related: [#473](https://github.com/daghovland/rdf-datalog/issues/473) (this issue),
[#390](https://github.com/daghovland/rdf-datalog/issues/390) (full-replace `POST /{dataset}/rules`,
the endpoint this extends), [#568](https://github.com/daghovland/rdf-datalog/issues/568)
(diff-driven incremental apply this reuses).

Branch: `feat/473-ruleset-scoped-delete`.

## Scope

Adds, alongside the existing full-replace `POST /{dataset}/rules`:

- `POST /{dataset}/rules/{ruleset-id}` — load/replace *one named ruleset's* rules,
  leaving other named rulesets' rules (and the plain/unnamed ruleset from the
  no-id `POST /{dataset}/rules` path, if any) untouched.
- `DELETE /{dataset}/rules/{ruleset-id}` — retract exactly that named ruleset's
  rules, re-deriving anything still provable via a surviving named ruleset (or
  the same rule appearing in more than one named ruleset).

## Design

`DatasetEntry` (`sparql_endpoint/src/registry.rs`) gains a new field:

```rust
pub rulesets: Arc<RwLock<HashMap<String, Vec<Rule>>>>
```

mapping ruleset id -> the exact rules last loaded under that id. This is
*separate* bookkeeping from the single combined `IncrementalReasoner`
(`entry.reasoner`) that actually holds the live, materialised ruleset — the
map exists purely so a `DELETE {id}` (or a `POST {id}` that replaces an
existing id's rules) knows which rules are safe to retract from the live
reasoner without disturbing a rule that also belongs to a *different* still-live
ruleset id.

Both new handlers work by computing what the *live combined ruleset* should
become, then reusing the existing `apply_ruleset_diff` (from #568) to get
there via the smallest sound path (incremental add/remove, falling back to a
full rebuild on `NotStratifiable`/contradiction):

- `POST {id}`: `new_combined = union(all other ids' rules) ∪ (rules parsed from this request's body)`.
  Store `rulesets[id] = parsed rules` (or remove the entry if the body is
  empty). Then `apply_ruleset_diff(reasoner, store, &new_combined)`
  (lazily creating a reasoner via the existing `full_rebuild` path if the
  dataset had none yet).
- `DELETE {id}`: 404 if `id` isn't present in `rulesets`. Otherwise
  `new_combined = union(all *other* ids' rules)`, remove `rulesets[id]`, then
  `apply_ruleset_diff(reasoner, store, &new_combined)` — this is exactly why
  a rule shared between two named rulesets survives deletion of one of them:
  it's still in `new_combined` via the surviving id, so the diff against the
  live reasoner sees no change for that rule.

Rule identity for the union/diff is by `Rule`'s existing `PartialEq`/`Hash`
(the same value-equality `diff_rulesets` in #568 already relies on), so two
rulesets containing the textually-identical rule collapse to one entry in
`new_combined` — matching the issue's explicit dedup requirement.

### Interaction with the existing unnamed/no-id `POST /{dataset}/rules`

The no-id path keeps its existing full-replace-everything semantics
unchanged (backward compat, per the issue). Because a full replace by
definition discards *all* previously-loaded rules regardless of source, it
also clears `rulesets` — any named ruleset that was loaded before an
unnamed full replace no longer exists as a distinct, independently-retractable
entity afterward (its rules may or may not still be part of the new flat
ruleset, but there's no longer an id to `DELETE` them by). This is called out
explicitly in a code comment and covered by a test
(`test_plain_post_rules_clears_named_rulesets`).

### Response shapes

- `POST /{dataset}/rules/{id}` — `200 OK` `{"rules_loaded": N, "ruleset_id": id}`
  on success (same status/error codes as the no-id path: `400` parse error/bad
  content-type, `403` read-only, `404` unknown dataset, `409` contradictory).
- `DELETE /{dataset}/rules/{id}` — `200 OK` `{"rules_removed": N, "ruleset_id": id}`
  where `N` is the number of rules that *were* registered under `id` (not the
  number of facts retracted); `403` read-only; `404` unknown dataset *or*
  unknown ruleset id.

## Test plan

New integration test file `sparql_endpoint/tests/ruleset_scoped_delete.rs`,
all `#[ignore]`d first, then unignored one at a time as implemented:

1. `test_post_ruleset_id_new_dataset_creates_reasoner`
2. `test_delete_ruleset_id_removes_only_that_rulesets_derivations`
3. `test_delete_ruleset_id_shared_rule_survives_if_other_ruleset_has_it`
4. `test_delete_nonexistent_ruleset_id_404`
5. `test_delete_ruleset_id_nonexistent_dataset_404`
6. `test_delete_ruleset_id_read_only_403`
7. `test_post_ruleset_id_replaces_only_that_id_leaving_others`
8. `test_plain_post_rules_clears_named_rulesets`

## Non-goals

- Persisting the ruleset-id map across a restart — in-memory only, same as
  the rest of the runtime ruleset state (#390's own deferral).
- A listing endpoint (`GET /{dataset}/rules`) for currently-loaded ruleset
  ids — not asked for by #473; left for a future issue if wanted.

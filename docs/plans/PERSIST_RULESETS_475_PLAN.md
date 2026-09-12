# Persist runtime-loaded rulesets across restart (#475)

Related: [#475](https://github.com/daghovland/rdf-datalog/issues/475) (this issue),
[#390](https://github.com/daghovland/rdf-datalog/issues/390) (runtime ruleset endpoint),
[#568](https://github.com/daghovland/rdf-datalog/issues/568) (incremental apply),
[#473](https://github.com/daghovland/rdf-datalog/issues/473) (per-ruleset-scoped delete).

Branch: `feat/475-persist-rulesets`.

## Prior art / provenance checked

`provenance/summaries/pr-470.ttl`, `pr-622.ttl`, `pr-641.ttl` (the three merged PRs
that built the in-memory ruleset model this issue persists) were read in full before
writing this plan. Key facts reused from them:

- `sparql_endpoint/src/registry.rs`'s `DatasetEntry { store, reasoner, rulesets }` —
  `rulesets: RulesetMap` is the id → exact-rules bookkeeping (#473); the live combined
  ruleset actually materialised lives only inside the `IncrementalReasoner`, not as a
  separately-stored rule list.
- `rules_endpoint.rs`'s three handlers (`dataset_rules_post`, `dataset_rules_id_post`,
  `dataset_rules_id_delete`) all funnel through `apply_combined_ruleset`, which is the
  single choke point that actually mutates `entry.reasoner` — the natural place to also
  durably log the operation.
- The existing `--data-dir` changelog (`sparql_endpoint/src/persistence.rs`) is **not
  dataset-aware today**: `LogEntry` carries no dataset name, `QuadChangelog` is a single
  file/table for the whole server, and only the default `"ds"` dataset's `Datastore` is
  ever replayed at startup (`serve_on_listener` replays into the single `store` passed
  in; datasets created later via `POST /$/datasets` are not reconstructed at all after a
  restart). This is a pre-existing gap outside this issue's scope — this plan follows
  the existing pattern (dataset name is still recorded in the new log entries, for
  forward-compatibility, but only entries whose dataset exists in the registry at
  startup — i.e., today, only `"ds"` — are actually replayed).

## Design

### 1. New `LogEntry` variants (`persistence.rs`)

Extend the existing `LogEntry` enum (already the general-purpose durable-log entry
type, not quad-specific despite the table's name) with three new variants recording
each rules-endpoint mutation **by raw request text**, not by parsed `Rule`/resource-id
values:

```rust
pub enum LogEntry {
    ClearGraph { .. },
    InsertQuad { .. },
    DeleteQuad { .. },
    /// POST /{dataset}/rules (#390/#568) — full ruleset replace.
    ReplaceRuleset { dataset: String, rules_text: String },
    /// POST /{dataset}/rules/{id} (#473) — named/scoped ruleset load or replace.
    SetNamedRuleset { dataset: String, ruleset_id: String, rules_text: String },
    /// DELETE /{dataset}/rules/{id} (#473) — named ruleset retraction.
    DeleteNamedRuleset { dataset: String, ruleset_id: String },
}
```

**Why raw text, not `Vec<Rule>`:** `Rule`'s `QuadPattern`s hold `Term::Resource(GraphElementId)`
— interned ids that are **not stable** across a restart (a fresh `GraphElementManager`
assigns ids in whatever order data happens to be re-loaded). This is exactly why the
existing quad log already uses the portable `ElementRepr` (string-based) rather than raw
ids. The equivalent portable representation for rules is simply the original Datalog
source text: `datalog_parser::parse` already interns IRIs into whatever store it's given
as it parses, so replaying is just "call parse again against the replayed store" — no
new serialisation code needed for `Rule` itself.

All three variants land in the *same* `QUAD_LOG` table/sequence as the quad entries
(not a separate table), so their relative order versus each other (and, incidentally,
versus quad mutations) is preserved exactly as it happened — required for correct
replay of the id-scoped bookkeeping (e.g. set id "a", delete id "a", set id "a" again
must replay in that order).

### 2. When to log: after success, not before

The quad-mutation path (`sparql_update.rs`) logs *before* applying to the in-memory
store, but only ever logs an already-fully-validated operation (parsing/preparation
happens first and returns a `Result`; only the `Ok` path reaches the log-then-apply
step) — so "log before apply" there really means "log before the in-memory mutation,
after validation".

The ruleset endpoints don't have a clean validate/apply split: `datalog_parser::parse`
itself interns new IRIs into the store as a side effect (harmless — matches the
existing "a failed parse may leave harmless interned IRIs behind" contract), and
whether the *combined* ruleset is even acceptable (not contradictory) is only known
after `apply_combined_ruleset` returns `Ok`. So: **log immediately after
`apply_combined_ruleset` succeeds**, before building the 200 response (and, for the
id-scoped handlers, before/alongside updating the `rulesets` bookkeeping map — which is
itself also only updated on success today). This guarantees the log never contains an
entry for an operation that didn't actually take effect live, at the cost of a narrow
window: if the changelog `append` itself fails (disk error) *after* a successful
in-memory apply, the in-memory state is now ahead of the durable log for that one
operation (returned as a `500` to the caller, but the live server keeps running with
the new ruleset in memory until the next restart, at which point that specific change
is lost). This mirrors the same category of risk already accepted elsewhere in this
codebase for any log-write failure that races a successful in-memory mutation; it is
not made worse by this change.

### 3. Failure modes considered

- **Partial write / crash mid-append**: identical to the existing quad log — `redb`
  commits are atomic and fsync on `commit()`; a crash before commit leaves the previous
  state, a crash after leaves the new entry durably recorded. No new risk.
- **Missing changelog file** (no `--data-dir`): `state.changelog` is `None`; the
  handlers simply skip logging, exactly like every other write path today. Runtime
  ruleset changes remain in-memory-only, matching the issue's stated scope ("only
  relevant for `--data-dir` deployments").
- **Corrupt/malformed log entry on replay** (disk corruption, not a normal code path):
  a `serde_json` deserialize failure on *any* entry (quad or ruleset) already propagates
  as a hard `Result::Err` from `replay_into`/the new ruleset-entry reader, which
  `serve_on_listener` turns into a startup failure (`std::io::Error`) — the server
  refuses to start rather than silently serving incomplete/wrong data. This is
  unchanged/consistent with existing quad-log behaviour.
- **A replayed ruleset op fails to re-apply** (parse error, or `apply_combined_ruleset`
  returns a contradiction): should not happen in the common case (it succeeded live to
  get logged in the first place), but replay runs `apply_combined_ruleset` against the
  *final*, fully-quad-replayed extensional data — a strict superset of what was present
  the first time this exact op ran live (quad replay always completes before ruleset
  replay begins, per the ordering decision below) — so a rule addition that was
  previously safe *could* now conflict with data added afterward in the same run.
  Treated as a **fatal startup error** (surfaced clearly, `std::io::Error`), exactly
  like a `--rules`/`Config::initial_rules` contradiction today — never silently
  dropped or silently mismaterialised.
- **Dataset named in a log entry no longer exists at startup** (would only happen if a
  future change actually persists `POST /$/datasets`-created datasets and then that
  dataset is later deleted before this restart, or — today — for any dataset other than
  `"ds"`, since no other dataset is ever reconstructed at startup at all): the entry is
  skipped with a `log::warn!`, not fatal. This keeps today's real behaviour (`"ds"` is
  the only dataset that ever meaningfully survives a restart) working correctly without
  this issue silently growing scope into "persist arbitrary datasets".
- **`POST /$/compact` interaction**: `QuadChangelog::compact` currently rewrites the
  *entire* table as a live-quad-only snapshot, which would silently discard any
  ruleset log entries mixed into the same table. Fixed as part of this issue:
  `compact()` now separates existing ruleset entries out before rewriting, and
  re-appends them (in original relative order) after the fresh quad snapshot, so
  compaction is a no-op with respect to ruleset persistence.

### 4. Ordering with `--rules` / `Config::initial_rules` at startup

Per the issue's own suggested resolution: `--data` loads first, the quad changelog
replays on top of that (unchanged, existing order), `Config::initial_rules` builds the
initial reasoner over that fully-replayed store (unchanged, existing order) — **then**,
if any ruleset log entries exist for a dataset that's in the registry (today: `"ds"`),
they're replayed in original order against that dataset's already-initialized entry,
using the exact same `apply_replace_ruleset` / `apply_set_named_ruleset` /
`apply_delete_named_ruleset` functions the live handlers call (extracted as `pub(crate)`
functions in `rules_endpoint.rs` so both call sites share one implementation). This
means a changelog replay can *override* a `--rules`-supplied ruleset (last write wins,
same as it did live), which matches "runtime `POST /{dataset}/rules` replaces the
current ruleset" semantics exactly — no special-casing needed.

### 5. Code changes

- `persistence.rs`: add the three `LogEntry` variants; `log_replace_ruleset`,
  `log_set_named_ruleset`, `log_delete_named_ruleset` appenders; a
  `ruleset_entries(&self) -> Result<Vec<LogEntry>, String>` reader (filters to just the
  three new variants, preserving order); update `compact()` per the failure-mode note
  above.
- `rules_endpoint.rs`: extract the "apply and update bookkeeping" bodies of the three
  handlers into `pub(crate) async fn apply_replace_ruleset(state, entry, name, rules_text) -> Result<usize, ApplyError>`-shaped
  functions (returning enough detail for both the HTTP handler and startup replay to
  build their own response/error), used by both the axum handlers and the new startup
  replay path. Handlers call the changelog-append (if `state.changelog.is_some()`)
  immediately after a successful apply.
- `lib.rs` (`serve_on_listener`): after the registry/reasoner are constructed, if a
  changelog is present, read `ruleset_entries()` and replay each against the registry,
  looking up the dataset by name (skip + warn if absent) — return a startup
  `std::io::Error` on any replay failure.

## Test plan

New test file `sparql_endpoint/tests/persist_rulesets.rs`, following the existing
`tests/persistence.rs` restart-via-`shutdown()` pattern:

1. `persist_replace_ruleset_survives_restart` — `POST /ds/rules` a ruleset, restart,
   confirm the derived facts are still present.
2. `persist_ruleset_replace_then_replace_again_survives_restart` — two full-replace
   `POST`s in sequence (second overrides first), restart, confirm only the *second*
   ruleset's derivations are present (proves op-order replay, not "last snapshot only"
   by accident of some other mechanism).
3. `persist_named_ruleset_survives_restart` — `POST /ds/rules/{id}` a named ruleset,
   restart, confirm its derivations are present.
4. `persist_named_ruleset_delete_survives_restart` — `POST` two named rulesets, `DELETE`
   one, restart, confirm the deleted one's derivations are absent and the surviving
   one's are present.
5. `persist_empty_ruleset_unload_survives_restart` — load a ruleset, then `POST` an
   empty body (unload), restart, confirm no derived facts and the reasoner is gone
   (base facts still queryable).
6. `persist_ruleset_without_data_dir_is_memory_only` — start non-persistent, `POST`
   rules, confirm no file is written to a tempdir passed only for the assertion (mirrors
   `persist_default_off_creates_no_files`).
7. `persist_compact_preserves_ruleset` — load a ruleset, `POST /$/compact`, restart,
   confirm the ruleset's derivations are still present (regression test for the
   compact-discards-ruleset-entries failure mode above).

All written `#[ignore]`d first (red — the log entries don't exist yet, so an assertion
that data survives a restart will fail), then unignored one at a time as implemented.

## Non-goals / explicit deferrals

- Persisting datasets other than `"ds"` across restart at all — pre-existing gap in
  `--data-dir` support unrelated to rulesets, out of scope here. Filed as a follow-up
  issue if not already covered.
- Genuinely incremental rule-addition inside `IncrementalReasoner` — unchanged from
  #390/#568, not touched by this issue.

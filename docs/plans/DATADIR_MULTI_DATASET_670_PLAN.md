# `--data-dir` persistence for every dataset, not just `"ds"` (#670)

Follow-up from [#475](https://github.com/daghovland/rdf-datalog/issues/475) /
PR [#671](https://github.com/daghovland/rdf-datalog/pull/671). See
[`PERSIST_RULESETS_475_PLAN.md`](PERSIST_RULESETS_475_PLAN.md) for the
ruleset-changelog design this extends.

## Problem

`sparql_endpoint`'s durable changelog (`persistence.rs`, `QuadChangelog`) is
one `redb` table (`quad_log`) for the *whole server process*. `LogEntry`'s
quad-mutation variants (`ClearGraph`/`InsertQuad`/`DeleteQuad`) carry no
dataset identifier at all. Every mutating route — including the per-dataset
ones (`/{name}/update`, `/{name}/data`, …) — appends into the *same* shared
log, and `serve_on_listener` replays that whole log into a single `Datastore`
that becomes the `"ds"` dataset.

This is not just "other datasets aren't restored" — it is active data
corruption today: a write to `POST /foo/update` is durably logged, but on
restart it replays into `"ds"`, not `foo`. `foo` itself doesn't exist at all
after a restart (it's not even present as an empty dataset), since
`POST /$/datasets` never touches the changelog.

## Design

### 1. `LogEntry` becomes dataset-aware

Add a `dataset: String` field to `ClearGraph`, `InsertQuad`, `DeleteQuad`,
with `#[serde(default = "default_dataset_name")]` (`"ds"`) so **existing
on-disk changelogs** (written before this change, with no `dataset` field at
all) still deserialize — an untagged historical entry is exactly the entries
this codebase has only ever written for `"ds"` so far, so defaulting to
`"ds"` is not just a workaround, it is the historically-correct
interpretation.

Add two new variants:

```rust
CreateDataset { name: String },
DeleteDataset { name: String },
```

logged by `POST /$/datasets` / `DELETE /$/datasets/{name}` respectively.
These exist so an *empty* created dataset (no quads ever written to it) still
survives a restart, and so a deleted dataset does not silently reappear just
because old quad/ruleset entries for that name are still physically present
earlier in the log (append-only; nothing is deleted on `DELETE`, only marked).

### 2. `AppState.dataset_name`

`AppState` gains a `dataset_name: String` field (default `"ds"` at the
top-level `serve_on_listener` construction; set explicitly by
`dataset_routes::dataset_state()` for every per-dataset request). Every call
site that builds a `LogEntry::{ClearGraph,InsertQuad,DeleteQuad}` — currently
in `graph_store.rs`, `sparql_update.rs` (via a new `dataset: &str` parameter
on `prepare_update`), `upload.rs`, `ottr_endpoint.rs`, `rml_endpoint.rs`,
`query.rs` (`LOAD`) — stamps `state.dataset_name.clone()` (or the threaded
parameter) onto the entry. This is mechanical but must be checked file by
file: it is easy to accidentally stamp from the *root* `state` instead of the
dataset-scoped one at a call site that has both in scope (e.g.
`dataset_routes::dataset_update_post`, which holds both `state` and
`ds_state`/`entry`).

### 3. Replay

- `QuadChangelog::replay_into(&mut Datastore)` keeps its existing signature
  and behavior for `"ds"` (filters quad-mutation entries to
  `dataset == "ds"`, same result as before for a changelog that has only ever
  seen `"ds"` traffic).
- New `QuadChangelog::discover_dataset_names() -> Vec<String>`: scans
  `CreateDataset`/`DeleteDataset` entries in log order, returns the set of
  extra (non-`"ds"`) dataset names still live (created, and not subsequently
  deleted — a delete followed by a later create for the same name makes it
  live again, empty).
- New `QuadChangelog::replay_dataset(&self, name: &str) -> Result<Datastore, String>`:
  fresh `Datastore`, replays only quad-mutation entries tagged `name`,
  **but only those at or after that name's most recent `CreateDataset`**
  (so a delete+recreate does not resurrect the deleted incarnation's quads).
- `serve_on_listener`, after the existing `"ds"` `replay_into` + initial
  reasoner construction + `DatasetRegistry::new_with_default`: calls
  `discover_dataset_names()`, and for each name, `replay_dataset(name)`,
  wraps it `Arc<RwLock<_>>`, `registry.insert(name, store)`. This runs
  **before** the existing ruleset-replay loop, so that loop's
  `registry.get_entry(&dataset)` lookup (currently only ever finding `"ds"`)
  starts finding the other datasets too — no change needed to the ruleset
  replay loop itself. The same recreate-purges-old-state rule applies to
  ruleset entries for a deleted-then-recreated dataset: `ruleset_entries()`
  is filtered the same way (entries before the name's last `CreateDataset`
  are dropped), otherwise a stale ruleset would reappear on an empty
  recreated dataset.

### 4. `POST /$/datasets` / `DELETE /$/datasets/{name}`

`admin_create_dataset`: after validating the name and taking the registry
write lock, if persistent, append `LogEntry::CreateDataset` to the changelog
**before** `registry.insert(...)` — a changelog append failure must leave the
dataset absent from the registry too (matches the "log before apply" ordering
used by `sparql_update.rs`'s prepare/log/apply split, appropriate here since
create is a single atomic step with no partial-apply risk). Return 500 on
changelog failure instead of the current unconditional 200.

`admin_delete_dataset`: same ordering — append `LogEntry::DeleteDataset`
before `registry.remove(...)`.

### 5. `POST /$/compact`

`admin_compact` currently rewrites the *whole* table from a single
`Datastore` (`state.store`, always `"ds"`), which — even before this
change — silently drops any interleaved entries for other datasets (today:
misfiled `"ds"`-tagged entries that actually belonged elsewhere; after this
change: correctly-tagged entries for other datasets, still dropped). Fix:
`compact` iterates the full `DatasetRegistry`, and for every dataset writes
a `CreateDataset` marker (so an empty dataset's existence still survives
compaction) followed by one `InsertQuad` per live quad tagged with that
dataset's name, then appends the preserved ruleset entries (already
dataset-tagged, unaffected by this change) for every dataset, not just
`"ds"`. This requires `admin_compact` to take the registry lock and iterate
all entries — lock order is registry → each dataset's store (read) →
changelog, consistent with the read-before-write pattern already used
elsewhere in this crate.

## Out of scope (follow-ups, filed separately at Status `Todo`)

- `DELETE /$/datasets/ds` (deleting the default dataset itself) — not
  currently handled specially by `DatasetRegistry::remove`, and its
  interaction with `AppState.store`/`AppState.reasoner` (which alias `"ds"`
  directly, not through the registry) is undefined. Out of scope for this
  issue; filed as a follow-up if it isn't already covered by an existing
  issue.

## Tests (`sparql_endpoint/tests/persist_multi_dataset.rs`)

1. `no_cross_leak_into_ds_before_fix_regression` — write to a non-`"ds"`
   dataset, restart, assert the quad is in that dataset and **not** in `"ds"`.
2. `non_ds_dataset_data_survives_restart`
3. `empty_created_dataset_survives_restart`
4. `deleted_dataset_does_not_reappear_after_restart`
5. `delete_then_recreate_is_empty_after_restart`
6. `non_ds_dataset_ruleset_survives_restart` (exercises the existing
   ruleset-replay loop's dataset lookup once the registry is populated)
7. `compact_preserves_all_datasets`
8. Existing `persistence.rs` unit tests and `persist_rulesets.rs` integration
   tests must stay green unmodified (old-format on-disk compatibility +
   `"ds"`-only regression coverage).

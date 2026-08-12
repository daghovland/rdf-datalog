# Runtime ruleset endpoint (#390)

Related: [#390](https://github.com/daghovland/rdf-datalog/issues/390) (this issue),
[#469](https://github.com/daghovland/rdf-datalog/issues/469) (per-dataset stores have no
reasoner at all — depends on this issue), [#110](https://github.com/daghovland/rdf-datalog/issues/110)
(original incremental-reasoner epic), [#457](https://github.com/daghovland/rdf-datalog/issues/457)
(default-dataset reasoner-aliasing fix this branch is based on).

Branch: `feat/390-runtime-ruleset-endpoint`.

## Investigation findings

### 1. Current startup-only wiring (`sparql_endpoint/src/lib.rs`)

`Config::initial_rules: Vec<Rule>` is consumed exactly once, in `serve_on_listener`
(~line 373-386): if non-empty, `IncrementalReasoner::new(rules, &mut store)` runs full
initial materialisation and the result is wrapped `Arc::new(Mutex::new(reasoner))` into
`AppState.reasoner: Option<Arc<Mutex<IncrementalReasoner>>>`. There is no code path that
ever changes `AppState.reasoner` from `None` to `Some` (or replaces its rule set) after
this point — the `Option` itself is baked into every per-request clone of `AppState` at
startup.

### 2. `IncrementalReasoner`'s public API (`datalog/src/incremental.rs`)

`new`, `apply_insertions`, `apply_deletions`, `apply_rule_deletions` (existing, from
#162/#434), `rebuild_from_base`. **There is no `apply_rule_insertions`-shaped method** —
nothing that adds new rules to an already-constructed reasoner incrementally (i.e.
materialising only the new rules' consequences without touching the rest of the closure).
`apply_rule_deletions` only *removes* rules the reasoner already knows about.

Building genuine incremental rule-*addition* (stratify only the delta, splice new strata
into the existing `Vec<DatalogProgram>`, re-run semi-naive only for affected strata) is a
materially bigger lift than this issue needs — it interacts with the stratifier's
topological ordering (new rules can create new dependency edges that reorder existing
strata) and isn't required by the issue's own "even a simpler ... full replace ... would
already be useful" allowance. **This PR does not build it.**

What this PR does add: a `replace_rules` method-shaped operation (implemented as a free
function in `sparql_endpoint` for now, see below) that:
1. reads back the *extensional* (base) facts only, via `Datastore`'s existing
   `extensional_quads()` (the same call `IncrementalReasoner::rebuild_from_base` already
   uses to discard derived facts before a rebuild),
2. rebuilds a fresh `QuadTable` from just those facts (discarding the old closure),
3. calls `IncrementalReasoner::new(new_rules, &mut store)` to fully re-stratify and
   re-materialise from scratch over the new rule set.

This is a full rebuild, not incremental rule-splicing — acceptable per the issue's own
explicit fallback allowance. It reuses 100% existing `IncrementalReasoner`/`Datastore`
machinery; no changes needed inside the `datalog` crate.

### 3. `DatasetRegistry` / `dataset_state()` (`sparql_endpoint/src/dataset_routes.rs`,
   `sparql_endpoint/src/registry.rs`)

Today `DatasetRegistry` maps `name -> Arc<RwLock<Datastore>>` only — no reasoner slot per
dataset. `dataset_state()` builds a per-request `AppState` from the registry entry; its
`reasoner` field is populated only via a special case: `Arc::ptr_eq(&ds_store,
&state.store)` (i.e. this request is for the `"ds"` dataset, which is *the same*
`Arc<RwLock<Datastore>>` as `state.store`) copies `state.reasoner.clone()`; every other
dataset gets `reasoner: None`, unconditionally. This is exactly #469's complaint: a
dataset created via `POST /$/datasets` can never get a reasoner, ever, regardless of what
this PR does for `"ds"`, unless the registry itself gains a reasoner slot.

**Root cause requiring a structural change**: `AppState.reasoner` is
`Option<Arc<Mutex<IncrementalReasoner>>>` — a plain `Option`, not wrapped in any
interior-mutability cell. Since `AppState` is `Clone`d fresh per request from a value
fixed at server startup (for the root `/sparql`, `/rdf-graph-store`, `/transaction/*`
routes, which read `state.reasoner` directly, not through `dataset_state()`), there is no
way to make a *future* request see a reasoner that didn't exist at startup — the `Option`
variant itself is immutable across requests. This blocks the runtime "go from zero rules
to some rules" transition needed for (a) the default dataset when it started with no
`--rules`, and (b) *every* dataset created via the admin API (which never has a startup
reasoner).

**Chosen fix**: change `AppState.reasoner`'s type from
`Option<Arc<Mutex<IncrementalReasoner>>>` to
`Arc<tokio::sync::RwLock<Option<Arc<Mutex<IncrementalReasoner>>>>>` — an outer
interior-mutable cell around the existing `Option`. All ~15 read call sites (`graph_store.rs`
×9, `query.rs` ×2, `transaction_routes.rs` ×2, `dataset_routes.rs` ×2) change from
`if let Some(ref reasoner_arc) = state.reasoner {` to
`let reasoner_slot = state.reasoner.read().await; if let Some(ref reasoner_arc) = *reasoner_slot {`
(and `state.reasoner.is_some()` to `state.reasoner.read().await.is_some()`) — mechanical,
no behavioural change for existing callers.

`DatasetRegistry` gains a `DatasetEntry { store, reasoner }` struct (replacing the bare
`Arc<RwLock<Datastore>>` value in its internal map) where `reasoner` is the *same* cell
type as `AppState.reasoner`. `DatasetRegistry::new_with_default` takes the startup
reasoner cell and registers it under `"ds"` — the same `Arc` instance `AppState.reasoner`
holds, so the existing `Arc::ptr_eq`-based aliasing hack in `dataset_state()` is no longer
needed: `dataset_state()` can just always pull `entry.reasoner.clone()` from the registry,
for *every* dataset name including `"ds"`, and the aliasing invariant (`"ds"`'s reasoner
cell literally *is* the root state's reasoner cell) holds by construction instead of by
special-casing. This is a net simplification of `dataset_state()`, not just new code.
`DatasetRegistry::insert` (used by `POST /$/datasets`) creates a fresh
`Arc::new(RwLock::new(None))` reasoner cell for newly-created datasets — they start with
no reasoner, exactly as today, but now *can* be given one at runtime.

### 4. `datalog_parser` crate

`datalog_parser::parse(input: &str, datastore: &mut Datastore) -> Result<Vec<Rule>, String>`
interns IRIs into the given `Datastore` as it parses, so it must be called while holding
that dataset's store write lock (not before). Reused as-is for the new endpoint's request
body — no second parser.

## Scope for this PR

**`POST /{dataset}/rules`**, `Content-Type: text/x-datalog` (also accepting the more
conventional `text/plain` since that's what many HTTP clients default to for a raw text
body) — parses the body via `datalog_parser::parse`, then **replaces** the target
dataset's entire live ruleset (lazily creating a reasoner if the dataset didn't have one)
and re-materialises from the dataset's current extensional (base) facts, per the
`replace_rules` procedure in finding #2 above. Works for **both** `"ds"` (the default
dataset — whether or not it started with `--rules`) and any per-dataset store created via
`POST /$/datasets`, per finding #3's registry change. An empty body / zero parsed rules is
accepted and clears the dataset's ruleset (equivalent to "unload"), covering the issue's
`DELETE /{ruleset-id}` unload use case in the single-ruleset-per-dataset shape this PR
supports — see "Deferred" below for why a real per-ruleset-scoped `DELETE` isn't built now.

Response: `200 OK` with a small JSON body `{"rules_loaded": N}` on success; `400 Bad
Request` with the parser's error message on a parse failure (dataset's existing ruleset is
left untouched — the store write lock is held for the whole parse+rebuild, so a failed
parse never partially mutates state); `404 Not Found` if the dataset doesn't exist; `403
Forbidden` if the server is `read_only` (this mutates the store's `QuadTable` layout even
though it doesn't change the *set* of base facts, and is consistent with how every other
per-dataset write route already treats `read_only`); `409 Conflict` (reusing the same body
shape as SPARQL Update's contradiction handling) if the new rule set is contradictory over
the existing base facts.

**Deferred, not built in this PR**: `DELETE /{dataset}/rules/{ruleset-id}` — per-ruleset
*named/scoped* addition and targeted retraction (the issue's more ambitious version, e.g.
loading rule set `"a"` and rule set `"b"` independently and retracting only `"a"`). This
PR's dataset-level reasoner slot holds exactly one `IncrementalReasoner` (one combined rule
set) per dataset, matching the issue's own "even a simpler... endpoint... would already be
useful" fallback. Multi-ruleset-per-dataset tracking (a `HashMap<RulesetId, Vec<Rule>>` per
dataset, retracting one via `apply_rule_deletions` while leaving siblings materialised)
is meaningfully more state to design and test correctly (ruleset-id collision handling,
partial-overlap rule dedup across rulesets, etc.) and is left as an explicit follow-up —
file a new issue for it if/when needed rather than scope-creeping this PR.

**Does this resolve #469?** #469's complaint is "genuinely-separate per-dataset stores have
no reasoner at all and no way to be assigned one." This PR's registry change (finding #3)
gives every dataset — default or admin-API-created — a reasoner slot that starts `None`
and can be populated via `POST /{dataset}/rules`, and once populated, `/{dataset}/update`
and `/{dataset}/data` (GSP) already thread it through correctly (same code paths used for
`"ds"` today, now dataset-agnostic since the special-casing is gone). This substantially
resolves #469's core ask. What it does *not* do: give a per-dataset store a reasoner
*automatically* at creation time (`POST /$/datasets` still creates datasets with no
reasoner, by design — a dataset with no rules loaded correctly has no reasoner, same as
`"ds"` with no `--rules`). That's consistent with #469's own framing, not a gap.

## Test plan

New test file `sparql_endpoint/tests/runtime_ruleset.rs` (integration tests via the
existing `tests/common` harness):

1. `test_post_rules_new_dataset_no_prior_reasoner` — create a dataset via `POST
   /$/datasets`, insert some base triples via GSP, `POST /{name}/rules` a rule that derives
   a new predicate from them, confirm the derived triple is now queryable via
   `/{name}/sparql` (proves lazy reasoner creation for a previously-reasoner-less dataset).
2. `test_post_rules_default_dataset_with_prior_reasoner` — start the *server* with
   `Config::initial_rules` non-empty (an existing reasoner from `--rules`-equivalent
   startup config), `POST /ds/rules` (or the bare default-dataset route) with a
   *different* rule set, confirm: (a) facts only derivable under the *old* ruleset are
   gone from `/sparql` results, (b) facts derivable under the *new* ruleset are present —
   proves replace-not-merge semantics and proves `/sparql`'s own root-route reasoner
   threading (not going through `dataset_state()`) picks up the swap.
3. `test_post_rules_dataset_isolation` — two datasets, each with distinct base facts;
   `POST` a ruleset to dataset A only; confirm dataset B's query results are completely
   unaffected (no reasoner appears on B, its facts are unchanged) — proves the registry
   change didn't leak reasoner state across datasets.
4. `test_post_rules_empty_body_clears_ruleset` — dataset with a reasoner already loaded and
   derived facts present; `POST` an empty/zero-rule body; confirm derived facts are gone
   and base facts remain (unload semantics).
5. `test_post_rules_parse_error_leaves_dataset_untouched` — dataset with an existing
   ruleset and derived facts; `POST` syntactically-invalid Datalog; confirm `400`, and that
   a subsequent query shows the *old* ruleset's derived facts still present (parse failure
   doesn't partially mutate state).
6. `test_post_rules_nonexistent_dataset` — `POST /{missing}/rules` → `404`.
7. `test_post_rules_read_only_server` — server started with `read_only: true`, `POST
   /{name}/rules` → `403`.

All written `#[ignore]`d first (red), then unignored one at a time as implemented (green),
per this repo's TDD convention.

## Non-goals / explicit deferrals

- Per-ruleset-scoped add/delete with an id (`DELETE /{dataset}/rules/{ruleset-id}`) — see
  "Deferred" above.
- Genuinely incremental rule-*addition* inside `IncrementalReasoner` (materialising only a
  new rule's consequences without a full rebuild) — see finding #2. `replace_rules` is a
  full rebuild; still O(base facts), not O(whole server), and only runs on an explicit
  admin-style write, not on the query hot path.
- Persisting the runtime ruleset across a restart (`--rules`/`Config::initial_rules`
  remains the only *startup*-time ruleset source; a `POST /{dataset}/rules` call is
  in-memory only for now, same as everything else in a non-`--data-dir` deployment). Could
  be extended to the changelog in a follow-up if durability is wanted.

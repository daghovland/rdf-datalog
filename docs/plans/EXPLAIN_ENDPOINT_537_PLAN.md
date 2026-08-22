# EXPLAIN / query-profiling endpoint — design (#537)

Split out of #533 (performance report). Related: join-reordering epic #35,
#507 (benchmark cyclic/star BGPs).

## Problem

No EXPLAIN or query-profiling endpoint exists in `sparql_endpoint` or
`sparql_parser`. `sparql_parser/src/join_ordering.rs` already reorders BGP
triple patterns by estimated selectivity (`order_patterns`,
`join_ordering.rs:27`), and `component_ordering.rs` reorders top-level
`QueryComponent`s (conjuncts vs. `UNION`), but there is no way for a caller
to see the resulting plan or per-operator timing short of instrumenting the
code by hand, as had to be done for #533's investigation.

## Decision 1 — Trigger mechanism: `?explain=true` on the existing endpoints, JSON response that bypasses content negotiation

The issue suggested either `?explain=true` on `GET`/`POST /sparql`, or a
dedicated route. I read `sparql_endpoint/src/query.rs` before deciding:

- `sparql_get_with_state` (`query.rs:54`) and `sparql_post_with_state`
  (`query.rs:98`) both funnel into a single `run_select_query`
  (`query.rs:551`), which is also reused, unmodified, by the per-dataset
  routes `dataset_sparql_get`/`dataset_sparql_post`
  (`dataset_routes.rs:70,83`, which call `query::sparql_get_with_state`/
  `sparql_post_with_state` directly). So a change inside
  `run_select_query` automatically covers `/sparql`, `/{name}/sparql`, and
  `/{name}/query` for free — a dedicated route would need to be duplicated
  (or plumbed through) across all three.
- `run_select_query` already branches on `QueryResult` (`Ask`/`Select`/
  `Construct`/`Describe`) and, for `Ask`/`Select`, on `negotiate_select_format`
  (`negotiate.rs`) to choose SPARQL-XML / CSV / SPARQL-JSON. An EXPLAIN
  report is structurally nothing like any of those three (it's a plan tree,
  not a result table/boolean), so feeding it through
  `negotiate_select_format` would either require inventing a fourth
  "format" that every match arm in `negotiate.rs` and `query.rs` has to
  special-case, or silently producing bogus SPARQL-XML/CSV for a plan.
  Neither is worth it for a debugging-oriented feature.

Decision: `run_select_query` checks the `explain` query parameter **before**
executing the query pipeline; if truthy (`"true"`/`"1"`, case-insensitive —
matching the loose-boolean convention already used elsewhere in this
codebase, e.g. `read_only` config parsing), it calls a new
`explain_query_response` helper that returns `application/json` directly,
**ignoring the `Accept` header entirely**. This is a deliberate, documented
deviation from SPARQL 1.1 Protocol content negotiation: EXPLAIN is not part
of that protocol, so there is no compliance obligation to negotiate it, and
a fixed JSON shape keeps the client contract simple ("append `?explain=true`
and you get JSON back, always"). `GET`/`POST` both plumb the param the same
way `txId` already does (`params.get("txId")` at `query.rs:76,160`), so no
new extraction machinery is needed. A dedicated `/explain` route was
rejected: it would need its own copy of the query-string/body parsing that
`sparql_get_with_state`/`sparql_post_with_state` already do, for a feature
that is really "the same query, evaluated in a different mode", not a
distinct resource.

## Decision 2 — Report contents: reuse `join_ordering`'s existing computations, don't recompute

Read `sparql_parser/src/join_ordering.rs` and `sparql_parser/src/execute/bgp.rs`
before designing the report shape:

- `order_patterns(patterns, already_bound, datastore)` (`join_ordering.rs:27`)
  already computes the exact permutation `eval_bgp` (`execute/bgp.rs:25`)
  uses. The EXPLAIN report calls this same function — it does not
  reimplement or approximate the ordering logic.
- `known_cardinality(tp, datastore)` (`join_ordering.rs:116`) already computes
  the exact index lookup `order_patterns`'s cost ranking uses, via one of
  seven cases on which of {subject, predicate, object} are constant genuinely
  registered in the datastore (`resolve_constant`, `join_ordering.rs:103`).
  EXPLAIN surfaces *which* of those seven cases fired (i.e. "which index was
  used") without a second, drift-prone copy of the match: `known_cardinality`
  is refactored into a thin wrapper around a new private
  `cardinality_and_index(tp, datastore) -> (usize, IndexUsed)`, which holds
  the exact match arms `known_cardinality` had (same hot-path behavior,
  same signature for every existing caller) and additionally returns a
  fieldless `IndexUsed` enum naming which arm fired. `known_cardinality`
  becomes `cardinality_and_index(tp, datastore).0`; EXPLAIN reads `.1`. This
  is not by inspecting `datalog`/`execute` internals, since index selection
  for a BGP triple
  pattern is entirely decided in `join_ordering.rs`/`bgp.rs`, not in
  `datalog/src/datalog.rs` (that crate's `evaluate_pattern` is the Datalog
  *rule* evaluator, a separate code path from SPARQL BGP matching — SPARQL
  patterns go through `Datastore::quads_matching_limited`, see
  `execute/bgp.rs:199`, which the `QuadTable` indexes described in
  `join_ordering.rs`'s doc comments back directly).
- The report is a static plan, computed by walking `Query`/`QueryComponent`
  without executing anything: a new `sparql_parser::explain` module recurses
  over `where_clause: &[QueryComponent]` exactly the way `eval_components`
  (`execute/components.rs`) does structurally (same match arms: `BGP`,
  `Optional`, `Union`, `Minus`, `Graph`, `Group`, `Filter`, `Bind`, `Values`,
  `PathPattern`, `Subquery`, `Service`), but instead of evaluating each
  arm, it produces a plan node describing it. For `BGP`, the plan node is
  the `order_patterns` permutation plus, per pattern (in chosen order): the
  pattern rendered as text, its estimated cardinality
  (`known_cardinality`), and its index description.

  **Known limitation, documented in the report itself and in code comments**:
  this walk uses `already_bound = ∅` at every BGP, since the real
  already-bound set at execution time depends on what upstream *rows* (not
  just upstream *components*) bound, which is a per-solution runtime fact,
  not a static property of the query tree. A future refinement could thread
  a conservative static over-approximation (e.g. "every variable that any
  earlier sibling component could ever bind", mirroring
  `component_ordering::variables_in_components`) through the walk; that's
  deferred to a follow-up (filed below) rather than built here, since it
  changes the *reported* order only in cases where a BGP is preceded by a
  sibling that provably binds one of its variables, which the empty-set
  approximation handles conservatively (never worse than reporting BGP-only
  selectivity) but not always exactly.

  **Component-level reordering must also be mirrored, not just BGP-level.**
  `eval_components_budgeted` (`execute/components.rs:58`) performs two purely
  *static* transformations before evaluating any component list: it
  stable-partitions `Filter` components to the end (lines 78–86, 124), and,
  when `component_ordering::should_reorder(&non_filters)` (line 89) is true
  (i.e. the list contains a `UNION`/`OPTIONAL`/`MINUS`), replaces the
  evaluation order with `component_ordering::order_components(&non_filters,
  &already_bound, &guaranteed_bound, datastore)` — Phase C of the
  join-reordering epic (#35/#173), which hoists a constraining conjunct
  ahead of a `UNION`/`OPTIONAL`/`MINUS` it shares variables with. At the
  query's top level (and at the start of every independently-evaluated
  scope — `UNION` arms, bare `Group` bodies, `MINUS`'s RHS, all of which
  begin from `vec![HashMap::new()]` via `eval_independent_then_join`/
  the `Minus` arm) both `already_bound` and `guaranteed_bound` are
  genuinely `∅`, so this reordering is statically reproducible exactly, with
  no approximation — the walk must call the same two functions with the
  same empty sets, not just print components in source order. This is not
  optional polish: per PR #173's provenance summary, #533's actual reported
  pathology was exactly this class of problem (a `UNION` evaluated before a
  constraining conjunct), so an EXPLAIN report that silently prints source
  order would misrepresent the one failure mode this endpoint most needs to
  surface. `OPTIONAL` bodies are the one case that isn't reproducible this
  way (their inner components are seeded per-row with `sub.clone()`,
  `execute/components.rs`'s `Optional` arm) — walked with the same `∅`
  approximation as the BGP case above, for the same reason.

- **Timing**: entirely separate from the static plan above, and zero-cost
  when `explain` isn't requested — it does not touch `eval_bgp`/
  `eval_components`/`eval_component`'s hot-path signatures at all (no new
  parameter threaded through ~10 mutually-recursive functions, no
  `Option<&mut Sink>` checked on every quad). Instead,
  `explain_query_response` wraps one `Instant::now()`/`.elapsed()` pair
  around the single top-level `execute_with_base` call it already makes for
  a normal query, and separately reuses the same
  `sparql_parser::explain` static-plan walk. This gives total
  wall-clock query time (a real, meaningful number — this is what #533
  actually needed: "which operators dominate runtime" starts with "is the
  whole query slow at all") without any per-operator instrumentation
  risking a perf regression on the un-explained hot path. Per-operator
  (per-BGP, per-triple-pattern) timing is *not* implemented in this PR —
  filed as a follow-up (see below) since it requires deciding how to thread
  an optional timing sink through the recursive evaluator without
  regressing the hot path, which is a bigger design question than this
  issue's scope.

## Decision 3 — Format: small JSON structure

Plain JSON (no `serde` dependency added to `sparql_parser`, which currently
has none — see `sparql_parser/Cargo.toml`): the new `explain` module returns
plain Rust structs/enums, and `sparql_endpoint` (which already depends on
`serde_json`, see `sparql_endpoint/Cargo.toml:23`) converts them to a
`serde_json::Value` via `serde_json::json!(...)` in the endpoint layer. This
avoids adding a new dependency to a parser/executor crate for what is purely
a presentation concern, matching this crate's existing pattern (e.g.
`sparql_json`/`sparql_xml` serialisers already live in `sparql_endpoint`,
not `sparql_parser`).

Report shape (illustrative; see `sparql_parser/src/explain.rs` for the
authoritative struct definitions):

```json
{
  "queryType": "Select",
  "totalTimeMs": 1.234,
  "rowCount": 3,
  "plan": [
    {
      "kind": "BGP",
      "patterns": [
        { "position": 0, "pattern": "?x <http://ex/p1> ?y", "estimatedCardinality": 1, "indexUsed": "predicate" },
        { "position": 1, "pattern": "?y <http://ex/p2> ?z", "estimatedCardinality": 5, "indexUsed": "subject_predicate" }
      ]
    }
  ]
}
```

Non-`BGP` components appear as `{"kind": "Optional"/"Union"/"Filter"/...,
"children": [...]}` nodes so the tree shape mirrors the query structure;
`Filter`/`Bind`/`Values`/`PathPattern` carry a short `detail` string
(rendered expression/path) instead of a `patterns` list.

## Smaller decisions

- **Execution error + explain.** The scenario EXPLAIN exists for (#533) is
  exactly "this query is slow/times out" — the case where the caller most
  wants the plan. So a failing execution (including the #372 cooperative
  timeout, normally a bare `503` with no body) still returns the static
  plan: `explain_query_response` computes the plan first (cheap, doesn't
  execute anything), then attempts execution; on error the JSON response
  carries the same HTTP status `query_execution_error_response` would have
  used (503 for a timeout, 500 otherwise) plus `{"plan": [...], "error":
  "...", "totalTimeMs": <elapsed before failure>}` — `rowCount` is omitted
  in the error case.
- **`txId` (transactional read) path.** `run_transactional_query`
  (`query.rs:305`) duplicates the execution logic and does not call
  `run_select_query`, so it does not see `explain` handling in this PR.
  Rather than silently ignoring the parameter (which would look like a bug
  to a caller combining the two), `explain=true` together with a `txId`
  parameter returns `400 Bad Request` with a message pointing at the
  follow-up issue tracking that combination (filed below).
- **Form-body POST.** `explain` is read from the URL query-string
  parameters only (`AxumQuery<HashMap<String, String>>`, same extraction
  `txId` already uses), not from an `application/x-www-form-urlencoded`
  request body. Documented in the handler's doc comment; a client using
  form-encoded POST must pass `?explain=true` in the URL.
- **Result-summary field varies by query type.** `rowCount` only makes
  sense for `Select`. The report uses a query-type-appropriate field
  instead of forcing one name: `rowCount` (Select), `result` (Ask, the
  boolean), `tripleCount` (Construct/Describe).
- **Same store-read guard.** `run_select_query` already holds
  `state.store.read().await` across both plan computation and execution
  (it's one `store` binding used throughout), so cardinalities/timing in
  an EXPLAIN response describe a single, consistent store generation —
  no separate read-lock acquisition needed for the plan step.

## Scope explicitly deferred (filed as follow-up issues)

- Per-operator/per-stage timing (see Decision 2 above).
- Using a non-empty, conservatively-computed `already_bound` set when
  walking sibling components for the static plan (see Decision 2's "Known
  limitation").
- `explain=true` combined with `txId` (transactional reads) — see "Smaller
  decisions" above.

Filed as GitHub issues (unlabeled, Status Todo, project #11) at the point
they were identified, per this repo's CLAUDE.md:
[#572](https://github.com/daghovland/rdf-datalog/issues/572) (per-operator
timing), [#573](https://github.com/daghovland/rdf-datalog/issues/573)
(conservative static already-bound set), and
[#574](https://github.com/daghovland/rdf-datalog/issues/574) (`explain` +
`txId`).

## Test plan

`sparql_endpoint/tests/explain_endpoint.rs` (new file, following the
existing convention of one behavior-focused integration test file per
feature area, e.g. `query_builder_sparql.rs`):

1. Single-pattern query's explain output: one-pattern BGP, assert the JSON
   has `plan[0].kind == "BGP"`, one entry in `patterns`, correct rendered
   pattern text.
2. Multi-pattern BGP: assert `patterns` appear in the `order_patterns`-chosen
   order (least-selective-last), matching the existing
   `join_ordering.rs` unit tests' fixtures/expectations so the two don't
   silently drift apart.
3. Component-level reordering (`test_explain_hoists_constraining_bgp_before_union`):
   a `UNION` written *before* a constraining BGP that shares its variable
   must be reported *after* it in the plan, mirroring
   `component_ordering::order_components`'s
   `moves_constraining_bgp_before_union` fixture. This is the specific
   class of pathology #533 actually reported (a component-level ordering
   issue, not a BGP-internal one — see Decision 2's "Component-level
   reordering must also be mirrored" note), so it gets its own test rather
   than being assumed to follow from tests 1–2.
4. Normal (non-`explain`) query behavior completely unaffected: same
   `sparql-results+json` body and status for an identical query with and
   without running through the `explain`-aware code path (i.e. issuing the
   same query both with and without `?explain=true` and diffing the
   non-explain response against a request made before this change existed —
   in practice, asserting the ordinary path still returns
   `application/sparql-results+json` and the same rows).

All three start `#[ignore]`d per this repo's TDD workflow and are
unignored one at a time during implementation.

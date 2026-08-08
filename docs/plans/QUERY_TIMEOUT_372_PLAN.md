# Plan: cooperative query-timeout cancellation in the SPARQL evaluator

Issue: [#372](https://github.com/daghovland/rdf-datalog/issues/372) (query-timeout half of the issue; the write-route concurrency-limit half stays deferred, per #367's original reasoning — not in scope here)

## Why this is bigger than a typical fix

`Config::max_query_timeout_secs` is dead code — nothing enforces it.
Investigation before writing anything found that a naive `tokio::time::timeout`
around query execution **would not work**: `sparql_parser::execute_with_base`
is a fully synchronous, non-yielding call, so wrapping it in an async timeout
future does nothing — the synchronous call blocks the polling task until it
returns regardless of any wrapping timeout, since there's no `.await` point
inside it for the timeout race to preempt. Real enforcement requires either
(a) `spawn_blocking` + an owned `Datastore` clone per query (a real perf
cost, and the option NOT chosen — see the issue's decision discussion), or
(b) cooperative cancellation: the evaluator itself periodically checks an
absolute deadline and aborts. This plan implements (b).

## Where the unbounded work actually happens

`sparql_parser/src/execute.rs` (~5200 lines) has two structurally distinct
sources of potentially-large/slow work reachable from a single query:

1. **The `eval_components`/`eval_components_budgeted`/`eval_component`
   recursive chain** — the main BGP/`OPTIONAL`/`UNION`/`MINUS`/`GRAPH`/
   subquery join evaluation. This already threads an `Option<usize>`
   "budget" (solution-count limit for `OFFSET`/`LIMIT` pushdown) through
   exactly this call graph — the deadline should be threaded the same way,
   alongside it.
2. **`eval_path_pattern` / `transitive_closure`** — property-path evaluation
   (`*`, `+`, sequences, alternatives). `transitive_closure`'s BFS
   (`while let Some(current) = queue.pop() { ... }`) already has a
   `visited: HashSet` cycle guard so it terminates even on cyclic graphs,
   but it can still do a large amount of work on a big/dense graph before
   terminating — this is the classic "runaway SPARQL query" vector (a
   transitive-closure property path with no useful bound) and needs its own
   deadline check independent of the components chain above, since it's a
   separate recursive call graph, not called through `eval_component`.

Both need deadline checks. Everything else (expression evaluation,
arithmetic, casts, string functions) is O(1) per solution row and not
itself an iteration source — no deadline check needed there.

## Design

1. Add a small `Deadline` type (e.g. in `execute.rs` or a new small module)
   wrapping `Option<std::time::Instant>` (an absolute deadline; `None` means
   "no timeout configured" — the common case, and must be a true no-op: no
   `Instant::now()` calls at all when `None`, to avoid a syscall-per-check
   regression for the default unconfigured case). A cheap `fn check(&self)
   -> Result<(), QueryTimeoutError>` method.
2. Introduce `QueryTimeoutError` (or extend whatever error type
   `execute`/`execute_with_base` already use — check their current
   `Result<QueryResult, String>` signature; a dedicated error variant is
   cleaner than stringly-typed errors if there's an existing enum to extend,
   otherwise a formatted string consistent with existing error messages is
   acceptable — match the codebase's existing convention here rather than
   introducing a new pattern gratuitously).
3. Thread `&Deadline` as an additional parameter through the loop-bearing
   functions only: `eval_components`, `eval_components_budgeted`,
   `eval_component`, `eval_independent_then_join`, `eval_bgp`,
   `eval_triple_pattern`/`eval_triple_pattern_core` (wherever the actual
   per-candidate-match loop lives), `eval_path_pattern`,
   `transitive_closure`. Do NOT thread it into the O(1) expression-evaluation
   functions (`eval_expression_value`, `eval_function_value`, arithmetic/cast
   helpers, etc.) — out of scope, no iteration there to bound.
4. Check the deadline at natural loop-iteration boundaries: once per outer
   solution row in `eval_component`'s per-row loops (`OPTIONAL`/`UNION`/
   `MINUS`/subquery/`GRAPH`), once per candidate in `eval_bgp`'s matching
   loop, and inside `transitive_closure`'s BFS `while` loop (check every
   iteration, or every N pops if profiling suggests per-iteration overhead
   matters — start with every iteration for correctness, only batch the
   check if `cargo test --workspace --release`'s existing large-ontology
   smoke tests in `tests/performance.rs` show a measurable regression).
5. `execute_inner`/`execute`/`execute_with_base` (the public entry points)
   need a new parameter (or an addition to an existing options/context
   struct, if refactoring the parameter list is getting unwieldy — use
   judgment) carrying the configured timeout `Duration` (or `None`),
   constructing the initial `Deadline` once at the top and threading it down.
6. Update `sparql_endpoint`'s two `execute_with_base` call sites
   (`sparql_endpoint/src/query.rs`, `run_select_query` and
   `run_transactional_query`) to pass `Some(Duration::from_secs(state.config.max_query_timeout_secs))`
   (or `None` if the config value is `0`/a sentinel for "disabled" — check
   `Config::max_query_timeout_secs`'s doc comment and default value for
   whether `0` means "no timeout" or is just an unusually short one; treat
   `0` as "disabled" if that's the existing convention elsewhere in this
   config struct, otherwise keep it literal). On a `QueryTimeoutError`,
   return an appropriate HTTP status (`503 Service Unavailable` or `500`
   with a clear message — check the SPARQL 1.1 Protocol spec section already
   referenced elsewhere in this codebase, `docs/architecture/PROTOCOLS.md`,
   for any guidance on timeout responses; if none, `503` with a clear body
   message is a reasonable default; don't invent a new pattern not already
   used by nearby error handling in the same file).
7. Update every other caller of `execute`/`execute_with_base`/the now-changed
   internal functions across the workspace (search broadly — tests in
   `tests/sparql12_suite.rs`, `tests/w3c_sparql11_suite.rs`, and any
   `sparql_parser`-internal test modules that call these functions directly)
   to pass `None`/no-timeout, preserving existing behavior exactly for every
   test that doesn't care about this feature.

## Tests (TDD)

- Unit test(s) for `Deadline::check` in isolation (already-expired deadline
  returns `Err`, not-yet-expired returns `Ok`, `None` deadline always `Ok`).
- An integration test constructing a query specifically designed to take
  measurable wall-clock time (e.g. a transitive-closure property path over a
  synthetic graph large enough to take, say, >100ms, or a Cartesian-product
  BGP over enough triples) with a very short configured timeout (e.g.
  `Duration::from_millis(1)`), asserting `execute_with_base` returns
  `Err(QueryTimeoutError)` rather than completing or hanging. Needs to be
  written carefully to not be flaky on a fast CI runner — prefer a
  synthetic dataset sized to reliably exceed a very small timeout rather
  than relying on real-world timing variance.
- Regression: confirm `cargo test --workspace` (the FULL existing suite,
  including `tests/sparql12_suite.rs` and `tests/w3c_sparql11_suite.rs`)
  passes unchanged with no timeout configured (`None` everywhere it's not
  explicitly tested) — this is the primary correctness bar, since this
  change touches the evaluator's hottest, most heavily-tested code paths.
  Do not skip or spot-check this — run the whole thing.
- A test confirming an ordinary query with a *generous* timeout still
  produces byte-identical results to before this change (no accidental
  behavior change from threading the extra parameter through).

## Out of scope

- The write-route concurrency-limit half of #372 — left exactly as #367
  originally deferred it (a follow-up issue if it turns out to matter).
- `spawn_blocking`-based hard cancellation — cooperative checks bound *new*
  work from starting, not work already in flight inside a single
  already-started loop iteration between checks; this is an accepted
  trade-off of the cooperative-cancellation approach, not a bug to fix here.
- Deadline-checking inside O(1) per-row expression evaluation — no
  meaningful iteration there to bound.

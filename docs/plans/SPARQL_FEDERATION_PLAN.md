# SPARQL SERVICE Federation Plan

Implementing actual remote-endpoint forwarding for `SERVICE <endpoint> { ... }`
under `NetworkPolicy::Allow`/`AllowList`, per the [SPARQL 1.1 Federated
Query](https://www.w3.org/TR/sparql11-federated-query/) spec.

---

## Current state

`SERVICE` is fully parsed (`QueryComponent::Service(endpoint, inner, silent)`)
and the [`ingress::NetworkPolicy`](../../ingress/src/network_policy.rs) framework
(epic [#117](https://github.com/daghovland/rdf-datalog/issues/117), closed)
already routes it correctly for `Deny`/`Ignore`:

| Policy | `SERVICE` behaviour |
|---|---|
| `Deny` (default) | non-`SILENT` → error; `SILENT` → empty results |
| `Ignore` | always empty results |
| `Allow` / `AllowList` | non-`SILENT` → `ExecError::ServiceNotImplemented`; **no actual forwarding happens** |

The sibling `Allow`-mode operations from the same epic — `LOAD <url>`
([#119](https://github.com/daghovland/rdf-datalog/issues/119)) and JSON-LD
`@import` ([#82](https://github.com/daghovland/rdf-datalog/issues/82)) — are
both fully implemented, including SSRF hardening
([#135](https://github.com/daghovland/rdf-datalog/issues/135): private-IP
blocking, redirect policy, response-body size cap). `SERVICE` is the one
remaining stub.

**The immediate problem that triggered this plan:** `ExecError`'s
`ServiceDenied`/`ServiceNotImplemented` messages (`sparql_parser/src/error.rs`)
and `NetworkPolicy::Allow`'s doc comment (`ingress/src/network_policy.rs`)
all point users at <https://github.com/daghovland/rdf-datalog/issues/51> —
which is **closed** (it tracked the short-term fix: turning the old silent
empty-result footgun into an explicit error, not the federation
implementation itself). Anyone who follows that link today lands on a closed
issue with no forward pointer, which looks like the project forgot about its
own TODO. Every reference needs to move to the new tracking issue(s) below as
part of this work.

---

## Design

### 1. Serializing the sub-pattern back to SPARQL text

`QueryComponent::Service`'s inner `Vec<QueryComponent>` (the `{ ... }` body)
needs to become a `SELECT * WHERE { ... }` query string to send to the remote
endpoint. There is currently no AST→SPARQL-text serializer in `sparql_parser`
(`explain.rs`'s `render_triple_pattern` only renders single triple patterns
for warning/EXPLAIN messages, not a full nested `QueryComponent` tree with
`FILTER`/`OPTIONAL`/`UNION`/property paths). This needs a real (if minimal)
serializer: round-tripping the inner components is enough — it doesn't need
to reproduce the user's original formatting, just something `parse_query`
elsewhere would accept straight back.

Project the query as `SELECT <vars actually used downstream> WHERE { ... }`
rather than `SELECT *`, once bind-join pushdown (step 5) exists — until then,
`SELECT *` keeps scope minimal.

### 2. Sending the request and getting results back

Reuse the resolver/SSRF-hardening pattern `sparql_endpoint::sparql_update`'s
`fetch_rdf_with_resolver` already applies to `LOAD` (private-IP blocking,
redirect policy, body-size cap, [#135](https://github.com/daghovland/rdf-datalog/issues/135))
rather than inventing a second HTTP-safety path. `SERVICE` differs from
`LOAD` in two ways that the shared helper needs to account for:

- **Method/body**: SPARQL federation sends the query as `POST` with
  `Content-Type: application/sparql-query` (or `application/x-www-form-urlencoded`
  with a `query=` parameter — either is spec-legal; `POST` with the query as
  the raw body is simplest to implement first), not a bare `GET`.
- **Accept header**: `application/sparql-results+json` (and optionally
  `application/sparql-results+xml` as a fallback), not an RDF serialization.

### 3. Parsing SPARQL results JSON back into bindings

`sparql_endpoint/src/serialize/sparql_json.rs` only *writes* the
[SPARQL 1.1 Query Results JSON Format](https://www.w3.org/TR/sparql11-results-json/)
today — there's no reader. A new deserializer is needed: JSON `results.bindings`
→ `Vec<PartialSub>` (or whatever the executor's internal solution-row type is;
check `sparql_parser::execute`'s `PartialSub`/`SolutionRow`). This is a clean,
independently-testable unit (parse a results-JSON fixture → assert the
solution rows), and the natural first sub-issue to land since everything else
depends on it existing.

### 4. The sync/async boundary

`sparql_parser::execute`/`execute_with_base` are fully synchronous — called
both from the CLI (`src/main.rs`, no tokio runtime) and from
`sparql_endpoint`'s async axum handlers. Doing a blocking HTTP call deep
inside that synchronous call tree would either stall the async runtime (if
called from an async handler without `spawn_blocking`) or need a second,
blocking HTTP client just for this path.

**Recommended approach: pre-resolve, don't inline.** Before calling the
synchronous executor, walk the top-level `WHERE` clause (reusing
`cyclic_bgp::collect_bgps`-style recursive descent, or a dedicated walk) to
find every non-`SILENT` `SERVICE` clause, resolve them all *concurrently* via
async HTTP from the caller's async context, and hand the executor a
pre-fetched `HashMap<ServiceKey, Vec<PartialSub>>` (or an `Err` per failed
fetch, handled per step 6) instead of letting `eval_components`'s `Service`
match arm do I/O inline. This mirrors how `LOAD`'s fetch already happens in
the async endpoint layer before the synchronous update-application step, and
means `execute`/`execute_with_base`'s public signature only grows an
additional optional pre-resolved-results parameter — no `async fn` virus
spreading through the whole evaluator.

The CLI path (`src/main.rs`) doesn't currently run ad hoc interactive SPARQL
queries against `NetworkPolicy::Allow` in a context where this matters much;
confirm this during implementation and, if the CLI does need it, give it a
small `tokio::runtime::Runtime::block_on` wrapper at that one call site rather
than restructuring the executor.

### 5. Joining results into the existing solution set

Standard SPARQL federation semantics: each binding returned by the remote
endpoint is joined (natural join on shared variable names) against the
solutions accumulated so far from the rest of the query, exactly like any
other `QueryComponent` already is in `eval_components`. No new join logic
should be needed here beyond wiring `Service`'s match arm to consult the
pre-resolved map from step 4 instead of returning `Vec::new()`/erroring.

### 6. Error / `SILENT` handling

- Connection failure, timeout, non-2xx response, or a malformed results body:
  - `SILENT` → treat as zero rows (already-correct existing behavior for the
    "not implemented" and `Ignore`-policy cases — just route the new failure
    modes through the same path).
  - non-`SILENT` → a new `ExecError` variant (e.g. `ServiceRequestFailed { endpoint, detail }`)
    rather than overloading `ServiceNotImplemented`, since the operation *is*
    implemented now and this is a genuine runtime failure worth distinguishing.
- Respect the query's existing [`Deadline`](../../sparql_parser/src/deadline.rs)
  budget for the remote HTTP call too — a hung remote endpoint shouldn't be
  able to make a federated query ignore `--timeout`.

### 7. Bind-join pushdown (explicitly deferred, not in initial scope)

The SPARQL 1.1 spec allows (but doesn't require) pushing already-bound
variables from the outer query into the `SERVICE` sub-query via an injected
`VALUES` clause, which can turn an otherwise-unconstrained remote scan into a
targeted lookup. Valuable for real-world federation performance, but it's a
genuine optimization layered on top of steps 1-6 working at all — track as an
explicit follow-up issue rather than bundling into the initial epic, so the
epic isn't blocked on getting the pushdown logic right too.

---

## Suggested issue breakdown

1. **Epic**: SPARQL SERVICE federation — implement real remote-endpoint
   forwarding under `NetworkPolicy::Allow`/`AllowList` (top-level, this plan).
2. SPARQL results JSON deserializer (`application/sparql-results+json` →
   solution bindings) — self-contained, no dependency on the rest.
3. Minimal `QueryComponent`→SPARQL-text serializer for a `SERVICE` sub-pattern.
4. Async pre-resolution pass (step 4) + wiring `Service`'s executor match arm
   to consult pre-fetched results, reusing the `LOAD`/`#135` SSRF-hardened
   resolver for the actual HTTP call.
5. `SILENT`/error-handling/timeout-budget integration (step 6) + end-to-end
   integration tests (a second in-process mock SPARQL endpoint, mirroring how
   `sparql_endpoint/tests/load_network.rs` already mocks a remote server for
   `LOAD`).
6. Follow-up (separate from the epic's initial scope): `VALUES`-pushdown
   bind-join optimization (step 7).

Every reference to the closed issue [#51](https://github.com/daghovland/rdf-datalog/issues/51)
in `sparql_parser/src/error.rs` and `ingress/src/network_policy.rs` gets
updated to point at the new epic as part of sub-issue 4 (where
`ServiceNotImplemented`'s message text itself changes anyway).

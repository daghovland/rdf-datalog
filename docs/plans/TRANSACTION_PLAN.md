# Transaction Model Plan

Dagalog's transaction model, isolation guarantees, and the roadmap to a
multi-request HTTP transaction API.

Tracked in epic [#122](https://github.com/daghovland/rdf-datalog/issues/122).

Sub-issues:
- [#126](https://github.com/daghovland/rdf-datalog/issues/126) — Phase 0: Fix atomicity (rollback on failure) **correctness bug**
- [#123](https://github.com/daghovland/rdf-datalog/issues/123) — Phase 1: Isolation and intra-request visibility tests
- [#124](https://github.com/daghovland/rdf-datalog/issues/124) — Phase 2: ETag / `If-Match` optimistic concurrency
- [#127](https://github.com/daghovland/rdf-datalog/issues/127) — Phase 2.5: Constraint violation class (`dagalog:ConstraintViolation`)
- [#125](https://github.com/daghovland/rdf-datalog/issues/125) — Phase 3: Multi-request transaction HTTP API

---

## Current state

### Single-request atomicity (already correct)

Every SPARQL Update HTTP request is already an atomic transaction at the
storage layer.  `run_update` in `sparql_endpoint/src/query.rs` acquires an
exclusive Tokio `write()` lock on `Arc<RwLock<Datastore>>` before calling
`apply_prepared_update`, and holds it until the function returns.  The lock is
never released between individual statements in a multi-statement request.

Consequences:

| Property | Value |
|---|---|
| Concurrent reads during a write | **Blocked** — they queue until the write lock is released and always see either the pre- or post-commit state, never an intermediate state. |
| Intra-request visibility | **Visible** — raw quad mutations are pre-applied in statement order (since [#114](https://github.com/daghovland/rdf-datalog/issues/114)), so a `PatternUpdate` `WHERE` clause sees the results of preceding `INSERT DATA` / `DELETE DATA` statements in the same request, as required by SPARQL 1.1 Update §3.1.3. |
| Incremental reasoning | **Batched** — the incremental reasoner is called once per HTTP request with the full delta (since [#114](https://github.com/daghovland/rdf-datalog/issues/114)), not once per statement. |

### What is NOT tested

The isolation guarantee comes from `RwLock` mechanics and is correct, but
there are no explicit tests for it.  Missing test coverage tracked in
[#123](https://github.com/daghovland/rdf-datalog/issues/123):

1. **Concurrent read isolation**: a read request issued while a write is in
   progress must see the pre-write state (or block until post-write), never an
   intermediate state.
2. **Intra-request WHERE visibility**: a `PatternUpdate` whose `WHERE` clause
   matches triples inserted by an earlier statement in the same request must
   find them.

### Atomicity gap — no rollback on failure

RDFox guarantees: *"if an operation inside a transaction starts changing the
store but then fails in the middle, the transaction will be rolled back."*

Dagalog's `apply_prepared_update` (since [#114](https://github.com/daghovland/rdf-datalog/issues/114)) pre-applies raw quad
mutations to the **live store** during iteration, then calls the reasoner once
at the end.  If a later operation fails (e.g. `INSERT DATA { … } ; LOAD <url>`
where LOAD is rejected by `NetworkPolicy::Deny`, or a reasoner panic due to
OOM), the already-applied mutations are **not rolled back**.  The HTTP response
is a 500/403 but the store is left in a partially-modified state.

This is a real Atomicity violation.  Tracked in
[#126](https://github.com/daghovland/rdf-datalog/issues/126).

**Fix approach**: buffer all deltas without touching the live store during
iteration (except for evaluating `PatternUpdate` WHERE clauses, which need a
delta-overlay view).  Only commit the batched delta to the live store after
every operation succeeds.  If any operation fails, return `Err` with zero
store mutations.

### ETags

SELECT responses carry an `ETag` header based on `Datastore::generation` (a
`u64` bumped on every mutation).  The server does not yet enforce `If-Match`
on writes, so the ETag is informational only and does not implement optimistic
concurrency control.

---

## Transaction semantics in datalog systems

Datalog programs are classically evaluated in batch: load the full EDB, derive
the IDB, done.  For incremental/streaming systems the standard approach is to
batch updates into discrete *epochs* or *deltas* and apply them atomically
(Differential Dataflow, Nemo, RETE networks).

For HTTP-accessible triple stores the common patterns are:

| Approach | Examples | Complexity |
|---|---|---|
| Single-request atomicity | Current Dagalog, Fuseki | Already done |
| ETag + `If-Match` OCC | GraphDB (partial), REST idiom | Low — one header check |
| Explicit multi-request transactions | Stardog, GraphDB, Virtuoso | Medium — server-side transaction state |
| MVCC snapshot reads | PostgreSQL, advanced stores | High — multiple in-flight versions |

---

## Roadmap

### Phase 0 — Fix atomicity (rollback on failure) ([#126](https://github.com/daghovland/rdf-datalog/issues/126))

This is a correctness bug, not a missing feature, and should be fixed before
the multi-request transaction work.

Change `apply_prepared_update` so that it:

1. Evaluates all operations and collects `batch_inserts` / `batch_deletes`
   **without touching the live store**.  `PatternUpdate` WHERE clauses are
   evaluated against a temporary "delta-overlay" view (a thin wrapper that
   presents pending inserts as present and pending deletes as absent).
2. If any operation returns `Err`, the function returns that `Err` immediately
   with zero mutations to the live store.
3. Only after all operations succeed, apply the batched delta atomically:
   call `store.named_graphs.add_quad` / `remove_quad` for every collected
   quad, then call the reasoner once.

### Phase 1 — Test the existing guarantees ([#123](https://github.com/daghovland/rdf-datalog/issues/123))

Add tests to `sparql_endpoint/tests/isolation.rs`:

- `test_concurrent_read_sees_pre_or_post_write`: spawn a write task that
  inserts triples, race a read task, assert the read returns either 0 triples
  or the full inserted set, never a partial view.
- `test_intra_request_pattern_update_sees_preceding_insert`: single HTTP
  request with `INSERT DATA { ex:a ex:p ex:b . } ; INSERT { ex:b ex:q ex:c }
  WHERE { ex:a ex:p ?x }` — after the request, `ex:b ex:q ex:c` must be
  present.
- `test_intra_request_delete_then_insert_same_subject_where`: delete a triple
  and re-insert a related one in one request; assert correct final state.

### Phase 2 — ETag-based optimistic concurrency ([#124](https://github.com/daghovland/rdf-datalog/issues/124))

Enforce `If-Match` on SPARQL Update POST requests:

- If the client sends `If-Match: "<generation>"` and the current
  `store.generation` does not match, respond HTTP 412 Precondition Failed.
- If the client omits `If-Match`, the update proceeds unconditionally (current
  behaviour).
- Clients can implement compare-and-swap: `GET /sparql` (captures ETag), then
  `POST /sparql` with `If-Match: <etag>`.

This is a small change in `run_update` in `sparql_endpoint/src/query.rs`.

### Phase 2.5 — `owl:Nothing` constraint check ([#127](https://github.com/daghovland/rdf-datalog/issues/127))

Any transaction that causes an instance of `owl:Nothing` to exist in the
default graph after reasoning is rejected with HTTP 409 Conflict and rolled
back.  Users express constraints as datalog rules whose head derives
`?v a owl:Nothing`; if any such triple is materialized, the commit fails.

Using `owl:Nothing` rather than a custom class:
- Reuses a standard, well-understood IRI — no new vocabulary namespace needed.
- OWL-RL already derives `owl:Nothing` for disjointness violations, so those
  automatically become blocking constraints.
- Semantically accurate: `owl:Nothing` having an instance means the data is
  inconsistent, which is exactly what a constraint violation represents.

The `OWL_NOTHING` constant is already available in `ingress/src/namespaces.rs`
(or is trivially added if absent).

#### Execution order

Constraint checking is the last step of every read/write transaction:

1. All update operations execute and the delta is collected (Phase 0).
2. The delta is applied atomically to the live store.
3. The incremental reasoner runs once with the full delta.
4. The default graph is queried: `ASK { ?v a owl:Nothing }`.
5. **If any instance exists**: undo the delta (reverse `apply_deletions` /
   `apply_insertions`), return **HTTP 409 Conflict** with details.
6. **If no instances**: return HTTP 204 No Content.

Step 5 requires Phase 0's rollback mechanism.  The undo is:
- `reasoner.apply_deletions(store, &applied_inserts)` — retract derived facts
  that depended on the inserted quads.
- `reasoner.apply_insertions(store, &applied_deletes)` — re-derive facts from
  quads that were deleted (reverting the deletion).
- `store.named_graphs.remove_quad` for each applied insert.
- `store.named_graphs.add_quad` for each applied delete.

#### Error response

HTTP 409 body (plain text):

```
Transaction rejected: owl:Nothing has instances after reasoning.

Instance 1: <http://example.org/alice>
  ex:missingProperty ex:mbox
  ex:violationDescription "Every foaf:Person must have at least one foaf:mbox."

(showing 1 of 1 instance)
```

Show up to 10 instances, up to 10 properties each.

#### Example constraint rule

```turtle
# Every foaf:Person must have a foaf:mbox.
[ ?person a owl:Nothing ;
          ex:description "Missing foaf:mbox" ] :-
  [ ?person a foaf:Person ],
  NOT EXISTS ?mbox IN [ ?person foaf:mbox ?mbox ] .
```

#### Implementation locations

- `ingress/src/namespaces.rs` — ensure `OWL_NOTHING` constant exists
- `sparql_endpoint/src/query.rs::run_update` — call `check_owl_nothing` after `apply_prepared_update` returns `Ok`
- `sparql_endpoint/src/constraints.rs` (new) — `check_owl_nothing(store: &Datastore) -> Vec<InstanceInfo>`
- `sparql_endpoint/tests/constraints.rs` (new) — integration tests

#### Dependency

Requires Phase 0 ([#126](https://github.com/daghovland/rdf-datalog/issues/126)) for correct rollback on violation.

### Phase 3 — Multi-request transaction API ([#125](https://github.com/daghovland/rdf-datalog/issues/125))

A proprietary HTTP extension (outside SPARQL 1.1 Protocol scope) providing
BEGIN / COMMIT / ROLLBACK for sequences of requests that need to share a
consistent view.

#### API sketch

```
POST   /transaction/begin           → { "txId": "abc123" }
GET    /sparql?txId=abc123&query=…  → snapshot reads
POST   /sparql?txId=abc123          → buffered updates
POST   /transaction/abc123/commit   → 204 No Content
POST   /transaction/abc123/rollback → 204 No Content
```

#### Implementation approach

Each open transaction holds:
- A **snapshot generation** (the `Datastore::generation` at `begin` time).
- A **pending delta** (`Vec<Quad>` inserts + deletes) buffered in memory.

Reads within the transaction apply the pending delta on top of the current
committed store.  On commit, the delta is applied to the shared store (with a
generation check against the snapshot — effectively merging OCC with the
multi-request concept).  On rollback, the delta is discarded.

Conflicts (another commit bumped the generation since the snapshot) return HTTP
409 Conflict.

The incremental reasoner is called once per commit with the full delta, exactly
as for single-request atomicity.

#### Caveats

- Transactions are in-memory only; a server restart loses open transactions.
- Long-held write transactions block other writes (writer starvation).
  A timeout (default 60 s) should abort stale transactions.
- This feature interacts with the changelog/persistence layer
  ([#52](https://github.com/daghovland/rdf-datalog/issues/52)).

---

## References

- [SPARQL 1.1 Update §3.1.3](https://www.w3.org/TR/sparql11-update/#graphStore) — intra-request ordering semantics
- [SPARQL 1.1 Protocol](https://www.w3.org/TR/sparql11-protocol/) — no multi-request transaction extension
- Stardog transactions: `POST /db/{db}/begin`
- GraphDB transactions: `POST /repositories/{repo}/transactions`
- [#114](https://github.com/daghovland/rdf-datalog/issues/114) — batch incremental reasoning (prerequisite)
- [#52](https://github.com/daghovland/rdf-datalog/issues/52) — persistence / changelog

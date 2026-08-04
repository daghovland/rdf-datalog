# Plan: request timeout + upload write-lock scope (#367)

Branch: `fix/367-endpoint-timeout-lock`.

## Findings

- `sparql_endpoint/src/upload.rs`'s `upload_turtle` handler **already** parses the
  uploaded Turtle body into a standalone `Datastore` (`tmp`) before acquiring
  `state.store.write().await` (this structure dates to #249, well before #367 was
  filed). The audit that produced #367 was stale on this specific claim — the
  write lock is not held across the parse.
- What *is* still held under the write lock, and does real (awaited) work:
  the persistence changelog append (`changelog.lock().await` +
  `cl.append_batch(...)`, a redb disk write) runs between acquiring the store
  lock and the quad-copy loop. This is the actual "slow work under the lock" in
  this handler and will be hoisted above the `state.store.write().await` line —
  it only reads `tmp` and `graph_iri`, neither of which need the store lock.
- The quad-copy loop remains under the write lock (it's the actual mutation and
  is O(triples) — that's inherent to a merge into the shared store, not a bug).
- `sparql_endpoint`'s router (`server.rs`) has no request-level timeout and no
  concurrency limiting. This is the primary, still-valid part of #367.

## Changes

1. **Upload handler**: hoist the changelog-append block above the
   `state.store.write().await` call in `upload.rs`. No behavior change to
   write-ahead-log ordering (log still happens before the mutation is applied).
2. **Request timeout**: add `Config::request_timeout_secs: u64` (default 30,
   distinct from the existing, currently-unused `max_query_timeout_secs` field
   — see follow-up note below), wired via a new `--request-timeout` CLI flag /
   `DAGALOG_REQUEST_TIMEOUT` env var, following the same pattern as
   `--max-rdf-upload-bytes`. Add `tower_http::timeout::TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, ...)`
   to the router in `server.rs`, placed inside (before) the CORS layer so
   408 responses still carry CORS headers. This layer is response-preserving
   (`Infallible` in, `Infallible` out) — no `HandleErrorLayer`/`tower::timeout`
   needed.
   - Caveat to document in the PR: the timeout bounds *connection occupancy*
     (the client gets a 408 and the connection is freed) but the handler
     future keeps running to completion in the background — it still does the
     parse/lock/mutation. This bounds how long a slow client can occupy a
     connection slot, not the server-side work itself.
3. **Concurrency limit**: considered; deferred. The write path is already
   serialized by the single `Arc<RwLock<Datastore>>`, so a concurrency limiter
   would mainly bound in-flight memory rather than lock contention, and a
   test that deterministically proves "N+1th request queues" needs either a
   real semaphore probe or synthetic slow requests — both add flakiness risk
   disproportionate to the benefit here. Left as a follow-up (unlabeled issue,
   per the `ready`-label gate) rather than forced into this PR.

## Follow-up issue (to file, left unlabeled)

- `Config::max_query_timeout_secs` is dead code: set in `Default` and in test
  helpers, read nowhere in `src/`. Either wire it into query execution or
  remove it.
- Concurrency-limit layer for write routes (deferred from this PR).

## Tests (written first, `#[ignore]`d until implemented)

- `sparql_endpoint/tests/rdf_upload_limit.rs` or a new
  `sparql_endpoint/tests/request_timeout.rs`:
  - A deliberately slow-by-construction request (raw TCP stream: send request
    line + headers + a `Content-Length` larger than the bytes actually sent,
    then stop) against a server configured with a 1s `request_timeout_secs`
    must get back `408` rather than hanging.
  - A normal, fast request must still succeed under the same short timeout
    (sanity check the timeout doesn't fire on ordinary traffic).
- `sparql_endpoint/tests/upload.rs` (or new test in the same file): two
  concurrent uploads to two different named graphs both complete and all
  their triples are present afterward — a correctness check that also
  exercises the changelog-hoist path without relying on a flaky timing
  assertion.

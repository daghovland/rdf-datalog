---
name: project-persistence
description: Durable persistence implementation using redb changelog over in-memory Datastore
metadata:
  type: project
---

Durable transactional persistence has been implemented in the `sparql_endpoint` crate.

**Why:** When a write returns 200 OK, the server must guarantee the data survives a crash or restart.

**Architecture chosen:** redb as a durable changelog (not as the live query store). The in-memory `Datastore` remains the query engine; `redb` records each mutation before applying it to memory. On startup, the log is replayed into a fresh `Datastore`.

**How to apply:** When discussing persistence, note:
- The default mode is still in-memory (backward compatible).
- `--data-dir <PATH>` / `DAGALOG_DATA_DIR` enables persistence via `dagalog.redb`.
- `--no-persist` / `DAGALOG_NO_PERSIST` overrides the env var.
- No `--db-file` flag exists (removed from docs — was a planned feature not yet implemented).
- Multi-dataset persistence is not correctly isolated (all goes to one changelog; per-dataset redb files are phase P6).

**Key files:**
- `sparql_endpoint/src/persistence.rs` — `QuadChangelog` struct, `LogEntry` enum, replay logic
- `sparql_endpoint/src/lib.rs` — `Config.data_dir`, `AppState.changelog`, startup replay in `serve_on_listener`
- `sparql_endpoint/src/graph_store.rs` — GSP PUT/POST/DELETE wired to changelog
- `sparql_endpoint/src/dataset_routes.rs` — SPARQL Update wired to changelog
- `sparql_endpoint/tests/persistence.rs` — 6 integration tests (all passing)

**Important invariant:** The store write lock must be acquired BEFORE committing to the changelog, so that commit-order equals apply-order under concurrent writers.

**Literal types:** Turtle and SPARQL parsers both use `TypedLiteral` for integers/booleans (not `IntegerLiteral`/`BooleanLiteral`). The `from_repr` function reconstructs as `TypedLiteral` — this is correct. `rdf_literal_from_typed` in `ingress` exists but is NOT called from persistence replay (calling it would break queries by changing the enum variant).

See [[project-bfincremental]] for the planned incremental Datalog maintenance.

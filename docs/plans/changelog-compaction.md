# Changelog Compaction Plan

## Problem

The `QuadChangelog` in `sparql_endpoint/src/persistence.rs` is an append-only `redb`
table indexed by a monotonically increasing u64 key.  Every `INSERT DATA` and
`DELETE DATA` operation appends new entries; CLEAR operations are also appended.
Over time the log grows without bound.

At restart the full log is replayed from entry 0, so startup time grows linearly
with the number of historical mutations.

## Proposed solution: manual compaction endpoint

Add an admin HTTP endpoint that triggers an atomic log compaction:

```
POST /$/compact
```

### Algorithm

1. Acquire the store write lock (blocks new writes during compaction).
2. Snapshot the entire in-memory `Datastore` to log entries:
   - One `LogEntry::InsertQuad` per quad in the default graph.
   - One `LogEntry::InsertQuad` per quad in each named graph, with the graph IRI.
3. Open a write transaction on the `redb` database.
4. Delete **all** existing rows from `QUAD_LOG`.
5. Re-insert the snapshot rows starting at key 0.
6. Commit the transaction.
7. Release the store write lock.

The snapshot is a logically equivalent re-encoding of the current store state,
so replaying it from scratch on the next restart produces the same Datastore.

### Failure safety

Compaction is an atomic `redb` transaction: either all old rows are replaced or
none are. A crash mid-compaction leaves the old log intact (redb rolls back the
uncommitted transaction).

### API

```
POST /$/compact
Authorization: Bearer <admin-key>     (Admin permission required)
```

Response: `200 OK` with a JSON body:
```json
{ "entries_before": 1234, "entries_after": 56, "duration_ms": 42 }
```

### Implementation checklist

- [ ] Add `QuadChangelog::compact(&mut self, store: &Datastore) -> Result<CompactionStats, String>`
      in `persistence.rs`.
- [ ] Add `compact_handler` in `sparql_endpoint/src/` (Admin permission).
- [ ] Register the route in `app_router` (e.g., `POST /$/compact`).
- [ ] Add integration test that inserts many triples, calls compact, verifies
      the log entry count drops and that a fresh replay produces the same graph.

### Notes

- The compaction holds the write lock for the duration of the snapshot write.
  For very large stores this could block reads for a noticeable period.
  A future optimisation could write the snapshot to a shadow table and then
  atomically swap it in, but this is not required for the MVP.
- Automatic background compaction (e.g., triggered when `entries_before > N × entries_after`)
  is a future extension; the admin endpoint provides a safe starting point with
  full operator control.

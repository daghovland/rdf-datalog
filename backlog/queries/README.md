# Backlog SPARQL query library

Named SPARQL queries answering the backlog questions issue
[#286](https://github.com/daghovland/rdf-datalog/issues/286) asked for —
currently answered by manually browsing the GitHub Projects UI. Part of the
[dagalog-on-dagalog epic](https://github.com/daghovland/rdf-datalog/issues/282).

## Running a query

```
backlog/queries/run.sh <query-name>
```

Reuses the `dagalog` binary's own `--query`/`--format table` path (see
`Cargo.toml`'s bin target) rather than inventing new CLI surface — nothing
here needed a `backlog` crate/binary of its own. Run with no arguments to
list available query names. By default, runs against the example fixtures
in `../examples/` (standing in for a real loader —
[#284](https://github.com/daghovland/rdf-datalog/issues/284) — snapshot
until that exists); pass `-D <dir>` to point at a different set of `.ttl`
files once it does.

## Queries

- **`ready_not_started`** — `ready`-labeled issues still open. Uses
  `bl:hasLabel bl:Ready` rather than `bl:status bl:Ready`, since the latter
  isn't reliably populated for every issue yet (see
  `../ontology/MODELING_NOTES.md`'s "Workflow status as its own axis") — the
  raw label is the one signal guaranteed present.
- **`epics_with_no_subissues`** — epics with zero children: either
  freshly created (legitimate — see `../ontology/vocabulary.ttl`'s `bl:Epic`
  comment) or possibly stalled; this query alone still can't distinguish the
  two (it doesn't itself compare against `bl:createdAt`/`bl:updatedAt` — see
  "Not implemented" below for the "how long" query those properties now
  make possible but that isn't written yet).
- **`epics_all_children_closed_but_open`** — epics where every existing
  child is closed but the epic itself is still open: usually just needs a
  final close-out pass.
- **`crates_with_open_bugs`** — every crate touched by a still-open issue,
  joining ticket data against `../examples/crates_and_dependencies.ttl`'s
  dependency graph.
- **`crate_dependents`** *(parameterized)* — what directly depends on a
  given crate. Edit the `VALUES` line in the `.sparql` file to change which
  crate.
- **`work_items_touching_crate`** *(parameterized)* — every issue/PR
  touching a given crate, open or closed. Edit the `VALUES` line to change
  which crate.

All six are exercised against the real example fixtures (not just parsed)
by `tests/backlog_queries.rs` at the repo root.

## Not implemented (no query written yet)

- **"Issues open longer than N days with no activity"** — `bl:createdAt`/
  `bl:updatedAt`/`bl:closedAt` now exist on every `bl:WorkItem`
  ([#379](https://github.com/daghovland/rdf-datalog/issues/379)), so this is
  now expressible as a plain `FILTER` against `bl:updatedAt` — just not
  written as a named query yet. File a follow-up issue if this view is
  wanted.

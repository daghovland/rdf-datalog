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
  comment) or possibly stalled, this query alone can't distinguish the two
  (no timestamp property exists yet to check "how long" — see "Not
  implemented" below).
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

## Not implemented (no property to query against yet)

- **"Issues open longer than N days with no activity"** — there is no
  `createdAt`/`updatedAt`/last-activity timestamp property in the ontology
  today (out of the "deliberately minimal v1" scope from #282). Revisit once
  #284 (the loader) needs to capture timestamps for some other reason, or
  file a follow-up against `../ontology/vocabulary.ttl` if this view is
  wanted badly enough to justify adding one now.

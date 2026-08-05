# Serve combined backlog+provenance dataset via `sparql_endpoint --serve` — plan

Concrete implementation plan for [#356](https://github.com/daghovland/rdf-datalog/issues/356),
item 2 of ["Concrete near-term steps"](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/SERVED_BACKLOG_PLAN.md#concrete-near-term-steps)
in `docs/plans/SERVED_BACKLOG_PLAN.md`. Working branch:
`feat/356-serve-backlog-provenance`.

## What already exists (investigated before writing this plan)

- `src/main.rs`'s `Cli.data: Vec<PathBuf>` (`-d`/`--data`, `clap` derive) is
  already repeatable, and `cli.serve` combined with `cli.data` already works
  as-is: `main()` loads every `--data` file into one `Datastore` *before*
  branching on `cli.serve` (see the load section ending around
  `src/main.rs:455`, followed by the `if cli.serve { ... }` block at
  `src/main.rs:462`). No `src/main.rs` change is needed for this issue —
  `dagalog --serve --data a.ttl --data b.ttl --data c.ttl` already serves the
  union.
- `backlog/queries/run.sh` and `provenance/queries/run.sh` already establish
  the "vocab file(s) + glob a directory of `.ttl` fixtures" pattern via
  repeated `--data` flags — `provenance/queries/run.sh` in particular loads
  BOTH `backlog/ontology/vocabulary.ttl` and
  `backlog/ontology/agentprov-vocabulary.ttl` before globbing
  `provenance/summaries/*.ttl`. The new script for this issue reuses that
  exact idiom, extended with `backlog/examples/snapshot.ttl`.
- Confirmed by direct inspection that the cross-dataset join this issue asks
  us to prove actually has real data on both sides: `ghpull:300` (i.e.
  `<https://github.com/daghovland/rdf-datalog/pull/300>`) is asserted as a
  `bl:PullRequest`/`bl:WorkItem` in BOTH `backlog/examples/snapshot.ttl`
  (with `bl:touchesCrate`, `bl:relatesToIssue`, `bl:closesIssue`,
  `bl:state bl:Closed`) and `provenance/summaries/pr-300.ttl` (with
  `agp:reasoningFor ghpull:300` on an `agp:TranscriptSummary`), using the
  identical real-GitHub-URL IRI scheme in both — so a query joining
  `agp:reasoningFor` against `bl:touchesCrate` genuinely requires both files
  loaded together, not just redundant same-file assertions.

## Design questions (per the issue body — resolved by the issue itself, not re-litigated here)

- **Refresh mechanism**: manual/on-demand. No cron, no webhook.
- **Where it runs**: this same box, alongside everything else already
  running here.
- **Reachability scope**: local-only (`localhost`/this box's own tooling).
  Not externally facing. No new auth/exposure hardening.

## Known, accepted properties (not defects to fix here)

- Combining the backlog snapshot with the provenance summaries will **not**
  necessarily pass `bl:IssueIsEpicXorHasParentShape` SHACL validation across
  the whole merged dataset — the real snapshot has standalone issues that
  are neither epics nor sub-issue-linked. Individual files still validate in
  isolation per `tests/provenance_queries.rs`'s
  `every_summary_file_conforms_to_shapes`. This issue does not add a SHACL
  gate on the combined dataset.
- `--serve` binds `0.0.0.0` (there is no `--bind`/`--host` flag). Per the
  issue's own explicit scope ("no external-facing auth/exposure hardening"),
  this stays as-is; not a gap this issue closes.
- `scripts/serve-backlog.sh` passes `--read-only`: the snapshot is a
  regenerable pull (`cargo run -p backlog --bin backlog-regenerate`), so
  accepting writes through the endpoint would be silently discarded on the
  next regeneration. Read-only keeps that invariant honest.

## Deliverable

1. `scripts/serve-backlog.sh` — a thin wrapper, mirroring
   `backlog/queries/run.sh` / `provenance/queries/run.sh`'s own style, that
   runs `dagalog --serve --read-only` with `--data` pointed at:
   - `backlog/ontology/vocabulary.ttl`
   - `backlog/ontology/agentprov-vocabulary.ttl`
   - `backlog/examples/snapshot.ttl`
   - every `provenance/summaries/*.ttl` (globbed)

   It supports a `--print-data-args` mode that prints the resolved file list
   (one path per line) and exits without starting a server — this exists
   specifically so the test suite can assert against the exact list the
   script itself resolves, instead of a second, hand-copied list that could
   silently drift from what the script actually loads.

2. `tests/serve_backlog_provenance.rs` — proves the combination genuinely
   works end-to-end, not just that each file loads individually:
   - Shells out to `scripts/serve-backlog.sh --print-data-args` and asserts
     the resolved list contains all four expected path fragments (the two
     vocab files, the snapshot, and at least one `provenance/summaries/*.ttl`
     file).
   - Loads exactly that resolved file list into one `Datastore` (via
     `dagalog::load_file`, the same helper `cli_integration.rs` /
     `backlog_queries.rs` / `provenance_queries.rs` already use) and asserts
     a sane combined triple count.
   - Runs an inline SPARQL query (kept in the test file, not under
     `provenance/queries/` or `backlog/queries/` — both of those directories
     are globbed by their own existing tests against a narrower corpus that
     doesn't include the other side, so a query needing both would silently
     return zero rows there) joining `agp:reasoningFor`/`agp:summaryText`
     (from `provenance/summaries/pr-300.ttl`) against `bl:touchesCrate`
     (from `backlog/examples/snapshot.ttl`) for `ghpull:300`, and asserts the
     join returns the expected crate and non-empty summary text.

   The equivalent-load-path-in-process approach (rather than actually
   binding a TCP listener and driving it over HTTP with `reqwest`) is
   deliberate: the HTTP layer itself already has exhaustive coverage under
   `sparql_endpoint/tests/` (see `sparql_endpoint/tests/common/mod.rs`'s
   `TestServer` harness); what this issue actually risks is the *data
   combination* (do the right files load together, does the cross-file join
   resolve), which the in-process test targets directly without adding a new
   HTTP test harness, `reqwest` dev-dependency, or tokio feature surface to
   the root crate.

## Sequence

Plan doc (this file) → commit/push/draft PR (`Closes #356`) early → ignored
red tests → `scripts/serve-backlog.sh` → unignore tests, green → quality
gate (`cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace`) →
provenance summary (`provenance/summaries/pr-<N>.ttl`).

## References

- [#356](https://github.com/daghovland/rdf-datalog/issues/356) (this issue)
- [`docs/plans/SERVED_BACKLOG_PLAN.md`](SERVED_BACKLOG_PLAN.md) (the parent plan this implements a step of)
- Related epics: [#282](https://github.com/daghovland/rdf-datalog/issues/282) (backlog), [#306](https://github.com/daghovland/rdf-datalog/issues/306) (agent provenance)

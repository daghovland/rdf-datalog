# Wire backlog snapshot into the production dataset (#565)

Follow-up from [#493](https://github.com/daghovland/rdf-datalog/issues/493)
(dereferenceable resource IRIs). See
[`DEREFERENCEABLE_RESOURCE_IRIS_493_PLAN.md`](DEREFERENCEABLE_RESOURCE_IRIS_493_PLAN.md),
"What's actually queryable today", for the gap this closes.

## What's actually on this host

`deploy/docker-compose.public.yml`'s `dagalog` service starts with a single
`--data /data/dataset.ttl` (volume `../data:/data`, i.e. the repo root's
untracked `data/` directory). Before writing any code, this session
inspected that real file (read-only) instead of assuming it was empty or
unrelated:

- `data/dataset.ttl` is 12,558 lines and **already contains** the full
  `bl:`/`agp:` vocabulary (`backlog/ontology/vocabulary.ttl` +
  `agentprov-vocabulary.ttl`, byte-identical prefix, verified by diff)
  **plus** backlog instance data (`bl:Issue`/`bl:PullRequest`/etc, full-IRI
  triples) **plus** provenance summaries (265 `agp:`/`summaryText` hits).
  Every triple in the file is under `dagalog.no/`, `github.com/`, or
  `w3.org/` — there is no other "real" demo dataset mixed in.
- Its `bl:CurrentSnapshot bl:generatedAt` value is
  `2026-08-11T16:36:42.735339342+00:00` — **exactly** the timestamp in
  `backlog/examples/snapshot.ttl`'s own header as of that regeneration.
  The file's mtime (Aug 13) is two days later.

Conclusion: **someone already solved this once, by hand** — concatenated
the two ontology files + that day's `backlog/examples/snapshot.ttl` +
`provenance/summaries/*.ttl` into `data/dataset.ttl` and left it there.
That's exactly the gap #565 names: it happened once, isn't repeatable, and
is now ~7 weeks stale (hundreds of merged PRs behind, per the commit log).

This finding changes the shape of the fix. It is **not** "add a second
`--data` file" (#565's own text suggested this as one option) — layering a
fresh snapshot alongside the existing stale one would double up
`bl:CurrentSnapshot`/`bl:Issue #N` individuals with conflicting
`bl:generatedAt`/`bl:Open`/`bl:Closed` values, which breaks #380's staleness
query outright. The existing merged-file shape already works (production
has presumably been serving it in this form); the actual gap is that
*building that file* is a manual, one-off act with no script and no
schedule.

## Decision

**Make the existing manual merge into a script, run on a schedule.**

1. `scripts/regenerate-production-dataset.sh` — rebuilds
   `data/dataset.ttl` (or an `--out` override) from:
   `backlog/ontology/vocabulary.ttl` + `agentprov-vocabulary.ttl` +
   a freshly-regenerated snapshot + every `provenance/summaries/*.ttl` —
   the same four-part shape already proven to work in the file on disk,
   and matching `scripts/serve-backlog.sh`'s existing file-selection
   policy for local dev (kept as a *second*, independent list rather than
   shared code — one resolves host paths for `cargo run`, one produces a
   single merged file for a read-only container mount; see "Why not share
   code with serve-backlog.sh" below).
   - `--skip-regenerate`: reuse the already-checked-in
     `backlog/examples/snapshot.ttl` instead of hitting `gh api` again —
     this is what makes the assembly step testable without live network
     access, and lets someone refresh provenance-only changes quickly.
   - `--no-restart`: skip the `docker compose restart dagalog` step — also
     for testability, and for a dry run.
   - Writes to a temp file and renames over the target
     (`std::fs::rename`/`mv` is atomic on the same filesystem), so a
     mid-write failure never leaves a truncated `dataset.ttl` that
     crash-loops the container on its next restart.
2. `backlog-regenerate` gains an `--out <PATH>` flag (default: unchanged,
   `backlog/examples/snapshot.ttl`, so every existing caller — local dev,
   `tests/`, this repo's CLAUDE.md-documented manual step — is unaffected).
   The production script points it at a scratch temp path, **not** the
   tracked file: writing to `backlog/examples/snapshot.ttl` from a daily
   timer on a live `git`-checked-out production host would leave that file
   permanently dirty and conflict with the next `git pull` (this file
   already gets updated by real PRs, e.g. #394, #431). Argument parsing is
   extracted into a small pure function so it's unit-testable without
   `gh`/network.
3. `deploy/systemd/dagalog-backlog-refresh.{service,timer}` — a template
   unit pair Dag installs manually on the server (this session has no
   access to that host, so — like every other prerequisite in
   `PUBLIC_DEPLOYMENT.md` — it's a documented step, not something run from
   here). `OnCalendar=daily`. The service unit sets `PATH` (needs
   `~/.cargo/bin` if running via `cargo run --release`, or a path to a
   prebuilt binary) and `HOME` explicitly (`gh` reads its token from
   `$HOME/.config/gh`) — a systemd unit does not inherit an interactive
   login shell's environment, unlike a script run by hand.
4. No changes to `Dockerfile`, `docker-compose.public.yml`, or the runtime
   image: the container-side loading mechanism (`--data /data/dataset.ttl`)
   already works and is already proven against real data. The entire fix
   is host-side: build the file correctly and on a schedule, then restart.

### Why not share code with `serve-backlog.sh`

`serve-backlog.sh` resolves *host* absolute paths and passes them as N
repeated `--data` flags to `cargo run --bin dagalog` for local development
— it never produces a file. The production path needs to produce exactly
*one* merged Turtle file (matching what's already deployed and proven, and
avoiding a docker-compose command-array that would need to grow by one
entry per merged PR, since `provenance/summaries/` already has 257 files).
Parameterizing one script to do both jobs would need a mode flag steering
two genuinely different code paths (multi-arg exec vs. single-file
concatenation) for one caller each — not worth it. A test (below) instead
keeps the two file *lists* from silently diverging.

### Why concatenation is safe here

Turtle files can be concatenated and parsed as one document as long as
`@prefix` declarations don't conflict, and as long as no two files reuse
the same *labeled* blank-node identifier (`_:x`) or rely on a per-file
`@base`/relative IRI that would resolve differently once concatenated.
Checked concretely, not assumed:

- All sources share the same `bl:`/`agp:` prefix→IRI mappings (verified:
  `vocabulary.ttl` and `agentprov-vocabulary.ttl` are literally
  byte-identical prefixes to what's already embedded in `data/dataset.ttl`).
- `grep -ho '_:[A-Za-z0-9_]*' backlog/ontology/*.ttl backlog/examples/snapshot.ttl
  provenance/summaries/*.ttl | sort | uniq -c` finds several `_:` tokens
  (`_:x`, `_:b0`, `_:c14n0`, …), but every hit traced back to plain prose
  *inside* an `agp:summaryText "..."` string literal (a PR summary
  describing someone else's blank-node-handling code) — never actual
  Turtle blank-node syntax. Same check for `grep -l '<#\|<>'` (candidate
  relative IRIs / `@base` use): every hit is also inside a
  `summaryText`/`abstractText` string, e.g. pr-419.ttl's summary
  *mentioning* `<#service>` fragment IRIs from an unrelated Fuseki
  assembler file. No source file actually declares `@base` or a labeled
  blank node as real RDF syntax. `tests/regenerate_production_dataset.rs`'s
  `merged_dataset_contains_backlog_and_provenance_data` test loading the
  real, live-regenerated snapshot + all 257 current summaries and getting
  a sane combined triple count is the concrete backstop for this, run
  every time CI runs — if a future summary ever *does* introduce a real
  labeled blank node or relative IRI, the resulting parse/count anomaly
  would show up there.
- Blank nodes are otherwise scoped per parse, so concatenating text before
  parsing is equivalent to parsing one larger document — no cross-file
  collision risk given the above.

`scripts/regenerate-production-dataset.sh --print-sources` and
`scripts/serve-backlog.sh --print-data-args` are asserted (as sets) to
resolve the same file list by
`tests/regenerate_production_dataset.rs::production_and_dev_scripts_resolve_the_same_source_files`,
so the two independently-maintained lists (see below) can't silently
diverge.

## Non-goals for this PR

- Installing/enabling the systemd timer on the actual production server —
  operational step for Dag, documented not automated (no access to that
  host from here).
- A GitHub Actions-based regeneration path that commits back to `main` —
  considered and rejected: a bot pushing directly to `main` bypasses this
  repo's own PR-only workflow, a bigger design decision than this issue.
- Changing `bl:`/`agp:` vocabulary, SHACL shapes, or the loader — pure
  deploy/infra wiring of data that already exists in loadable form.

## Validation plan

1. Unit test(s) for `backlog-regenerate`'s new `--out` argument parsing
   (pure function, no network).
2. Integration test for the assembly script: run
   `scripts/regenerate-production-dataset.sh --skip-regenerate --no-restart
   --out <tempfile>` against this repo's own checked-in fixtures (no `gh`
   call), then load the resulting file with `dagalog::load_file` +
   `run_sparql_query` and assert both a `bl:Issue` row and an
   `agp:AgentSession`/`agp:summaryText` row come back from ONE file load —
   concretely proving the merge produces a working combined dataset, not
   just "the script looks right." Mirrors `tests/serve_backlog_provenance.rs`'s
   existing pattern.
3. `cargo fmt`/`clippy`/`cargo test --workspace` as usual.
4. Manually run the full script (with a live `--skip-regenerate` off, i.e.
   an actual `backlog-regenerate` call) once in this worktree against a
   scratch output path, and load the result, as an extra concrete check
   beyond the test suite.

## Follow-ups filed

None yet — filed during implementation if anything genuinely out-of-scope
turns up (e.g. if actually installing the timer surfaces a real blocker
worth tracking).

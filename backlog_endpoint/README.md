# backlog_endpoint

A standalone HTTP server for the read-only, schema-*specific* dashboard over
this repository's own `bl:`/`agp:` backlog and provenance data — issues,
epics, PRs, crates and their dependencies, and agent-session transcript
summaries.

This is **not** part of dagalog's own SPARQL endpoint. It's a separate
binary that talks to a *running* dagalog instance purely over HTTP, the way
an application talks to Postgres. See the crate's own doc comment
(`src/lib.rs`) and
[`docs/plans/BACKLOG_PROVENANCE_DASHBOARD_PLAN.md`](../docs/plans/BACKLOG_PROVENANCE_DASHBOARD_PLAN.md)
("Decision: dogfood tool, not a product feature") for why: dagalog is a
domain-agnostic RDF/SPARQL engine, and this dashboard hardcodes GitHub- and
`bl:`/`agp:`-specific vocabulary throughout, so it's kept out of dagalog's
own core HTTP server binary. Part of epic
[#378](https://github.com/daghovland/rdf-datalog/issues/378).

## Quickstart

The easiest way to run both halves (a `dagalog --serve` instance loaded
with the backlog/provenance dataset, plus this dashboard pointed at it) is
the wrapper script from the repo root:

```sh
scripts/serve-backlog.sh
```

This starts `dagalog --serve --read-only` on port 3030 with the checked-in
`backlog/examples/snapshot.ttl` and every `provenance/summaries/*.ttl` file
loaded, and `backlog_endpoint` on port 3031 pointed at it. Open
`http://localhost:3031` (or `http://localhost:3031/backlog`, kept as an
alias for continuity with the dashboard's original route). Ctrl-C stops
both processes.

Useful flags:

```sh
# Different ports
scripts/serve-backlog.sh --dagalog-port 4030 --dashboard-port 4031

# See which data files would be loaded, without starting anything
scripts/serve-backlog.sh --print-data-args

# Pass extra arguments through to the dagalog process only
scripts/serve-backlog.sh -- --base-iri http://example.org/
```

### Running the two processes by hand

If you already have a dagalog SPARQL endpoint running elsewhere (a
different port, a remote host, a dataset loaded some other way), point
`backlog_endpoint` at it directly instead of using the wrapper script:

```sh
cargo run -p backlog-endpoint -- \
  --port 3031 \
  --sparql-endpoint http://localhost:3030/sparql
```

| Flag | Env var | Default | Meaning |
|---|---|---|---|
| `--port` | `BACKLOG_ENDPOINT_PORT` | `3031` | Port this dashboard server listens on |
| `--sparql-endpoint` | `BACKLOG_ENDPOINT_SPARQL_ENDPOINT` | `http://localhost:3030/sparql` | The dagalog `/sparql` endpoint this dashboard's JS queries |

The configured `--sparql-endpoint` is injected into the served page as
`window.SPARQL_ENDPOINT`, so the dashboard's JS knows where to send its
queries — it does not assume same-origin `/sparql`.

## What's on the dashboard

Four tabs, each backed by unparameterized SPARQL queries run client-side
against `--sparql-endpoint`:

- **Board** — open issues grouped by `bl:status`, plus an epic/sub-issue
  tree (arbitrary `bl:subIssueOf` depth).
- **Crates** — the crate list with each crate's open `bl:touchesCrate`
  work items, plus a Cytoscape.js dependency graph of direct
  `bl:dependsOnCrate` edges (pan/zoom/fullscreen/PNG export).
- **Provenance** — `agp:AgentSession`s ordered by `prov:startedAtTime`,
  each expandable to its `agp:TranscriptSummary` and any
  `agp:decisionPoint`/`agp:alternative` drill-down.
- **What's relevant** — given a file path or crate name, every
  `bl:PullRequest` that touched it plus the `agp:TranscriptSummary`
  reasoning behind it (via `agp:reasoningFor`) — "before I touch this,
  what's already been done here and why."

## Data freshness

The dashboard reads whatever dataset the configured `--sparql-endpoint` has
loaded — it doesn't fetch from GitHub itself. To refresh the underlying
snapshot from live GitHub state:

```sh
cargo run -p backlog --bin backlog-regenerate
```

This regenerates `backlog/examples/snapshot.ttl` (needs `gh api` access)
and records a `bl:generatedAt` timestamp on `bl:CurrentSnapshot`, which the
dashboard's freshness banner reads.

## Tests

```sh
cargo test -p backlog-endpoint
```

Tests spin up a local server (`TestServer`, binding `127.0.0.1:0`) and
assert on markers in the served static HTML body — see
`tests/backlog_frontend.rs`. There's no live SPARQL execution in these
tests; the dashboard's queries are verified by hand against real data (see
individual PR descriptions under
[`provenance/summaries/`](../provenance/summaries/) for how each view was
checked).

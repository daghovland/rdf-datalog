# Plan: backlog + provenance web dashboard

Epic: [#378](https://github.com/daghovland/rdf-datalog/issues/378). Part of the
wider dogfooding effort under [#282](https://github.com/daghovland/rdf-datalog/issues/282)
(backlog-as-RDF) and [#306](https://github.com/daghovland/rdf-datalog/issues/306)
(agent-provenance ontology).

## Goal

A read-only web frontend over this repo's own backlog (issues, epics, PRs,
crates — [`bl:`](../../backlog/ontology/vocabulary.ttl)) and agent-provenance
data (agent sessions, transcript summaries, decisions —
[`agp:`](../../backlog/ontology/agentprov-vocabulary.ttl)), so a human can
browse "what's the state of the backlog," "what crates exist and how do they
depend on each other," "what have agents actually done and why," and "what's
already been touched near this file/crate" — without hand-running
`dagalog --query` against checked-in `.ttl` snapshots.

## Decision: dogfood tool, not a product feature

This is a purpose-built tool for *this repository's own* backlog, not a
generic feature of the dagalog product. It deliberately hardcodes `bl:`/`agp:`
class and property IRIs into its views and queries, the same way
`backlog/queries/*.sparql` and `provenance/queries/*.sparql` already do.

This is a real fork with a real alternative: the existing
[`frontend.html`](../../sparql_endpoint/src/frontend.html) (query box, resource
browser, class hierarchy, visual query builder) is deliberately
**schema-agnostic** — it discovers classes/properties from whatever dataset
happens to be loaded and knows nothing about `bl:`/`agp:` specifically. Folding
backlog-specific views into that file would compromise that generality and
create file-level conflicts with two other already-`ready`-labeled issues that
also touch it ([#42](https://github.com/daghovland/rdf-datalog/issues/42) SPARQL
syntax highlighting, [#47](https://github.com/daghovland/rdf-datalog/issues/47)
Query Builder Cytoscape canvas).

**Chosen approach**: a new, separate route and file, following `frontend.html`'s
existing implementation pattern — single self-contained HTML file via
`include_str!`, vanilla JS, no build step, no framework, no external JS
dependency beyond what's already loaded lazily (Cytoscape.js) — hardcoding the
`bl:`/`agp:` vocabulary throughout rather than discovering it generically.

**Revised (Stage 1 of the dagalog/rdf-backlog separation, see below): its own
crate and binary, not a route inside `sparql_endpoint`.** The first
implementation (#381, initially) put this route inside `sparql_endpoint`
itself (`GET /backlog`, `sparql_endpoint/src/backlog_frontend.rs` +
`backlog_frontend.html`) — reviewing that against the actual product
boundary surfaced a real problem: dagalog is meant to be a domain-agnostic
RDF/SPARQL engine, and rdf-backlog (this dashboard, the `bl:`/`agp:`
vocabularies, the GitHub loader) is a distinct *application* that uses
dagalog as a backend, the way an app uses Postgres. Baking a GitHub-specific,
`bl:`-hardcoded page into dagalog's own core HTTP server binary means the
generic triplestore product ships application-specific code it shouldn't
know about, and the two can never be released/versioned independently.

Rather than jump straight to splitting into two Git repositories (real cost:
cross-repo dependency pinning instead of path deps, duplicate CI, losing
"test rdf-backlog against dagalog's tip-of-tree with no version-bump
ceremony" — not justified yet, since nothing today demands independent
release trains or a second real consumer of dagalog beyond this dogfooding
case), the dashboard moves into its **own crate and binary** within the same
Cargo workspace: `backlog_endpoint` (`backlog_endpoint/src/main.rs` — a small
standalone axum server, `backlog_endpoint/src/backlog_frontend.html`). It
does not link against `sparql_endpoint` at all; its JS talks to a *running*
dagalog SPARQL endpoint over plain HTTP (configurable base URL, defaulting to
`http://localhost:3030/sparql` — the same way `frontend.html`'s JS already
talks to `/sparql`, just not assuming same-origin). This is the real
`rdf-backlog uses dagalog as a backend` relationship, expressed as a process/
network boundary rather than an in-process one, while still living in one
repo/workspace for now. A full repo split (Stage 2) remains available later, once there's an actual
forcing function — dagalog wanting an independent release/versioning story
decoupled from rdf-backlog's churn, rdf-backlog growing a second real backend
loader (Jira/Linear) or consumer proving it's genuinely reusable, or monorepo
friction (build times, onboarding) becoming a real rather than theoretical
pain. Nothing about this crate/binary structure blocks that move later — it
only makes it lower-stakes when it happens, since `backlog_endpoint` already
depends on dagalog only via its public HTTP API, not its Rust internals.

## What already exists (don't rebuild)

- **The vocabulary and data model** — `bl:` and `agp:` are both mature,
  SHACL-shaped, and already exercised by real queries
  (`backlog/queries/*.sparql`, `provenance/queries/*.sparql`) and a real test
  harness (`tests/provenance_queries.rs`).
- **The GitHub-sync loader** — `backlog/src/loader.rs` / `backlog-regenerate`
  already turns live GitHub state into `bl:` triples (issues, epics, PRs,
  crates, dependencies, touched files).
- **21+ real provenance summaries** — `provenance/summaries/*.ttl`, one per
  merged PR, hand-authored per
  [`TRANSCRIPT_SUMMARY_GUIDELINES.md`](TRANSCRIPT_SUMMARY_GUIDELINES.md).
- **Graph-rendering and query infrastructure in `frontend.html`** — Cytoscape.js
  wiring (pan/zoom/fullscreen/export), a working `/sparql` JSON round-trip
  pattern, and the `query_builder`/`vqs_routes` machinery for property
  discovery. The dashboard should reuse these mechanics (either via a shared
  JS module or targeted duplication of the minimal subset) rather than
  reinvent graph rendering.
- **[#356](https://github.com/daghovland/rdf-datalog/issues/356)** — already
  covers serving the combined backlog+provenance dataset via
  `sparql_endpoint --serve`. This plan depends on it as the dashboard's data
  source; it is not re-specified or duplicated here.
- **[#351](https://github.com/daghovland/rdf-datalog/issues/351)** — already
  covers the `related_to_file`/`related_to_crate` SPARQL queries the "what's
  relevant" panel needs. Not re-specified here either.

## What's genuinely missing (verified, not assumed)

Two real gaps were confirmed while writing this plan, not just inherited from
older docs:

1. **No timestamps anywhere on `bl:WorkItem`.** `bl:` has no `createdAt`/
   `updatedAt`/`closedAt`, even though GitHub's Issues API (the loader's own
   data source) exposes all three. This makes every time-ordered view
   (activity feed, "recently updated," staleness) impossible today — not a
   UI limitation, a data-model gap. Tracked as
   [#379](https://github.com/daghovland/rdf-datalog/issues/379).
2. **No snapshot generation timestamp.** `backlog-regenerate` is a manual,
   one-shot pull with no scheduled sync; the checked-in
   `backlog/examples/snapshot.ttl` records no "generated at" timestamp at all.
   Confirmed stale by inspection while writing this plan (last regenerated
   2026-08-02; already lags real GitHub state by several dozen issues/PRs
   with no way to detect that from the file itself). Tracked as
   [#380](https://github.com/daghovland/rdf-datalog/issues/380).

The one thing that *isn't* a gap, verified directly rather than assumed: PR
subject IRIs match between the two datasets — `backlog/examples/snapshot.ttl`
types `bl:PullRequest`s at `https://github.com/.../pull/N`, and every
provenance summary's `ghpull:` prefix expands to the identical form. The core
`agp:reasoningFor` → `bl:PullRequest` join the dashboard depends on
(provenance ↔ backlog) works on the data as it exists today.

One thing to design around rather than "fix": combining the backlog snapshot
and provenance summaries into one dataset will **not** cleanly pass
`bl:IssueIsEpicXorHasParentShape` SHACL validation as a whole — individual
summary files validate in isolation (per their own guidelines), and the real
snapshot has genuine standalone issues that are neither epics nor
sub-issue-linked (see `backlog/examples/real_gap_standalone_issue_274.ttl`).
The dashboard and the serving path from #356 should not gate on strict
combined-dataset SHACL conformance — that's a known, pre-existing property of
the real data, not a defect to block on.

## Sub-issues (concrete slices)

- [#379](https://github.com/daghovland/rdf-datalog/issues/379) — `bl:createdAt`/`updatedAt`/`closedAt` vocabulary + loader addition
- [#380](https://github.com/daghovland/rdf-datalog/issues/380) — snapshot generation timestamp
- [#381](https://github.com/daghovland/rdf-datalog/issues/381) — dashboard page shell + issue/epic board view
- [#382](https://github.com/daghovland/rdf-datalog/issues/382) — crate list + dependency graph view
- [#383](https://github.com/daghovland/rdf-datalog/issues/383) — agent/session/provenance timeline view
- [#384](https://github.com/daghovland/rdf-datalog/issues/384) — "what's relevant" panel (file/crate → related PRs + reasoning)

Suggested order: #379 and #380 first (small, unblock the freshness/activity
views but don't block the rest); #381 next (page shell — everything else
depends on it existing); #382/#383/#384 in any order after that, each
independent of the others. All depend on #356 landing for a real served
dataset to point at.

## Explicitly out of scope for this epic

- **Write access from the dashboard** (e.g. changing `bl:status`, adding
  comments) — this stays read-only; GitHub write-back is its own separate,
  already-deferred concern (see #287, referenced from
  [`SERVED_BACKLOG_PLAN.md`](SERVED_BACKLOG_PLAN.md)).
- **Reachability/deployment scope** (who can reach the served endpoint,
  whether it needs auth) — this is a call for the repo owner, not something
  this plan or #356 should decide unilaterally, since the endpoint's default
  is unauthenticated.
- **A scheduled/automatic backlog-sync mechanism** — #380 only adds a
  timestamp *to* the existing manual regeneration; building automatic
  periodic sync is a separate future concern, not assumed here.

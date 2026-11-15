# Board: configurable columns, including a Closed column (#489)

[Issue #489](https://github.com/daghovland/rdf-datalog/issues/489)

## Problem

The Board tab's "Done" column is effectively always empty: `bl:status bl:Done`
means "PR open, CI green, awaiting merge" while the issue is still
`bl:state bl:Open`. The instant the PR actually merges, the issue flips to
`bl:state bl:Closed` and is excluded from `loadBoard()`'s query entirely
(`?issue a bl:Issue ; bl:state bl:Open ; ...`), so "Done" only shows anything
in the narrow window between setting Status=Done and the human merging.

## Decision

Dag's [follow-up comment on #489](https://github.com/daghovland/rdf-datalog/issues/489#issuecomment-5302244166)
supersedes the issue body's original "pick option 1 (widen Done with a time
window) or option 2 (separate Recently Shipped view)" framing with a more
general design, confirmed as the one to build by the Status transition to
`Agent` on 2026-08-18 — three days *after* that comment, so it reflects his
intent at the time of authorization, not a stale initial framing:

- **Columns are derived from the data**, not a hardcoded `STATUS_COLUMNS`
  list: one column per distinct `bl:status` value actually present, in the
  vocabulary's defined Todo -> Ready -> InProgress -> Done order, with any
  value not in that fixed list appended afterward (forward-compatible with a
  future `bl:WorkflowStatus` individual this code doesn't know about yet).
- **Plus a `bl:state bl:Closed` column**, always present by default,
  independent of `bl:status`. This is what actually fixes the reported bug:
  merged/closed work now has somewhere to land instead of vanishing from the
  board. No time window or `closedAt` recency filter is used — Dag's
  original option 1 (time-windowed Done) is explicitly **rejected**: the
  refined design keeps "Done" itself strictly meaning "not yet merged" and
  gives Closed issues their own permanent (but hideable) column instead of
  overloading Done with "recently shipped" semantics. See
  `agp:alternative` in the PR's provenance summary for this reasoning
  recorded against the decision.
- **Hideable columns**, persisted in `localStorage` (no server-side user
  profile store exists today — see Dag's comment). The Closed column is the
  one most likely to be hidden once the board gets busy, since closed issues
  only grow monotonically.
- **Renamed column labels**: Dag's comment identifies two distinct possible
  mechanisms (canonical `rdfs:label` edit vs. a personal client-side
  override) and explicitly says this needs confirming before
  implementation. **Out of scope for this PR** — filed as a separate
  Status=Todo follow-up issue per CLAUDE.md's "follow-up work discovered
  mid-task must become a real issue" rule.

## Query shape

Two small, unparameterized queries (mirroring the existing `loadBoard`/
`loadEpics`/#429 "several small queries, join client-side" pattern, not one
big join):

```sparql
# open issues, bucketed by bl:status client-side (unchanged except for
# being one of two queries now, not the only one)
SELECT ?issue ?title ?status WHERE {
  ?issue a bl:Issue ; bl:state bl:Open ; rdfs:label ?title .
  FILTER NOT EXISTS { ?issue a bl:Epic }
  OPTIONAL { ?issue bl:status ?status }
}
```

```sparql
# closed issues — new; these form the Closed column regardless of bl:status
SELECT ?issue ?title WHERE {
  ?issue a bl:Issue ; bl:state bl:Closed ; rdfs:label ?title .
  FILTER NOT EXISTS { ?issue a bl:Epic }
}
```

Column set = `{distinct bl:status values present in query 1, in vocabulary
order} + {Closed} + {No status recorded, if any open issue lacks bl:status}`.

## Verification

No Rust test exercises the JS query strings (confirmed via
`backlog_endpoint/tests/backlog_frontend.rs`, which only checks routing/
content-type/markers) — per PR #448's established pattern for this file,
verify query shapes directly against a live `scripts/serve-backlog.sh`
instance with `curl`, not just by reading the JS.

## Prior provenance

`provenance/queries/run.sh related_to_file '"backlog_endpoint/src/backlog_frontend.html"'`
surfaces #427 (Stage 1 restructuring), #428 (Crates view), #429 (Provenance
view — the small-queries/join-client-side pattern this plan reuses), #430
(Relevant view), #442 (namespace rename), #448 (item detail page — "verify
every query shape against a live endpoint" pattern this plan also reuses).

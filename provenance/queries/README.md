# Provenance SPARQL query library

Named SPARQL queries answering the agent-provenance questions
[`docs/plans/AGENT_PROVENANCE_PLAN.md`](../../docs/plans/AGENT_PROVENANCE_PLAN.md)
asked for. Part of the agent-provenance epic
[#306](https://github.com/daghovland/rdf-datalog/issues/306), issue
[#327](https://github.com/daghovland/rdf-datalog/issues/327). Mirrors
[`backlog/queries/`](../../backlog/queries/)'s own pattern.

## Running a query

```
provenance/queries/run.sh <query-name>
```

Reuses the `dagalog` binary's own `--query`/`--format table` path (see
`backlog/queries/run.sh` for the identical precedent). Run with no
arguments to list available query names. By default, runs against the
worked grounding example(s) in `../summaries/`; pass `-D <dir>` to point at
a different set of `.ttl` files.

## Queries

- **`reasoning_for_pr`** *(parameterized)* — "Why was PR #N merged?":
  looks up the `agp:TranscriptSummary` `agp:reasoningFor` a given
  `bl:PullRequest` and prints its `agp:summaryText`. Shipped pointing at PR
  #300.
- **`reasoned_about_by_agent`** *(parameterized)* — "What has agent X
  reasoned about?": every summary `prov:wasAttributedTo` a given agent.
  Shipped pointing at the "Claude Sonnet 5" agent.
- **`sessions_for_issue`** *(parameterized)* — "Which sessions worked on
  issue #N?": every `agp:AgentSession` with `prov:used` a given issue.
  Shipped pointing at issue #264 (the issue PR #300 closed).
- **`all_decision_points`** — "All decision points across the backlog":
  flattens `agp:decisionPoint` across every summary currently loaded, one
  row per `agp:Decision` (not per `agp:alternative` — see the query file's
  own comment for why alternatives aren't joined in here).

All four are exercised against every real `../summaries/*.ttl` file (not
just parsed) by `tests/provenance_queries.rs` at the repo root, which globs
the directory rather than naming files individually — a new
`pr-<N>.ttl` an agent writes (see
[`docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md`](../../docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md))
is picked up automatically, with no test code change.

## Worked examples

- `../summaries/pr-300.ttl` distills the actual reasoning from
  [PR #300](https://github.com/daghovland/rdf-datalog/pull/300) (which
  closed [#264](https://github.com/daghovland/rdf-datalog/issues/264)): two
  genuine decision-point forks recorded during that PR's review round —
  `sh:sourceShape` resolution and the `sh:qualifiedMinCount`/
  `sh:qualifiedMaxCount` split — are captured as `agp:Decision` resources
  alongside the summary.
- `../summaries/pr-328.ttl` distills the actual reasoning from
  [PR #328](https://github.com/daghovland/rdf-datalog/pull/328) (which
  closed [#307](https://github.com/daghovland/rdf-datalog/issues/307)):
  the `shacl/src/path.rs` design for complex `sh:path` property paths.
  Added for [#334](https://github.com/daghovland/rdf-datalog/issues/334)
  specifically as a second example, to prove the glob-based loader above
  actually generalizes past "the one hardcoded file."

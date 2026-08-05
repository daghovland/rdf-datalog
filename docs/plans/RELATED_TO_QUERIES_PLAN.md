# `related_to_file` / `related_to_crate` canned queries — plan

Sub-issue [#351](https://github.com/daghovland/rdf-datalog/issues/351) of
the agent-provenance epic [#306](https://github.com/daghovland/rdf-datalog/issues/306),
second slice of
[`docs/plans/PROVENANCE_QUERY_WORKFLOW_PLAN.md`](PROVENANCE_QUERY_WORKFLOW_PLAN.md)
(gap #3). Depends on the `bl:touchesFile`/`agp:abstractText` schema addition
already merged in [#358](https://github.com/daghovland/rdf-datalog/pull/358).

## Motivation

The existing canned queries under `provenance/queries/` (`reasoning_for_pr`,
`reasoned_about_by_agent`, `sessions_for_issue`, `all_decision_points`) all
take a *known* PR/issue/agent as input. Nothing answers "I'm about to touch
this file/crate — has anyone reasoned about it before", which is the
actual pre-work lookup gap #4 of the workflow plan wants agents to run.

## Design

Two new query files, mirroring the existing four's shape (header comment,
`PREFIX` block, `VALUES`-parameterized, shipped pointing at a real worked
example):

### `related_to_file.sparql`

Two-hop join:
`agp:TranscriptSummary --agp:reasoningFor--> bl:PullRequest --bl:touchesFile--> "path"`.

`bl:touchesFile`'s range is `xsd:string` (repo-relative path, see
`vocabulary.ttl`), so the `VALUES` line binds a plain string literal, not a
resource — string equality, not a join through an IRI.

Prints `agp:abstractText` when present, falling back to `agp:summaryText`
otherwise (`agp:abstractText` is optional per `agentprov-shapes.ttl` —
`sh:maxCount 1`, no `sh:minCount`), plus the PR IRI itself (so the caller
gets a clickable GitHub link straight out of the result table, matching
`reasoning_for_pr.sparql`'s existing `?summary`/`?summaryText` style).

Fallback mechanism: `OPTIONAL { ?summary agp:abstractText ?abstract }` then
`BIND(COALESCE(?abstract, ?summaryText) AS ?text)`. `COALESCE` is already
implemented in `sparql_parser` (`execute.rs`, returns the first *bound*
argument) so this is the natural fit over hand-rolling the same behavior
with nested `IF`/`BOUND`.

### `related_to_crate.sparql`

Same shape, one hop shallower via `bl:touchesCrate` instead of
`bl:touchesFile`, `VALUES` binding a `bl:Crate` resource (`bl:touchesCrate`'s
range is `bl:Crate`, an object property — unlike `bl:touchesFile` this *is*
a resource join) rather than a string.

`bl:touchesCrate`'s domain is `bl:WorkItem` (broader than `bl:PullRequest`,
also covers `bl:Issue`), but this query still matches only
`bl:PullRequest`s in the pattern — same choice `related_to_file.sparql` is
forced into anyway (only PRs carry `bl:touchesFile`, so restricting
`related_to_crate` to PRs too keeps both queries' "what's relevant"
semantics consistent: both answer "which *finished, reasoned-about* work
touched this", not "which still-open issue mentions this crate as
expected"). An issue's own expected-crate impact isn't reasoned-about yet
by definition (no `agp:TranscriptSummary` exists for unfinished work), so
this restriction doesn't lose any real answer, just keeps the join
consistent across the two new queries.

## Testing

`bl:touchesFile`/`bl:touchesCrate` facts live in `backlog/examples/snapshot.ttl`
(generated, real GitHub data), not in `provenance/summaries/`. The
`agp:reasoningFor` links in the summaries point at the *same* PR IRIs
(`ghpull:<N>`) the snapshot describes, so the test needs to load *both*
datasets — the actual cross-dataset join being proven.

`backlog/examples/snapshot.ttl` currently has real `bl:touchesCrate` data
(e.g. `ghpull:300 bl:touchesCrate crate:shacl`, matching the real
`provenance/summaries/pr-300.ttl` worked example) but no `bl:touchesFile`
data yet — the snapshot predates PR #358's loader change and hasn't been
regenerated from a live `gh api` call since (regeneration needs a real
network call per PR, out of scope for this issue -- tracked as follow-up
[#395](https://github.com/daghovland/rdf-datalog/issues/395), unlabeled,
awaiting review). `related_to_crate`'s test therefore runs against the real
snapshot as-is. `related_to_file`'s test supplements it with one small
inlined Turtle fixture asserting
`ghpull:300 bl:touchesFile "shacl/src/evaluate.rs"` — a real file from
[PR #300](https://github.com/daghovland/rdf-datalog/pull/300)'s actual
diff (checked via `gh pr view 300 --json files`), added by hand only
because the generated snapshot hasn't caught up yet, not a fabricated
value.

`agp:abstractText` exists on most current summary files already (added
retrospectively by #358), so the primary-path (not just the fallback) is
exercised by at least one real example.

## Deliverables

- `provenance/queries/related_to_file.sparql`
- `provenance/queries/related_to_crate.sparql`
- `provenance/queries/run.sh` / `README.md` updates
- Tests in `tests/provenance_queries.rs` mirroring the existing four

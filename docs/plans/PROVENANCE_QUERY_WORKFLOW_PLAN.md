# Provenance query-driven workflow — plan

Extends the agent-provenance epic [#306](https://github.com/daghovland/rdf-datalog/issues/306).
Builds on what's already merged: the `agp:` vocabulary/shapes
([#326](https://github.com/daghovland/rdf-datalog/issues/326)), hand-authored
summaries + canned queries
([#327](https://github.com/daghovland/rdf-datalog/issues/327)), the authoring
guidelines + generalized SHACL/CI wiring
([#334](https://github.com/daghovland/rdf-datalog/issues/334)), and the
backlog GitHub-loader crate
([#284](https://github.com/daghovland/rdf-datalog/issues/284)). Raw
transcripts are now consolidated off-repo
([#333](https://github.com/daghovland/rdf-datalog/issues/333), done
2026-08-02).

## Motivation

Everything built so far answers "write down the reasoning after finishing a
PR." Nothing yet answers the other half of the stated goal: **before**
starting new work, can an agent ask the datastore "has this been touched
before, what was decided, why does this file look the way it does" — and
actually get useful hits back? This plan is about making that query side
real, not just theoretically possible.

## Current gaps, and what to do about each

### 1. Granularity stops at PR/issue — no file linkage

`AGENT_PROVENANCE_PLAN.md`'s Phase 1 deliberately scoped out file/function
granularity, reasoning that a stable code-location identifier scheme would
be a big separate design problem. That's still true for function/line level
— but **file-path level is cheaper than assumed**, because the data is
already sitting unused: `backlog`'s GitHub loader (#284) already fetches
each PR's changed-files list (`gh api .../pulls/{n}/files`) to compute
`bl:touchesCrate`, then discards the individual file paths after rolling
them up to crate level.

**Proposal**: add `bl:touchesFile` (`owl:DatatypeProperty`, domain
`bl:PullRequest`, range `xsd:string` — a repo-relative path, mirroring
`bl:Crate`'s own `bl:path`) populated from data the loader already has in
hand. No new API calls, no new identifier scheme, no revisiting the
Phase-1 granularity decision — just don't throw away information already
fetched. A summary's relevance to a file then becomes a two-hop join:
`agp:TranscriptSummary → agp:reasoningFor → bl:PullRequest → bl:touchesFile`.

### 2. No cheap way to scan many summaries before reading any

Right now every summary is one paragraph-or-two of `agp:summaryText` (30–4000
chars) — fine for reading one, expensive for scanning fifty to find the
relevant one. The user's own framing ("summaries, **abstracts**, and
relevant files") points at a two-tier design.

**Proposal**: add `agp:abstractText` (`owl:DatatypeProperty`, domain
`agp:TranscriptSummary`, range `xsd:string`, short — propose `sh:maxLength
160`, roughly a commit-subject-line length) alongside the existing
`agp:summaryText`. A query can then `SELECT ?pr ?abstract` over the whole
corpus cheaply, and only follow up with the full `agp:summaryText` for the
handful of hits that look relevant. Backward compatible: existing summaries
don't need to add it immediately (make it optional, not required, in the
strengthened shape — retrofit the three existing files opportunistically).

### 3. No query answers "what's relevant to what I'm about to do"

The existing canned queries (`provenance/queries/*.sparql`) all take a
*known* PR/issue/agent as input — useful for looking something up you
already have the ID for, useless for "I'm about to touch
`shacl/src/evaluate.rs`, has anyone reasoned about this file before."

**Proposal**: a new canned query, `provenance/queries/related_to_file.sparql`
(parameterized on a file path), doing exactly the two-hop join from gap #1:
find every summary whose PR touched a given file, print `agp:abstractText`
(or `agp:summaryText` if no abstract yet) plus the PR link. A second query,
`related_to_crate.sparql`, does the same one hop shallower via
`bl:touchesCrate` for a coarser "anyone worked in this crate before" scan
when the exact file isn't known yet (e.g. before deciding which file to
even look at).

### 4. Nothing in the workflow actually tells an agent to run these queries

CLAUDE.md's step 6b (write a summary) only fires at the *end* of a PR. There's
no equivalent nudge at the *start*.

**Proposal**: a new CLAUDE.md workflow step — inserted early, before step 3's
"delegate implementation" — something like: *"Before implementing, check
accumulated provenance for related past work: `provenance/queries/run.sh
related_to_file.sparql -- <path>` for each file the issue looks likely to
touch (or `related_to_crate.sparql` if scope is still fuzzy). Read any hits'
`agp:summaryText` before starting — this is how past decisions actually get
reused instead of re-litigated."* This is a cheap, low-risk addition (a
lookup step, not a gate) — worth doing once gaps #1–#3 give it something
real to find.

### 5. The corpus is currently too small to prove any of this is useful

Only 3 summary files exist (`pr-300`, `pr-328`, `pr-344`) against a *much*
larger set of PRs actually merged in this repo's history (including
everything from this same session — #300, #303, #305, #309–#347 and
counting). The query-driven workflow in gaps #3–#4 can't be meaningfully
validated against a corpus this thin.

**Proposal**: a backfill pass — retrospectively write
`provenance/summaries/pr-<N>.ttl` for the other significant already-merged
PRs from this repo's history, using the guidelines doc's already-documented
"retrospective" authoring mode (`gh pr view <N> --json commits,mergedAt` for
real timestamps, `bl:state bl:Closed`). Doesn't need to be exhaustive —
enough real summaries (say, the dozen-plus SHACL/reasoner/OTTR/RML PRs from
this session) to make gaps #1–#4 testable against real, varied content
rather than three cherry-picked examples.

### 6. Nothing verifies a summary's self-declared PR stub is real

Every summary file self-declares its own `ghpull:<N> a bl:PullRequest, ...`
stub inline (per the guidelines) — SHACL validates the *shape* of that
stub, not that PR #N actually exists/merged/says what the file claims.
A summary could reference a fabricated or wrong PR number and still pass
CI today.

**Proposal**: flag only, not committing to a fix here — a possible future
check (either at authoring time or as a periodic CI job) that cross-references
each summary's declared PR stub against the *real* GitHub API record (now
that #284's loader crate exists and already knows how to fetch PR metadata —
this could reuse that fetching code directly). Low urgency: summaries are
authored by trusted agents under normal PR review today, so this is an
integrity nice-to-have, not a live gap being exploited.

## Suggested phasing / sub-issues

Mirrors how #306 was broken down before (#326, #327, #334):

1. **`bl:touchesFile` + `agp:abstractText`** — the two schema additions
   (gaps #1, #2). Small, mechanical, unblocks everything else.
2. **`related_to_file`/`related_to_crate` queries** (gap #3) — depends on 1.
3. **Backfill summaries for prior merged PRs** (gap #5) — independent of 1/2,
   can run in parallel; makes 2's queries testable against real content.
4. **CLAUDE.md workflow step** (gap #4) — depends on 2 actually returning
   useful results against a real corpus (i.e., should land after 3 has put
   some volume in, not before).
5. Gap #6 (PR-stub integrity check) — no sub-issue proposed yet, flagged for
   later consideration only.

## Resolved by the repo owner (2026-08-02)

1. **Backfill scope** (gap #5): a representative sample for now, not a full
   backfill — "that way we catch bugs and modelling issues before too much
   work is done." #352 scoped down to the sample; full backfill split out
   as its own follow-up, #355, deliberately sequenced after the sample has
   proven the schema/queries sound.
2. **`agp:abstractText` length**: leave the proposed 160-char cap as-is —
   "trying to be agile here, and adjust when need arises" rather than
   pre-optimizing a number with no real data behind it yet.
3. **Persistent query serving — scope raised significantly**: not just "a
   `dagalog --serve` instance once the provenance corpus gets big." The
   repo owner wants to plan for the **whole backlog system** (issues, PRs,
   history, provenance — everything) to become something persistently
   *served*, and explicitly does NOT want its data model tightly bound to
   GitHub's own representation long-term — #287 (the GitHub write-back
   design) is "not that exciting" precisely because binding to GitHub's API
   surface constrains what the served system could become. This is now its
   own dedicated plan — see
   [`docs/plans/SERVED_BACKLOG_PLAN.md`](SERVED_BACKLOG_PLAN.md) — since it
   spans beyond this issue's provenance-only scope into #282's whole
   backlog epic and #287's write-back design.

## References

- `docs/plans/AGENT_PROVENANCE_PLAN.md` — the design this extends
- `docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md` — authoring conventions this
  reuses as-is (retrospective mode, decision points, privacy rules)
- `backlog/ontology/vocabulary.ttl`, `agentprov-vocabulary.ttl` — vocabularies
  gaps #1/#2 extend
- `backlog/src/` (the #284 loader) — source of the already-fetched file-list
  data gap #1 proposes not discarding
- `provenance/queries/` — existing canned-query pattern gap #3 extends

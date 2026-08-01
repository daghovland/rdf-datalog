# Transcript-summary authoring guidelines

Sub-issue [#334](https://github.com/daghovland/rdf-datalog/issues/334) of
the agent-provenance epic [#306](https://github.com/daghovland/rdf-datalog/issues/306),
following the Phase-1 hand-authored convention established by
[#327](https://github.com/daghovland/rdf-datalog/issues/327) and the
vocabulary/shapes from [#326](https://github.com/daghovland/rdf-datalog/issues/326).
See [`AGENT_PROVENANCE_PLAN.md`](AGENT_PROVENANCE_PLAN.md) for the full
design this extends; this document is the concrete "how to actually write
one" spec that plan explicitly left open ("This is a NEW step to add to
CLAUDE.md's workflow once implementation lands — not yet added").

**Scope note.** #334 was originally framed (and its GitHub issue body still
reads, uncorrected) as an automated ML/heuristic transcript-extraction
pipeline. The repo owner reframed it (issue comment, 2026-08-01): no
automation — trust the agent that just finished a PR to write its own
summary, and build the infrastructure (this doc, strengthened SHACL shapes,
generalized test/CI wiring) that makes a hand-authored summary trustworthy.
Nothing here reads a transcript file; it's written by the same agent that
did the work, from its own live session context, immediately after
finishing.

## When to write one

At the end of CLAUDE.md's Implementation workflow step 6 ("Commit, push,
open a PR"), before removing the worktree: write one
`provenance/summaries/pr-<N>.ttl` file, where `<N>` is the merged PR's
number. One file per finished PR — not per issue, not per session, even if
a PR took several sessions to land (see "Session window", below, for how
`pr-300.ttl` handled a multi-day PR).

Skip it for: trivial single-file documentation edits committed directly to
main (CLAUDE.md's own stated exception to the worktree/PR workflow), and
any PR where the "why" is already fully obvious from the PR title alone
(rare in practice — most PRs here have earned at least one real design
decision worth recording).

## What the file must contain

Copy the shape of
[`provenance/summaries/pr-300.ttl`](../../provenance/summaries/pr-300.ttl)
or
[`provenance/summaries/pr-328.ttl`](../../provenance/summaries/pr-328.ttl)
(both real, worked examples — read one end-to-end before writing your
first). At minimum, one `agp:TranscriptSummary` with:

- **`agp:summaryText`** — the distilled reasoning, in prose. **30–4000
  characters** (enforced by `agp:TranscriptSummaryRequiredFieldsShape` in
  `backlog/ontology/agentprov-shapes.ttl`, added for #334). That range is
  calibrated against the three real summaries already in `pr-300.ttl`
  (461/506/1101 chars) — a paragraph or two. If you're pushing past 4000
  chars, you're writing a transcript, not a summary; cut it down to the
  actual decision(s) that mattered.
- **Exactly one `prov:wasAttributedTo`** — the agent (a `prov:SoftwareAgent`
  individual, e.g. `session:claudeSonnet5` — reuse the same IRI across
  files for the same agent identity rather than minting a new one per PR).
- **At least one `agp:reasoningFor`**, pointing at a real
  `bl:PullRequest`/`bl:WorkItem` — see "The referenced PR/issue stubs"
  below for exactly what that target needs.
- **Exactly one `prov:wasGeneratedBy`**, pointing at an `agp:AgentSession`
  resource with `prov:wasAssociatedWith` the same agent, and real
  `prov:startedAtTime`/`prov:endedAtTime` (see "Session window" below —
  don't invent placeholder timestamps).

Optional, add only where genuinely applicable:

- **`agp:transcriptRef`** — a pointer to the full transcript/session
  (e.g. a `claude.ai/code/session_...` URL), if one exists and is safe to
  reference. At most one per summary (also now SHACL-enforced).
- **`agp:decisionPoint`** — see "Using `agp:decisionPoint`" below.

## What NOT to include (privacy)

Per `AGENT_PROVENANCE_PLAN.md`'s "Privacy / size" section: **only
distilled summaries go into RDF, never raw transcript text.** Concretely:

- No verbatim excerpts of tool output, file contents, or conversation turns
  — describe *what was found/decided*, not *what the transcript said*, in
  your own words, after the fact.
- No user-identifying or otherwise private detail beyond what's already
  public in the PR/issue itself (this repo's own practice: if it wouldn't
  appear in a PR description, it doesn't belong in a summary either).
  `agp:transcriptRef` is the one sanctioned pointer to "the rest of it,"
  and it must be omittable — it's for someone who already has legitimate
  access to go deeper, not a way to smuggle detail past the "distilled"
  requirement.
- No speculative/unconfirmed claims presented as settled fact — if a
  decision was contested or later revised, say so (see `pr-300.ttl`'s two
  `agp:Decision` entries for the pattern: what was decided, and what
  alternative was rejected and why).

## The referenced PR/issue stubs

`agp:reasoningFor`'s target must resolve to a real `bl:WorkItem` — SHACL
now enforces this (`sh:class bl:WorkItem` on `agp:reasoningFor`, #334).
Concretely, your summary file must itself declare:

```turtle
ghpull:<N> a bl:PullRequest, bl:WorkItem ;
    rdfs:label "<the PR's actual title>" ;
    bl:number <N> ;
    bl:state bl:Closed ;   # bl:Open if writing this prospectively, before merge -- see "Session window"
    bl:closesIssue ghissues:<M> .
```

Both the literal `bl:PullRequest` type AND the literal `bl:WorkItem` type
are required — `bl:RequiresWorkItemTypeShape` in `backlog/ontology/shapes.ttl`
checks for the explicit triple, not inferred subclass membership (this
engine does no RDFS entailment at plain-SHACL time; see
`backlog/ontology/MODELING_NOTES.md`'s "`rdfs:subClassOf` is not free").

**Do not type the referenced issue (`ghissues:<M>`) as `bl:Issue`.**
Reference it only as an object (`bl:closesIssue`, `prov:used`) — typing it
would subject it to `bl:IssueIsEpicXorHasParentShape`, which demands either
an Epic type or a `bl:subIssueOf` parent, neither of which this grounding
fixture is trying to model. Both `pr-300.ttl` and `pr-328.ttl` follow this
exactly; verify it yourself once if in doubt (type the issue and watch
`every_summary_file_conforms_to_shapes` in `tests/provenance_queries.rs`
fail).

## Session window

`prov:startedAtTime`/`prov:endedAtTime` on the `agp:AgentSession` must be
real, not invented. Two cases, because CLAUDE.md's step 6b writes the
summary file **before** the PR merges (it happens right after step 6,
"Commit, push, open a PR," and before step 7's worktree removal) — there
is no real `mergedAt` yet at authoring time:

- **Retrospective** (summarizing a PR that already merged — e.g. writing
  a summary for older history, or catching up on a PR that landed without
  one): pull real timestamps from
  `gh pr view <N> --json commits,mergedAt`. `endedAtTime` is the PR's
  actual `mergedAt`; `startedAtTime` is the first commit's `authoredDate`
  if the whole PR was one continuous session, or the final commit's
  `authoredDate` if the PR spanned multiple days/sessions (as PR #300 did
  — first two commits landed two days before the final review-and-fix
  pass; the window covers just that closing session, per `pr-300.ttl`'s
  own comment on why a multi-day span would misrepresent what "one
  session" means). `bl:state bl:Closed` on the referenced PR stub, since
  it's a real, already-merged fact. Both `pr-300.ttl` and `pr-328.ttl` are
  retrospective examples.
- **Prospective** (the normal CLAUDE.md step 6b case — writing your own
  summary right after opening the PR, in the same session that did the
  work): `startedAtTime` is still real — the first (or, for a multi-session
  PR, the most recent relevant) commit's `authoredDate` from `git log`.
  `endedAtTime` is the real wall-clock time the summary itself is being
  written (this session's own "now"), NOT a guessed future `mergedAt` — an
  invented merge timestamp would be a fabricated fact, worse than an
  honestly-labeled "this is when the session concluded, not when the PR
  merged." The referenced PR stub gets `bl:state bl:Open` (the real,
  current state at authoring time — do not write `bl:Closed` prospectively;
  no shape currently checks this value against the PR's real GitHub state,
  but writing a false fact defeats the whole point). Nothing in this
  workflow re-opens the file to flip `bl:Open` to `bl:Closed` once the PR
  actually merges — the summary is a record of what was known and decided
  at authoring time, not a live-synced mirror of PR state. See
  `provenance/summaries/pr-344.ttl` (this very PR, #334/#344) for a worked
  prospective example.

## Using `agp:decisionPoint`

Add one `agp:Decision` per **distinct fork actually reasoned through** —
not for every code change, only where there was a real choice with at
least one considered-and-rejected alternative. Each `agp:Decision` needs
its own `agp:summaryText` (same 30–4000 char bound,
`agp:DecisionRequiredFieldsShape`, #334) describing what was decided, plus
one or more `agp:alternative` strings for what was rejected and, ideally,
why. Zero, one, or several decision points per summary are all valid —
most PRs will have zero or one; `pr-300.ttl` has two because that PR's
review round surfaced two genuinely separate forks
(`sh:sourceShape` resolution, and the `sh:qualifiedMinCount`/
`sh:qualifiedMaxCount` split).

Don't force a decision point where there wasn't a real fork — "implemented
the obvious fix" is not a decision point; "considered X, went with Y
because Z" is.

## Validating before you commit

Run the same checks CI runs (see
[`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)'s "Unit &
Integration Tests" job, which already covers this — no separate job was
added for #334, see `tests/provenance_queries.rs`'s module doc for why):

```bash
export CARGO_TARGET_DIR=/home/dag/.cargo-shared-target/rdf-datalog
cargo test --test provenance_queries
```

`every_summary_file_conforms_to_shapes` validates your new file, on its
own (not merged with any other summary file — a file must be
self-contained), against `backlog/ontology/shapes.ttl` and
`backlog/ontology/agentprov-shapes.ttl`. `all_summary_files_parse` catches
plain Turtle syntax errors. Both run automatically against every
`provenance/summaries/*.ttl` file present at test time (globbed, not
hardcoded) — a new file needs no test code change to be picked up.

## Worked examples

- [`provenance/summaries/pr-300.ttl`](../../provenance/summaries/pr-300.ttl)
  — the original #327 example (two decision points).
- [`provenance/summaries/pr-328.ttl`](../../provenance/summaries/pr-328.ttl)
  — a second real, retrospective example added for #334, specifically to
  prove the glob-based test/CI wiring above actually generalizes past "the
  one hardcoded file."
- [`provenance/summaries/pr-344.ttl`](../../provenance/summaries/pr-344.ttl)
  — a **prospective** example: this very PR's own summary, written under
  step 6b before merge, following the "Session window" section's
  prospective case exactly (`bl:state bl:Open`, `endedAtTime` = authoring
  time, not a guessed `mergedAt`).

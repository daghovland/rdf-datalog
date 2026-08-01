# Agent provenance & transcript-summary ontology — plan

## Motivation

The `backlog/` ontology (#283, #293, #296, #299) already mirrors GitHub
issues/PRs/crates as RDF, queryable via SPARQL. It answers *what* changed
and *when* (`bl:closesIssue`, `bl:touchesCrate`, workflow status). It does
not answer *why* — the reasoning an agent went through, the alternatives it
rejected, or which agent/session is responsible for a given piece of code.

Goal: extend the RDF representation so a future agent or author can ask,
in SPARQL, questions like "why does `eval_qualified_value` split min/max
into two checks?" or "what did the agent conclude about the languageIn
suspicion, and which PR did that reasoning land in?" — by querying
distilled summaries of agent transcripts, linked to code via PROV-O-style
provenance relations, not by re-reading full conversation logs.

## Prior art / what already exists

- `backlog/ontology/vocabulary.ttl` — `bl:` namespace, models `bl:Issue`,
  `bl:PullRequest`, `bl:Epic`, `bl:Crate`; reuses `doap:Project` for the
  repo itself. No file/function/commit-level entities, no agent/authorship
  modeling at all today. **`backlog/` is planned to be extracted into its
  own separate repo** (repo owner, 2026-07-31) — it's a generic,
  project-agnostic ontology + tooling package, not something specific to
  `rdf-datalog`. This directly shapes where the new agent-provenance
  material goes — see "Placement" below.
- PROV-O is **not used anywhere in live code or ontologies** — the only
  presence is `tests/testdata/prov-o.ttl`, the real W3C ontology, exercised
  by `tests/real_world_ontologies.rs` purely as a "can we load a real OWL
  ontology" smoke test. No `prov:` constants exist in
  `ingress/src/namespaces.rs`.
- No GitHub API ingestion pipeline exists yet (tracked separately as
  #284) — `backlog/`'s Turtle files are all hand-authored grounding
  fixtures, validated by SHACL, exercised by canned SPARQL queries
  (`backlog/queries/`). This plan follows the same bootstrapping pattern:
  hand-authored fixtures first, live ingestion later and separately scoped.

## Design

### Namespace and reuse strategy

New vocabulary at `https://dagalog.dev/ns/agentprov#` (prefix `agp:`),
following the `bl:` precedent of reusing a real external vocabulary
wherever the term already exists (`bl:partOfProject` reuses `doap:Project`)
rather than reinventing it. Concretely: reuse `prov:Entity`, `prov:Activity`,
`prov:Agent`, `prov:SoftwareAgent`, `prov:Person`, and the relations
`prov:wasGeneratedBy`, `prov:used`, `prov:wasAssociatedWith`,
`prov:wasAttributedTo`, `prov:actedOnBehalfOf`, `prov:startedAtTime`,
`prov:endedAtTime` directly. `agp:` supplies only what PROV-O doesn't have:
the summary text itself, and a direct shortcut from a summary to the
`bl:` work item it explains.

### Classes (agp:)

- **`agp:AgentSession`** (`rdfs:subClassOf prov:Activity`) — one agent
  run: a top-level Claude Code session, or a delegated sub-agent
  invocation. Has `prov:startedAtTime`/`prov:endedAtTime`,
  `prov:wasAssociatedWith` an agent, optionally `agp:parentSession` (a
  sub-agent's dispatching session — mirrors this repo's own
  orchestrator/sub-agent pattern) and `prov:used` (the issue(s) it worked
  from).
- **`agp:TranscriptSummary`** (`rdfs:subClassOf prov:Entity`) — a short,
  distilled piece of reasoning, NOT a raw transcript dump (see Privacy
  below). `prov:wasGeneratedBy` an `agp:AgentSession`. Has exactly one
  `agp:summaryText` and `prov:wasAttributedTo` exactly one agent (shortcut
  — see below on why this is asserted directly rather than always derived
  by re-tracing `wasGeneratedBy`/`wasAssociatedWith`).
- Agents themselves reuse `prov:SoftwareAgent` (e.g. "Claude Sonnet 5") and
  `prov:Person` (the human, e.g. Dag) directly — no new class needed.
  A session's agent `prov:actedOnBehalfOf` the human who authorized it,
  matching PROV's own delegation pattern (and mirroring how sub-agents in
  this repo act on behalf of the orchestrating session, which in turn acts
  on behalf of the user).
- **Code entities, Phase 1**: reuse `bl:Issue`/`bl:PullRequest`/`bl:Crate`
  as-is — a summary explains its reasoning **for a PR or issue**, not (yet)
  for an individual file or function. See "Granularity" below for why this
  is the recommended starting scope, and what Phase 2 adds.

### Properties (agp:)

- `agp:summaryText` (`owl:DatatypeProperty`, domain `agp:TranscriptSummary`,
  range `xsd:string`) — the distilled prose. Short (a paragraph or two),
  not a full transcript.
- `agp:reasoningFor` (`owl:ObjectProperty`, domain `agp:TranscriptSummary`,
  range `bl:WorkItem`) — direct shortcut from a summary to the issue/PR it
  explains. Kept as an explicit assertion (not derived by chaining
  `prov:wasGeneratedBy`/`prov:used`) for the same reason `bl:status` isn't
  always derived from `bl:hasLabel`: the common query ("what's the
  reasoning behind PR #300") shouldn't require a property-path traversal
  through the full PROV chain every time — mirrors this repo's existing
  practice of adding a direct shortcut property alongside a more general
  indirect path when the direct query is the primary use case.
- `agp:transcriptRef` (`owl:DatatypeProperty`, range `xsd:string` or
  `xsd:anyURI`) — OPTIONAL pointer to where the full transcript lives
  (e.g. a local path or internal session ID), for someone with access who
  wants to go deeper. Not resolvable by SPARQL itself; purely informational.
  Must be safe to omit or redact — see Privacy.
- `agp:decisionPoint` (`owl:ObjectProperty`, domain `agp:TranscriptSummary`,
  range a new small `agp:Decision` class with just `agp:summaryText` and
  `agp:alternative` (repeatable string) properties) — OPTIONAL finer
  breakdown, for summaries that cover more than one distinct fork in the
  reasoning (e.g. this very session's "decision point 1" / "decision point
  2" review exchange on PR #300). Not required on every summary; add only
  where a session actually reasoned through multiple distinct forks worth
  recording separately.

### SHACL shapes (mirroring `backlog/ontology/shapes.ttl`)

- `agp:TranscriptSummaryRequiredFieldsShape` — exactly one `agp:summaryText`,
  exactly one `prov:wasAttributedTo`, at least one `agp:reasoningFor`.
- `agp:SessionHasAgentShape` — every `agp:AgentSession` has exactly one
  `prov:wasAssociatedWith`.
- `agp:SummaryGeneratedByShape` — every `agp:TranscriptSummary` has exactly
  one `prov:wasGeneratedBy` pointing at an `agp:AgentSession`.

### SPARQL query library additions (mirroring `backlog/queries/`)

- "Why was PR #N merged?" — `agp:reasoningFor` lookup, print
  `agp:summaryText`.
- "What has agent X reasoned about?" — all summaries where
  `prov:wasAttributedTo` = a given `prov:SoftwareAgent`.
- "Which sessions worked on issue #N?" — `prov:used` lookup.
- "All decision points across the backlog" — flatten `agp:decisionPoint`
  across every summary, for a review dashboard.

### Granularity: PR/issue-level only, not file/function-level (recommended)

Phase 1 deliberately stops at "reasoning behind this PR/issue," not "reasoning
behind this specific function." Reasons:
- Every code entity this repo currently tracks in RDF (`bl:Issue`,
  `bl:PullRequest`, `bl:Crate`) is already coarse-grained; PR-level
  provenance can be built entirely by reusing those existing IRIs (a real
  GitHub PR URL) with zero new code-location identifier scheme.
- File- or function-level attribution needs a stable naming scheme for
  code locations (path + optional symbol/line range) that doesn't yet
  exist anywhere in this codebase and would drift against refactors/renames
  — a much bigger design problem on its own, better scoped as its own
  follow-up once PR-level provenance is proven useful.
- The stated aim — "ask for reasoning behind code" — is already well
  served at PR granularity for the overwhelming majority of real questions
  ("why does the SHACL evaluator do X" is almost always answered by "read
  the PR that introduced X"), since this repo's own workflow already
  requires one focused PR per issue.

If finer granularity turns out to matter in practice, Phase 2 would add an
`agp:CodeLocation` class (file path + optional symbol name, no line
numbers — lines drift too fast to be worth tracking) that
`agp:reasoningFor` can also target, without changing anything in Phase 1's
design.

### Placement (resolved 2026-07-31)

Split by whether the material is generic/portable or specific to this
repo's own history, matching how `backlog/` itself is heading for
extraction into a separate repo:

- **The `agp:` vocabulary and its SHACL shapes are generic** — reusable by
  any project that wants agent-provenance triples, exactly like `bl:`
  itself. They live alongside the existing ontology files, as new sibling
  files (not folded into `vocabulary.ttl`/`shapes.ttl` themselves, to keep
  each concern independently reviewable):
  - `backlog/ontology/agentprov-vocabulary.ttl`
  - `backlog/ontology/agentprov-shapes.ttl`

  These travel with `backlog/` when it's extracted.
- **Individual transcript summaries are specific to `rdf-datalog`'s own
  history** — real PR numbers, real session reasoning about this
  codebase — and must NOT move when `backlog/` is extracted. They live in
  a new top-level directory that stays in this repo:
  - `provenance/summaries/pr-<N>.ttl` (one file per finished PR)
  - `provenance/queries/*.sparql` (mirroring `backlog/queries/`, for the
    provenance-specific query library in "SPARQL query library additions"
    above)

  `provenance/`'s Turtle files `@import`/reference the `agp:`/`bl:`
  vocabulary by IRI as normal (no local copy), the same way
  `backlog/examples/*.ttl` references `bl:` today.

## Authoring workflow (how the data actually gets created)

**Phase 1 (near-term, hand-authored, mirrors `backlog/examples/`):**
When an agent finishes a PR under this repo's existing workflow (CLAUDE.md
step 6, "Commit, push, open a PR"), it also writes one small Turtle file
under `provenance/summaries/pr-<N>.ttl` (see "Placement" above) containing
exactly one `agp:TranscriptSummary` — a few sentences distilling the
actual reasoning (not a transcript dump), `agp:reasoningFor` the PR/issue,
`prov:wasGeneratedBy` a session resource, `prov:wasAttributedTo` the agent.
**Self-authored summaries are acceptable for now** (repo owner,
2026-07-31) — the PR-finishing agent writes its own summary rather than a
separate reviewing agent; revisit if self-reporting bias turns out to be a
real problem in practice. Added as CLAUDE.md's Implementation workflow step
6b (issue [#334](https://github.com/daghovland/rdf-datalog/issues/334)),
which also links the concrete authoring spec: see
[`TRANSCRIPT_SUMMARY_GUIDELINES.md`](TRANSCRIPT_SUMMARY_GUIDELINES.md).

**Phase 2 (future, separate issue, depends on #284):** an automated
ingestion tool that reads real transcripts (or PR descriptions, which
already contain a lot of this reasoning by convention — see how #300 and
#303's PR bodies in this very session were written) and generates
`agp:TranscriptSummary` triples without a human/agent hand-authoring each
one. Explicitly out of scope for the first PR under this plan.

## Privacy / size

- Only distilled summaries go into RDF — never raw transcript text (some
  sessions handle private data explicitly excluded from repos, per this
  session's own SHACL work touching data the user "couldn't share").
- `agp:transcriptRef` is optional and MUST be omittable — a summary with
  no `transcriptRef` at all is a fully valid, complete
  `agp:TranscriptSummary`.

## Review status

Resolved by the repo owner, 2026-07-31:

1. **Authoring**: self-authored summaries (the PR-finishing agent writes
   its own) are acceptable for now — see "Authoring workflow" above.
2. **Granularity**: not explicitly revisited; the recommended PR/issue-level
   scope (no `agp:CodeLocation` in Phase 1) stands as the default. Flag if
   that reading is wrong.
3. **Placement**: resolved with a split — generic `agp:` vocabulary/shapes
   go in `backlog/ontology/` (travels with `backlog/`'s eventual repo
   extraction); this repo's own transcript-summary instance data goes in a
   new `provenance/` top-level directory that stays put — see "Placement"
   above.

Nothing is implemented yet. No issue this plan produces should be labeled
`ready` without the repo owner's explicit go-ahead, per this repo's own
agent workflow rules — this review resolves the design, not the
`ready` gate itself.

**Update (2026-08-01, issue #334):** the originally-planned Phase 2
(automated transcript-extraction pipeline) was reframed by the repo owner
— no ML/heuristic extraction; instead, trust the PR-finishing agent to
hand-author its own summary (this section's existing "self-authored is
acceptable" resolution), and build the supporting infrastructure: the
concrete authoring spec
([`TRANSCRIPT_SUMMARY_GUIDELINES.md`](TRANSCRIPT_SUMMARY_GUIDELINES.md)),
strengthened SHACL shapes (`backlog/ontology/agentprov-shapes.ttl`'s
`sh:minLength`/`sh:maxLength` on `agp:summaryText` and `sh:class
bl:WorkItem` on `agp:reasoningFor` — see `MODELING_NOTES.md`'s "#334 SHACL
strengthening"), a glob-based (not hardcoded-single-file) test/CI loader
for `provenance/summaries/*.ttl` (`tests/provenance_queries.rs`), and
CLAUDE.md's new step 6b. See #334 for the full discussion.

## References

- `backlog/ontology/vocabulary.ttl`, `backlog/ontology/shapes.ttl`,
  `backlog/ontology/MODELING_NOTES.md` — the pattern this plan mirrors.
- `tests/testdata/prov-o.ttl`, `tests/real_world_ontologies.rs` — existing
  (smoke-test-only) PROV-O exposure in this repo.
- W3C PROV-O: <https://www.w3.org/TR/prov-o/>

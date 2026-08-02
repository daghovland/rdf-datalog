<!-- Render with: npx @marp-team/marp-cli fagkaffe-dagalog-journey.md -o slides.pdf  (or .pptx / .html) -->
---
marp: true
theme: default
paginate: true
---

# dagalog: the journey so far

A fagkaffe about a triplestore, a lot of markdown, and learning to
delegate to sub-agents so I don't burn out my own context window

---

## What is dagalog again?

- An RDF triplestore, written in Rust
- Native OWL-RL reasoning, compiled down to datalog rules
- SPARQL 1.2, JSON-LD 1.1, SHACL, Turtle/TriG, Manchester Syntax, RML...
- ~348 commits in, still very much alive

Started as a rewrite. Turned into its own thing.

---

## Phase 1: it started as a port

`DagSemTools` (F#/.NET) → Rust, crate by crate.

The plan doc (`docs/architecture/PLAN.md`) literally has a mapping table:

| DagSemTools project | Rust crate | Status |
|---|---|---|
| `Rdf` | `dag_rdf` | Done |
| `Datalog` | `datalog` | Done |
| `OwlOntology` | `owl_ontology` | Done |
| `Turtle.Parser` | `turtle_parser` | Done |
| `AlcTableau`, `OWL2ALC` | `alc_tableau` | Deferred |

Goal stated up front: fast triplestore, native Rust OWL-RL/datalog reasoning.

---

## Phase 2: the markdown-plan habit

`docs/plans/` currently has **47** planning documents.

This isn't accidental clutter — it's the actual workflow: write the plan
*before* the code. A few real ones sitting there right now:

- `SHACL_PLAN.md` / `SHACL_COMPLEX_PATHS_PLAN.md`
- `MANCHESTER_SYNTAX_PLAN.md`
- `OTTR_PLAN.md`, `RML_PLAN.md` (and five more `RML_*_PLAN.md` siblings)
- `AGENT_PROVENANCE_PLAN.md` — plot twist, more on this later

If it's a real feature, there's a markdown doc for it somewhere first.

---

## Phase 3: test-driven, for real

Not a platitude — it's a written-down phase order in `CLAUDE.md`:

1. Write a plan (markdown)
2. Write tests, stub just enough to compile, **mark them `#[ignore]`**,
   implement nothing
3. Go test-by-test: un-ignore, implement just enough to go green,
   check for code smell, *then* move to the next test

Tests get checked by the user before implementation even starts.
Red before green, every time.

---

## Phase 4: from plans to a GitHub backlog

Somewhere along the way, "what's done" moved out of markdown and into
GitHub issues — tracked under the **Dagalog** project
(github.com/users/daghovland/projects/11).

- Roughly 354 issues and 349 PRs opened so far (highest numbers so far —
  rough proxies, not exact totals)
- ~131 PRs merged
- Structure: top-level issues are **epics**, actual work happens on
  **sub-issues**

Docs stay in markdown; *status* lives in issues now.

---

## Current rule #1: the `ready` label

An agent may only start work on an issue **labeled `ready`**.

- That label is applied by Dag, after he's reviewed it
- Doesn't matter how obviously-correct or well-scoped it looks
- If an agent writes its own follow-up issue, it stays unlabeled —
  awaiting review, not picked up in the same session

No self-service. Even the orchestrator has to wait for the label.

---

## Current rule #2: worktrees + push early

Once an issue is `ready`:

```bash
git worktree add .claude/worktrees/<branch-name> -b <branch-name>
```

- Work happens in an isolated worktree, on its own branch
- PR gets opened **early** — even with minimal info — and pushed often

Why so eager? The agent environment is prone to token limits and
reboots. A pushed branch survives a crash; an unpushed one doesn't.

---

## Current rule #3: never merge your own PR

Probably the single most repeated line in the whole workflow:

> **Never merge the PR yourself, under any circumstance** — not even
> with green CI, not even after independently reviewing it yourself.

Applies to every agent, including one reviewing another agent's work.
Dag always does the final merge, after his own look at the diff.

The point: a human stays in the loop on *every single change*,
no carve-outs for "this one's obviously fine."

---

## Full circle: dagalog tracking dagalog

Since PR #328/#334 (`docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md`,
epic #306): before removing the worktree, the agent writes a short
**transcript summary** — the real reasoning behind the PR — as RDF:

```
provenance/summaries/pr-<N>.ttl
```

`tests/provenance_queries.rs` SHACL-validates it against
`backlog/ontology/agentprov-shapes.ttl` automatically.

This project builds RDF/SHACL/SPARQL tooling — and is now using that
exact stack to keep an RDF record of its own development history.

---

## Why sub-agents at all?

A single long session doing *everything* — reading files, running
tests, going back and forth on a fix — fills up its own context long
before a batch of real work is done.

So the orchestrating session's job became:

- Plan, pick a `ready` issue
- Brief a sub-agent thoroughly: issue, affected files, exact TDD
  phase, the quality-gate commands to run
- Dispatch it into an isolated worktree to do the actual work
- **Verify** the result — real CI status, not just the sub-agent's
  self-report

---

## The orchestrator stays a manager, not an implementer

- Sub-agents burn their own context on file contents and tool output
- The orchestrator's context stays free to keep orchestrating —
  one task, or several in parallel, without ballooning
- On purpose, not by accident

That's the actual shape of how work gets done in this repo right now.

---

## Questions?

- Repo: `daghovland/rdf-datalog`
- Backlog: github.com/users/daghovland/projects/11
- Everything in this talk came from `git log`, `CLAUDE.md`,
  `docs/architecture/PLAN.md`, and `gh issue/pr list` — no invented
  numbers

Coffee's probably cold by now. Thanks!

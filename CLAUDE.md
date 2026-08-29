# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Test-driven development

Implementation of new features follow test-driven development and go in these phases

2. First a plan is created in a markdown document
2. Then tests are created, necessary code for the tests to compile is stubbed, the tests are ignored and no implementation is done
3. Implementation is done by going through all tests in some order that makes sence, probably from easiest first or after some phase or feature grouping.  For each test, unignore it, make enough code to implement and make it green, finally check for code smells. Only then go on to the next test.

Always create tests that cover new functionality before creating the functionality. The tests are initially ignored and tests are usually checked by the user before implementaiton.

## Github backlog

The backlog and progress overview is in the github project "Dagalog" https://github.com/users/daghovland/projects/11. 
All issues must be made under the Dagalog project, and they must either be epics, or sub-issues of epics. 
Documentation and architecture can be in local markdown, but information about what is complete, what is planned, what is in progress is 
in issues under this project in github. 
The top-level issues under the project are larger "epics". Most concrete work will be on a sub-issue and not on the top-level.

Include links to relevant epics (or issues) in markdwon documentation, and avoid mentioning work status in repository documentaton, use the issues for this.
Include links to relevant documentation in the issues and epics. Whenever mentioning documentation in the issue, create actual clickable links. 
Reference the current working branch of the repository in the issue when working on it. 

When marking code as incomplete, f.ex. tests that are ignored, dead code that is allowed, or comments with todo's, always link to one or more issues or epics that will fix it

**Follow-up work discovered mid-task must always become a real GitHub issue, not just prose.** When a plan doc, PR description, or investigation identifies deferred/out-of-scope work (e.g. "not built in this PR", "left as a follow-up", "a bigger lift than this issue's scope"), file it as an actual issue in the Dagalog project at Status `Todo`, awaiting the user's review (per the rule below), at the point it's identified — do not just mention it in the plan/PR text and leave it there. Prose in a merged PR is easy to lose track of; an issue is trackable and searchable. This applies to every agent, including sub-agents delegated a task — brief them to file the issue themselves (at Status `Todo`) rather than only noting the gap for the orchestrating session to review later.

When creating an issue leave its Status at `Todo` — it needs the user to review it and set Status to `Agent` before any agent may pick it up (see "Implementation workflow" below). When working on it mark it as `In Progress` **as early as possible** — see step 1 of "Implementation workflow" below — use a worktree to create a new branch and note the agent and worktree id in the issue.

The GitHub project's own Status field (Todo/Agent/In Progress/Review/Done) is the authoritative "what's being worked on" signal when scanning for work — an issue already `In Progress` (or later) must never be picked up by a second agent even if it looks otherwise idle. Status only ever advances Todo → Agent → In Progress → Review → Done: `Agent` is set by the user to authorize an agent to start (replacing the older `ready` label as the pickup gate — some issues may still carry that label for reference, but Status is authoritative); `Review` is set by the agent once the PR is open and CI is green (ready for review, not yet merged); `Done` means the PR has actually merged (set by the user, not agents) — it is **not** the same event as marking `Review`, and the issue only actually closes on merge via `Closes #N`. Use `scripts/set-issue-status.sh <issue#> <Todo|Agent|"In Progress"|Review|Done>` rather than hand-writing the Projects v2 GraphQL each time.

The environment the agents and subagents run in is prone to token limitation and rebooots.
Agents should therefore early in the work create a branch, push that branch to the repo and create a pull request.
The initial pull request can have minimal infomration initialy, but the branch should be pushed often, to avoid information loss.
But only the work relevant branch should be pushed to.

When finished finalize the pull request between that branch and main before removing the worktree. The pull request and issue should be linked so the issue becomes closed when the pull request is merged.

## Implementation workflow

All code changes (bug fixes, features) follow this workflow:

1. **Pick an issue** from the [Dagalog backlog](https://github.com/users/daghovland/projects/11) — **only an issue whose Status is `Agent`** (not `Todo`, and not already `In Progress`/`Review`/`Done` — a Status past `Agent` means another agent already claimed it, or it's not been reviewed yet, even if this session doesn't remember doing so). Status `Agent` is set by the user (Dag) after reviewing the issue, and replaces the old `ready` label as the "you may start this" gate (some older issues may still carry the `ready` label for reference, but Status is authoritative — see the Github backlog section above). An agent (including Claude when orchestrating other agents) must never start work on an issue whose Status isn't `Agent`, no matter how well-scoped or obviously-correct the issue looks. If you write an issue yourself (e.g. after finding a bug or filing a follow-up), leave its Status at `Todo` and tell the user it's awaiting review — do not set it to `Agent` yourself and do not start work on it in the same session.

   **Mark the issue's Status `In Progress` immediately** — before creating the worktree, definitely before delegating to a sub-agent (`bash scripts/set-issue-status.sh <issue#> "In Progress"`) — and post the working branch name as a comment once you do start. This is the single most important ordering in this whole workflow: doing it late is what lets two agents pick up the same issue.
2. **Create a worktree** for isolation:
   ```bash
   git worktree add .claude/worktrees/<branch-name> -b <branch-name>
   ```
2b. **Check accumulated provenance for related past work** before delegating (issue [#353](https://github.com/daghovland/rdf-datalog/issues/353), part of the agent-provenance epic [#306](https://github.com/daghovland/rdf-datalog/issues/306) — see [`docs/plans/PROVENANCE_QUERY_WORKFLOW_PLAN.md`](docs/plans/PROVENANCE_QUERY_WORKFLOW_PLAN.md)). This is a lookup/nudge, not a hard gate — cheap, and worth doing even when nothing turns up.

   For each file the issue looks likely to touch:
   ```bash
   provenance/queries/run.sh related_to_file '"path/to/file.rs"'
   ```
   (note the literal double quotes inside the single quotes — `bl:touchesFile` is a string-valued path, not an IRI). If the exact files aren't known yet, scan the whole crate instead:
   ```bash
   provenance/queries/run.sh related_to_crate crate:crate_name
   ```
   Read any hits' `agp:summaryText` (or the shorter `agp:abstractText` when present) before starting — this is how past decisions actually get reused instead of re-litigated. Note which files/crates you checked and what you found (or didn't); step 6 requires reporting this in the PR description, in the fixed format below, so record it now rather than reconstructing it later.

   **Trace format (required in the PR description, step 6):** a `## Prior provenance checked` section with two lines, so the outcome is greppable and consistent across PRs rather than free-form prose that could be phrased differently every time:
   ```
   queried: <files/crates checked, e.g. `sparql_parser/src/lib.rs`, `crate:sparql_parser`>
   provenance-checked: applied — #<PR>[, #<PR>...]
   ```
   or
   ```
   queried: <files/crates checked>
   provenance-checked: none-relevant
   ```
   or, when the step was deliberately skipped (e.g. the issue only touches brand-new files with no prior history to look up) — an explicit `skipped` line, not silence, so "chose not to check" stays distinguishable from "forgot to write the section":
   ```
   provenance-checked: skipped — <reason>
   ```
   This is intentionally measurable, not just a compliance nudge: the point (per Dag, [#353](https://github.com/daghovland/rdf-datalog/issues/353)) is to later be able to tell whether checking provenance actually made agents more efficient/grounded, which requires a visible, auditable trace of what happened at this step, not just "read it and proceed" silently.
3. **Delegate implementation to a sub-agent** by pointing it at the worktree path created in step 2 (plain `Agent` call, no `isolation` parameter — tell the sub-agent in the prompt to `cd` into that exact path for every command). The main session orchestrates; the sub-agent does the actual editing, building, and testing. Brief the sub-agent with: the issue description, affected files, the TDD phase required, the worktree path, and any provenance hits found in step 2b.

   **Do not also pass `isolation: "worktree"` here.** It does not attach to the worktree you just created — it makes the harness create a *second*, separately-named worktree/branch of its own (`.claude/worktrees/agent-<id>` on a branch like `worktree-agent-<id>`) and the sub-agent works there instead, leaving the one from step 2 empty. You end up tracking two worktrees for one task, and anything referencing the step-2 branch name (the GitHub issue comment, this session's own bookkeeping) points at the wrong one. If you don't need a predictable branch name and are fine letting the harness pick, skip step 2 entirely and use `isolation: "worktree"` on its own instead — but then read the actual worktree/branch name back from the agent's result rather than assuming it matches what you asked for, and use *that* name in the issue comment and everywhere else.
4. **TDD inside the worktree** — sub-agent follows the red→green phases above. For pure refactors with no observable behavior change, one-pass (tests alongside implementation) is acceptable.
5. **Quality checks** before committing (run in the worktree):
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Commit, push, open a PR** with `Closes #<issue>` in the body (so the merge auto-closes the issue) and the `## Prior provenance checked` section from step 2b (required, not optional — see that step for the exact format). **Never merge the PR yourself, under any circumstance** — not even when CI is fully green and you've independently verified the change. This applies to every agent, including Claude reviewing another agent's work. The user (Dag) always does the merge after their own look at the diff; your job ends at "PR open, reviewed, CI green, ready for you." Once CI is fully green, mark the issue's Status `Review` (`bash scripts/set-issue-status.sh <issue#> Review`) — this reflects "the work is finished and awaiting your review," not "the issue is closed" or "merged"; `Done` is reserved for after the PR actually merges (set by the user, not by agents), and the issue only actually closes when you merge.
6b. **Write a transcript summary** before removing the worktree: one `provenance/summaries/pr-<N>.ttl` file distilling the actual reasoning behind the PR you just finished, per [`docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md`](docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md) (issue [#334](https://github.com/daghovland/rdf-datalog/issues/334), part of the agent-provenance epic [#306](https://github.com/daghovland/rdf-datalog/issues/306)). Self-authored — you write your own summary, not a separate reviewing agent. `tests/provenance_queries.rs` picks up any new file under `provenance/summaries/` automatically and SHACL-validates it against `backlog/ontology/agentprov-shapes.ttl`.
7. **Remove the worktree** once the PR merges (keep it around until then — conflict-resolution or review-feedback commits may still need to land on the branch):
   ```bash
   git worktree remove .claude/worktrees/<branch-name>
   ```

**Disk usage:** all worktrees share one Cargo target dir at `/home/dag/.cargo-shared-target/rdf-datalog` via `CARGO_TARGET_DIR` set in the shell profile (`~/.bashrc` and `~/.profile`), instead of each worktree building its own ~15GB `target/`. This dir is never pruned automatically and grows unbounded (observed 24GB+ after a few parallel feature branches) — run `cargo clean` in it periodically once no worktree is mid-build. Concurrent builds across worktrees serialize on Cargo's own lock file, but incremental artifacts are keyed by crate, not by worktree: two worktrees editing the same crate's public API at the same time (e.g. both touching `sparql_parser::ast::Term`) can transiently see stale/phantom compile errors from each other's fingerprints. This has only produced transient failures so far (final builds came out clean, and CI is authoritative regardless of local state) but is a known hazard — **if a build error doesn't match what's actually in the file, `touch` the files it complains about and rebuild before debugging the "error"** (e.g. `touch sparql_endpoint/src/lib.rs && cargo build -p sparql-endpoint --tests`) — this has resolved every instance of this hazard seen so far. Still remove worktrees promptly once merged (step 7 above): the checkout itself (source files) takes space too, and stale worktrees clutter `git worktree list`.

**Recovering from interrupted sub-agents:** the environment is prone to mid-task interruption (reboots, hitting an API spend limit). A sub-agent that reports "done" or shows green CI is not proof the work is real — check `scripts/worktree-status.sh` first: it lists every worktree, its branch's ahead/behind vs `origin/<branch>`, and any uncommitted changes, so real-but-stranded work (committed locally but never pushed, or written but never committed) is visible in one command instead of manually `cd`-ing into each worktree. A PR with all-green CI can still be trivially green because only a plan/test-only commit ever reached origin — always check `gh pr diff --name-only` against the PR's stated scope, not just `statusCheckRollup`, before trusting it.

**Scripts:**
- `scripts/worktree-status.sh` — ahead/behind + dirty-state summary across all worktrees (see above).
- `scripts/pr-ready-if-green.sh <PR#>...` — for each PR, checks CI is fully green and, if so, marks it ready for review (`gh pr ready`) and posts a short confirmation comment. Never merges.
- `scripts/new-provenance-summary.sh <PR#> <issue#> <branch>` — scaffolds a `provenance/summaries/pr-<N>.ttl` file (prefixes, `PullRequest`/`AgentSession` triples, real timestamps from `git log <branch>`) with a `TODO` placeholder for the actual `summaryText`/`decisionPoint` prose, which must still be written by hand per [`docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md`](docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md).
- `scripts/set-issue-status.sh <issue#> <Todo|Agent|"In Progress"|Review|Done>` — sets the Dagalog project's Status field via the Projects v2 GraphQL API (not a label). Agents use this at step 1 (`In Progress`, before delegating) and step 6 (`Review`, once CI is green) of the workflow above; `Todo`/`Agent`/`Done` are set by the user, not agents.

There is no exception for "trivial" changes, including documentation-only ones: every change goes through worktree → branch → PR, no direct pushes to `main`.

## Commands

```bash
# Build all workspace members
cargo build

# Run tests (all workspace members)
cargo test

# Run tests for a specific crate
cargo test -p dag-rdf
cargo test -p ingress

# Run a single test by name
cargo test test_add_and_get_resource

# Run the main binary
cargo run

# End-of-task quality checks (run before handing work back)
# These mirror the CI jobs in .github/workflows/ci.yml exactly.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --release
cargo check --workspace --all-targets
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
cargo audit

```

## Planning and protocol documents

- **`docs/architecture/PLAN.md`** — full implementation roadmap (phases 1–8, crate mapping from DagSemTools, suggested order)
- **`docs/architecture/PROTOCOLS.md`** — W3C protocol compliance reference (SPARQL 1.1 Protocol, Graph Store HTTP Protocol, Service Description, VoID, content negotiation, CORS)
- **`docs/plans/`** — feature area plans and known-issues tracking

## Architecture

Goal: fast RDF triplestore with native OWL-RL reasoning over datalog, JSON-LD 1.1 support, and a standards-compliant SPARQL HTTP endpoint.

```
dagalog (root binary + library)
├── ingress/             — RDF data types and vocabulary constants
├── dag_rdf/             — Graph element storage, quad indexing, Datastore
├── datalog/             — Datalog engine (rules, stratifier, reasoner)
├── owl_ontology/        — OWL 2 type hierarchy (axioms, ontology)
├── eli/                 — EL profile → datalog (ELI2RL)
├── owl2rl2datalog/      — OWL 2 RL → datalog (W3C spec §4.3)
├── rdf_owl_translator/  — RDF triples → OWL 2 axiom extraction
├── turtle_parser/       — Turtle/TriG parser (rio_turtle)
├── jsonld_parser/       — JSON-LD 1.1 parser + serialiser (serde_json)
├── sparql_parser/       — SPARQL 1.2 SELECT parser (nom) + executor
├── datalog_parser/      — Datalog rules parser (nom)
├── sparql_endpoint/     — HTTP SPARQL endpoint (axum + tokio)
└── manchester_parser/   — OWL Manchester syntax parser (nom-based, `.omn` → `owl_ontology::Ontology`)
```

## Architecture decisions
Update this document if architecture changes. Update relevent elements in README.md. 

### `ingress` crate
Core RDF type hierarchy: `IriReference`, `RdfResource`, `RdfLiteral`, `GraphElement`, `PrefixDeclaration`, `OntologyVersion`. Also exports all RDF/RDFS/OWL/XSD namespace constants from `namespaces.rs`.

### `dag_rdf` crate
Storage layer on top of `ingress`:
- `GraphElementManager` — interning store: `GraphElement` → `GraphElementId` (`u32`). ID 0 is always the default graph (`urn:x-arq:DefaultGraph`), pre-populated on construction.
- `QuadTable` — multi-index store for quads with indexes by predicate, subject+predicate, object+predicate, graph ID, and full-quad dedup.
- `Datastore` — pairs two `QuadTable`s (`named_graphs` + `reified_triples`) with a `GraphElementManager`. The main data container passed through the whole pipeline.
- `query.rs` — `Term` (Resource/Variable) and `QuadPattern`, plus `get_default_graph_pattern()` helper.

### `datalog` crate
Datalog evaluation engine:
- `types.rs` — `Rule`, `RuleHead`, `RuleAtom`, `Substitution`, `QuadWildcard`, `PartialRule`
- `datalog.rs` — `evaluate_pattern`, substitution building, `apply_substitution_quad`, wildcard expansion
- `unification.rs` — `quad_patterns_unifiable`, `PatternEdge`, `depending_rules`, `intentional_rules`
- `stratifier.rs` — `RulePartitioner`: topological sort with negation cycle detection (Kahn's algorithm)
- `reasoner.rs` — `DatalogProgram` (naive forward-chaining materialisation), `evaluate_rules(rules, datastore)`

### `owl_ontology` crate
Pure OWL 2 data types: `ClassExpression`, `ObjectPropertyExpression`, `DataRange`, `Axiom` (and all variants), `Ontology`, `OntologyDocument`. No logic, just the type hierarchy from the W3C OWL 2 spec.

### `eli` + `owl2rl2datalog` crates
Two-stage OWL → datalog translation:
1. `eli`: ELI class axioms → normalized `Formula`s → datalog `Rule`s (via `eli_axiom_extractor` + `generate_tbox_rl`)
2. `owl2rl2datalog`: full OWL 2 RL ontology → `Vec<Rule>` via `owl2datalog(resources, ontology)`. ABox `Assertion` axioms (from frame-based syntaxes like Manchester, which have no RDF-quad stage) are materialised into `Datastore` quads by `assert_abox(datastore, ontology)` (module `abox`) — the ground-triple counterpart to `owl2datalog`'s rule compilation. Non-atomic class/property expressions in assertions are skipped with a `log::warn!`. Anonymous individuals are interned via `GraphElementManager::get_or_create_named_anon_resource`, keyed by a namespaced string derived from the parser-assigned id, so they draw from the same monotonic blank-node counter as RDF-ingested blank nodes and can never numerically collide with them ([#183](https://github.com/daghovland/rdf-datalog/issues/183)). `.omn` is wired into the CLI (`dagalog::load_file`/`apply_ontologies`) and the `dagalog-kernel` notebook kernel (`%%manchester <path>`) — see [#161](https://github.com/daghovland/rdf-datalog/issues/161). Graph Store Protocol content negotiation for Manchester Syntax is deferred to [#291](https://github.com/daghovland/rdf-datalog/issues/291) pending [#177](https://github.com/daghovland/rdf-datalog/issues/177) (Manchester TBox has no RDF triple representation yet, so a GSP round-trip would silently drop it).

### `jsonld_parser` crate
JSON-LD 1.1 parser (`parse_jsonld`) and serialiser (`serialize_jsonld`, `serialize_jsonld_expanded`, `serialize_jsonld_flattened`). Uses `serde_json` for JSON handling. The parser populates a `Datastore` directly; the serialiser reads all quads back and emits expanded JSON-LD value objects. Context processing supports: term mappings, prefixes, `@vocab`, `@base`, `@language`, compact IRIs, `@type` coercion, all container types, `@reverse`, `@included`, `@nest`, keyword aliasing, property-scoped and type-scoped contexts. External context URL fetching (`@import`) is tracked in [#82](https://github.com/daghovland/rdf-datalog/issues/82).

### `sparql_parser` crate
nom-based SPARQL 1.2 parser and in-memory executor. Supports: `SELECT`, `DESCRIBE`, `ASK`, `CONSTRUCT`; basic graph patterns, `FILTER`, `OPTIONAL`, `UNION`, `GRAPH`, `BIND`, `VALUES`, `DISTINCT`, `LIMIT`, `OFFSET`, `SELECT *`; property paths; aggregates (`GROUP BY`, `HAVING`, `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `SAMPLE`, `GROUP_CONCAT`); `FROM`/`FROM NAMED`. Missing features are tracked in [#48](https://github.com/daghovland/rdf-datalog/issues/48). `BASE <iri>` directive parsing and relative-IRI resolution (RFC 3986, via `oxiri`) are supported: `ParserContext::base` holds the effective base (caller-supplied default, overridable by an in-query `BASE`); with no base at all, relative IRIs are kept verbatim rather than erroring, matching pre-existing behavior relied on by some W3C suite fixtures. See [#217](https://github.com/daghovland/rdf-datalog/issues/217).

### `sparql_endpoint` crate
`axum`-based HTTP server exposing SPARQL 1.1 Protocol endpoints (`GET /sparql`, `POST /sparql`), Service Description, content negotiation, and CORS. State is an `Arc<RwLock<Datastore>>`.

### `manchester_parser` crate
nom-based OWL 2 Manchester Syntax (`.omn`) parser producing an `owl_ontology::Ontology`. Covers ontology headers/prefixes/imports, entity frames (`Class:`, `ObjectProperty:`, `DataProperty:`, `Individual:`, `AnnotationProperty:`), common class expressions/restrictions, and the `Class:` frame's `DisjointUnionOf:` section. See [`docs/plans/MANCHESTER_SYNTAX_PLAN.md`](docs/plans/MANCHESTER_SYNTAX_PLAN.md) for the exact grammar subset in scope; remaining deferred productions (SWRL `Rule:` frames [#498](https://github.com/daghovland/rdf-datalog/issues/498), `HasKey:` [#499](https://github.com/daghovland/rdf-datalog/issues/499), property chains [#500](https://github.com/daghovland/rdf-datalog/issues/500), compound data ranges [#501](https://github.com/daghovland/rdf-datalog/issues/501), `Datatype:` frame [#502](https://github.com/daghovland/rdf-datalog/issues/502)) were split out of the original umbrella issue [#157](https://github.com/daghovland/rdf-datalog/issues/157).

### Key design pattern
All graph elements are interned through `GraphElementManager`: store a `GraphElement` → get back a `GraphElementId` (`u32`). Triples and Quads only hold IDs. Resolve IDs back to values via `get_graph_element` / `get_resource_triple` / `get_resource_quad`.

## Integration tests

The test suite is the best reference for what actually works:

| Test file | Coverage |
|---|---|
| `tests/readme_examples.rs` | Every code example in `README.md` |
| `tests/api_integration.rs` | Turtle parsing, SPARQL SELECT, Datalog reasoning (ported from DagSemTools) |
| `tests/owl_integration.rs` | OWL ontology loading, OWL-RL reasoning (ported from DagSemTools) |
| `tests/sparql12_suite.rs` | SPARQL 1.2 spec conformance (§2–§15) |
| `tests/jsonld_suite.rs` | JSON-LD 1.1 spec examples (§3–§5), serialisation, round-trips |
| `tests/datalog_integration.rs` | Datalog rule parsing and evaluation |
| `tests/performance.rs` | Large-ontology smoke tests (ignored by default; require download) |

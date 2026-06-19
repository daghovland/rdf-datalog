# Documentation improvement plan

## Problem

The current docs are a single flat folder (`docs/architecture/`, `docs/plans/`) aimed at
developers and Claude sessions. There is nothing tailored to a new user who just wants to
load data and run queries.

## Goal

1. A clearly signposted user docs section that is easy to navigate for someone who has never
   heard of Datalog or RDF triplestores.
2. Developer docs that stay useful and don't rot.
3. Runnable `examples/` crate.
4. A live playground in the web UI (pre-filled sample data + queries).
5. Better `cargo doc` coverage on the public library API.
6. A `CONTRIBUTING.md` at the root.
7. Architecture Decision Records (ADRs) for key design choices.

---

## Two tracks

### Track 1 — Pure documentation (no TDD cycle)

These are file creates/moves with prose. No compilation, no test stubs.

### Track 2 — Code changes (TDD cycle applies)

Code changes follow the normal red → green workflow: tests first, ignored, reviewed, then
implemented.

---

## Track 1: Documentation restructure

### Constraint: do NOT move existing paths

The current `docs/architecture/` and `docs/plans/` paths are referenced in:

- `CLAUDE.md` (2 references)
- `README.md` (7 references)
- Rust source comments in `sparql_endpoint/tests/fuseki_compat.rs` (~40 references),
  `shacl/src/lib.rs`, `sparql_parser/tests/parser_tests.rs`, `tests/owl_integration.rs`,
  `sparql_endpoint/src/auth.rs`

Moving them would require updating ~50+ source locations and risks breaking deep-links in
external references (GitHub, bookmarks). **Leave `docs/architecture/` and `docs/plans/`
exactly where they are.**

Instead, **add** a new `docs/user/` folder and update the root `README.md` to link it
prominently.

### New files to create

```
docs/user/
  index.md          — overview and navigation for users
  quickstart.md     — 5-minute guide: install → load data → first query
  sparql-guide.md   — SPARQL query guide with copy-paste examples
  formats.md        — supported input/output formats and how to use them
  reasoning.md      — OWL-RL reasoning and custom Datalog rules
  deployment.md     — configuration, serving, auth, Docker
docs/dev/
  index.md          — overview and navigation for contributors
  adr/
    0001-nom-parser.md     — why nom over pest for SPARQL parser
    0002-naive-eval.md     — why naive forward-chaining for now
    0003-axum.md           — why axum for the HTTP layer
CONTRIBUTING.md     — at repo root: how to build, test, open a PR
```

`docs/dev/index.md` links to the existing `docs/architecture/` and `docs/plans/` files;
it does not move them.

### README strategy

The README is 1068 lines and its code examples are backed by integration tests
(`tests/readme_examples.rs`). **Do not duplicate that content** into `docs/user/` — it will
drift. Instead:

- The README gets a prominent "New here? → [5-minute quickstart](docs/user/quickstart.md)"
  banner near the top.
- `docs/user/quickstart.md` shows the minimal happy path and links back to the README for
  depth.
- `docs/user/sparql-guide.md` etc. are topical expansions that also link README sections.

### cargo doc comments

Add short `//!` module-level doc comments and `///` item doc comments to the public items in
the root `dagalog` library (`src/lib.rs`) and the key crate entry-points that currently lack
them. This is prose work, not a code change.

---

## Track 2: Code changes

### 2a. `examples/` crate

Create a top-level `examples/` directory with three runnable examples. These are compiled by
`cargo test --workspace` and `cargo doc --workspace`, so they stay honest automatically.

| File | What it shows |
|---|---|
| `examples/load_and_query.rs` | Load a `.ttl` file, run a SPARQL SELECT, print results |
| `examples/with_reasoning.rs` | Load an OWL ontology, trigger OWL-RL reasoning, query inferred facts |
| `examples/with_datalog.rs`   | Define custom Datalog rules inline, query derived facts |

Each example ships with a small companion data file in `examples/data/` (10–15 triples).
These data files are used as the sample dataset in the web UI (see 2b).

**Tests:** The examples themselves are the tests — `cargo build --examples` and
`cargo run --example load_and_query` must succeed. A test in `tests/examples_compile.rs`
(or added to an existing test file) can assert `cargo build --examples` exits 0.

**TDD stubs:** Add empty `fn main() {}` stubs for each example and the data files as first
step; run `cargo build --examples` to confirm they compile; mark as `#[ignore]`'d shell
command tests if needed for CI gating. Implement content one example at a time.

### 2b. Web UI: sample data playground

**Decision needed (choose one):**

**Option A — "Load sample data" button (recommended)**
Add a button to the web UI's data upload panel that pre-fills the Turtle textarea with the
same 15-triple dataset used in `examples/data/`. The user can then click "Load" to POST it
to the server. The query textarea gets a matching `SELECT *` query pre-filled. No server
changes needed — it is a pure frontend change using the existing upload + query mechanism.

Behaviour: clicking the button overwrites the textareas; there is a visible label so the
user knows it is sample data.

**Option B — Server pre-loads on startup (not recommended)**
The server inserts sample triples into the datastore before accepting connections. This means
the "Load" page always has data, but it silently pollutes any production deployment's dataset
and would confuse users who supply their own data.

→ **Please decide which option you prefer before implementation begins.**

**TDD:** A frontend-only change has no Rust tests. The "test" is manual: start the server,
click the button, confirm the query returns results. This can be gated with a doc-comment
test or a `cargo test` integration test that POSTs the sample data and runs the query against
it (that test would also validate the sample data is well-formed Turtle).

### 2c. Web UI: inline query help

Add a small "?" icon next to the SPARQL query textarea that, when clicked, shows a modal with:
- A cheat-sheet of common SPARQL patterns (5–6 copy-paste snippets)
- A link to `docs/user/sparql-guide.md` on GitHub/docs site

This is a pure HTML/JS change in `sparql_endpoint/src/frontend.html`.

**TDD:** Manual verification only — no Rust test needed for a static help modal.

### 2d. CLI `--help` improvements

The current `--help` output is already well structured (via `clap`). Add a `after_help` or
`before_help` string pointing to `docs/user/quickstart.md`. This is a one-line change in
`src/main.rs`.

No new tests needed — `clap` guarantees `--help` compiles if it compiles.

---

## Order of execution

1. **This session:** Plan reviewed and approved by user. (Done — that's this document.)
2. **Session 2 (Track 1):** Create all `docs/user/`, `docs/dev/`, ADRs, `CONTRIBUTING.md`,
   add README banner, write doc-comments. No code, no test stubs.
3. **Session 3 (Track 2, red phase):** Create `examples/` stubs + data files + compile test.
   User reviews before any implementation.
4. **Session 4 (Track 2, green phase):** Implement examples one at a time. Web UI changes
   (2b, 2c, 2d) can be done in this session since they have no Rust test cycle.

---

## Open question for the user

**Before Session 2 starts:** Which web UI approach do you want? Option A (button pre-fills
the form) or Option B (server pre-loads on startup)? Recommendation is A.

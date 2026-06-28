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
Documentation and architecture can be in local markdown, but information about what is complete, what is planned, what is in progress is 
in issues under this project in github. 
The top-level issues under the project are larger "epics". Most concrete work will be on a sub-issue and not on the top-level.

Include links to relevant epics (or issues) in markdwon documentation, and avoid mentioning work status in repository documentaton, use the issues for this.
Include links to relevant documentation in the issues and epics. Whenever mentioning documentation in the issue, create actual clickable links. 
Reference the current working branch of the repository in the issue when working on it. 

When marking code as incomplete, f.ex. tests that are ignored, dead code that is allowed, or comments with todo's, always link to the issue or epic that will fix it

When creating an issue mark it as TODO When Working on it mark is as In Progress, use a worktree to create a new branch and when done, create a pull request between that branch and 
main before closing the worktree. The pull request and issue should be linked so the issue becomes closed when the pull request is merged

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
# Note: the CI minimal-versions job requires nightly and mutates Cargo.lock;
# run it manually only when adding/removing dependencies:
#   cargo +nightly update -Z minimal-versions && cargo check --workspace --all-targets

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
└── manchester_parser/   — OWL Manchester syntax parser   [stub]
```

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
2. `owl2rl2datalog`: full OWL 2 RL ontology → `Vec<Rule>` via `owl2datalog(resources, ontology)`

### `jsonld_parser` crate
JSON-LD 1.1 parser (`parse_jsonld`) and serialiser (`serialize_jsonld`, `serialize_jsonld_expanded`, `serialize_jsonld_flattened`). Uses `serde_json` for JSON handling. The parser populates a `Datastore` directly; the serialiser reads all quads back and emits expanded JSON-LD value objects. Context processing supports: term mappings, prefixes, `@vocab`, `@base`, `@language`, compact IRIs, `@type` coercion, all container types, `@reverse`, `@included`, `@nest`, keyword aliasing, property-scoped and type-scoped contexts. External context URL fetching (`@import`) is tracked in [#82](https://github.com/daghovland/rdf-datalog/issues/82).

### `sparql_parser` crate
nom-based SPARQL 1.2 parser and in-memory executor. Supports: `SELECT`, `DESCRIBE`, `ASK`, `CONSTRUCT`; basic graph patterns, `FILTER`, `OPTIONAL`, `UNION`, `GRAPH`, `BIND`, `VALUES`, `DISTINCT`, `LIMIT`, `OFFSET`, `SELECT *`; property paths; aggregates (`GROUP BY`, `HAVING`, `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `SAMPLE`, `GROUP_CONCAT`); `FROM`/`FROM NAMED`. Missing features are tracked in [#48](https://github.com/daghovland/rdf-datalog/issues/48).

### `sparql_endpoint` crate
`axum`-based HTTP server exposing SPARQL 1.1 Protocol endpoints (`GET /sparql`, `POST /sparql`), Service Description, content negotiation, and CORS. State is an `Arc<RwLock<Datastore>>`.

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

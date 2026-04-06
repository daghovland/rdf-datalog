# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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
```

## Planning and protocol documents

- **`PLAN.md`** — full implementation roadmap (phases 1–8, crate mapping from DagSemTools, suggested order)
- **`PROTOCOLS.md`** — W3C protocol compliance reference (SPARQL 1.1 Protocol, Graph Store HTTP Protocol, Service Description, VoID, content negotiation, CORS)

## Architecture

Goal: fast RDF triplestore with native OWL-RL reasoning over datalog, plus a standards-compliant SPARQL HTTP endpoint.

```
dagalog (root binary)
├── ingress/             — RDF data types and vocabulary constants
├── dag_rdf/             — Graph element storage, quad indexing, Datastore
├── datalog/             — Datalog engine (rules, stratifier, reasoner)
├── owl_ontology/        — OWL 2 type hierarchy (axioms, ontology)
├── eli/                 — EL profile → datalog (ELI2RL)
├── owl2rl2datalog/      — OWL 2 RL → datalog (W3C spec §4.3)
├── turtle_parser/       — Turtle/TriG parser (ANTLR4)      [planned]
├── manchester_parser/   — OWL Manchester syntax (ANTLR4)   [planned]
├── sparql_parser/       — SPARQL 1.1 parser (ANTLR4)       [planned]
├── datalog_parser/      — Datalog rules parser (ANTLR4)    [planned]
└── sparql_endpoint/     — HTTP SPARQL endpoint (axum)      [planned]
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

### `sparql_endpoint` crate (planned)
`axum`-based HTTP server. See `PROTOCOLS.md` for the full specification of each endpoint. State is an `Arc<RwLock<Datastore>>`. See `PLAN.md` Phase 8 for the full implementation plan.

### Key design pattern
All graph elements are interned through `GraphElementManager`: store a `GraphElement` → get back a `GraphElementId` (`u32`). Triples and Quads only hold IDs. Resolve IDs back to values via `get_graph_element` / `get_resource_triple` / `get_resource_quad`.

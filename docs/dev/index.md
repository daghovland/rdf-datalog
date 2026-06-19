# dagalog developer documentation

This section is for contributors, maintainers, and anyone working on the dagalog codebase.

---

## Where to start

| I want to… | Go to |
|---|---|
| Build, test, and submit a PR | [CONTRIBUTING.md](../../CONTRIBUTING.md) |
| Understand the overall architecture | [Architecture overview](#architecture-overview) |
| See the W3C protocol compliance map | [docs/architecture/PROTOCOLS.md](../architecture/PROTOCOLS.md) |
| See the implementation roadmap | [docs/architecture/PLAN.md](../architecture/PLAN.md) |
| Find a specific feature plan | [docs/plans/](../plans/) |
| Understand why a design decision was made | [Architecture Decision Records](#architecture-decision-records) |

---

## Architecture overview

```
dagalog (root binary + library)
├── ingress/             — Core RDF types and vocabulary constants
├── dag_rdf/             — Datastore: quad indexing + graph-element interning
├── datalog/             — Datalog engine: rules, stratifier, reasoner
├── owl_ontology/        — OWL 2 data types (axioms, ontology)
├── eli/                 — EL profile normalisation → datalog
├── owl2rl2datalog/      — OWL 2 RL → datalog (W3C §4.3)
├── rdf_owl_translator/  — RDF triples → OWL 2 axiom extraction
├── turtle_parser/       — Turtle/TriG parser (rio_turtle)
├── jsonld_parser/       — JSON-LD 1.1 parser + serialiser
├── sparql_parser/       — SPARQL 1.2 SELECT parser (nom) + executor
├── datalog_parser/      — Datalog rules parser (nom)
├── sparql_endpoint/     — HTTP SPARQL endpoint (axum + tokio)
└── manchester_parser/   — OWL Manchester syntax parser (stub)
```

See [CLAUDE.md](../../CLAUDE.md) for a per-crate description of each module's
responsibilities and key types.

### Key design pattern

Every graph element (IRI, blank node, literal) is **interned** through
`GraphElementManager`: store a `GraphElement` and get back a `GraphElementId` (`u32`).
Triples and quads only hold IDs. Resolve IDs back to values via
`get_graph_element` / `get_resource_triple` / `get_resource_quad`.

This makes the triple store very compact — quads are 16 bytes — and makes
equality checks O(1).

---

## Architecture Decision Records

Short records explaining *why* key design choices were made. Useful context when
considering changes to these areas.

- [ADR-0001: nom for the SPARQL parser](adr/0001-nom-parser.md)
- [ADR-0002: naive forward-chaining materialisation](adr/0002-naive-eval.md)
- [ADR-0003: axum for the HTTP layer](adr/0003-axum.md)

---

## Feature plans and known issues

Current work is tracked in [`docs/plans/`](../plans/):

| Plan | Area |
|---|---|
| [`PLAN.md`](../architecture/PLAN.md) | Full implementation roadmap (phases 1–8) |
| [`PERSISTENCE_PLAN.md`](../plans/PERSISTENCE_PLAN.md) | Durable storage (`redb`) + incremental Datalog |
| [`SHACL_PLAN.md`](../plans/SHACL_PLAN.md) | SHACL Core validation |
| [`AUTH.md`](../plans/AUTH.md) | Authentication (complete; Managed Identity docs pending) |
| [`SPARQL_AGGREGATES_AND_PATHS_PLAN.md`](../plans/SPARQL_AGGREGATES_AND_PATHS_PLAN.md) | Aggregates + property paths |
| [`SUBQUERY_PLAN.md`](../plans/SUBQUERY_PLAN.md) | SPARQL subqueries |
| [`EXPRESSION_PLAN.md`](../plans/EXPRESSION_PLAN.md) | SPARQL expressions |
| [`CONSTRUCT_PLAN.md`](../plans/CONSTRUCT_PLAN.md) | SPARQL CONSTRUCT |
| [`QUERY_BUILDER_PLAN.md`](../plans/QUERY_BUILDER_PLAN.md) | Web UI visual query builder |
| [`FRONTEND_PLAN.md`](../plans/FRONTEND_PLAN.md) | Web UI improvements |
| [`SERIALIZE_PLAN.md`](../plans/SERIALIZE_PLAN.md) | RDF serialisation |
| [`SHACL_PLAN.md`](../plans/SHACL_PLAN.md) | SHACL validation |
| [`overview.md`](../plans/overview.md) | Cross-cutting feature overview |

---

## Integration tests as documentation

The test suite is the best guide to what actually works and how to call each API:

| Test file | Coverage |
|---|---|
| `tests/readme_examples.rs` | Every code example in `README.md` |
| `tests/api_integration.rs` | Turtle parsing, SPARQL SELECT, Datalog reasoning |
| `tests/owl_integration.rs` | OWL ontology loading, OWL-RL reasoning |
| `tests/sparql12_suite.rs` | SPARQL 1.2 spec conformance |
| `tests/jsonld_suite.rs` | JSON-LD 1.1 spec examples, serialisation, round-trips |
| `tests/datalog_integration.rs` | Datalog rule parsing and evaluation |
| `sparql_endpoint/tests/fuseki_compat.rs` | Fuseki-compatible HTTP API |

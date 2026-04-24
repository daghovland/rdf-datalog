# Translation Plan: DagSemTools → Rust

This document describes the plan for translating DagSemTools (F#/.NET) into this Rust workspace.
The goal is a fast triplestore with a native Rust implementation of datalog-based OWL-RL reasoning.

---

## Crate mapping

Each F# project becomes a Rust crate. Names are kept as close as possible:

| DagSemTools project | Rust crate | Status |
|---|---|---|
| `Ingress` | `ingress` | Done |
| `Rdf` | `dag_rdf` | Done |
| `Datalog` | `datalog` | Done |
| `OwlOntology` | `owl_ontology` | Done |
| `OWL2RL2Datalog` | `owl2rl2datalog` | Done |
| `ELI` | `eli` | Done |
| `RdfOwlTranslator` | `rdf_owl_translator` | Done |
| `Turtle.Parser` | `turtle_parser` | Done (uses rio_turtle; now includes TriG support) |
| `Manchester.Parser` | `manchester_parser` | Not started |
| `Sparql.Parser` | `sparql_parser` | Done (nom-based; `a` shorthand added) |
| `Datalog.Parser` | `datalog_parser` | Not started — see Phase 6 for plan |
| `Api` | root crate `dagalog` | Done — CLI + library (`src/lib.rs` + `src/main.rs`) |
| `AlcTableau`, `OWL2ALC` | `alc_tableau` | Deferred |

---

## ANTLR4 in Rust

The official ANTLR4 tool does not target Rust natively. The recommended approach:

- Use the **`antlr4rust`** crate (by rrevenantt on GitHub) which provides:
  - A Rust runtime (`antlr4rust` on crates.io)
  - A code generator (a fork of the ANTLR4 tool that emits Rust)
- The existing `.g4` grammar files in `grammars/` can be reused **with minor modifications** (the Rust target has some syntax differences in actions/predicates, but pure grammar rules are portable).
- Each parser crate will have a `build.rs` that invokes the ANTLR4 tool to generate Rust parser/lexer source from the grammar files, similar to how the C# projects reference them.

Alternative if `antlr4rust` proves too unstable: **`pest`** (PEG-based, clean Rust, good for Turtle/SPARQL), but this would mean rewriting grammars. Try `antlr4rust` first.

---

## Phase 1 — Complete the foundation crates

### 1a. Fix `ingress` crate gaps

DagSemTools `Ingress` defines a `defaultGraphElementId = 0u` that is pre-populated in the element manager. Currently the Rust `ingress` crate has no concept of a default graph.

- Add `pub const DEFAULT_GRAPH_ELEMENT_ID: GraphElementId = 0;` in `dag_rdf/src/ingress.rs`
- Add `pub const DEFAULT_GRAPH_IRI: &str = "urn:x-default-graph"` (or use the DagSemTools sentinel)

### 1b. Complete `dag_rdf` crate

Missing from DagSemTools `Rdf` module:

**`query.rs`** — add `Term` and `QuadPattern` types (from `DagSemTools.Rdf.Query`):
```rust
pub enum Term {
    Resource(GraphElementId),
    Variable(String),
}

pub struct QuadPattern {
    pub graph: Term,
    pub subject: Term,
    pub predicate: Term,
    pub object: Term,
}
```
Also add `get_default_graph_pattern(subject, predicate, object)` helper.

**`triple_table.rs`** — add `TripleTable` (currently only `QuadTable` exists). In DagSemTools, `TripleTable` is a plain triple store without a graph ID; the Rust code may unify this with `QuadTable` using the default graph ID.

**`named_triple_table.rs`** — `NamedTripleTable` is a view over a `QuadTable` filtered to one `graph_id`. Can be a thin wrapper struct with iterator adapters.

**`datastore.rs`** — add `Datastore` struct mirroring DagSemTools:
```rust
pub struct Datastore {
    pub reified_triples: QuadTable,
    pub named_graphs: QuadTable,
    pub resources: GraphElementManager,
}
```
Add the full query API (`get_triples_with_subject`, `get_triples_with_subject_predicate`, `get_quads`, etc.) and `add_quad`, `add_triple`, `add_named_graph_triple`, `add_reified_triple`.

---

## Phase 2 — `datalog` crate

New crate `datalog/` depending on `dag_rdf`.

Translate `DagSemTools.Datalog` (files: `Library.fs`, `Reasoner.fs`, `Stratifier.fs`, `Unification.fs`, `PredicateGrounder.fs`).

Key types:
```rust
pub enum ResourceOrWildcard {
    Resource(GraphElementId),
    Wildcard,
}

pub struct QuadWildcard {
    pub graph: ResourceOrWildcard,
    pub subject: ResourceOrWildcard,
    pub predicate: ResourceOrWildcard,
    pub object: ResourceOrWildcard,
}

pub enum RuleHead {
    NormalHead(QuadPattern),
    Contradiction,
}

pub enum RuleAtom {
    PositivePattern(QuadPattern),
    NotPattern(QuadPattern),
    NotEqualsAtom(Term, Term),
}

pub struct Rule {
    pub head: RuleHead,
    pub body: Vec<RuleAtom>,
}

pub type Substitution = HashMap<String, GraphElementId>;
```

Submodules:
- `datalog.rs` — `evaluate_pattern`, `get_substitutions`, `apply_substitution_quad`, `wildcard_quad_pattern`, safety checks
- `reasoner.rs` — `DatalogProgram` (naive materialisation), `evaluate(rules, datastore)`
- `stratifier.rs` — `RulePartitioner`, `order_rules()` (topological sort with cycle detection, stratification for negation)
- `unification.rs` — `quad_patterns_unifiable`, `depending_rules`, `intentional_rules`

The `Stratifier` is the most complex piece — it implements topological sorting with a Kahn-style algorithm and handles negation stratification. Port it carefully, maintaining the `OrderedRule` struct with `successors`, `num_predecessors`, `uses_intensional_negative_edge`.

---

## Phase 3 — `owl_ontology` crate

New crate `owl_ontology/` depending on `ingress`.

Translate `DagSemTools.OwlOntology` (files: `Axioms.fs`, `Ontology.fs`, `Library.fs`).

These are pure data types — enums for OWL axioms, class expressions, property expressions, individuals, data ranges. No logic, just the full OWL 2 type hierarchy:
- `ClassExpression` (ClassName, ObjectIntersectionOf, ObjectSomeValuesFrom, …)
- `ObjectPropertyExpression` (NamedObjectProperty, InverseObjectProperty, ObjectPropertyChain)
- `DataRange` (NamedDataRange, DataIntersectionOf, …)
- `Axiom` / `ClassAxiom` / `ObjectPropertyAxiom` / `DataPropertyAxiom`
- `Ontology { iri, version_iri, axioms }`

---

## Phase 4 — `eli` crate

New crate `eli/` depending on `owl_ontology` and `dag_rdf`.

Translate `DagSemTools.ELI` (files: `ELIAxiom.fs`, `ELIExtractor.fs`, `ELI2RL.fs`, `Library.fs`).

This implements the translation from EL profile class axioms to datalog rules, inspired by https://arxiv.org/abs/2008.02232.
- `EliAxiom` enum — the EL fragment of OWL class axioms
- `eli_axiom_extractor` — pattern-matches OWL `ClassAxiom` into `Option<EliAxiom>`
- `eli2rl` — generates datalog `Rule`s from `EliAxiom`

---

## Phase 5 — `owl2rl2datalog` crate

New crate `owl2rl2datalog/` depending on `owl_ontology`, `eli`, `dag_rdf`, `datalog`.

Translate `DagSemTools.OWL2RL2Datalog` (`Library.fs`, `Equality.fs`).

Implements section 4.3 of https://www.w3.org/TR/owl2-profiles/#OWL_2_RL :
- `owl2_datalog(ontology, resources) -> Vec<Rule>` — main entry point
- `object_property_axiom2datalog`, `data_property_axiom2datalog`, `owl_axiom2datalog`
- `get_class_expression_resource`, `get_object_property_expression_resource`
- Many cases currently have `failwith "todo"` in F# — these become `todo!()` initially

---

## Phase 6 — Parser crates

Four crates, each with a `build.rs` that runs the ANTLR4 tool:

### `turtle_parser`
Depends on `dag_rdf`. Translates `DagSemTools.Turtle.Parser`.
- Grammar: reuse `grammars/turtle/TurtleDoc.g4` and `TriGDoc.g4`
- Visitors: `IriGrammarVisitor`, `ResourceVisitor`, `StringVisitor`, `PredicateObjectListVisitor`, `TurtleListener`
- Entry point: `parse(input: &str, datastore: &mut Datastore)`

### `manchester_parser`
Depends on `owl_ontology`, `dag_rdf`. Translates `DagSemTools.Manchester.Parser`.
- Grammar: reuse `grammars/manchester/Manchester.g4`, `Concept.g4`, `ManchesterCommonTokens.g4`
- Visitors: `ManchesterVisitor`, `ConceptVisitor`, `FrameVisitor`, `ClassAssertionVisitor`, etc.
- Entry point: `parse(input: &str, datastore: &mut Datastore) -> Ontology`

### `sparql_parser`
Depends on `dag_rdf`. Translates `DagSemTools.Sparql.Parser`.
- Grammar: reuse `grammars/sparql/Sparql.g4`
- Produces `SelectQuery` (in `dag_rdf::query`)

### `datalog_parser`
Depends on `datalog`, `dag_rdf`. Translates `DagSemTools.Datalog.Parser`.
- Grammar: reuse `grammars/datalog/Datalog.g4`
- Produces `Vec<Rule>`
- **Status**: stub only — `parse()` always returns an error.
- **To implement**: translate the F# parser from DagSemTools similarly to how
  `sparql_parser` was ported (nom-based, no ANTLR4 needed). The parser should
  parse Datalog rules of the form `head :- body.` and return `Vec<datalog::types::Rule>`.
- The CLI `--rules <file>` flag is present but rejects with an error until this is done.

---

## Phase 7 — `api` / root crate integration ✓ Done

The root crate `dagalog` is the integration layer, implemented as both a library (`src/lib.rs`) and a CLI binary (`src/main.rs`).

### Library API (`src/lib.rs`)
- `load_file(datastore, path)` — loads Turtle (`.ttl`) or TriG (`.trig`) files
- `apply_ontologies(datastore, paths)` — loads OWL ontology files and runs OWL-RL materialisation
- `run_sparql_query(datastore, sparql)` — executes a SPARQL SELECT query
- `format_results(result, format)` — renders results as table, CSV, or JSON

### CLI (`src/main.rs`)
- `--data <FILE>` / `-d` — Turtle/TriG data files (repeatable)
- `--ontology <FILE>` / `-o` — OWL ontology files, triggers reasoning (repeatable)
- `--rules <FILE>` / `-r` — Datalog rules files (not yet supported, rejects at startup)
- `--query-file <FILE>` / `-q` — SPARQL query from file
- `--query <SPARQL>` / `-Q` — inline SPARQL query
- `--format <FMT>` / `-f` — output format: `table` (default), `csv`, `json`
- `--verbose` / `-v` — print pipeline stats to stderr

---

## Phase 8 — `sparql_endpoint` HTTP server

New crate `sparql_endpoint/` depending on `dag_rdf`, `datalog`, `sparql_parser`, `turtle_parser`.

See `PROTOCOLS.md` for the full specification of each protocol.

### Technology choices

- **HTTP framework**: `axum` (tokio-based, ergonomic, composable)
- **Async runtime**: `tokio`
- **Serialization**: `serde` + `serde_json` for SPARQL JSON results; hand-written Turtle/N-Triples serializers (or `rio_turtle` crate)
- **Content negotiation**: implemented via `axum`'s `TypedHeader` extractors and a custom `Accept` parser

### Crate layout

```
sparql_endpoint/
├── Cargo.toml
└── src/
    ├── lib.rs           — public App builder
    ├── server.rs        — axum router, startup
    ├── query.rs         — GET/POST /sparql (SELECT, ASK, CONSTRUCT, DESCRIBE)
    ├── update.rs        — POST /sparql (SPARQL Update)
    ├── graph_store.rs   — GET/PUT/POST/DELETE /rdf-graph-store
    ├── service_desc.rs  — GET /sparql → Service Description (no query param)
    ├── void.rs          — GET /.well-known/void
    ├── negotiate.rs     — content negotiation helpers
    └── serialize/
        ├── sparql_json.rs   — application/sparql-results+json
        ├── sparql_xml.rs    — application/sparql-results+xml
        ├── turtle.rs        — text/turtle serializer
        └── ntriples.rs      — application/n-triples serializer
```

### Endpoints to implement (in order)

#### P0 — SPARQL 1.1 Protocol

```
GET  /sparql?query=<encoded>          — query (SELECT/ASK/CONSTRUCT/DESCRIBE)
POST /sparql                          — query (form or direct body)
POST /sparql                          — update (SPARQL Update, form or direct body)
```

Distinguishes query vs. update by Content-Type and/or `query=` vs. `update=` parameter.

Response codes per spec:
- `200 OK` — success
- `400 Bad Request` — malformed query/update
- `406 Not Acceptable` — no matching Accept type
- `500 Internal Server Error` — execution failure

#### P1 — SPARQL 1.1 Graph Store HTTP Protocol

```
GET    /rdf-graph-store?graph=<iri>   — fetch named graph
GET    /rdf-graph-store?default       — fetch default graph
PUT    /rdf-graph-store?graph=<iri>   — replace named graph
POST   /rdf-graph-store?graph=<iri>   — merge into named graph
DELETE /rdf-graph-store?graph=<iri>   — delete named graph
HEAD   /rdf-graph-store?graph=<iri>   — headers only
```

#### P1 — SPARQL Service Description

```
GET /sparql               (no query param, or Accept: text/turtle)
```

Returns an RDF document (Turtle or N-Triples) describing endpoint capabilities.

#### P2 — VoID description

```
GET /.well-known/void
GET /void
```

Returns dataset statistics and metadata as RDF.

### State management

The endpoint needs shared mutable access to the `Datastore`. Two models:

**Simple (single-writer):** wrap `Datastore` in `Arc<RwLock<Datastore>>`.
- Reads (queries) acquire a read lock — concurrent.
- Writes (updates, graph store PUT/POST/DELETE) acquire a write lock — exclusive.
- Suitable for single-node deployments.

**Advanced (future):** copy-on-write or MVCC — readers see a snapshot while a
write transaction is in progress. Deferred until benchmarks show read/write
contention is a bottleneck.

### CORS

All routes emit:
```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, POST, OPTIONS
Access-Control-Allow-Headers: Accept, Content-Type
```
Implement as an `axum` middleware layer (`tower_http::cors::CorsLayer`).

### Configuration

The `App` builder exposes:
```rust
pub struct Config {
    pub bind_addr: std::net::SocketAddr,   // default 0.0.0.0:3030
    pub base_iri: String,                   // used in Service Description
    pub read_only: bool,                    // disable update endpoint
    pub max_query_timeout_secs: u64,        // default 30
}
```

### Example end-to-end usage (root crate)

```rust
let mut store = Datastore::new(1_000_000);
turtle_parser::parse_file("data.ttl", &mut store)?;
let rules = owl2rl2datalog::owl2datalog(&mut store.resources, &ontology);
datalog::evaluate_rules(rules, &mut store);

let config = sparql_endpoint::Config::default();
sparql_endpoint::serve(Arc::new(RwLock::new(store)), config).await?;
```

---

## Suggested implementation order

1. `dag_rdf`: add `query.rs` (Term, QuadPattern), `datastore.rs`, fix default graph ID ✓
2. `datalog` crate: types + `datalog.rs` module + `reasoner.rs` (naive materialise) ✓
3. `datalog` crate: `stratifier.rs` + `unification.rs` ✓
4. `owl_ontology` crate: pure data types ✓
5. `eli` crate ✓
6. `owl2rl2datalog` crate ✓
7. `turtle_parser` crate (needed for loading any real data) ✓
8. `sparql_parser` crate (needed for query endpoint) ✓
9. Wire up `dagalog` root for end-to-end reasoning ✓ (via new `rdf_owl_translator` crate)
10. `sparql_endpoint` crate: P0 query endpoint (SELECT/ASK)
11. `sparql_endpoint` crate: P0 update endpoint + P1 graph store
12. `sparql_endpoint` crate: Service Description + VoID
13. `manchester_parser`, `datalog_parser`

---

## Notes on translation idioms

- F# discriminated unions → Rust `enum`
- F# records → Rust `struct`
- F# `Map<K,V>` → `HashMap<K,V>` (or `BTreeMap` where ordering matters)
- F# `Seq` (lazy) → Rust iterators (`impl Iterator<Item=...>`)
- F# `Option.bind` / `Option.map` chains → Rust `Option::and_then` / `Option::map`
- F# `Seq.fold` → `Iterator::fold`
- F# `List.collect` → `Iterator::flat_map`
- F# mutable class fields → Rust `&mut self` methods or `Cell`/`RefCell` where needed
- F# interface implementations (e.g. `ITripleTable`) → Rust traits
- Logging (`Serilog.ILogger`) → `log` crate traits (`log::warn!`, `log::error!`)
- No equivalent to `failwith "todo"` found during review → use `todo!()` macro

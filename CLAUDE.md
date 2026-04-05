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

## Architecture

This is a Rust workspace with three crates targeting RDF/datalog processing:

```
datalog (root, binary `dagalog`)
├── ingress/          — RDF data types and vocabulary constants
└── dag_rdf/          — Graph element storage and indexing
```

### `ingress` crate
Defines the core RDF type hierarchy:
- `IriReference` — newtype wrapper around `String` for IRIs
- `RdfResource` — `Iri(IriReference)` or `AnonymousBlankNode(u32)`
- `RdfLiteral` — typed literals (string, bool, decimal, float, integer, datetime, etc.)
- `GraphElement` — union of `NodeOrEdge(RdfResource)` or `GraphLiteral(RdfLiteral)`
- `PrefixDeclaration`, `OntologyVersion` — OWL/ontology metadata types
- `namespaces.rs` — `&str` constants for all RDF/RDFS/OWL/XSD IRIs

### `dag_rdf` crate
Builds on `ingress` to provide storage:
- `GraphElementManager` (`lib.rs`) — interning store that maps `GraphElement` values to `GraphElementId` (`u32`). Deduplicates on insert; supports named and anonymous blank nodes.
- `ingress.rs` (module inside dag_rdf) — index types: `Triple` (subject/predicate/object as IDs), `Quad` (adds `triple_id` for named graphs), resolved `TripleResource`/`QuadResource`, and helper functions `try_get_non_negative_integer_literal` / `try_get_bool_literal`.
- `quadtable.rs` — `QuadTable`, a multi-index store for quads. Maintains indexes keyed by predicate, subject+predicate, object+predicate, triple_id, and the full quad for deduplication.

### Key design pattern
All graph elements are interned through `GraphElementManager`: store a `GraphElement` → get back a `GraphElementId` (`u32`). Triples and Quads only hold IDs, not the actual values. Resolve IDs back to values via `get_graph_element` / `get_resource_triple` / `get_resource_quad`.

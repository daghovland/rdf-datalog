# dagalog

A fast RDF triplestore with native Rust implementation of Datalog-based OWL-RL reasoning and a SPARQL 1.1 HTTP endpoint.

This is a Rust port of [DagSemTools](https://github.com/daghovland/DagSemTools) (F#/.NET).

---

## Goals

- Load RDF data from Turtle/TriG files
- Apply OWL 2 RL reasoning via Datalog materialisation
- Answer SPARQL 1.1 SELECT queries over a materialised dataset
- Expose a W3C-compliant SPARQL 1.1 HTTP endpoint

---

## Workspace layout

| Crate | Description |
|---|---|
| `ingress` | Core RDF data types: `GraphElement`, `RdfLiteral`, `RdfResource`, `IriReference` |
| `dag_rdf` | Datastore, quad tables, query patterns (`Term`, `QuadPattern`) |
| `datalog` | Datalog rule types, naive forward-chaining reasoner, stratifier |
| `owl_ontology` | OWL 2 axiom and ontology data types (pure data, no logic) |
| `eli` | EL profile normalisation and ELI→Datalog translation |
| `owl2rl2datalog` | OWL 2 RL → Datalog rule translation (W3C §4.3) |
| `turtle_parser` | Turtle/TriG parser using `rio_turtle`; populates a `Datastore` |
| `sparql_parser` | SPARQL 1.1 SELECT parser (nom-based) + in-memory query executor |
| `sparql_endpoint` | SPARQL 1.1 HTTP endpoint (axum + tokio) |
| `manchester_parser` | OWL Manchester Syntax parser (stub — not yet implemented) |
| `datalog_parser` | Datalog rule syntax parser (stub — not yet implemented) |
| `.` (`dagalog`) | Root crate: CLI entry point; loads Turtle, runs reasoning |

---

## Building

```sh
cargo build
cargo test
```

Run tests for a single crate:

```sh
cargo test -p dag-rdf
cargo test -p sparql-parser
```

---

## Running the CLI

The root binary loads a Turtle file, applies OWL 2 RL reasoning, and prints the number of derived rules:

```sh
cargo run -- path/to/data.ttl
```

---

## Running the SPARQL endpoint

Add `sparql_endpoint` as a dependency in your binary crate and call:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use dag_rdf::datastore::Datastore;
use sparql_endpoint::{Config, serve};

#[tokio::main]
async fn main() {
    let mut store = Datastore::new(1_000_000);
    turtle_parser::parse_turtle(&mut store, std::io::BufReader::new(
        std::fs::File::open("data.ttl").unwrap()
    )).unwrap();

    let config = Config::default(); // binds to 0.0.0.0:3030
    serve(Arc::new(RwLock::new(store)), config).await.unwrap();
}
```

The endpoint exposes:

| Route | Description |
|---|---|
| `GET /sparql?query=<encoded>` | SPARQL 1.1 query (SELECT) |
| `POST /sparql` | SPARQL 1.1 query (form or direct body) |
| `GET /sparql` (no query param) | SPARQL 1.1 Service Description (Turtle) |

Response format is negotiated via the `Accept` header. Default is `application/sparql-results+json`.

---

## Protocol compliance

See [PROTOCOLS.md](PROTOCOLS.md) for full details on the W3C protocols implemented and planned.

| Priority | Protocol |
|---|---|
| P0 (done) | SPARQL 1.1 Protocol — SELECT query, content negotiation, CORS |
| P0 (done) | SPARQL 1.1 Service Description |
| P1 (planned) | SPARQL 1.1 Graph Store HTTP Protocol |
| P2 (planned) | VoID dataset description |

---

## Implementation plan

See [PLAN.md](PLAN.md) for the full phased implementation plan and F# → Rust translation notes.

---

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).

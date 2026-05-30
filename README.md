# dagalog

A fast RDF triplestore with native Rust implementation of Datalog-based OWL-RL reasoning, custom Datalog rules, and a SPARQL HTTP endpoint.

This is a Rust port of [DagSemTools](https://github.com/daghovland/DagSemTools) (F#/.NET).

---

## Features

- Load RDF data from Turtle (`.ttl`) and TriG (`.trig`) files
- Apply OWL 2 RL reasoning via Datalog materialisation
- Load and apply custom Datalog rules (`.datalog` files)
- Answer SPARQL 1.2 SELECT-subset queries over the materialised dataset
- Expose a W3C-compliant SPARQL 1.1 HTTP endpoint

---

## Workspace layout

| Crate | Description |
|---|---|
| `ingress` | Core RDF types: `GraphElement`, `RdfLiteral`, `RdfResource`, `IriReference` |
| `dag_rdf` | Datastore, quad tables, `Term`, `QuadPattern` |
| `datalog` | Datalog rule types, naive forward-chaining reasoner, stratifier |
| `owl_ontology` | OWL 2 axiom and ontology data types (pure data) |
| `eli` | EL profile normalisation and ELI→Datalog translation |
| `owl2rl2datalog` | OWL 2 RL → Datalog rule translation (W3C §4.3) |
| `rdf_owl_translator` | RDF triples → OWL 2 axiom extraction |
| `turtle_parser` | Turtle/TriG parser (`rio_turtle`); populates a `Datastore` |
| `sparql_parser` | SPARQL 1.2 SELECT-subset parser (nom-based) + in-memory query executor |
| `datalog_parser` | **Datalog rule syntax parser (nom-based) — complete** |
| `sparql_endpoint` | SPARQL 1.1 HTTP endpoint (axum + tokio) |
| `manchester_parser` | OWL Manchester Syntax parser (stub — not yet implemented) |
| `.` (`dagalog`) | Root crate: CLI + library (`src/lib.rs` + `src/main.rs`) |

---

## Building

```sh
cargo build
cargo test
```

Run tests for a single crate:

```sh
cargo test -p datalog-parser
cargo test -p sparql-parser
cargo test -p dag-rdf
```

---

## CLI usage

### Load data and run a SPARQL query

```sh
dagalog --data data.ttl --query "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10"
dagalog -d data.ttl -Q query.sparql
```

### Apply OWL-RL reasoning

```sh
dagalog --data data.ttl --ontology schema.ttl \
        --query "PREFIX ex: <...> SELECT ?x WHERE { ?x a ex:Person . }"
```

The `--ontology` files are loaded, converted to Datalog rules via the OWL 2 RL
profile, and materialised before the query runs.

### Apply custom Datalog rules

```sh
dagalog --data data.ttl --rules rules.datalog \
        --query "PREFIX ex: <...> SELECT ?x WHERE { ?x a ex:Person . }"
```

Multiple `--data`, `--ontology`, and `--rules` flags can be given.
OWL-RL reasoning is applied first; Datalog rules are applied afterwards.

### Output formats

```sh
dagalog -d data.ttl -Q query.sparql --format csv
dagalog -d data.ttl -Q query.sparql --format json
dagalog -d data.ttl -Q query.sparql --format table   # default
```

### Verbose mode

```sh
dagalog -d data.ttl -o schema.ttl -r rules.datalog --verbose -Q query.sparql
```

Prints triple counts, OWL axiom counts, Datalog rule counts, and inference statistics to stderr.

### Start the SPARQL HTTP endpoint

```sh
# Serve pre-loaded, pre-reasoned data over HTTP
dagalog --data data.ttl --ontology schema.ttl --rules rules.datalog --serve
dagalog --data data.ttl --serve --port 8080
```

The endpoint is then available at `http://localhost:3030/sparql` (or the specified port).

---

## Datalog syntax

The Datalog language for RDF supports the following constructs:

```datalog
# Prefix declarations (SPARQL-style or Turtle-style)
PREFIX ex: <https://example.com/data#>
@prefix ex2: <https://example.com/data2#> .

# Fact (rule with empty body)
[ex:Alice, a, ex:Person] .

# Bracket triple syntax:  head :- body .
[?x, a, ?c] :- [?x, ?p, ?y], [?p, rdfs:range, ?c] .

# Predicate-first syntax:  predicate[subject, object]
ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj], ex:prop3[?s, ex:obj] .

# Type atom:  predicate[subject]  means  subject rdf:type predicate
ex:Employee[?x] :- ex:Manager[?x] .

# Negation
ex:Eligible[?x] :- ex:Applicant[?x], NOT ex:Rejected[?x] .

# Contradiction (signals inconsistency — for constraint checking)
false :- [?X, a, ex:MutuallyExclusive1], [?X, a, ex:MutuallyExclusive2] .

# Named graph
[?s, ex:p, ex:o] ?graph :- ex:p[?s, ex:o] ?graph .
```

**Built-in prefixes** (pre-declared, no explicit declaration needed):
- `rdf:` → `http://www.w3.org/1999/02/22-rdf-syntax-ns#`
- `rdfs:` → `http://www.w3.org/2000/01/rdf-schema#`
- `xsd:` → `http://www.w3.org/2001/XMLSchema#`
- `owl:` → `http://www.w3.org/2002/07/owl#`

**`a` shorthand**: expands to `rdf:type` everywhere it appears.

---

## SPARQL HTTP endpoint

When `--serve` is used, the `sparql_endpoint` crate exposes:

| Route | Description |
|---|---|
| `GET /sparql?query=<encoded>` | SPARQL 1.1 SELECT query |
| `POST /sparql` | SPARQL 1.1 SELECT (form or direct body) |
| `GET /sparql` (no query param) | SPARQL 1.1 Service Description (Turtle) |

Response format is negotiated via the `Accept` header.
Default is `application/sparql-results+json`.

### SPARQL query language support (SPARQL 1.2 target)

The parser/executor targets SPARQL 1.2 for `SELECT` queries, with implemented support for:

- `SELECT`, `DISTINCT`, `LIMIT`, `OFFSET`, `ORDER BY`, `GROUP BY`, `HAVING`
- `FILTER`, `BIND`, `VALUES`, `OPTIONAL`, `UNION`, `MINUS`, `EXISTS`, `NOT EXISTS`
- `GRAPH <iri>` and `GRAPH ?g` named-graph patterns
- Turtle-style triple shorthand (`;` and `,`) in group patterns
- Simple sequence property paths (`p1/p2/...`), expanded to equivalent triple joins

Current scope/limits:

- Query forms other than `SELECT` are not implemented yet
- Full SPARQL 1.2 property path algebra is not implemented yet (only simple `/` sequences)

### Using the endpoint as a library

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use dag_rdf::Datastore;
use sparql_endpoint::{Config, serve};

#[tokio::main]
async fn main() {
    let mut store = Datastore::new(1_000_000);
    // … load data, apply reasoning, apply rules …

    let config = Config::default(); // binds to 0.0.0.0:3030
    serve(Arc::new(RwLock::new(store)), config).await.unwrap();
}
```

---

## Protocol compliance

See [PROTOCOLS.md](PROTOCOLS.md) for full details.

| Priority | Protocol | Status |
|---|---|---|
| P0 | SPARQL 1.1 Protocol — SELECT query, content negotiation, CORS | Done |
| P0 | SPARQL 1.1 Service Description | Done |
| P1 | SPARQL 1.1 Graph Store HTTP Protocol | Planned |
| P2 | VoID dataset description | Planned |

---

## Implementation plan

See [PLAN.md](PLAN.md) for the full phased plan and F# → Rust translation notes.

---

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).

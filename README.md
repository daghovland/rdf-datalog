# dagalog

A fast RDF triplestore with native Rust implementation of Datalog-based OWL-RL reasoning,
custom Datalog rules, JSON-LD 1.1 parsing/serialisation, and a SPARQL HTTP endpoint.

Rust port of [DagSemTools](https://github.com/daghovland/DagSemTools) (F#/.NET).

---

## Features

| Feature | Status |
|---|---|
| Load RDF from Turtle (`.ttl`) and TriG (`.trig`) | ✓ |
| Load RDF from JSON-LD 1.1 (`.jsonld`) | ✓ |
| Serialise to JSON-LD (expanded, compacted, flattened) | ✓ |
| SPARQL 1.2 SELECT queries (in-process) | ✓ |
| SPARQL 1.1 HTTP endpoint | ✓ |
| SPARQL 1.1 Graph Store Protocol (GET/PUT/POST/DELETE/HEAD) | ✓ |
| SPARQL 1.1 Update (INSERT/DELETE/CLEAR/DROP/…) | ✓ |
| Multi-dataset server (Fuseki-compatible routing and admin API) | ✓ |
| OWL 2 RL reasoning via Datalog materialisation | ✓ |
| Custom Datalog rules with stratified negation | ✓ |
| Named graphs (load, query, reason over) | ✓ |
| OWL Manchester Syntax parser | planned |

> Every code example in this file is also an integration test in
> [`tests/readme_examples.rs`](tests/readme_examples.rs).
> If a test breaks, the README is out of date — update both together.

---

## Workspace layout

| Crate | Description |
|---|---|
| `ingress` | Core RDF types: `GraphElement`, `RdfLiteral`, `RdfResource`, `IriReference` |
| `dag_rdf` | `Datastore`, quad tables, graph-element interning |
| `datalog` | Rule types, naive forward-chaining reasoner, stratifier |
| `owl_ontology` | OWL 2 axiom and ontology data types (pure data model) |
| `eli` | EL profile normalisation and ELI→Datalog translation |
| `owl2rl2datalog` | OWL 2 RL → Datalog rule translation (W3C §4.3) |
| `rdf_owl_translator` | RDF triples → OWL 2 axiom extraction |
| `turtle_parser` | Turtle/TriG parser (`rio_turtle`); populates a `Datastore` |
| `jsonld_parser` | JSON-LD 1.1 parser and serialiser (expanded, compacted, flattened) |
| `sparql_parser` | SPARQL 1.2 SELECT parser (nom) + in-memory query executor |
| `datalog_parser` | Datalog rule syntax parser (nom) |
| `sparql_endpoint` | SPARQL 1.1 HTTP endpoint (axum + tokio) |
| `manchester_parser` | OWL Manchester Syntax parser (stub) |
| `.` (`dagalog`) | Root crate: CLI binary + public Rust library |

---

## Building

```sh
cargo build
cargo test
```

Test a single crate:

```sh
cargo test -p jsonld-parser
cargo test -p sparql-parser
cargo test -p dag-rdf
```

---

## Installation

**Prerequisites:** Rust toolchain 1.85 or later (the workspace uses Rust edition 2024).
Install via [rustup](https://rustup.rs/) if needed.

### Install from Git (no local clone required)

```sh
cargo install --git https://github.com/daghovland/rdf-datalog dagalog
```

This places the `dagalog` binary in `~/.cargo/bin/`, which `rustup` adds to
`$PATH` automatically.

### Install from a local checkout

```sh
git clone https://github.com/daghovland/rdf-datalog
cd rdf-datalog
cargo install --path .
```

### Build a release binary manually

```sh
cargo build --release
# Binary is at target/release/dagalog
sudo cp target/release/dagalog /usr/local/bin/
# or
cp target/release/dagalog ~/.local/bin/   # if ~/.local/bin is in your PATH
```

---

## JSON-LD 1.1

### Parsing

Parse any JSON-LD 1.1 document into the in-memory `Datastore` and query it with SPARQL:

```rust
use dag_rdf::Datastore;
use dagalog::run_sparql_query;

let jsonld = r#"{
  "@context": { "foaf": "http://xmlns.com/foaf/0.1/" },
  "@id": "http://example.org/alice",
  "@type": "foaf:Person",
  "foaf:name": "Alice"
}"#;

let mut ds = Datastore::new(10_000);
jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes()).unwrap();

let result = run_sparql_query(
    &ds,
    "SELECT ?name WHERE { \
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }",
).unwrap();
assert_eq!(result.rows.len(), 1);
// test: readme_jsonld_parse_inline
```

#### Supported JSON-LD 1.1 features

| Feature | Notes |
|---|---|
| `@context` — term mappings, prefixes, `@vocab`, `@base` | Full |
| `@type` → `rdf:type` triples | Full |
| Compact IRIs (`foaf:name`) | Full |
| Language-tagged strings (`@language`) | Full |
| Typed literals (`@type: xsd:date`, `xsd:integer`, …) | Full |
| JSON literals (`@type: @json` → `rdf:JSON` datatype) | Full |
| `@graph` — named graphs | Full |
| `@list` → RDF list encoding (`rdf:first` / `rdf:rest` / `rdf:nil`) | Full |
| `@set`, `@index`, `@language`, `@id`, `@type`, `@graph` containers | Full |
| `@reverse` properties | Full |
| `@included` | Full |
| `@nest` grouping | Full |
| Keyword aliasing (`"id": "@id"`) | Full |
| Property-scoped and type-scoped contexts | Full |
| `@protected` term definitions | Full |
| External context URL fetching (`@import`) | Not implemented (skipped silently) |

Language-tagged and typed literals:

```rust
let jsonld = r#"{
  "@context": {
    "dc": "http://purl.org/dc/elements/1.1/",
    "xsd": "http://www.w3.org/2001/XMLSchema#",
    "published": { "@id": "dc:date", "@type": "xsd:date" }
  },
  "@id": "http://example.org/article/1",
  "dc:title": [
    { "@value": "Hello RDF", "@language": "en" },
    { "@value": "Hallo RDF", "@language": "de" }
  ],
  "published": "2025-01-15"
}"#;
// Two dc:title language variants; one xsd:date typed literal.
// test: readme_jsonld_literals
```

Named graphs via `@graph`:

```rust
let jsonld = r#"{
  "@context": { "ex": "http://example.org/" },
  "@id": "http://example.org/myGraph",
  "@graph": [
    { "@id": "http://example.org/alice", "ex:knows": { "@id": "http://example.org/bob" } },
    { "@id": "http://example.org/bob",   "ex:name":  "Bob" }
  ]
}"#;
// GRAPH <http://example.org/myGraph> { ?s ?p ?o } returns the inner triples.
// test: readme_jsonld_named_graph
```

### Serialisation

Three output forms are available; all are re-parseable (round-trip fidelity is tested):

```rust
// Compacted form: {"@context": {}, "@graph": [...]}
// Full IRIs everywhere; @context is present but empty (re-parseable without prefix knowledge).
let jsonld = jsonld_parser::serialize_jsonld(&ds);

// Expanded form: JSON array, no @context, absolute IRIs for every key.
let expanded = jsonld_parser::serialize_jsonld_expanded(&ds);

// Flattened form: {"@graph": [all subjects at top level, cross-referenced by @id]}.
let flat = jsonld_parser::serialize_jsonld_flattened(&ds);
```

Round-trip (Turtle → JSON-LD → re-parse → same triple count):

```rust
let mut ds1 = Datastore::new(10_000);
turtle_parser::parse_turtle(&mut ds1, ttl.as_bytes()).unwrap();

let jsonld = jsonld_parser::serialize_jsonld(&ds1);

let mut ds2 = Datastore::new(10_000);
jsonld_parser::parse_jsonld(&mut ds2, jsonld.as_bytes()).unwrap();
// ds1 and ds2 contain the same triples.
// test: readme_jsonld_serialize_roundtrip
```

---

## Turtle / TriG

```rust
use dag_rdf::Datastore;

let mut ds = Datastore::new(10_000);

// Turtle (.ttl)
turtle_parser::parse_turtle(&mut ds, ttl_bytes).unwrap();

// TriG (.trig) — Turtle with named graph blocks
turtle_parser::parse_trig(&mut ds, trig_bytes).unwrap();
// test: readme_turtle_parse_basic, readme_trig_named_graph
```

The `load_file` helper dispatches on extension (`.ttl`, `.trig`, `.owl`, `.jsonld`, …):

```rust
dagalog::load_file(&mut ds, Path::new("data.ttl")).unwrap();
```

---

## SPARQL queries

`run_sparql_query` executes a SELECT query directly against a `Datastore`:

```rust
use dagalog::run_sparql_query;

let result = run_sparql_query(&ds, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }").unwrap();
for row in &result.rows {
    println!("{}", dagalog::graph_element_display(row.get("s").unwrap()));
}
```

### Supported SPARQL 1.2 features

| Feature | Notes |
|---|---|
| `SELECT`, `SELECT DISTINCT`, `SELECT *` | Full |
| Basic graph patterns — `;` and `,` shorthand | Full |
| `FILTER` — comparisons, `regex()`, `lang()`, `bound()`, `EXISTS`, `NOT EXISTS` | Full |
| `OPTIONAL` | Full |
| `UNION` | Full |
| `GRAPH <iri>` and `GRAPH ?var` — named-graph patterns | Full |
| `BIND` | Full |
| `VALUES` — inline data | Full |
| `LIMIT`, `OFFSET` | Full |
| Property paths — `/` (sequence), multi-hop | Full |
| `GROUP BY`, `HAVING`, `ORDER BY` | Parsed; execution not yet implemented |
| Aggregates (`COUNT`, `SUM`, `AVG`, …) | Not yet implemented |
| `CONSTRUCT`, `ASK`, `DESCRIBE` | Not yet implemented |

### SPARQL examples

**FILTER:**

```sparql
PREFIX ns: <http://example.org/ns#>
SELECT ?title ?price WHERE {
    ?x ns:price ?price ;
       ns:title ?title .
    FILTER (?price < 20)
}
-- test: readme_sparql_filter
```

**OPTIONAL:**

```sparql
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name ?mbox WHERE {
    ?x foaf:name ?name .
    OPTIONAL { ?x foaf:mbox ?mbox }
}
-- ?mbox is unbound for subjects that have no foaf:mbox.
-- test: readme_sparql_optional
```

**GRAPH clause (named graphs):**

```sparql
SELECT ?person ?field WHERE {
    GRAPH <http://example.org/scientists> {
        ?person <http://example.org/field> ?field
    }
}
-- test: readme_sparql_graph_clause
```

**DISTINCT + LIMIT:**

```sparql
SELECT DISTINCT ?tag WHERE { ?s <http://example.org/tag> ?tag }
LIMIT 3
-- test: readme_sparql_distinct_limit
```

---

## OWL 2 RL reasoning

OWL 2 RL ontologies are translated to Datalog rules and materialised in-memory.

```rust
use dagalog::load_file;
use datalog::evaluate_rules;
use owl2rl2datalog::owl2datalog;
use rdf_owl_translator::rdf2owl;

let mut ds = Datastore::new(100_000);
load_file(&mut ds, Path::new("ontology.ttl")).unwrap();

let ontology = rdf2owl(&mut ds).ontology;
let rules    = owl2datalog(&mut ds.resources, &ontology);
evaluate_rules(rules, &mut ds);
// test: readme_owl_same_as — owl:sameAs equality propagation
```

**Supported OWL 2 RL patterns** (non-exhaustive):

- `owl:sameAs` — equality propagation
- `rdfs:subClassOf`, `rdfs:subPropertyOf`
- `owl:intersectionOf`, `owl:unionOf`
- `owl:someValuesFrom`, `owl:allValuesFrom`
- `owl:minQualifiedCardinality`
- Inverse object properties

Use `--ontology` on the CLI to apply reasoning before running a query.

---

## Custom Datalog rules

### Syntax

```datalog
# Prefix declarations (SPARQL-style or Turtle-style)
PREFIX ex: <https://example.com/data#>
@prefix ex2: <https://example.com/data2#> .

# Bracket triple syntax:  head :- body .
[?x, a, ?c] :- [?x, ?p, ?y], [?p, rdfs:range, ?c] .

# Predicate-first syntax:  predicate[subject, object]
ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj], ex:prop3[?s, ex:obj] .

# Type atom:  predicate[subject]  means  subject rdf:type predicate
ex:Employee[?x] :- ex:Manager[?x] .

# Stratified negation
ex:Eligible[?x] :- ex:Applicant[?x], NOT ex:Rejected[?x] .

# Named-graph rule
[?s, ex:p, ex:o] ?graph :- ex:p[?s, ex:o] ?graph .

# Inconsistency constraint
false :- [?X, a, ex:Disjoint1], [?X, a, ex:Disjoint2] .
```

Built-in prefixes (no declaration needed): `rdf:`, `rdfs:`, `xsd:`, `owl:`.  
`a` expands to `rdf:type` everywhere.

### Applying rules from Rust

```rust
use dagalog::apply_rules;

apply_rules(&mut ds, &[PathBuf::from("rules.datalog")]).unwrap();
// test: readme_datalog_rule_forward_chain
```

Stratified negation is supported: the stratifier (`datalog::RulePartitioner`)
computes a topological ordering that resolves negation-dependency strata before
materialisation begins.

```rust
// test: readme_datalog_stratified_negation
// rules.datalog: Type3[?x] :- Type[?x], NOT Type2[?x].
// After applying: nodes of type Type become Type3 (not Type2).
```

---

## CLI usage

### Basic query

```sh
dagalog --data data.ttl --query "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10"
dagalog -d data.ttl    -Q query.sparql
dagalog -d data.jsonld -Q query.sparql    # JSON-LD input accepted
```

### OWL-RL reasoning

```sh
dagalog --data data.ttl --ontology schema.ttl \
        --query "SELECT ?x WHERE { ?x a <http://schema.org/Person> }"
```

### Custom Datalog rules

```sh
dagalog --data data.ttl --rules rules.datalog \
        --query "SELECT ?x WHERE { ?x a <https://example.com/data#Employee> }"
```

Multiple `--data`, `--ontology`, and `--rules` flags may be given.

### Output formats

```sh
dagalog -d data.ttl -Q q.sparql --format table   # default
dagalog -d data.ttl -Q q.sparql --format csv
dagalog -d data.ttl -Q q.sparql --format json
```

### Start the HTTP endpoint

```sh
dagalog --data data.ttl --ontology schema.ttl --serve
dagalog --data data.ttl --serve --port 8080
```

---

## Running with Docker

A `Dockerfile` and `docker-compose.yml` are included in the repository.

### Build and run locally

```sh
# Build the image
docker build -t dagalog .

# Start an empty server on port 3030
docker run -p 3030:3030 dagalog

# Load a local Turtle file at startup
docker run -p 3030:3030 -v ./data:/data dagalog --serve --data /data/my.ttl
```

Open <http://localhost:3030> in your browser for the interactive UI.

### With docker-compose

The default `docker-compose.yml` mounts a local `./data/` directory and
loads `./data/dataset.ttl` on startup:

```sh
docker compose up
```

To start with an empty store instead, override the command:

```sh
docker compose run --rm -p 3030:3030 dagalog --serve
```

### Environment variables

All CLI flags can also be set via environment variables (CLI flags take precedence):

| Variable | CLI flag | Description | Default |
|---|---|---|---|
| `DAGALOG_PORT` | `--port` | Port to listen on | `3030` |
| `DAGALOG_BASE_IRI` | `--base-iri` | Base IRI for the Service Description | `http://localhost:PORT` |
| `DAGALOG_READ_ONLY` | `--read-only` | Disable all mutating endpoints | `false` |
| `DAGALOG_QUERY_TIMEOUT` | `--query-timeout` | Maximum query time in seconds | `30` |

---

## SPARQL HTTP endpoint

### Root endpoints

| Route | Description |
|---|---|
| `GET /` | Browser UI (query + upload) |
| `GET /sparql?query=<encoded>` | SPARQL 1.1 SELECT |
| `POST /sparql` | SPARQL 1.1 SELECT (form body or direct) |
| `GET /sparql` (no `query=`) | SPARQL 1.1 Service Description (Turtle) |
| `POST /upload` | Load Turtle data into the default graph (legacy alias) |

### Graph Store Protocol (GSP)

| Route | Description |
|---|---|
| `GET /rdf-graph-store?default` or `?graph=<iri>` | Retrieve a graph (Turtle or N-Triples) |
| `PUT /rdf-graph-store?default` or `?graph=<iri>` | Replace a graph |
| `POST /rdf-graph-store?default` or `?graph=<iri>` | Merge triples into a graph |
| `POST /rdf-graph-store` | Create a new graph (server assigns IRI, returns `Location` header) |
| `DELETE /rdf-graph-store?default` or `?graph=<iri>` | Delete a graph |
| `HEAD /rdf-graph-store?default` or `?graph=<iri>` | Existence check, no body |
| `GET /rdf-graphs/{name}` | Direct graph identification (§4.1) |
| `PUT /rdf-graphs/{name}` | Direct graph identification — replace |

### Fuseki-compatible per-dataset routes

The server exposes a `default` dataset at `/ds` (and any datasets created via the admin API):

| Route | Description |
|---|---|
| `GET /{name}/sparql` or `/{name}/query` | SPARQL SELECT |
| `POST /{name}/sparql` or `/{name}/query` | SPARQL SELECT (form or direct body) |
| `POST /{name}/update` | SPARQL Update (INSERT/DELETE/CLEAR/DROP/…) |
| `GET|PUT|POST|DELETE|HEAD /{name}/data` | GSP read-write |
| `GET|HEAD /{name}/get` | GSP read-only |

### Admin API (`/$/…`)

| Route | Description |
|---|---|
| `GET /$/ping` | Liveness check |
| `GET /$/server` | Server info (version, dataset list) |
| `GET /$/datasets` | List all datasets |
| `POST /$/datasets` | Create a dataset (form body: `dbName=…&dbType=mem`) |
| `GET /$/datasets/{name}` | Dataset info |
| `DELETE /$/datasets/{name}` | Drop a dataset |

Response format negotiated via `Accept`; default `application/sparql-results+json`.

### Library usage

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use dag_rdf::Datastore;
use sparql_endpoint::{Config, serve};

#[tokio::main]
async fn main() {
    let mut store = Datastore::new(1_000_000);
    // load data, apply reasoning, apply rules …
    let config = Config::default(); // 0.0.0.0:3030
    serve(Arc::new(RwLock::new(store)), config).await.unwrap();
}
```

---

## Web UI

Navigate to `http://localhost:3030` in your browser for the interactive interface.

| Feature | Description |
|---|---|
| SPARQL query editor | Prefix manager (persisted), query templates, Ctrl+Enter shortcut, query history |
| Result export | Download results as CSV or JSON |
| Resource browser | Click any IRI to explore its properties and back-links |
| Class hierarchy | `/?view=classes` — collapsible tree of `rdfs:subClassOf` relationships |
| Graph visualisation | Three-variable queries render as an interactive node-edge graph |
| Turtle upload | Paste Turtle or drag-and-drop `.ttl`/`.owl`/`.jsonld` files |
| Store statistics | Live triple count shown in the page header |

### Query editor

- **Prefix manager** — collapsible panel above the textarea; pre-populated with common
  prefixes (rdf, rdfs, owl, xsd, skos, dc, foaf, schema). Prefixes are persisted to
  `localStorage` and automatically prepended to every submitted query.
- **Query templates** — dropdown with example queries (all triples, all classes, class
  hierarchy, labels).
- **Ctrl+Enter** — keyboard shortcut to run the query.
- **Query history** — last 50 queries stored in `localStorage`, shown in a collapsible
  panel below the textarea. Click any entry to restore it.
- **Export** — "Download CSV" and "Download JSON" buttons appear beneath every result table.

### Resource browser

Clicking any IRI in query results opens a resource page (`/?resource=<iri>`) showing:

- `rdfs:label` as the page heading when present
- `rdf:type` class memberships shown as badges
- All outgoing properties (`?p ?o`) — collapsible table
- All incoming back-links (`?s ?p`) — collapsible table (capped at 200)

All IRIs on the resource page are also clickable, enabling linked-data browsing.

### Class hierarchy

`/?view=classes` runs `SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }` and
renders the result as a collapsible `<details>` tree. Useful for exploring OWL ontologies.

### Graph view

When a SELECT query returns exactly three variables (subject, predicate, object), a **Graph**
tab appears next to the Table tab. The graph is rendered via
[Cytoscape.js](https://js.cytoscape.org/) (loaded from CDN on first use):

- **Blue nodes** — URI resources (click to open the resource browser)
- **Grey nodes** — blank nodes
- **Directed edges** — labelled with the shortened predicate name

Graphs are capped at 200 nodes; add `LIMIT` to reduce the dataset.

---

## Protocol compliance

See [`PROTOCOLS.md`](PROTOCOLS.md) for full details.

| Priority | Protocol | Status |
|---|---|---|
| P0 | SPARQL 1.1 Protocol — SELECT, content negotiation, CORS | Done |
| P0 | SPARQL 1.1 Service Description | Done |
| P1 | SPARQL 1.1 Graph Store HTTP Protocol | Done (§4.1 direct identification + §5.2–§5.6) |
| P1 | SPARQL 1.1 Update | Done (INSERT/DELETE/CLEAR/DROP/CREATE/COPY/MOVE/ADD) |
| P1 | Fuseki-compatible dataset routing and admin API | Done |
| P2 | VoID dataset description | Planned |

---

## Implementation plan

See [`PLAN.md`](PLAN.md) for the full phased roadmap.

---

## License

GNU General Public License v3.0 — see [`LICENSE`](LICENSE).

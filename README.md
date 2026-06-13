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
| Static API key authentication (library API; `--api-key` CLI flag pending) | ✓ |
| OWL 2 RL reasoning via Datalog materialisation | ✓ |
| Custom Datalog rules with stratified negation | ✓ |
| Named graphs (load, query, reason over) | ✓ |
| SHACL Core validation via Datalog translation | in progress |
| SHACL-AF SPARQL-based constraints (§5–6) | planned |
| OWL Manchester Syntax parser | planned |
| Durable transactional persistence (`redb`-backed WAL) | planned |
| Incremental Datalog materialisation (Backward/Forward algorithm) | planned |

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
| `shacl` | SHACL Core → Datalog translation + `ValidationReport` types |
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

## SHACL validation

SHACL (Shapes Constraint Language — [W3C TR](https://www.w3.org/TR/shacl/)) lets
you declare constraints over RDF graphs and validate data against them.

**Implementation approach:** SHACL Core constraints are translated to stratified Datalog
rules (the same engine that powers OWL-RL reasoning), then materialised over the data
graph. SHACL-AF §5–6 SPARQL-based constraints are executed directly by the built-in
SPARQL engine. No external processes or dependencies are required.

The `shacl` crate types are defined; validation is in progress — see
[`SHACL_PLAN.md`](SHACL_PLAN.md) for the phased implementation roadmap.

### API (planned)

```rust
use dag_rdf::Datastore;
use dagalog::load_file;

let mut data = Datastore::new(100_000);
load_file(&mut data, Path::new("data.ttl")).unwrap();

let mut shapes = Datastore::new(10_000);
load_file(&mut shapes, Path::new("shapes.ttl")).unwrap();

let report = shacl::validate(&data, &shapes).unwrap();
if report.conforms {
    println!("data graph conforms to all shapes");
} else {
    for r in &report.results {
        println!("violation at {:?}: {:?}", r.focus_node, r.message);
    }
}
```

### §1.4 Introductory example

The W3C SHACL specification §1.4 introduces a `PersonShape` that constrains every
`ex:Person` instance:

```turtle
PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX sh:  <http://www.w3.org/ns/shacl#>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
PREFIX ex:  <http://example.com/ns#>

ex:PersonShape
    a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [
        sh:path ex:ssn ;
        sh:maxCount 1 ;
        sh:datatype xsd:string ;
        sh:pattern "^\\d{3}-\\d{2}-\\d{4}$" ;
    ] ;
    sh:property [
        sh:path ex:worksFor ;
        sh:class ex:Company ;
        sh:nodeKind sh:IRI ;
    ] ;
    sh:closed true ;
    sh:ignoredProperties ( rdf:type ) .
```

The spec data graph (`shacl_s1_intro_data.ttl`) produces 4 violations:

| Focus node | Constraint | Reason |
|---|---|---|
| `ex:Alice` | `sh:pattern` | `"987-65-432A"` does not match `^\d{3}-\d{2}-\d{4}$` |
| `ex:Bob` | `sh:maxCount 1` | Two `ex:ssn` values present |
| `ex:Calvin` | `sh:class ex:Company` | `ex:UntypedCompany` is not an `ex:Company` |
| `ex:Calvin` | `sh:closed` | `ex:birthDate` is not listed in any `sh:property` |

```
# test: spec_s1_4_intro_person_shape_violations
```

### SHACL Core constraint components covered

All tests are in [`tests/shacl_suite.rs`](tests/shacl_suite.rs) and are `#[ignore]`
until the `shacl` crate implementation is complete. Test data lives in
`tests/testdata/shacl_*.ttl`; all files are verified to parse by `shacl_testdata_parses`.

| Spec section | Constraint component | Test name |
|---|---|---|
| §1.4 | Intro: `sh:closed`, `sh:pattern`, `sh:class`, `sh:maxCount` | `spec_s1_4_intro_person_shape_violations` |
| §2.1.3.1 | `sh:targetNode` | `spec_s2_1_1_target_node` |
| §2.1.3.2 | `sh:targetClass` | `spec_s2_1_2_target_class` |
| §2.1.3.3 | Implicit class target (`rdfs:Class` + `sh:NodeShape`) | `spec_s2_1_3_target_implicit_class` |
| §2.1.3.4 | `sh:targetSubjectsOf` | `spec_s2_1_4_target_subjects_of` |
| §2.1.3.5 | `sh:targetObjectsOf` | `spec_s2_1_5_target_objects_of` |
| §4.1.1 | `sh:class` | `spec_s4_1_1_class` |
| §4.1.2 | `sh:datatype` | `spec_s4_1_2_datatype` |
| §4.1.3 | `sh:nodeKind` | `spec_s4_1_3_nodekind` |
| §4.2.1 | `sh:minCount` | `spec_s4_2_1_mincount` |
| §4.2.2 | `sh:maxCount` | `spec_s4_2_2_maxcount` |
| §4.3 | `sh:minInclusive`, `sh:maxInclusive` | `spec_s4_3_value_range` |
| §4.4.1 | `sh:minLength` | `spec_s4_4_1_minlength` |
| §4.4.2 | `sh:maxLength` | `spec_s4_4_2_maxlength` |
| §4.4.3 | `sh:pattern` | `spec_s4_4_3_pattern` |
| §4.4.4 | `sh:languageIn` | `spec_s4_4_4_languagein` |
| §4.4.5 | `sh:uniqueLang` | `spec_s4_4_5_uniquelang` |
| §4.5.1 | `sh:equals` | `spec_s4_5_1_equals` |
| §4.5.2 | `sh:disjoint` | `spec_s4_5_2_disjoint` |
| §4.5.3 | `sh:lessThan` | `spec_s4_5_3_lessthan` |
| §4.5.4 | `sh:lessThanOrEquals` | `spec_s4_5_4_lessthanorequals` |
| §4.6.1 | `sh:not` | `spec_s4_6_1_not` |
| §4.6.2 | `sh:and` | `spec_s4_6_2_and` |
| §4.6.3 | `sh:or` | `spec_s4_6_3_or` |
| §4.6.4 | `sh:xone` | `spec_s4_6_4_xone` |
| §4.7.1 | `sh:node` | `spec_s4_7_1_node` |
| §4.7.3 | `sh:qualifiedValueShape` + `sh:qualifiedMinCount` | `spec_s4_7_3_qualified_value_shape` |
| §4.8.1 | `sh:closed` + `sh:ignoredProperties` | `spec_s4_8_1_closed` |
| §4.8.2 | `sh:hasValue` | `spec_s4_8_2_has_value` |
| §4.8.3 | `sh:in` | `spec_s4_8_3_in` |

> Not yet covered: §4.3.1 `sh:minExclusive`, §4.3.3 `sh:maxExclusive`,
> §4.7.2 `sh:property` (inline shape), §4.7.3 `sh:qualifiedMaxCount`,
> §6 SPARQL-based constraints (SHACL-AF).

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
| `DAGALOG_DATA_DIR` | `--data-dir` | Directory for durable storage (`redb`); omit for in-memory mode | *(in-memory)* |
| `DAGALOG_NO_PERSIST` | `--no-persist` | Force in-memory mode even if `DAGALOG_DATA_DIR` is set | `false` |
| `DAGALOG_DB_FILE` | `--db-file` | Database filename inside `--data-dir` | `dagalog.redb` |

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
use sparql_endpoint::{AuthConfig, Config, serve};

#[tokio::main]
async fn main() {
    let mut store = Datastore::new(1_000_000);
    // load data, apply reasoning, apply rules …

    // No authentication (default):
    let config = Config::default(); // 0.0.0.0:3030, no auth

    // Or protect writes with a static API key:
    let config = Config {
        auth: AuthConfig::ApiKey {
            key: "my-secret".to_string(),
            require_for_reads: false, // reads stay open
        },
        ..Config::default()
    };

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
| Visual query builder | `/?view=build` — point-and-click SPARQL composition; no query syntax required |
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

### Visual query builder

Accessible at `/?view=build`, or via the **"Build query"** link that appears next to each
`rdf:type` badge on a resource page.

The builder lets you compose SELECT queries by clicking rather than writing SPARQL.
The generated query is always shown and can be pushed into the full SPARQL editor for
further hand-editing.

**Class picker** — type any substring of a class name or IRI to filter the dropdown; all
classes known to the store (declared as `owl:Class` or used as a `rdf:type` target) are
listed. Selecting a class immediately populates the property panes.

**Property panes** — properties are discovered by sampling the store:
- *Data properties* (literal-valued) appear as checkboxes. Checking one adds an
  `OPTIONAL { ?s <prop> ?var }` block to the query.
- *Object properties* (IRI-valued) appear as **Follow →** buttons. Clicking one adds a
  linked node card to the canvas and shifts focus to it.

**Canvas** — node cards are laid out left-to-right connected by labelled arrows. Clicking
any card makes it active; its property pane updates accordingly. A `×` button on each
non-root card removes that node and its subtree.

**Data-property filters** — when a data property is checked, a filter input appears.
Typing text makes the property required (not OPTIONAL) and adds
`FILTER(regex(?var, "text", "i"))` to the query.

**Generated SPARQL rules:**
- `?node a <Class>` triples are always required.
- Object-property links are required (inner-join semantics — only instances with the link appear).
- Unchecked data properties are excluded entirely.
- Checked data properties without a filter are `OPTIONAL`.
- Checked data properties with a filter become required triples with a top-level `FILTER`.
- Variables follow the pattern `?s`, `?n1`, `?n2`, … for nodes and `?s_label`, `?n1_age`, …
  for data properties.

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

Upcoming areas:
- [`PERSISTENCE_PLAN.md`](PERSISTENCE_PLAN.md) — durable transactional storage (`redb`) and incremental Datalog maintenance (Backward/Forward algorithm)
- [`SHACL_PLAN.md`](SHACL_PLAN.md) — SHACL Core validation via Datalog translation
- [`AUTH.md`](AUTH.md) — API-key and Entra ID authentication

---

## License

GNU General Public License v3.0 — see [`LICENSE`](LICENSE).

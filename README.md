# dagalog

A fast RDF triplestore in Rust: Datalog-based OWL-RL reasoning, custom Datalog rules,
SHACL validation, RML mapping of CSV/JSON/XML to RDF, and a SPARQL HTTP endpoint. Reads
and writes Turtle, TriG, N-Triples, N-Quads, and JSON-LD 1.1, with SPARQL 1.2 triple-term
support.

The core is a Rust port of [DagSemTools](https://github.com/daghovland/DagSemTools) (F#/.NET).
Almost all implementation is done by LLM-based agents.

> **New here?** Start with the [5-minute quickstart](docs/user/quickstart.md) — load data and
> run your first query without needing to read the rest of this file.
>
> **User docs:** [docs/user/](docs/user/) — quickstart, SPARQL guide, formats, reasoning, deployment  
> **Developer docs:** [docs/dev/](docs/dev/) — architecture, ADRs, contributing  
> **Contributing:** [CONTRIBUTING.md](CONTRIBUTING.md)

---

## Features

- **Formats** — read and write Turtle, TriG, N-Triples, N-Quads, and JSON-LD 1.1 ([details](docs/user/formats.md))
- **SPARQL** — SPARQL 1.1 Query and Update, in-process or over HTTP, with SPARQL 1.2 triple terms ([guide](docs/user/sparql-guide.md))
- **Reasoning** — OWL 2 RL materialisation, plus custom Datalog rules with stratified negation ([guide](docs/user/reasoning.md))
- **OWL parsing** — Turtle-encoded OWL ontologies and OWL 2 Manchester Syntax (`.omn`)
- **SHACL** — SHACL Core validation via Datalog translation
- **RML** — map CSV, JSON, JSONL, and XML to RDF ([guide](docs/user/rml-mapping.md))
- **OTTR** — reusable, typed RDF templates (stOTTR), expandable in-process, over HTTP, or from Jupyter ([guide](docs/user/ottr-templates.md))
- **HTTP server** — SPARQL 1.1 Protocol, Graph Store Protocol, multi-dataset (Fuseki-compatible) routing, VoID, ETag caching ([deployment](docs/user/deployment.md))
- **Transactions** — `BEGIN`/`COMMIT`/`ROLLBACK` API for atomic multi-statement updates
- **Persistence** — durable `redb`-backed changelog, or pure in-memory
- **Incremental reasoning** — SPARQL Update maintains OWL-RL/Datalog materialisation incrementally (no full re-evaluation) when the server starts with reasoning rules loaded
- **Auth** — static API key or OIDC JWT (Azure Entra ID, Google, Keycloak, Auth0) ([deployment](docs/user/deployment.md))
- **Web UI** — query editor, resource browser, class hierarchy, graph visualisation, visual query builder

Not yet supported: SHACL-AF (SPARQL-based constraints), RML SQL/JDBC sources.

> Every code example in this file is also an integration test in
> [`tests/readme_examples.rs`](tests/readme_examples.rs).
> If a test breaks, the README is out of date — update both together.
>
> Each section below also links a **"Proven by"** test suite — usually a larger,
> spec-organised file with many more examples than fit here. Every one of those
> files runs standalone with `cargo test --test <filename-without-.rs>`.

---

## Installation

**Prerequisites:** Rust toolchain 1.85 or later (the workspace uses Rust edition 2024).
Install via [rustup](https://rustup.rs/) if needed.

```sh
# From Git, no local clone required
cargo install --git https://github.com/daghovland/rdf-datalog dagalog

# Or from a local checkout
git clone https://github.com/daghovland/rdf-datalog
cd rdf-datalog
cargo install --path .
```

Both place the `dagalog` binary in `~/.cargo/bin/`, which `rustup` adds to `$PATH`
automatically.

Or run it via Docker without installing Rust at all — see [Docker](#docker) below.

---

## Data formats

The format is auto-detected from the file extension: `.ttl` (Turtle), `.trig` (TriG,
named graphs), `.nt`/`.nq` (N-Triples/N-Quads), `.jsonld` (JSON-LD 1.1). See
[docs/user/formats.md](docs/user/formats.md) for the full reference and serialisation options.

```rust
use dag_rdf::Datastore;
use dagalog::run_sparql_query;

let mut ds = Datastore::new(10_000);
turtle::parse_turtle(&mut ds, ttl_bytes).unwrap();

let result = run_sparql_query(&ds, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }").unwrap();
// test: readme_turtle_parse_basic
```

```rust
let jsonld = r#"{
  "@context": { "foaf": "http://xmlns.com/foaf/0.1/" },
  "@id": "http://example.org/alice",
  "@type": "foaf:Person",
  "foaf:name": "Alice"
}"#;

let mut ds = Datastore::new(10_000);
jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes(), ingress::NetworkPolicy::Deny).unwrap();
// test: readme_jsonld_parse_inline
```

`load_file` dispatches on extension automatically, for any of the formats above:

```rust
dagalog::load_file(&mut ds, Path::new("data.ttl")).unwrap();
```

**Proven by:** [`tests/jsonld_suite.rs`](tests/jsonld_suite.rs) (`cargo test --test jsonld_suite`)
and the W3C conformance suites [`tests/w3c_rdf_conformance.rs`](tests/w3c_rdf_conformance.rs) /
[`tests/w3c_rdf12_conformance.rs`](tests/w3c_rdf12_conformance.rs) — one test per spec example,
runnable individually.

---

## SPARQL queries

`run_sparql_query` executes SPARQL 1.1 Query (SELECT/ASK/CONSTRUCT/DESCRIBE) directly
against a `Datastore` — property paths, aggregates, subqueries, `OPTIONAL`/`UNION`/`MINUS`,
named graphs, and SPARQL 1.2 triple terms are all supported. See the
[SPARQL guide](docs/user/sparql-guide.md) for a full walkthrough and feature list.

```rust
use dagalog::run_sparql_query;

let result = run_sparql_query(
    &ds,
    "PREFIX ns: <http://example.org/ns#>
     SELECT ?title ?price WHERE {
         ?x ns:price ?price ; ns:title ?title .
         FILTER (?price < 20)
     }",
).unwrap();
// test: readme_sparql_filter
```

**Proven by:** [`tests/sparql12_suite.rs`](tests/sparql12_suite.rs) (`cargo test --test sparql12_suite`),
organised by spec section, and the [W3C SPARQL 1.1 conformance suite](tests/w3c_sparql11_suite.rs).

---

## RML: mapping CSV, JSON, and XML to RDF

The `rml` crate implements [RML 1.0](https://www.w3.org/TR/rml/) mapping documents: a
mapping file declares how CSV columns, JSON fields, or XML elements become RDF triples.
Mapped triples land in the same `Datastore` as any other loaded data, so they're
immediately queryable and reasoning-ready. See the
[RML mapping guide](docs/user/rml-mapping.md) for the full syntax reference, including
JSON/XML sources, named graphs, and the HTTP endpoints.

```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/PersonMap>
    a rml:TriplesMap ;
    rml:logicalSource [ rml:source "people.csv" ; rml:referenceFormulation rml:CSV ] ;
    rml:subjectMap [ rml:template "http://example.com/Person/{id}" ; rml:class ex:Person ] ;
    rml:predicateObjectMap [ rml:predicate ex:name ; rml:objectMap [ rml:reference "name" ] ] .
```

```rust
use rml::apply_rml_mapping;

let mut ds = Datastore::new(100_000);
apply_rml_mapping(Path::new("mapping.ttl"), Path::new("."), &mut ds).unwrap();
```

**Proven by:** [`tests/rml_integration.rs`](tests/rml_integration.rs) (CSV, `cargo test --test rml_integration`),
[`tests/rml_json_integration.rs`](tests/rml_json_integration.rs), and
[`tests/rml_xml_integration.rs`](tests/rml_xml_integration.rs).

---

## OTTR: reusable RDF templates

The `ottr` crate implements [OTTR (Reasonable Ontology Templates)](https://ottr.xyz/)'s
stOTTR text syntax: define a typed, parameterised triple-pattern template once, then call
it for each instance instead of repeating triples by hand. See the
[OTTR templates guide](docs/user/ottr-templates.md) for the full syntax reference —
nested template calls, `none` arguments, and the `cross`/`zipMin` list expanders.

```stottr
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person, xsd:string ?name ] :: {
  ottr:Triple (?person, rdf:type,  foaf:Person),
  ottr:Triple (?person, foaf:name, ?name)
} .

ex:Person(<http://example.com/alice>, "Alice") .
ex:Person(<http://example.com/bob>,   "Bob")   .
```

```rust
use dag_rdf::Datastore;
use ottr::{expand_documents, parser::parse_stottr};

let src = r#"
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person, xsd:string ?name ] :: {
  ottr:Triple (?person, rdf:type,  foaf:Person),
  ottr:Triple (?person, foaf:name, ?name)
} .

ex:Person(<http://example.com/alice>, "Alice") .
"#;

let mut ds = Datastore::new(100_000);
let doc = parse_stottr(src).unwrap();
expand_documents(&[doc], &mut ds).unwrap();
```

Templates also expand over HTTP — `POST /{dataset}/ottr` with one or more stOTTR document
parts as `multipart/form-data` materialises the expansion directly into the named dataset:

```sh
curl -F "document=@templates.stottr" -F "document=@instances.stottr" \
     http://localhost:3030/mydataset/ottr
```

...and inline in a [Jupyter notebook](docs/user/jupyter.md) via the `%%ottr` cell magic.

**Proven by:** [`ottr/tests/`](ottr/tests/) (parser, expansion, list expanders — `cargo test -p ottr`)
and [`sparql_endpoint/tests/ottr_endpoint.rs`](sparql_endpoint/tests/ottr_endpoint.rs) (HTTP endpoint).

---

## OWL 2 RL reasoning

OWL 2 RL ontologies are translated to Datalog rules and materialised in-memory, following
the [W3C OWL 2 RL profile](https://www.w3.org/TR/owl2-profiles/). See the
[reasoning guide](docs/user/reasoning.md) for the supported axiom patterns and how to
combine reasoning with custom rules.

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

Use `--ontology` on the CLI to apply reasoning before running a query. Ontologies written
in OWL 2 Manchester Syntax (`.omn`) parse directly to the same `Ontology` type via the
`manchester_parser` crate, so they reason identically.

**Proven by:** [`tests/owl_integration.rs`](tests/owl_integration.rs) (`cargo test --test owl_integration`)
and [`tests/manchester_owl_reasoning.rs`](tests/manchester_owl_reasoning.rs).

---

## Custom Datalog rules

When OWL-RL doesn't cover a case, write forward-chaining Datalog rules directly:

```datalog
PREFIX ex: <https://example.com/data#>

# head :- body   (bracket triple syntax, or predicate[args] shorthand)
[?x, a, ?c] :- [?x, ?p, ?y], [?p, rdfs:range, ?c] .
ex:Employee[?x] :- ex:Manager[?x] .

# Stratified negation
ex:Eligible[?x] :- ex:Applicant[?x], NOT ex:Rejected[?x] .

# FILTER guard — any SPARQL 1.1 expression
ex:Minor[?x] :- [?x, ex:age, ?a], FILTER(?a < 18) .
```

Built-in prefixes (no declaration needed): `rdf:`, `rdfs:`, `xsd:`, `owl:`. `a` expands to
`rdf:type`. See the [reasoning guide](docs/user/reasoning.md#custom-datalog-rules) for the
full syntax reference.

```rust
use dagalog::apply_rules;

apply_rules(&mut ds, &[PathBuf::from("rules.datalog")]).unwrap();
// test: readme_datalog_rule_forward_chain
```

**Proven by:** [`tests/datalog_integration.rs`](tests/datalog_integration.rs) — `cargo test --test datalog_integration`.

---

## SHACL validation

[SHACL](https://www.w3.org/TR/shacl/) (Shapes Constraint Language) declares constraints
over RDF graphs and validates data against them. SHACL Core is fully supported, translated
to stratified Datalog rules — the same engine that powers OWL-RL reasoning, with no
external processes required. SHACL-AF (SPARQL-based targets and constraints, §5–6) is not
yet implemented.

```rust
use dagalog::load_file;

let mut data = Datastore::new(100_000);
load_file(&mut data, Path::new("data.ttl")).unwrap();

let mut shapes = Datastore::new(10_000);
load_file(&mut shapes, Path::new("shapes.ttl")).unwrap();

let report = shacl::validate(&data, &shapes).unwrap();
if !report.conforms {
    for r in &report.results {
        println!("violation at {:?}: {:?}", r.focus_node, r.message);
    }
}
```

**Proven by:** [`tests/shacl_suite.rs`](tests/shacl_suite.rs) — `cargo test --test shacl_suite`,
one test per W3C SHACL spec example.

---

## CLI usage

```sh
# Query
dagalog --data data.ttl --query "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10"
dagalog -d data.ttl -Q query.sparql             # query from a file
dagalog -d data.jsonld -Q query.sparql          # any supported format works

# OWL-RL reasoning
dagalog --data data.ttl --ontology schema.ttl --query "SELECT ?x WHERE { ?x a <http://schema.org/Person> }"

# Custom Datalog rules
dagalog --data data.ttl --rules rules.datalog --query "..."

# RML mapping
dagalog --mapping mapping.ttl --query "SELECT ?name WHERE { ?p <http://example.com/name> ?name }"

# Output format
dagalog -d data.ttl -Q q.sparql --format csv    # table (default), csv, or json

# Start the HTTP server
dagalog --data data.ttl --ontology schema.ttl --serve
```

Multiple `--data`, `--ontology`, `--rules`, and `--mapping` flags may be given; mappings
run after `--data` and before `--ontology`/`--rules`, so mapped triples participate in
reasoning.

**Proven by:** [`tests/cli_integration.rs`](tests/cli_integration.rs) — `cargo test --test cli_integration`.

---

## Docker

```sh
docker run -p 3030:3030 ghcr.io/daghovland/dagalog                       # empty store
docker run -p 3030:3030 -v ./data:/data ghcr.io/daghovland/dagalog \
    --serve --data /data/my.ttl                                         # load a file
```

Open <http://localhost:3030> for the interactive UI. A `docker-compose.yml` is also
included (`docker compose up`). See [deployment](docs/user/deployment.md) for building the
image locally, environment-variable configuration, and read-only mode.

---

## Authentication

Three tiers, selected at startup:

| Tier | Mechanism | When to use |
|------|-----------|-------------|
| 0 — None (default) | No check | Local / trusted-network deployments |
| 1 — API key | Static Bearer token (`--api-key`) | Single-tenant, simple deployments |
| 2 — OIDC | JWT validation (`--oidc-issuer` + `--oidc-audience`) | Multi-user deployments (Azure Entra ID, Google, Keycloak, Auth0) |

```sh
dagalog --serve --data data.ttl --api-key "my-secret-key"
```

Requests are classified into `Read`/`Write`/`Admin` permissions; `--api-key` protects
writes by default (`--require-auth-for-reads` to protect reads too). See
[deployment](docs/user/deployment.md#authentication) for the permission model and
step-by-step OIDC setup guides per provider.

---

## SPARQL HTTP endpoint

`dagalog --serve` exposes the SPARQL 1.1 Protocol (`GET`/`POST /sparql`), the Graph Store
HTTP Protocol (`/rdf-graph-store`), Fuseki-compatible multi-dataset routing, an admin API
(`/$/…`), and VoID dataset description (`/.well-known/void`). Response formats (SPARQL
JSON/XML, CSV, Turtle, N-Quads, JSON-LD, …) are negotiated via `Accept`. See
[deployment](docs/user/deployment.md#http-api-reference) for the full route reference.

```sh
curl "http://localhost:3030/sparql?query=SELECT+*+WHERE+%7B%3Fs+%3Fp+%3Fo%7D"
```

---

## Web UI

Navigate to `http://localhost:3030` for the interactive interface:

- **Query editor** — prefix manager, query templates, query history, Ctrl+Enter to run
- **Result export** — download as CSV or JSON
- **Resource browser** — click any IRI to explore its properties and back-links
- **Class hierarchy** view (`/?view=classes`) — collapsible `rdfs:subClassOf` tree
- **Graph visualisation** — three-variable queries render as an interactive node-edge graph
- **Visual query builder** (`/?view=build`) — compose SELECT queries by clicking, no SPARQL required
- **Turtle upload** — paste or drag-and-drop `.ttl`/`.owl`/`.jsonld` files

---

## License

GNU General Public License v3.0 — see [`LICENSE`](LICENSE).

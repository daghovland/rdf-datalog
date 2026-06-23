# dagalog

A fast RDF triplestore with native Rust implementation of Datalog-based OWL-RL reasoning,
custom Datalog rules, JSON-LD 1.1 parsing/serialisation, and a SPARQL HTTP endpoint.

Rust port of [DagSemTools](https://github.com/daghovland/DagSemTools) (F#/.NET).

> **New here?** Start with the [5-minute quickstart](docs/user/quickstart.md) — load data and
> run your first query without needing to read the rest of this file.
>
> **User docs:** [docs/user/](docs/user/) — quickstart, SPARQL guide, formats, reasoning, deployment  
> **Developer docs:** [docs/dev/](docs/dev/) — architecture, ADRs, contributing  
> **Contributing:** [CONTRIBUTING.md](CONTRIBUTING.md)

---

## Features

| Feature | Status |
|---|---|
| Load RDF from Turtle (`.ttl`) and TriG (`.trig`) | ✓ |
| Load RDF from JSON-LD 1.1 (`.jsonld`) | ✓ |
| Map CSV / JSON / JSONL / XML to RDF via RML 1.0 (Rust API, CLI, and REST) | ✓ |
| Serialise to JSON-LD (expanded, compacted, flattened) | ✓ |
| SPARQL 1.2 SELECT queries (in-process) | ✓ |
| SPARQL 1.1 HTTP endpoint (SELECT/ASK/CONSTRUCT, SPARQL XML and CSV output) | ✓ |
| SPARQL 1.1 Graph Store Protocol (GET/PUT/POST/DELETE/HEAD; JSON-LD output) | ✓ |
| SPARQL 1.1 Update (`POST /sparql`, form body, and per-dataset `/update`) | ✓ |
| Multi-dataset server (Fuseki-compatible routing and admin API) | ✓ |
| VoID dataset description (`GET /.well-known/void`) | ✓ |
| ETag caching headers on all query responses | ✓ |
| Static API key authentication (`--api-key` / `DAGALOG_API_KEY`) | ✓ |
| OIDC JWT authentication (Azure Entra ID, Google, Keycloak, Auth0) | ✓ |
| OWL 2 RL reasoning via Datalog materialisation | ✓ |
| Custom Datalog rules with stratified negation and SPARQL FILTER guards | ✓ |
| Named graphs (load, query, reason over) | ✓ |
| SHACL Core validation via Datalog translation | ✓ complete |
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
| `rml` | RML 1.0 mapping engine: CSV, JSON, JSONL, XML → RDF triples |
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

## RML: mapping CSV, JSON, and XML to RDF

The `rml` crate implements [RML 1.0](https://www.w3.org/TR/rml/) mapping documents.
A mapping file declares how CSV columns, JSON fields, or XML elements become RDF
subjects, predicates, and objects. The same `Datastore` can hold both the mapped
triples and any separately loaded Turtle/JSON-LD ontologies — they are immediately
queryable together with SPARQL.

### CSV example

```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/PersonMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "people.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Person/{id}" ;
        rml:class ex:Person
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
```

### JSON example

```turtle
rml:logicalSource [
    rml:source "students.json" ;
    rml:referenceFormulation rml:JSONPath ;
    rml:iterator "$.students[*]"    # optional: select array from document
] ;
rml:subjectMap [
    rml:template "http://example.com/Student/{$.id}"
] ;
rml:predicateObjectMap [
    rml:predicate ex:name ;
    rml:objectMap [ rml:reference "$.name" ]
] .
```

JSONL files (`.jsonl`, `.ndjson`) are detected by extension and read line by line.

### XML example

```turtle
rml:logicalSource [
    rml:source "students.xml" ;
    rml:referenceFormulation rml:XPath ;
    rml:iterator "/students/student"    # XPath selecting the repeating nodes
] ;
rml:subjectMap [
    rml:template "http://example.com/Student/{id}"
] ;
rml:predicateObjectMap [
    rml:predicate ex:name ;
    rml:objectMap [ rml:reference "name" ]
] .
```

`rml:reference` and template expressions are XPath 1.0, evaluated relative to
each node selected by `rml:iterator` — element names (`name`), attributes
(`@id`), and relative paths (`address/city`) are all supported.

### Rust API

```rust
use rml::apply_rml_mapping;
use dag_rdf::Datastore;
use std::path::Path;

let mut ds = Datastore::new(100_000);
apply_rml_mapping(
    Path::new("mapping.ttl"),  // RML mapping file
    Path::new("."),            // base directory for rml:source paths
    &mut ds,
).unwrap();
```

### What is supported

| Feature | Status |
|---|---|
| CSV sources (`rml:CSV`) | ✓ |
| JSON array sources (`rml:JSONPath`) | ✓ |
| JSONL sources (auto-detected by extension) | ✓ |
| XML sources (`rml:XPath`) | ✓ |
| `rml:iterator` for nested arrays/elements | ✓ |
| Template IRI subjects | ✓ |
| Reference literal objects | ✓ |
| Language-tagged literals (`rml:language`) | ✓ |
| Typed literals (`rml:datatype`) | ✓ |
| Named graphs (`rml:graphMap`) | ✓ |
| Blank node subjects | ✓ |
| `rml:class` shorthand | ✓ |
| Join conditions (`rml:JoinCondition`) | planned |
| SQL/JDBC sources | planned |

See [docs/user/rml-mapping.md](docs/user/rml-mapping.md) for the full reference.

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

SHACL Core (§1–§4.8) is fully implemented. SHACL-AF §5–6 (SPARQL-based targets
and constraints) is planned — see [`docs/plans/SHACL_PLAN.md`](docs/plans/SHACL_PLAN.md) Phase 4.

### API

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

All tests are in [`tests/shacl_suite.rs`](tests/shacl_suite.rs). Test data lives in
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
| §4.3 | `sh:minExclusive`, `sh:maxExclusive` | `spec_s4_3_exclusive_range` |
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
| §4.7.2 | `sh:property` referencing a named `sh:PropertyShape` by IRI | `spec_s4_7_2_property_shape_ref` |
| §4.7.3 | `sh:qualifiedValueShape` + `sh:qualifiedMinCount` | `spec_s4_7_3_qualified_value_shape` |
| §4.7.3 | `sh:qualifiedValueShape` + `sh:qualifiedMaxCount` | `spec_s4_7_3_qualified_max_count` |
| §4.8.1 | `sh:closed` + `sh:ignoredProperties` | `spec_s4_8_1_closed` |
| §4.8.2 | `sh:hasValue` | `spec_s4_8_2_has_value` |
| §4.8.3 | `sh:in` | `spec_s4_8_3_in` |

> Not yet covered: §5–6 SPARQL-based constraints (SHACL-AF).
> See [`docs/plans/SHACL_PLAN.md`](docs/plans/SHACL_PLAN.md) Phase 4 for the implementation plan.

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

# FILTER guard — any SPARQL 1.1 expression
ex:Minor[?x]   :- [?x, ex:age, ?a], FILTER(?a < 18) .
ex:ShortName[?x] :- [?x, ex:name, ?n], FILTER(STRLEN(?n) < 4) .
ex:WrongType[?x] :- [?x, ex:val, ?v], FILTER(DATATYPE(?v) != xsd:integer) .
ex:BadKind[?x]   :- [?x, ex:val, ?v], FILTER(!isIRI(?v)) .
```

Built-in prefixes (no declaration needed): `rdf:`, `rdfs:`, `xsd:`, `owl:`.  
`a` expands to `rdf:type` everywhere.

`FILTER(expr)` in a rule body acts as a guard: the rule fires only when the
SPARQL 1.1 expression evaluates to `true`.  All SPARQL 1.1 operators and
functions are supported (`<`, `>=`, `!=`, `=`, `+`, `-`, `*`, `/`, `&&`, `||`,
`!`, `regex()`, `strlen()`, `datatype()`, `isIRI()`, `isLiteral()`,
`isBlankNode()`, `lang()`, `langMatches()`, `str()`, …).

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

### RML mapping (CSV / JSON / XML → RDF)

```sh
dagalog --mapping mapping.ttl \
        --query "SELECT ?name WHERE { ?p <http://example.com/name> ?name }"

dagalog --data ontology.ttl --mapping mapping.ttl --ontology ontology.ttl \
        --query "SELECT ?x WHERE { ?x a <http://example.com/Person> }"
```

`--mapping` applies an [RML](#rml-mapping-csv-json-and-xml-to-rdf) mapping file, generating
triples from the CSV/JSON/XML sources it references (resolved relative to the mapping
file's own directory). Mappings run after `--data` is loaded and before `--ontology`/
`--rules`, so mapped triples participate in reasoning. Multiple `--mapping` flags may
be given; see the [RML mapping guide](docs/user/rml-mapping.md) for the mapping syntax.

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
| `DAGALOG_DATA_DIR` | `--data-dir` | Directory for durable storage (`redb` changelog at `<dir>/dagalog.redb`); omit for in-memory mode | *(in-memory)* |
| `DAGALOG_NO_PERSIST` | `--no-persist` | Force in-memory mode even if `DAGALOG_DATA_DIR` is set | `false` |
| `DAGALOG_API_KEY` | `--api-key` | Static Bearer token; omit to disable Tier 1 auth | *(none)* |
| `DAGALOG_AUTH_READS` | `--require-auth-for-reads` | Protect read endpoints with the API key too | `false` |
| `DAGALOG_OIDC_ISSUER` | `--oidc-issuer` | OIDC provider base URL | *(none)* |
| `DAGALOG_OIDC_AUDIENCE` | `--oidc-audience` | Expected `aud` JWT claim | *(none)* |
| `DAGALOG_OIDC_JWKS_URI` | `--oidc-jwks-uri` | Explicit JWKS URI (skips OIDC discovery) | *(auto-discovered)* |
| `DAGALOG_OIDC_ROLES_CLAIM` | `--oidc-roles-claim` | JWT claim path holding the roles array | `roles` |
| `DAGALOG_OIDC_READ_ROLE` | `--oidc-read-role` | Role name that grants read access | `dagalog.Read` |
| `DAGALOG_OIDC_WRITE_ROLE` | `--oidc-write-role` | Role name that grants write access | `dagalog.Write` |
| `DAGALOG_OIDC_ADMIN_ROLE` | `--oidc-admin-role` | Role name that grants admin access | `dagalog.Admin` |
| `DAGALOG_OIDC_BROWSER_CLIENT_ID` | `--oidc-browser-client-id` | App client ID for MSAL.js sign-in button in browser UI | *(none)* |

---

## Authentication

The server supports three authentication tiers. The tier is selected at startup
and cannot be changed at runtime.

| Tier | Mechanism | `--serve` flag(s) | When to use |
|------|-----------|-------------------|-------------|
| 0 — None | No check | *(default)* | Local / trusted-network deployments |
| 1 — API key | Static Bearer token | `--api-key` | Single-tenant, simple deployments |
| 2 — OIDC | JWT validation | `--oidc-issuer` + `--oidc-audience` | Multi-user deployments (Azure, Google, Keycloak, …) |

### Permission model

Every request is classified before the auth check:

| Permission | Operations |
|------------|-----------|
| `Read` | `GET /sparql`, `GET /{name}/sparql`, `GET /{name}/data`, GSP GET, admin reads |
| `Write` | `POST /{name}/update`, `POST /{name}/rml`, `PUT`/`POST`/`DELETE` on data and GSP endpoints |
| `Admin` | `POST /$/datasets` (create), `DELETE /$/datasets/{name}` (drop) |

`Write` implies `Read`. `Admin` implies both.

### Tier 1 — Static API key

Protects write (and optionally read) endpoints with a shared Bearer token:

```sh
dagalog --serve --data data.ttl --api-key "my-secret-key"
```

Reads are open by default. To require the key everywhere:

```sh
dagalog --serve --data data.ttl --api-key "my-secret-key" --require-auth-for-reads
```

Clients send the key in the `Authorization` header:

```sh
curl -H "Authorization: Bearer my-secret-key" \
     "http://localhost:3030/ds/update" \
     --data "INSERT DATA { <urn:s> <urn:p> <urn:o> }" \
     -H "Content-Type: application/sparql-update"
```

### Tier 2 — Azure Entra ID (OIDC)

Dagalog acts as a pure resource server: it validates incoming JWTs locally using
the public keys from Entra ID's JWKS endpoint. No OIDC library or redirect flow is
needed on the dagalog side.

**Step 1 — Register an app in Azure portal**

1. Entra ID → App registrations → New registration. Name: `dagalog`.
2. Under *Expose an API*, set the Application ID URI (e.g. `api://dagalog`).

**Step 2 — Create app roles**

In the app registration → *App roles* → Create:

| Display name | Value | Allowed member types |
|---|---|---|
| Dagalog Read | `dagalog.Read` | Applications + Users |
| Dagalog Write | `dagalog.Write` | Applications + Users |
| Dagalog Admin | `dagalog.Admin` | Applications + Users |

**Step 3 — Assign roles**

In *Enterprise applications → dagalog → Users and groups*, assign users, security
groups, or service principals to the roles above.

For a service principal (app-to-app), use *API permissions → Add permission →
My APIs → dagalog → Application permissions*, then grant admin consent.

**Step 4 — Start dagalog**

```sh
dagalog --serve --data data.ttl \
  --oidc-issuer "https://login.microsoftonline.com/<tenant-id>/v2.0" \
  --oidc-audience "api://dagalog"
```

Or via environment variables (useful in Docker / Azure Container Apps):

```sh
export DAGALOG_OIDC_ISSUER="https://login.microsoftonline.com/<tenant-id>/v2.0"
export DAGALOG_OIDC_AUDIENCE="api://dagalog"
dagalog --serve --data data.ttl
```

**Calling the API**

A client (service principal) acquires a token with the client-credentials flow:

```sh
TOKEN=$(curl -s -X POST \
  "https://login.microsoftonline.com/<tenant-id>/oauth2/v2.0/token" \
  -d "grant_type=client_credentials" \
  -d "client_id=<client-id>" \
  -d "client_secret=<secret>" \
  -d "scope=api://dagalog/.default" \
  | jq -r .access_token)

curl -H "Authorization: Bearer $TOKEN" \
     "http://localhost:3030/sparql?query=SELECT+*+WHERE+%7B%7D"
```

**Browser sign-in (MSAL.js)**

Set `--oidc-browser-client-id` to your app registration's Application (client) ID
to enable a *Sign in* button in the browser UI:

```sh
dagalog --serve \
  --oidc-issuer "https://login.microsoftonline.com/<tenant-id>/v2.0" \
  --oidc-audience "api://dagalog" \
  --oidc-browser-client-id "<application-client-id>"
```

Users then authenticate interactively via the Entra ID popup flow and tokens are
acquired and refreshed automatically by MSAL.js.

### Tier 2 — Google (OIDC)

Google issues standard RS256 JWTs for service accounts and for users via Google
Identity Platform.

**Service-to-service (Google service account)**

```sh
dagalog --serve --data data.ttl \
  --oidc-issuer "https://accounts.google.com" \
  --oidc-audience "https://dagalog.example.com" \
  --oidc-roles-claim "dagalog_roles"
```

Google JWTs do not carry application roles by default. You must either add a custom
claim (`dagalog_roles`) to the token (via Workspace custom attributes or IAP policy),
or use a custom claim mapper on the identity-provider side to flatten roles into a
top-level claim.

A client acquires a token with:

```sh
gcloud auth print-identity-token \
  --audiences="https://dagalog.example.com"
```

Then calls the API:

```sh
curl -H "Authorization: Bearer $(gcloud auth print-identity-token \
       --audiences=https://dagalog.example.com)" \
     "https://dagalog.example.com/sparql?query=SELECT+*+WHERE+%7B%7D"
```

### Custom role names

The three role values are configurable, so you can use names already defined in
your identity provider without renaming them:

```sh
dagalog --serve \
  --oidc-issuer "https://keycloak.example.com/realms/myrealm" \
  --oidc-audience "dagalog" \
  --oidc-roles-claim "realm_access.roles" \
  --oidc-read-role  "my-read-role" \
  --oidc-write-role "my-write-role" \
  --oidc-admin-role "my-admin-role"
```

### Auth config endpoint

`GET /auth/config` is always public and returns the active auth mode, which the
browser UI uses to decide what to show:

```sh
curl http://localhost:3030/auth/config
# {"mode":"oidc","oidc":{"issuer":"https://…","audience":"api://dagalog"}}
```

### Library usage (OIDC)

```rust
use sparql_endpoint::{AuthConfig, Config, OidcConfig, serve};

// Azure Entra ID convenience constructor:
let config = Config {
    auth: AuthConfig::Oidc(OidcConfig::azure(
        "<tenant-id>",
        "api://dagalog",
    )),
    ..Config::default()
};

// Generic OIDC (Google, Keycloak, Auth0, …):
let config = Config {
    auth: AuthConfig::Oidc(OidcConfig {
        issuer:      "https://accounts.google.com".to_owned(),
        jwks_uri:    None,                       // auto-discovered
        audience:    "https://dagalog.example.com".to_owned(),
        roles_claim: "dagalog_roles".to_owned(),
        read_role:   "dagalog.Read".to_owned(),
        write_role:  "dagalog.Write".to_owned(),
        admin_role:  "dagalog.Admin".to_owned(),
        browser_client_id: None,
    }),
    ..Config::default()
};
```

---

## SPARQL HTTP endpoint

### Root endpoints

| Route | Description |
|---|---|
| `GET /` | Browser UI (query + upload) |
| `GET /sparql?query=<encoded>` | SPARQL 1.1 SELECT / ASK / CONSTRUCT |
| `POST /sparql` (`application/sparql-query`) | SPARQL 1.1 query (direct body) |
| `POST /sparql` (`application/x-www-form-urlencoded`) | SPARQL 1.1 query or update (form body) |
| `POST /sparql` (`application/sparql-update`) | SPARQL 1.1 Update (direct body) |
| `GET /sparql` (no `query=`) | SPARQL 1.1 Service Description (Turtle) |
| `GET /.well-known/void` | VoID dataset description |
| `GET /void` | VoID dataset description (alias) |
| `POST /upload` | Load Turtle data into the default graph (legacy alias) |

Response format for SELECT/ASK is negotiated via the `Accept` header:

| `Accept` | Format |
|---|---|
| `application/sparql-results+json` (default) | SPARQL JSON |
| `application/sparql-results+xml` | SPARQL XML |
| `text/csv` | CSV with header row |
| Unrecognised format | `406 Not Acceptable` |

All responses include an `ETag` header based on the dataset's write generation
counter, enabling efficient HTTP caching with conditional `If-None-Match` requests.

### Graph Store Protocol (GSP)

| Route | Description |
|---|---|
| `GET /rdf-graph-store?default` or `?graph=<iri>` | Retrieve a graph |
| `PUT /rdf-graph-store?default` or `?graph=<iri>` | Replace a graph |
| `POST /rdf-graph-store?default` or `?graph=<iri>` | Merge triples into a graph |
| `POST /rdf-graph-store` | Create a new graph (server assigns IRI, returns `Location` header) |
| `DELETE /rdf-graph-store?default` or `?graph=<iri>` | Delete a graph |
| `HEAD /rdf-graph-store?default` or `?graph=<iri>` | Existence check, no body |
| `GET /rdf-graphs/{name}` | Direct graph identification (§4.1) |
| `PUT /rdf-graphs/{name}` | Direct graph identification — replace |

Graph Store responses support content negotiation for output format:

| `Accept` | Format |
|---|---|
| `text/turtle` (default) | Turtle |
| `application/n-triples` | N-Triples |
| `application/n-quads` | N-Quads |
| `application/trig` | TriG |
| `application/ld+json` | JSON-LD |

### Fuseki-compatible per-dataset routes

The server exposes a `default` dataset at `/ds` (and any datasets created via the admin API):

| Route | Description |
|---|---|
| `GET /{name}/sparql` or `/{name}/query` | SPARQL SELECT |
| `POST /{name}/sparql` or `/{name}/query` | SPARQL SELECT (form or direct body) |
| `POST /{name}/update` | SPARQL Update (INSERT/DELETE/CLEAR/DROP/…) |
| `POST /{name}/rml` | Apply an RML mapping (`multipart/form-data`), merge into the dataset |
| `GET|PUT|POST|DELETE|HEAD /{name}/data` | GSP read-write |
| `GET|HEAD /{name}/get` | GSP read-only |

`POST /rml/map` (root-level, not dataset-scoped) applies an RML mapping and
returns the generated RDF directly, touching no dataset — see the
[RML mapping guide](docs/user/rml-mapping.md#applying-mappings-over-http).

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

See [`docs/architecture/PROTOCOLS.md`](docs/architecture/PROTOCOLS.md) for full details.

| Priority | Protocol | Status |
|---|---|---|
| P0 | SPARQL 1.1 Protocol — SELECT/ASK/CONSTRUCT, CORS | Done |
| P0 | Content negotiation — SPARQL JSON (default), SPARQL XML, CSV, 406 | Done |
| P0 | SPARQL 1.1 Service Description | Done |
| P0 | SPARQL 1.1 Update via `POST /sparql` (direct + form body) | Done |
| P1 | SPARQL 1.1 Graph Store HTTP Protocol (indirect + direct) | Done |
| P1 | Graph Store output: Turtle, N-Triples, N-Quads, TriG, JSON-LD | Done |
| P1 | SPARQL 1.1 Update (INSERT/DELETE/CLEAR/DROP/CREATE) | Done |
| P1 | Fuseki-compatible dataset routing and admin API | Done |
| P2 | VoID dataset description (`GET /.well-known/void`, `GET /void`) | Done |
| P2 | HTTP caching headers (ETag via generation counter) | Done |

---

## Implementation plan

See [`docs/architecture/PLAN.md`](docs/architecture/PLAN.md) for the full phased roadmap.
See [`docs/plans/`](docs/plans/) for feature area plans and known-issues tracking.
See [`docs/architecture/`](docs/architecture/) for protocol compliance and architecture references.

Upcoming areas:
- [`docs/plans/PERSISTENCE_PLAN.md`](docs/plans/PERSISTENCE_PLAN.md) — durable transactional storage (`redb`) and incremental Datalog maintenance
- [`docs/plans/SHACL_PLAN.md`](docs/plans/SHACL_PLAN.md) — SHACL Core validation via Datalog translation
- [`docs/plans/AUTH.md`](docs/plans/AUTH.md) — API-key and OIDC authentication (complete; details and Managed Identity docs)

---

## License

GNU General Public License v3.0 — see [`LICENSE`](LICENSE).

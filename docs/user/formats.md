# Supported formats

dagalog can read RDF data from several standard formats. The format is auto-detected
from the file extension.

---

## Input formats

| Extension | Format | Notes |
|---|---|---|
| `.ttl` | Turtle | Most common; compact, human-readable |
| `.trig` | TriG | Turtle with named graph blocks |
| `.owl` | OWL/Turtle | Treated as Turtle |
| `.nt` | N-Triples | One triple per line; no prefix declarations |
| `.nq` | N-Quads | N-Triples with a fourth graph column |
| `.jsonld` | JSON-LD 1.1 | JSON-based RDF; context-driven compaction |

### Turtle (`.ttl`)

The most common format. Uses `@prefix` declarations for compact URIs:

```turtle
@prefix ex: <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

ex:Alice a ex:Person ;
    rdfs:label "Alice" ;
    ex:age 30 .
```

### TriG (`.trig`)

Turtle extended with named graph blocks:

```turtle
@prefix ex: <http://example.org/> .

<http://example.org/graph1> {
    ex:Alice a ex:Person .
    ex:Bob   a ex:Person .
}
```

### JSON-LD 1.1 (`.jsonld`)

JSON-based RDF with an `@context` block that maps JSON keys to RDF predicates:

```json
{
  "@context": {
    "ex": "http://example.org/",
    "rdfs": "http://www.w3.org/2000/01/rdf-schema#",
    "label": "rdfs:label"
  },
  "@id": "ex:Alice",
  "@type": "ex:Person",
  "label": "Alice"
}
```

**Supported JSON-LD 1.1 features:** term mappings, prefixes, `@vocab`, `@base`,
`@graph` (named graphs), `@list` (RDF lists), `@reverse`, `@included`, `@nest`,
keyword aliasing, property-scoped and type-scoped contexts, `@protected`.

For security reasons, external context URL fetching (`@import`) is not possible.

---

## Output formats

### CLI output formats

The CLI `--format` flag controls how query results are printed:

| Format | Flag | Notes |
|---|---|---|
| Table | `--format table` | Default; aligned columns with header |
| CSV | `--format csv` | Plain comma-separated values |
| JSON | `--format json` | SPARQL 1.1 JSON results format |

```sh
dagalog -d data.ttl -Q query.sparql --format csv
dagalog -d data.ttl -Q query.sparql --format json
```

### JSON-LD serialisation (Rust API)

Three forms are available from the Rust API:

```rust
// Compacted — @context present, full IRIs everywhere
let out = jsonld_parser::serialize_jsonld(&ds);

// Expanded — JSON array, no @context, absolute IRIs only
let expanded = jsonld_parser::serialize_jsonld_expanded(&ds);

// Flattened — {"@graph": [all subjects at top level]}
let flat = jsonld_parser::serialize_jsonld_flattened(&ds);
```

All three forms are re-parseable by `jsonld_parser::parse_jsonld` (round-trip tested).

---

## Loading files

### CLI

```sh
# Single file
dagalog --data people.ttl --query "SELECT * WHERE { ?s ?p ?o }"

# Multiple files (all loaded into the same store)
dagalog --data people.ttl --data org.jsonld --query "SELECT * WHERE { ?s ?p ?o }"
```

### Rust API

```rust
use dagalog::load_file;
use dag_rdf::Datastore;
use std::path::Path;

let mut ds = Datastore::new(100_000);
load_file(&mut ds, Path::new("people.ttl")).unwrap();
load_file(&mut ds, Path::new("org.jsonld")).unwrap();
```

`load_file` dispatches on the file extension automatically.

---

---

## Mapping structured data to RDF (RML)

CSV, JSON, JSONL, and XML files are not native RDF formats, but dagalog can map
them to RDF triples using **RML** (RDF Mapping Language). A mapping file declares
how columns, JSON fields, or XML elements become subjects, predicates, and objects.

```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/PersonMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "people.csv" ;
        rml:referenceFormulation rml:CSV       # or rml:JSONPath / rml:XPath for JSON/JSONL / XML
    ] ;
    rml:subjectMap [ rml:template "http://example.com/Person/{id}" ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
```

See the [RML mapping guide](rml-mapping.md) for the full reference.

---

## See also

- [RML mapping guide](rml-mapping.md) — map CSV/JSON/JSONL to RDF
- [SPARQL guide](sparql-guide.md) — querying the loaded data
- [`tests/jsonld_suite.rs`](../../tests/jsonld_suite.rs) (`cargo test --test jsonld_suite`) and
  the W3C conformance suites [`tests/w3c_rdf_conformance.rs`](../../tests/w3c_rdf_conformance.rs) /
  [`tests/w3c_rdf12_conformance.rs`](../../tests/w3c_rdf12_conformance.rs) — one runnable test
  per spec example, the executable proof behind this page

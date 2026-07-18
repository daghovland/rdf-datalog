# Mapping structured data to RDF with RML

RML (RDF Mapping Language) is a W3C standard for mapping structured data —
CSV files, JSON files, JSONL streams, and XML files — to RDF triples. Instead
of writing Rust code or manual Turtle conversion, you declare the mapping
rules in a `.ttl` file and dagalog does the rest.

---

## When to use RML

Use RML when your data already exists as CSV, JSON, or XML and you want to
bring it into dagalog for querying, reasoning, or linking with ontologies.
RML handles column/field extraction, IRI template expansion, literal typing,
language tags, named graphs, and blank nodes declaratively.

---

## Quick example

Given a CSV file `people.csv`:

```csv
id,name,age
1,Alice,30
2,Bob,25
```

And a mapping file `mapping.ttl`:

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
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:age ;
        rml:objectMap [ rml:reference "age" ]
    ] .
```

Apply the mapping with the Rust API:

```rust
use rml::apply_rml_mapping;
use dag_rdf::Datastore;
use std::path::Path;

let mut ds = Datastore::new(100_000);
apply_rml_mapping(
    Path::new("mapping.ttl"),   // the RML mapping
    Path::new("."),             // base directory for resolving rml:source paths
    &mut ds,
).unwrap();
```

After this call, `ds` contains the triples:

```turtle
<http://example.com/Person/1>  a              <http://example.com/Person> ;
                                ex:name        "Alice" ;
                                ex:age         "30" .
<http://example.com/Person/2>  a              <http://example.com/Person> ;
                                ex:name        "Bob" ;
                                ex:age         "25" .
```

Or apply it directly from the CLI with `--mapping`:

```sh
dagalog --mapping mapping.ttl \
        --query "SELECT ?name WHERE { ?p <http://example.com/name> ?name }"
```

`rml:source` paths inside the mapping are resolved relative to the mapping
file's own directory, so `mapping.ttl` and `data.csv` can live anywhere as
long as they're next to each other. Multiple `--mapping` flags may be given;
each is applied in order, and mapped triples can be combined with `--data`,
`--ontology`, and `--rules` in the same run.

---

## JSON sources

Use `rml:referenceFormulation rml:JSONPath` and JSONPath expressions
(`$.field`) as references.

Given `students.json`:

```json
[
  {"id": "10", "name": "Alice"},
  {"id": "11", "name": "Bob"}
]
```

Mapping:

```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.json" ;
        rml:referenceFormulation rml:JSONPath
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{$.id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "$.name" ]
    ] .
```

The template placeholder `{$.id}` and the reference `$.name` are both JSONPath
expressions evaluated against each JSON object in the array.

### Nested JSON with rml:iterator

Use `rml:iterator` to drill into a nested array:

```json
{"data": {"students": [{"id": "10", "name": "Alice"}, {"id": "11", "name": "Bob"}]}}
```

```turtle
rml:logicalSource [
    rml:source "data.json" ;
    rml:referenceFormulation rml:JSONPath ;
    rml:iterator "$.data.students[*]"
] ;
```

Each element selected by the iterator becomes one row. References like `$.name`
are then evaluated against the selected element, not the document root.

### JSONL (newline-delimited JSON)

Files with extension `.jsonl` or `.ndjson` are automatically read line by line.
Each non-blank line is parsed as a JSON object and becomes one row. The same
JSONPath references apply.

```
{"id": "1", "name": "Alice"}
{"id": "2", "name": "Bob"}
```

---

## XML sources

Use `rml:referenceFormulation rml:XPath` and XPath 1.0 expressions as
references. `rml:iterator` selects the repeating nodes (e.g. one element per
row); `rml:reference` expressions are then evaluated relative to each
selected node.

Given `students.xml`:

```xml
<students>
  <student>
    <id>1</id>
    <name>Alice</name>
  </student>
  <student>
    <id>2</id>
    <name>Bob</name>
  </student>
</students>
```

Mapping:

```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.xml" ;
        rml:referenceFormulation rml:XPath ;
        rml:iterator "/students/student"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{id}" ;
        rml:class ex:Student
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
```

The `ql:XPath` namespace (`http://semweb.mmlab.be/ns/ql#XPath`) from older
Dimou-lab tooling is accepted as an alias for `rml:XPath`.

### Nested elements

`rml:iterator` can select nodes at any depth, and `rml:reference` expressions
are evaluated relative to the selected node — so deeply nested wrapper
elements don't need to be repeated in every reference:

```xml
<root>
  <group>
    <student><id>1</id><name>Eve</name></student>
    <student><id>2</id><name>Frank</name></student>
  </group>
</root>
```

```turtle
rml:logicalSource [
    rml:source "data.xml" ;
    rml:referenceFormulation rml:XPath ;
    rml:iterator "/root/group/student"
] ;
```

`rml:reference` and template placeholders accept element names (`name`),
attribute references (`@id`), and relative paths (`address/city`) evaluated
against each iterated node.

---

## Combining RML with Turtle ontologies and reasoning

Load an ontology and a mapping into the same store — they live side by side:

```rust
use dagalog::{apply_ontologies, load_file};

load_file(&mut ds, Path::new("hierarchy.ttl")).unwrap();
apply_rml_mapping(Path::new("mapping.ttl"), Path::new("."), &mut ds).unwrap();
apply_ontologies(&mut ds, &[]).unwrap();
```

If the ontology declares `ex:Student rdfs:subClassOf ex:Person` and the mapping
generates `rdf:type ex:Student` triples, reasoning will infer `rdf:type ex:Person`
for every mapped student automatically.

---

## Subject term types

| Mapping form | Subject type | Example value |
|---|---|---|
| `rml:template` | IRI | `"http://example.com/Person/{id}"` |
| `rml:template` + `rml:termType rml:BlankNode` | Blank node | keyed by template expansion |
| `rml:constant` | IRI (constant) | `<http://example.com/Alice>` |

---

## Object term types

| Mapping form | Object type |
|---|---|
| `rml:reference` (default) | Plain string literal |
| `rml:reference` + `rml:language "en"` | Language-tagged literal |
| `rml:reference` + `rml:datatype xsd:integer` | Typed literal |
| `rml:template` (default) | IRI |
| `rml:constant <iri>` | IRI (constant) |

---

## Named graphs

Use `rml:graphMap` to place triples in a named graph:

```turtle
rml:predicateObjectMap [
    rml:predicate ex:country ;
    rml:objectMap [ rml:reference "country" ] ;
    rml:graphMap [ rml:constant <http://example.com/MyGraph> ]
] .
```

---

## Class shorthand

`rml:class` is a shorthand for generating `rdf:type` triples without a separate
`rml:predicateObjectMap`:

```turtle
rml:subjectMap [
    rml:template "http://example.com/Person/{id}" ;
    rml:class ex:Person
] ;
```

This is equivalent to an explicit `rml:predicate rdf:type ; rml:object ex:Person`
predicate-object map.

---

## Spec conformance

The implementation follows [RML 1.0](https://www.w3.org/TR/rml/),
[RFC 9535 JSONPath](https://www.rfc-editor.org/rfc/rfc9535), and
[XPath 1.0](https://www.w3.org/TR/xpath/). The `ql:JSONPath` and `ql:XPath`
namespaces (`http://semweb.mmlab.be/ns/ql#JSONPath`,
`http://semweb.mmlab.be/ns/ql#XPath`) from older Dimou-lab tooling are
accepted as aliases for `rml:JSONPath`/`rml:XPath`.

**In scope:** CSV, JSON, JSONL, XML, template IRIs, reference literals,
language/datatype annotations, named graphs, blank nodes, `rml:class`,
`rml:iterator`, nested JSONPath/XPath, join conditions (`rml:JoinCondition`).

**Not yet implemented:** SQL/JDBC sources, FunctionMap (FNML).

---

## Applying mappings over HTTP

The SPARQL HTTP endpoint (`sparql_endpoint` crate) exposes two REST routes
that run an RML mapping without needing the Rust API or CLI. Both accept the
mapping document and its source files as `multipart/form-data`: one part
named `mapping` holding the mapping Turtle (required), and one part per
source file, each with a `filename` matching the `rml:source` value used in
the mapping.

```sh
curl -X POST http://localhost:3030/ds/rml \
     -F "mapping=@mapping.ttl;type=text/turtle" \
     -F "people.csv=@people.csv"
```

### `POST /{name}/rml` — map into a dataset

Applies the mapping and merges the generated triples into the named
dataset's store (`/ds` is the default dataset). This is a write operation:
it requires write permission, is rejected with `403 Forbidden` when the
server is in read-only mode, returns `404 Not Found` for an unknown dataset,
and — like other writes — is appended to the changelog when persistence is
enabled.

### `POST /rml/map` — stateless mapping

Applies the mapping and returns the generated RDF directly in the response
body, without touching any dataset, the changelog, or the store. Useful for
previewing a mapping's output or using dagalog as a one-shot RML processor
from another application. The response format is content-negotiated via
`Accept` the same way as the Graph Store Protocol (`text/turtle` by default;
also `application/n-triples`, `application/n-quads`, `application/trig`,
`application/ld+json`).

Both routes accept uploads up to `Config::max_rml_upload_bytes` (default
64 MiB, configurable when embedding the server via the Rust API), well
above the server's default 2 MB body limit, since source files are often
larger than typical SPARQL Update bodies.

---

## See also

- [Reasoning and rules](reasoning.md) — combine mapped data with OWL-RL or Datalog
- [Formats](formats.md) — native RDF formats (Turtle, JSON-LD, etc.)
- [RML 1.0 W3C spec](https://www.w3.org/TR/rml/)

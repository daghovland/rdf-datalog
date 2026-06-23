# Mapping structured data to RDF with RML

RML (RDF Mapping Language) is a W3C standard for mapping structured data —
CSV files, JSON files, and JSONL streams — to RDF triples. Instead of
writing Rust code or manual Turtle conversion, you declare the mapping rules
in a `.ttl` file and dagalog does the rest.

---

## When to use RML

Use RML when your data already exists as CSV or JSON and you want to bring it
into dagalog for querying, reasoning, or linking with ontologies. RML handles
column/field extraction, IRI template expansion, literal typing, language tags,
named graphs, and blank nodes declaratively.

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

The implementation follows [RML 1.0](https://www.w3.org/TR/rml/) and
[RFC 9535 JSONPath](https://www.rfc-editor.org/rfc/rfc9535). The
`ql:JSONPath` namespace (`http://semweb.mmlab.be/ns/ql#JSONPath`) from
older Dimou-lab tooling is accepted as an alias.

**In scope:** CSV, JSON, JSONL, template IRIs, reference literals,
language/datatype annotations, named graphs, blank nodes, `rml:class`,
`rml:iterator`, nested JSONPath.

**Not yet implemented:** SQL/JDBC sources, XML/XPath, join conditions
(`rml:JoinCondition`), FunctionMap (FNML).

---

## See also

- [Reasoning and rules](reasoning.md) — combine mapped data with OWL-RL or Datalog
- [Formats](formats.md) — native RDF formats (Turtle, JSON-LD, etc.)
- [RML 1.0 W3C spec](https://www.w3.org/TR/rml/)

# OTTR Templates

[OTTR (Reasonable Ontology Templates)](https://ottr.xyz/) is a template language for RDF.
Instead of repeating the same triple patterns for every instance of a class, you define a
template once — with typed parameters — and call it for each individual.
This is the stOTTR text format ([OTTR Phase 9 — GitHub #22](https://github.com/daghovland/rdf-datalog/issues/22)).

---

## Why templates?

Without templates, describing 100 people means writing three triples per person by hand.
With OTTR, you write the pattern once:

```stottr
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person, xsd:string ?name, ottr:IRI ?email ] :: {
  ottr:Triple (?person, rdf:type,   foaf:Person),
  ottr:Triple (?person, foaf:name,  ?name),
  ottr:Triple (?person, foaf:mbox,  ?email)
} .
```

And then call it:

```stottr
ex:Person(<http://example.com/alice>, "Alice", <mailto:alice@example.com>) .
ex:Person(<http://example.com/bob>,   "Bob",   <mailto:bob@example.com>) .
```

Each call expands to three triples automatically.

---

## Rust API

The `ottr` crate provides two entry points depending on where your stOTTR content lives.

### Inline / in-memory

Parse a stOTTR string directly and expand all instances into a `Datastore`:

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
ex:Person(<http://example.com/bob>,   "Bob")   .
"#;

let mut ds = Datastore::new(100_000);
let doc = parse_stottr(src).unwrap();
expand_documents(&[doc], &mut ds).unwrap();
// ds now contains 4 triples (2 persons × 2 predicates)
```

### From files

Use `load_stottr_file` to read from disk. Templates and instances can live in separate files —
pass all documents to `expand_documents` and it merges them before expanding:

```rust
use dag_rdf::Datastore;
use ottr::{expand_documents, load_stottr_file};
use std::path::Path;

let mut ds = Datastore::new(100_000);
let template_doc = load_stottr_file(Path::new("person_template.stottr")).unwrap();
let instance_doc = load_stottr_file(Path::new("person_instances.stottr")).unwrap();
expand_documents(&[template_doc, instance_doc], &mut ds).unwrap();
```

---

## stOTTR syntax quick reference

### Template definition

```stottr
prefix:TemplateName [ type ?param1, type ?param2, ... ] :: {
  body_instance1,
  body_instance2,
  ...
} .
```

`type` is optional and is currently used for documentation — the expander does not enforce types.
Common type URIs: `ottr:IRI`, `ottr:Literal`, `xsd:string`, `xsd:integer`.

### Instance call

```stottr
prefix:TemplateName(arg1, arg2, ...) .
```

Arguments can be:
- IRIs: `<http://example.com/Alice>` or prefixed names `ex:Alice`
- String literals: `"Alice"`
- Typed literals: `"42"^^xsd:integer`
- Blank nodes: `_:b1`
- The `none` keyword — drops any triple that references it

### List expanders

OTTR supports generating multiple triples from a single call by passing lists and an expander:

**`cross`** — cartesian product of all list arguments:

```stottr
ex:Types [ ottr:IRI ?thing, ottr:IRI ?type ] :: {
  cross | ottr:Triple (++?thing, rdf:type, ++?type)
} .

ex:Types(
  (<http://example.com/Alice>, <http://example.com/Bob>),
  (<http://example.com/Person>, <http://example.com/Agent>)
) .
```

Produces 4 triples: every combination of {Alice, Bob} × {Person, Agent}.

**`zipMin`** — pairs lists by index, stopping at the shortest:

```stottr
ex:Names [ ottr:IRI ?person, xsd:string ?name ] :: {
  zipMin | ottr:Triple (++?person, foaf:name, ++?name)
} .

ex:Names(
  (<http://example.com/Alice>, <http://example.com/Bob>),
  ("Alice", "Bob", "Charlie")
) .
```

Produces 2 triples (min(2, 3) = 2). Charlie is ignored.

The `++` prefix on a variable name (`++?name`) marks it as a list-expand position.

### The `none` keyword

Passing `none` as an argument suppresses any `ottr:Triple` in the template body that uses
that parameter — the rest of the triples in the same call still expand normally:

```stottr
ex:Person [ ottr:IRI ?person, xsd:string ?name, ottr:IRI ?email ] :: {
  ottr:Triple (?person, rdf:type,  foaf:Person),
  ottr:Triple (?person, foaf:name, ?name),
  ottr:Triple (?person, foaf:mbox, ?email)   -- dropped when ?email is none
} .

ex:Person(<http://example.com/alice>, "Alice", none) .
```

Alice gets `rdf:type foaf:Person` and `foaf:name "Alice"` but no `foaf:mbox` triple.

---

## Jupyter kernel: `%%ottr`

In a [Dagalog Jupyter notebook](jupyter.md), use `%%ottr` to expand stOTTR templates inline.
The expanded triples are added to the session datastore and persist across cells like any other load.

```stottr
%%ottr
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person, xsd:string ?name ] :: {
  ottr:Triple (?person, rdf:type,  foaf:Person),
  ottr:Triple (?person, foaf:name, ?name)
} .

ex:Person(<http://example.com/alice>, "Alice") .
ex:Person(<http://example.com/bob>,   "Bob") .
```

To load from a file on disk:

```
%%ottr path/to/templates.stottr
```

After either form, you can query the expanded triples immediately:

```sparql
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?person ?name WHERE { ?person foaf:name ?name }
```

---

## Combining with OWL-RL reasoning

OTTR templates expand into plain triples and integrate transparently with reasoning.
Load an OWL ontology alongside the expanded data and run `%%reason`:

```
%%load ontology.ttl
%%ottr templates.stottr
%%reason
```

If the ontology says `foaf:Person rdfs:subClassOf ex:Agent`, reasoning infers
`rdf:type ex:Agent` for every person generated by the template.

---

## See also

- [Jupyter kernel guide](jupyter.md) — all `%%` magics
- [RML mapping](rml-mapping.md) — for CSV / JSON / XML sources
- [Reasoning and rules](reasoning.md) — OWL-RL + Datalog
- [OTTR spec](https://spec.ottr.xyz/stOTTR/0.1/) — full stOTTR language reference

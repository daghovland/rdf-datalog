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

```rust,no_run
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

## wOTTR: templates as plain RDF

[wOTTR](https://spec.ottr.xyz/wOTTR/0.4.5/) is the RDF/Turtle serialisation of OTTR: the same
templates and instances as above, but expressed as ordinary triples in the `ottr:` vocabulary
instead of the bespoke stOTTR text grammar. This means a template library can be published,
loaded, and queried like any other Turtle data — no separate parser invocation needed to see
what templates exist ([#246](https://github.com/daghovland/rdf-datalog/issues/246)).

The stOTTR example from the top of this page, in wOTTR:

```turtle
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

ex:Person a ottr:Template ;
    ottr:parameters (
        [ ottr:type ottr:IRI ;    ottr:variable _:person ]
        [ ottr:type xsd:string ;  ottr:variable _:name ]
    ) ;
    ottr:pattern
        [ ottr:of ottr:Triple ; ottr:values ( _:person rdf:type foaf:Person ) ] ,
        [ ottr:of ottr:Triple ; ottr:values ( _:person foaf:name _:name ) ] .

[] ottr:of ex:Person ; ottr:values ( <http://example.com/alice> "Alice" ) .
[] ottr:of ex:Person ; ottr:values ( <http://example.com/bob>   "Bob" ) .
```

A parameter's `ottr:variable` is a blank node; the *same* blank node reused inside a pattern
instance's `ottr:values`/`ottr:arguments` is what marks that argument position as bound to the
parameter — this is the whole trick that lets wOTTR encode variables in plain RDF. `ottr:none`
is the individual used for a missing/suppressed argument value, and list-typed arguments (for
`cross`/`zipMin` expansion, or `ottr:type (rdf:List ...)`/`(ottr:NEList ...)` parameters) are
ordinary RDF lists (`rdf:first`/`rdf:rest`). Both the compact `ottr:values` (a flat list of
argument terms) and canonical `ottr:arguments` (a list of `ottr:Argument` nodes, each with
`ottr:value` and an optional `ottr:modifier ottr:listExpand`) encodings from the spec are
supported.

### Rust API

```rust
use dag_rdf::Datastore;
use ottr::wottr::parse_wottr_str;

let doc = parse_wottr_str(r#"
    @prefix ex: <http://example.com/> .
    @prefix ottr: <http://ns.ottr.xyz/0.4/> .

    ex:IsThing a ottr:Template ;
        ottr:parameters ( [ ottr:type ottr:IRI ; ottr:variable _:x ] ) ;
        ottr:pattern
            [ ottr:of ottr:Triple ;
              ottr:values ( _:x <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ex:Thing ) ] .

    [] ottr:of ex:IsThing ; ottr:values ( ex:Widget ) .
"#).unwrap();

let mut ds = Datastore::new(100);
ottr::expand_documents(&[doc], &mut ds).unwrap();
```

`parse_wottr_str` parses Turtle into a fresh `Datastore` and hands it to
`ottr::wottr::parse_wottr(&datastore)`, which reads templates/instances out of an
already-populated `Datastore` — the same store you may already be loading other RDF data into.
The result is the identical `ast::StottrDocument` that `parser::parse_stottr` builds from
stOTTR text, so it composes with `expand_documents` exactly as above; a document list can even
mix stOTTR- and wOTTR-sourced documents together.

### Format detection in the CLI, kernel, and HTTP endpoint

The `--ottr` CLI flag, the `%%ottr` Jupyter magic, and the `POST /{dataset}/ottr` HTTP endpoint
(all described below) accept either format:

- **By file extension** (`--ottr file.ttl` / `%%ottr file.ttl`): `.ttl`/`.turtle`/`.trig` is
  parsed as wOTTR, anything else (including the conventional `.stottr`) as stOTTR — see
  `ottr::load_ottr_file`.
- **By `Content-Type`** (HTTP endpoint, per multipart part): `text/turtle`,
  `application/x-turtle`, or `application/trig` is parsed as wOTTR, anything else as stOTTR.
- **By trial parse** (inline `%%ottr` cell, which has neither a filename nor a `Content-Type`):
  stOTTR is tried first, falling back to wOTTR Turtle on failure — see `ottr::parse_ottr_str`.
  stOTTR's `[...] :: {...}` template syntax and bare `name(args)` instance calls are not valid
  Turtle, so a genuine wOTTR document reliably fails the stOTTR attempt.

### Scope

Custom `ottr:BaseTemplate`s beyond the built-in `ottr:Triple`, the `ottr:zipMax` expander, and
multi-level composed parameter types (`ottr:LUB`, `ottr:Bot`, chained wrappers) are not yet
supported — unsupported constructs are skipped with a warning rather than erroring. See
[`docs/plans/WOTTR_PLAN.md`](../plans/WOTTR_PLAN.md) for the full vocabulary-to-AST mapping and
current scope.

---

## Jupyter kernel: `%%ottr`

In a [Dagalog Jupyter notebook](jupyter.md), use `%%ottr` to expand OTTR templates inline —
stOTTR text by default; wOTTR Turtle also works (auto-detected, see below).
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

To load from a file on disk (format dispatched by extension — `.ttl`/`.turtle`/`.trig` for
wOTTR, anything else as stOTTR):

```text
%%ottr path/to/templates.stottr
%%ottr path/to/templates.ttl
```

After either form, you can query the expanded triples immediately:

```sparql
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?person ?name WHERE { ?person foaf:name ?name }
```

---

## HTTP endpoint

`POST /{dataset}/ottr` expands OTTR templates/instances directly into a named dataset on
a running server (`dagalog --serve`) — mirroring the existing `/{dataset}/rml` endpoint.
The body is `multipart/form-data` with one or more parts; each part is an independent (or
partial) OTTR document — stOTTR text by default, or wOTTR if the part's `Content-Type` is
`text/turtle`/`application/x-turtle`/`application/trig` — and all parts are merged (templates
pooled, instances concatenated) before expansion — so a template library and its instance
data can be sent as separate parts in one request, or combined in a single part, mixing
formats freely.

```sh
# One self-contained stOTTR document
curl -F "document=@templates_and_instances.stottr" \
     http://localhost:3030/mydataset/ottr

# Template library and instance data as separate parts
curl -F "templates=@person_template.stottr" \
     -F "instances=@person_instances.stottr" \
     http://localhost:3030/mydataset/ottr

# A wOTTR (Turtle) document — Content-Type selects the wOTTR parser
curl -F "document=@templates.ttl;type=text/turtle" \
     http://localhost:3030/mydataset/ottr
```

Part *names* (`document`, `templates`, `instances` above) carry no meaning — every part is
parsed independently (by format, per its `Content-Type`) and all resulting documents are
pooled before expansion.

On success, responds `200 OK` with the number of triples inserted. The endpoint requires
write permission (same guard as `/rml` and `/update`) and returns `400` for malformed
stOTTR/wOTTR syntax or an undefined template reference, `404` if the dataset doesn't exist. See
[`docs/plans/OTTR_HTTP_ENDPOINT_PLAN.md`](../plans/OTTR_HTTP_ENDPOINT_PLAN.md) for the full
design, including why a single-call multipart shape was chosen over a stateful
upload-then-trigger flow.

---

## CLI

The `dagalog` binary supports a repeatable `--ottr <FILE>` flag ([#247](https://github.com/daghovland/rdf-datalog/issues/247)).
Each file is parsed independently — dispatched by extension between stOTTR text and wOTTR
Turtle (`.ttl`/`.turtle`/`.trig`) — and all resulting documents are pooled before expansion,
so a template library and its instance data can be split across files (in either format,
mixed), or combined in one:

```sh
# Templates and instances in separate files
dagalog --ottr person_template.stottr --ottr person_instances.stottr \
        --query "SELECT ?person ?name WHERE { ?person <http://xmlns.com/foaf/0.1/name> ?name }"

# Or a single self-contained file
dagalog --ottr combined.stottr --query "SELECT ?s ?p ?o WHERE { ?s ?p ?o }"

# A wOTTR (Turtle) template file, dispatched by its .ttl extension
dagalog --ottr templates.ttl --query "SELECT ?s ?p ?o WHERE { ?s ?p ?o }"
```

`--ottr` runs after `--data`/`--mapping` and before `--ontology`/`--rules`, so
template-expanded triples participate in OWL-RL reasoning and Datalog rule evaluation.
See the [CLI usage section of the README](../../README.md#cli-usage) for how it composes
with the other flags.

---

## Combining with OWL-RL reasoning

OTTR templates expand into plain triples and integrate transparently with reasoning.
Load an OWL ontology alongside the expanded data and run `%%reason`:

```text
%%load ontology.ttl
%%ottr templates.stottr
%%reason
```

If the ontology says `foaf:Person rdfs:subClassOf ex:Agent`, reasoning infers
`rdf:type ex:Agent` for every person generated by the template.

---

## See also

- [Jupyter kernel guide](jupyter.md) — all `%%` magics
- [Deployment](deployment.md) — running `dagalog --serve` and the HTTP API generally
- [RML mapping](rml-mapping.md) — for CSV / JSON / XML sources
- [Reasoning and rules](reasoning.md) — OWL-RL + Datalog
- [OTTR spec](https://spec.ottr.xyz/stOTTR/0.1/) — full stOTTR language reference

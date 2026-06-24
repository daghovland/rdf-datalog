# Data Pipeline Backlog

Planned extensions to dagalog's data pipeline capabilities, to be designed and
implemented after RML Core (CSV) is complete. See `RML_PLAN.md` for the active
first phase.

---

## 1. JSON Source for RML

> **Status: COMPLETE.** Detailed plan: `RML_JSON_PLAN.md`. XML/XPath sources
> were also added afterward (not originally scoped here) — see
> `RML_XML_PLAN.md` — and the mapping engine is now also exposed over HTTP
> (`POST /{name}/rml`, `POST /rml/map`) — see `RML_REST_ENDPOINT_PLAN.md`.
> Remaining RML gaps: `rml:JoinCondition` (cross-source joins — plan at
> `RML_JOIN_PLAN.md`, now in **red phase**: AST/loader/plan scaffolding plus
> 12 ignored stub tests exist, execution not yet implemented); SQL/JDBC
> sources (plan at `RML_SQL_PLAN.md`, phase-1 design only, no tests yet);
> FunctionMap (FNML, not yet planned).

### Goal

Extend the `rml` crate's source layer to support JSON and JSONL (newline-
delimited JSON) as `LogicalSource` inputs, using JSONPath as the reference
formulation.

### Spec reference

- RML 1.0 §LogicalSource — <https://www.w3.org/TR/rml/#logical-source>
- JSONPath (RFC 9535) — <https://www.rfc-editor.org/rfc/rfc9535>
- `rml:referenceFormulation ql:JSONPath` — Dimou-lab extension, widely used

### What changes

**`sources/json.rs`** — new `JsonSource`:
- `LogicalSourceRef::File(PathBuf)` with `iterator: Option<String>` holding a
  JSONPath expression that selects the iterable array from the document
- Each "row" is a JSON object (or scalar, for flat paths)
- `reference` values are JSONPath expressions evaluated against the current row

**`ast.rs`** additions:
```rust
pub enum ReferenceFormulation {
    Csv,
    JsonPath,   // new
}

pub enum LogicalSourceRef {
    File(PathBuf),
    // …
}
```

**Template expansion**: JSONPath `$.name` against a row JSON object returns a
string value. Nested paths (`$.address.city`) supported. Arrays return the
first element or skip the triple if empty.

### Dependencies

Add `jsonpath-rust` or `serde_json_path` crate (TBD — evaluate API surface
at implementation time).

### Test plan

W3C RML test cases JSON subset:
- `RMLTC0001b`, `RMLTC0002b`, `RMLTC0007c`, etc.
- JSONL (one JSON object per line) as a common practical format

---

## 2. Jupyter Kernel

> **Status: MOSTLY COMPLETE.** Detailed plan: `JUPYTER_KERNEL_PLAN.md`. The
> `dagalog-kernel` crate exists with phases 1–5 done, including the `%%rml`
> magic. Only phase 6 (SPARQL keyword completion) remains.

### Goal

Make dagalog available as a Jupyter kernel so data engineers can write
interactive pipeline notebooks — load data, write SPARQL/Datalog, inspect
results inline — using the standard Jupyter UI (JupyterLab, VS Code, etc.).
This is the "pipelines as code" interface: each notebook cell is a pipeline
step.

### Why Jupyter

Jupyter's cell model maps directly to pipeline stages:
```
[Cell 1: SPARQL UPDATE]  load triples
[Cell 2: %%rml]          apply CSV mapping
[Cell 3: %%reason]       run OWL-RL
[Cell 4: SPARQL SELECT]  inspect results  → rendered as HTML table
[Cell 5: %%shacl]        validate shapes
```

Each cell is executed in order; state (the `Datastore`) persists across cells
within a session.

### Spec reference

- Jupyter messaging protocol v5.3 — <https://jupyter-client.readthedocs.io/en/stable/messaging.html>
- Kernel spec — <https://jupyter-client.readthedocs.io/en/stable/kernels.html>
- Connection file format — <https://jupyter-client.readthedocs.io/en/stable/connection_files.html>

### Crate: `dagalog-kernel` (binary)

New workspace member. A standalone binary that speaks the Jupyter wire protocol.
Installed via `dagalog kernel install` which writes a `kernel.json` to
`~/.local/share/jupyter/kernels/dagalog/`.

```
dagalog-kernel/
├── Cargo.toml
└── src/
    ├── main.rs          — startup: read connection file, bind sockets
    ├── session.rs       — Datastore per kernel session + execution dispatch
    ├── protocol.rs      — Jupyter message types (serialize/deserialize)
    ├── sockets.rs       — ZMQ socket setup (shell, iopub, control, heartbeat)
    ├── cell/
    │   ├── mod.rs       — detect cell type from magic prefix
    │   ├── sparql.rs    — SPARQL SELECT/CONSTRUCT/UPDATE execution
    │   ├── rml.rs       — apply RML mapping (inline or file path)
    │   ├── datalog.rs   — parse + assert datalog rules
    │   └── turtle.rs    — load inline Turtle into the session datastore
    └── output/
        ├── mod.rs
        ├── table.rs     — SELECT results → HTML <table>
        └── turtle.rs    — CONSTRUCT results → Turtle code block
```

### Cell magic syntax

Cells without a magic prefix are treated as SPARQL (the default language,
matching convention from other SPARQL kernels):

```sparql
SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10
```

Cells with a `%%` magic line use that mode:

```
%%rml path/to/mapping.ttl
```
```
%%load path/to/data.ttl
```
```
%%reason
```
```
%%validate path/to/shapes.ttl
```
```
%%datalog
?x rdf:type owl:Thing :- ?x rdf:type ex:Person .
```
```
%%turtle
<http://example.com/Alice> a <http://example.com/Person> .
```

### ZMQ / messaging

Jupyter uses five ZMQ sockets:

| Socket | Pattern | Purpose |
|---|---|---|
| `shell` | ROUTER/DEALER | execute_request, complete_request, kernel_info |
| `iopub` | PUB | stream output, display_data, status |
| `stdin` | ROUTER/DEALER | input_request (unused by dagalog) |
| `control` | ROUTER/DEALER | interrupt_request, shutdown_request |
| `heartbeat` | REP | liveness ping |

All messages are multipart ZMQ frames: `[id..., <IDS|MSG>, hmac, header,
parent_header, metadata, content]`.

Messages to implement (minimum viable kernel):
- `kernel_info_request` → `kernel_info_reply` (language name, version)
- `execute_request` → `execute_reply` + `display_data` / `stream` / `error`
- `is_complete_request` → `is_complete_reply` (for auto-indent)
- `shutdown_request` → graceful exit
- heartbeat: echo REQ as REP

Nice-to-have (can be added incrementally):
- `complete_request` → `complete_reply` (SPARQL keyword + prefix completion)
- `inspect_request` → `inspect_reply` (hover docs for SPARQL functions)

### Output formats

| Cell type | MIME type | Format |
|---|---|---|
| SPARQL SELECT | `text/html` | `<table>` with header row + result rows |
| SPARQL SELECT | `text/plain` | TSV fallback |
| SPARQL CONSTRUCT | `text/plain` | Turtle |
| SPARQL ASK | `text/plain` | `true` / `false` |
| %%load / %%rml / %%reason / %%validate | `text/plain` | status line: `Loaded 1 243 triples.` |
| Error | `application/vnd.jupyter.stderr` | message + traceback |

### Dependencies

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
async-zmq = "0.4"       # or zeromq crate — evaluate at impl time
serde = { version = "1", features = ["derive"] }
serde_json = "1"
hmac = "0.12"
sha2 = "0.10"
uuid = { version = "1", features = ["v4"] }
# plus all dagalog crates
```

### Phasing

1. **Protocol skeleton**: connect sockets, handshake, reply to heartbeat and kernel_info
2. **SPARQL execute**: run SELECT, return HTML table; run UPDATE, return status
3. **Session state**: persistent Datastore across cells; %%load and %%turtle magics
4. **RML magic**: %%rml; depends on `rml` crate being ready
5. **%%reason, %%validate**: wire OWL-RL and SHACL
6. **Completion**: keyword completion for SPARQL

### Installation

```
dagalog kernel install [--user] [--sys-prefix]
```

Writes `~/.local/share/jupyter/kernels/dagalog/kernel.json`:

```json
{
  "argv": ["dagalog", "kernel", "launch", "--connection-file", "{connection_file}"],
  "display_name": "Dagalog (SPARQL + RDF)",
  "language": "sparql"
}
```

---

## 3. OTTR Template Expansion

### Goal

Add an `ottr` crate implementing OTTR (Reasonable Ontology Templates) template
definition and expansion. OTTR is complementary to RML: where RML maps raw
data rows to flat RDF, OTTR templates define typed, reusable patterns for
generating well-structured RDF instances.

Pipeline position: data comes in via RML, is optionally reshaped by OTTR
templates, then reasoning and SHACL validation run on the result.

### Spec reference

- OTTR specification — <https://spec.ottr.xyz/>
- Stottr syntax — <https://spec.ottr.xyz/stOTTR/>
- OTTR test suite (lutra) — <https://gitlab.com/ottr/lutra/lutra-test-suite>
- OTTR vocabularies: `ottr:` = `http://ns.ottr.xyz/0.4/`

### What OTTR adds

Templates are named patterns that expand to sets of triples. A template
definition:

```
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .

ex:Person[ottr:IRI ?uri, xsd:string ?name, ottr:IRI ?org] :- {
  ?uri rdf:type ex:Person ;
       ex:name ?name ;
       ex:worksFor ?org .
} .
```

A template instance file (`.ottr` or `.stottr` instances section):

```
ex:Person(<http://example.com/Alice>, "Alice", <http://example.com/Acme>) .
ex:Person(<http://example.com/Bob>,   "Bob",   <http://example.com/Acme>) .
```

Expansion produces the expected triples for each instance call.

### Crate: `ottr`

```
ottr/
├── Cargo.toml
└── src/
    ├── lib.rs         — pub API: expand_instances(templates, instances, datastore)
    ├── ast.rs         — Template, Parameter, TemplateBody, Instance types
    ├── parser.rs      — Stottr syntax parser (nom-based, like sparql_parser)
    ├── expander.rs    — substitute arguments into template body → emit quads
    └── types.rs       — OTTR type system (IRI, Literal, list types, None)
```

### Stottr grammar (key productions)

```
template_def := prefix_decl* template_signature ":-" template_body "."
template_signature := IRI "[" parameter_list "]"
parameter_list := parameter ("," parameter)*
parameter := type? "?"? variable

template_body := "{" pattern_list "}"
pattern_list := pattern ("," pattern)*
pattern := triple_pattern | template_instance | list_expander

instance := IRI "(" argument_list ")" "."
argument_list := argument ("," argument)*
argument := term | "none"
```

Types: `ottr:IRI`, `ottr:Literal`, `xsd:string`, `xsd:integer`, etc. Type
checking is permissive at this phase (warn, don't error).

### Expansion algorithm

For each instance call `T(a1, a2, …)`:
1. Look up template `T` by IRI
2. Bind parameters: `?p1 = a1`, `?p2 = a2`, …
3. For each triple in the template body:
   - Substitute bound values for variables
   - Emit quad to Datastore (default graph unless graph clause present)
4. For nested template calls in the body, recurse

`none` arguments: if a parameter receives `none` and is marked optional (`?`),
all triples in the body that reference that parameter are silently omitted.

### List expanders

OTTR supports `cross` and `zipMin` expanders over list arguments:

```
cross | ex:T(?x, ++?list)
```

This expands to one instance call per element of `?list` crossed with `?x`.
Defer list expanders to a later sub-phase; core template expansion without
lists is sufficient to demonstrate value.

### Integration

```
dagalog --load base.ttl --ottr templates.stottr --instances data.stottr --reason
```

In a Jupyter notebook cell:
```
%%ottr path/to/templates.stottr
ex:Person(<http://example.com/Alice>, "Alice", <http://example.com/Acme>) .
```

### Phasing

1. AST types + Stottr template definition parser
2. Instance file parser
3. Basic expansion (no lists, no nested templates)
4. Nested template calls
5. Optional parameters (`none`)
6. List expanders (`cross`, `zipMin`)
7. Integration with CLI and Jupyter magic

### Test plan

Use the lutra test suite (CSV-based, each test has a `.stottr` template file,
an instances file, and expected N-Triples output). Copy fixtures into
`ottr/tests/fixtures/`.

---

## Dependency ordering

```
CSV ingestion (rml crate, CSV source)                          — done
    ↓
JSON ingestion (rml crate, JSON source extension)               — done
    ↓
XML ingestion (rml crate, XPath source extension)                — done (not originally scoped)
    ↓
REST endpoints (sparql_endpoint crate, POST /{name}/rml, /rml/map) — done (not originally scoped)
    ↓
OTTR templates (ottr crate — independent of rml, but most useful after mapping) — not started
    ↓
Jupyter kernel (dagalog-kernel crate — depends on all above for full magic coverage) — mostly done; %%ottr magic still pending the ottr crate
```

OTTR and JSON source can be developed in parallel after the `rml` CSV core is
done, since they are independent crates with no mutual dependency.

# Data Pipeline Backlog

Planned extensions to dagalog's data pipeline capabilities, to be designed and
implemented after RML Core (CSV) is complete. See `RML_PLAN.md` for the active
first phase.

---

## 1. JSON Source for RML

> Detailed plan: `RML_JSON_PLAN.md`. XML/XPath sources were also added
> afterward (not originally scoped here) — see `RML_XML_PLAN.md` — and the
> mapping engine is now also exposed over HTTP (`POST /{name}/rml`,
> `POST /rml/map`) — see `RML_REST_ENDPOINT_PLAN.md`. `rml:JoinCondition`
> (cross-source joins): see `RML_JOIN_PLAN.md`. Tracked in
> [epic #25](https://github.com/daghovland/rdf-datalog/issues/25):
> [SQL/JDBC sources](https://github.com/daghovland/rdf-datalog/issues/26),
> [FunctionMap (FNML)](https://github.com/daghovland/rdf-datalog/issues/27).

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

> Detailed plan: `JUPYTER_KERNEL_PLAN.md`. Tracked in
> [epic #28](https://github.com/daghovland/rdf-datalog/issues/28).

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
- `complete_request` → `complete_reply` (SPARQL keyword + prefix completion) — [issue #23](https://github.com/daghovland/rdf-datalog/issues/23)
- `inspect_request` → `inspect_reply` (hover docs for SPARQL functions) — [issue #24](https://github.com/daghovland/rdf-datalog/issues/24)

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

> **Status: PLANNED.** Detailed plan: `OTTR_PLAN.md`. That plan replaces the
> Turtle-like template body syntax originally sketched here with the real
> stOTTR grammar (`::` signature/body separator, `ottr:Triple` base template
> instances) so the `lutra` test suite fixtures can be used directly for TDD.
> Tracked as GitHub issues in the "Dagalog Ottr" project (9 phases, AST
> through CLI/Jupyter integration).

### Goal

Add an `ottr` crate implementing OTTR (Reasonable Ontology Templates) template
definition and expansion. OTTR is complementary to RML: where RML maps raw
data rows to flat RDF, OTTR templates define typed, reusable patterns for
generating well-structured RDF instances.

Pipeline position: data comes in via RML, is optionally reshaped by OTTR
templates, then reasoning and SHACL validation run on the result. See
`OTTR_PLAN.md` for the full crate layout, grammar, expansion algorithm, and
9-phase TDD test plan.

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
OTTR templates (ottr crate — independent of rml, but most useful after mapping) — planned, see OTTR_PLAN.md
    ↓
Jupyter kernel (dagalog-kernel crate — depends on all above for full magic coverage) — mostly done; %%ottr magic still pending the ottr crate, see https://github.com/daghovland/rdf-datalog/issues/22
```

OTTR and JSON source can be developed in parallel after the `rml` CSV core is
done, since they are independent crates with no mutual dependency.

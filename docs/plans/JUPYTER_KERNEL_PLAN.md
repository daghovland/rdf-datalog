# Jupyter Kernel Plan: `dagalog-kernel`

> **Status: MOSTLY COMPLETE** — phases 1–5 implemented and green (protocol
> skeleton, SPARQL execute, session state, `%%rml`/`%%load`/`%%turtle`
> magics, `%%reason`/`%%validate`). `%%validate` wires to the `shacl` crate
> (see `VALIDATE_MAGIC_PLAN.md`). Phase 6 remains:
> [complete_request](https://github.com/daghovland/rdf-datalog/issues/23),
> [inspect_request](https://github.com/daghovland/rdf-datalog/issues/24).
> `%%ottr` magic also pending:
> [issue #22](https://github.com/daghovland/rdf-datalog/issues/22).

## Goal

Make dagalog available as a Jupyter kernel so data engineers can write
interactive pipeline notebooks — load data, write SPARQL/Datalog, inspect
results inline — using the standard Jupyter UI (JupyterLab, VS Code, etc.).

Each notebook cell is a pipeline step; the `Datastore` persists across cells
within a kernel session.

---

## Spec references

- Jupyter messaging protocol v5.3 — <https://jupyter-client.readthedocs.io/en/stable/messaging.html>
- Kernel spec — <https://jupyter-client.readthedocs.io/en/stable/kernels.html>
- Connection file format — <https://jupyter-client.readthedocs.io/en/stable/connection_files.html>
- ZeroMQ — <https://zeromq.org/>

---

## Crate: `dagalog-kernel`

New workspace member at `dagalog-kernel/`. A standalone binary that speaks the
Jupyter wire protocol over ZMQ. Installed via `dagalog kernel install`.

```
dagalog-kernel/
├── Cargo.toml
└── src/
    ├── main.rs          — startup: read connection file, bind sockets, start loop
    ├── session.rs       — Datastore per kernel session + execution dispatch
    ├── protocol.rs      — Jupyter message types (serialize/deserialize)
    ├── sockets.rs       — ZMQ socket setup (shell, iopub, control, heartbeat)
    ├── cell/
    │   ├── mod.rs       — detect cell type from magic prefix → CellType
    │   ├── sparql.rs    — SPARQL SELECT/CONSTRUCT/UPDATE execution
    │   ├── rml.rs       — apply RML mapping (file path)
    │   ├── datalog.rs   — parse + assert Datalog rules
    │   └── turtle.rs    — load inline Turtle into session datastore
    └── output/
        ├── mod.rs
        ├── table.rs     — SELECT results → HTML <table>
        └── turtle_fmt.rs — CONSTRUCT results → Turtle code block
```

---

## ZMQ crate choice: `zeromq` (pure Rust)

Use `zeromq = "0.6"` — pure Rust implementation, no system `libzmq` dependency.
This avoids a C dev dependency for every developer and in CI.

`async-zmq` and `zmq-rs` both bind to `libzmq` (C library) and require
`apt install libzmq3-dev` / `brew install zeromq`, which is contrary to the
goal of a smooth developer setup.

---

## Cell magic syntax

Cells without a `%%` magic prefix are treated as SPARQL (matching convention
from SPARQL kernels such as SPARQLkernel and YASGUI-based kernels):

```sparql
SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10
```

Cells with a `%%` magic line:

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
[?x, rdf:type, owl:Thing] :- [?x, rdf:type, ex:Person] .
```
```
%%turtle
<http://example.com/Alice> a <http://example.com/Person> .
```

---

## ZMQ socket layout

Five sockets per Jupyter specification:

| Socket | Pattern | Purpose |
|---|---|---|
| `shell` | ROUTER | execute_request, complete_request, kernel_info |
| `iopub` | PUB | stream output, display_data, status |
| `stdin` | ROUTER | input_request (unused by dagalog) |
| `control` | ROUTER | interrupt_request, shutdown_request |
| `heartbeat` | REP | liveness ping |

Message multipart frame: `[ids..., "<IDS|MSG>", hmac, header, parent_header, metadata, content]`.

---

## Message types to implement (minimum viable kernel)

- `kernel_info_request` → `kernel_info_reply`
- `execute_request` → `execute_reply` + `display_data` / `stream` / `error`
- `is_complete_request` → `is_complete_reply`
- `shutdown_request` → graceful exit
- heartbeat: echo REQ as REP

---

## Output formats

| Cell type | MIME type | Format |
|---|---|---|
| SPARQL SELECT | `text/html` | `<table>` with header + result rows |
| SPARQL SELECT | `text/plain` | TSV fallback |
| SPARQL CONSTRUCT | `text/plain` | Turtle |
| SPARQL ASK | `text/plain` | `true` / `false` |
| `%%load` / `%%rml` / `%%reason` / `%%validate` | `text/plain` | `Loaded N triples.` |
| Error | `text/plain` | message string |

---

## Unit-testable components (red-phase tests)

The following are testable without a live ZMQ connection:

1. **`protocol.rs`** — `JupyterMessage` serialize/deserialize round-trip
2. **`protocol.rs`** — HMAC-SHA256 signature over `[header, parent_header, metadata, content]`
3. **`cell/mod.rs`** — `detect_cell_type()`: magic-prefix → `CellType` enum
4. **`output/table.rs`** — SELECT result rows → HTML `<table>` string

ZMQ plumbing, socket wiring, and end-to-end execute are validated by running
a live Jupyter client (see developer setup below).

---

## Phasing

1. **Protocol skeleton** (this plan) — connect sockets, reply to heartbeat + `kernel_info`
2. **SPARQL execute** — SELECT returns HTML table; UPDATE returns status
3. **Session state** — persistent Datastore, `%%load` and `%%turtle` magics
4. **RML magic** — `%%rml` (depends on `rml` crate)
5. **`%%reason`, `%%validate`** — wire OWL-RL and SHACL
6. **Completion** — keyword completion for SPARQL ([issue #23](https://github.com/daghovland/rdf-datalog/issues/23)), hover docs ([issue #24](https://github.com/daghovland/rdf-datalog/issues/24))

---

## Developer setup

### Prerequisites

```bash
# Python + Jupyter
pip install jupyterlab
# or: conda install -c conda-forge jupyterlab
```

No system ZMQ library needed (pure-Rust `zeromq` crate).

### Build and install the kernel

```bash
# Build
cargo build -p dagalog-kernel

# Install kernel spec into ~/.local/share/jupyter/kernels/dagalog/
./target/debug/dagalog-kernel install

# Verify Jupyter sees it
jupyter kernelspec list
# Should show: dagalog   ~/.local/share/jupyter/kernels/dagalog
```

### Launch JupyterLab and open the example notebook

```bash
jupyter lab --ServerApp.root_dir=. notebooks/dagalog_intro.ipynb
```

Select "Dagalog (SPARQL + RDF)" as the kernel. Run cells in order.

> **Why `--ServerApp.root_dir=.`**: when given a notebook path, Jupyter Server
> defaults `root_dir` (and therefore the kernel's working directory) to the
> *directory containing the notebook* (`notebooks/`), not the directory you
> launched from. Without pinning `root_dir` to the repo root, relative paths
> in `%%rml`/`%%load` cells (e.g. `tests/testdata/...`) won't resolve.

### What the example notebook does

1. Loads a small Turtle dataset inline (`%%turtle`)
2. Runs a SPARQL SELECT — results appear as an HTML table
3. Applies an RML CSV mapping (`%%rml`)
4. Runs OWL-RL reasoning (`%%reason`)
5. Queries the enriched graph

---

## Installation (kernel.json written by `install` subcommand)

`~/.local/share/jupyter/kernels/dagalog/kernel.json`:

```json
{
  "argv": ["<path-to-dagalog-kernel>", "launch", "--connection-file", "{connection_file}"],
  "display_name": "Dagalog (SPARQL + RDF)",
  "language": "sparql"
}
```

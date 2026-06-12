# Dagalog HTTP Server — Implementation Plan

This document covers the four main areas of work needed to turn the current
single-endpoint SPARQL server into a production-ready service.

---

## 1. SPARQL Graph Store HTTP Protocol (GSP)

**W3C spec:** https://www.w3.org/TR/sparql11-http-rdf-update/ (Recommendation 21 March 2013)

The current `/upload` endpoint is a deliberate stopgap: it merges Turtle into
the default graph with no graph selection, no replacement, and no deletion.
GSP replaces it with a full REST interface over named graphs.

### Implementation status

| Operation | Spec section | Test group | Status |
|---|---|---|---|
| `GET ?default` / `GET ?graph=<iri>` | §5.2 [`#http-get`][get] | A (tests A-1 – A-8) | ✓ Done |
| `PUT ?default` / `PUT ?graph=<iri>` | §5.3 [`#http-put`][put] | B (tests B-1 – B-9) | ✓ Done |
| `DELETE ?default` / `DELETE ?graph=<iri>` | §5.4 [`#http-delete`][del] | C (tests C-1 – C-5) | ✓ Done |
| `POST ?default` / `POST ?graph=<iri>` (merge) | §5.5 [`#http-post`][post] | D (tests D-1 – D-6, D-10 – D-12) | ✓ Done |
| `POST /rdf-graph-store` (create graph) | §5.5 [`#http-post`][post] | D-7 – D-9 | ✓ Done |
| `HEAD ?default` / `HEAD ?graph=<iri>` | §5.6 [`#http-head`][head] | E (tests E-1 – E-5) | ✓ Done |
| Direct graph identification (`/rdf-graphs/<name>`) | §4.1 [`#direct-graph-identification`][direct] | F (tests F-1 – F-2) | ✓ Done (GET, PUT) |

[get]: https://www.w3.org/TR/sparql11-http-rdf-update/#http-get
[put]: https://www.w3.org/TR/sparql11-http-rdf-update/#http-put
[del]: https://www.w3.org/TR/sparql11-http-rdf-update/#http-delete
[post]: https://www.w3.org/TR/sparql11-http-rdf-update/#http-post
[head]: https://www.w3.org/TR/sparql11-http-rdf-update/#http-head
[direct]: https://www.w3.org/TR/sparql11-http-rdf-update/#direct-graph-identification

All 46 tests live in
[`sparql_endpoint/tests/graph_store.rs`](sparql_endpoint/tests/graph_store.rs).
All tests pass — no `#[ignore]` attributes remain in this file.

### Endpoint table

```
GET    /rdf-graph-store?default         — serialise the default graph
GET    /rdf-graph-store?graph=<iri>     — serialise a named graph
PUT    /rdf-graph-store?default         — replace the default graph
PUT    /rdf-graph-store?graph=<iri>     — replace (or create) a named graph
POST   /rdf-graph-store?default         — merge RDF into the default graph
POST   /rdf-graph-store?graph=<iri>     — merge RDF into a named graph
POST   /rdf-graph-store                 — create a new graph (server assigns IRI)
DELETE /rdf-graph-store?default         — clear the default graph
DELETE /rdf-graph-store?graph=<iri>     — delete a named graph
HEAD   /rdf-graph-store?default         — headers only (check default graph)
HEAD   /rdf-graph-store?graph=<iri>     — headers only (check named graph)
```

### Usage examples (once implemented)

```sh
# Retrieve the default graph as Turtle
curl -H "Accept: text/turtle" http://localhost:3030/rdf-graph-store?default

# Load a Turtle file into the default graph (replace)
curl -X PUT -H "Content-Type: text/turtle" \
     --data-binary @data.ttl \
     http://localhost:3030/rdf-graph-store?default

# Load a Turtle file into a named graph (replace)
curl -X PUT -H "Content-Type: text/turtle" \
     --data-binary @data.ttl \
     "http://localhost:3030/rdf-graph-store?graph=http%3A//example.org/mygraph"

# Merge additional triples into the default graph
curl -X POST -H "Content-Type: text/turtle" \
     --data-binary @more.ttl \
     http://localhost:3030/rdf-graph-store?default

# Create a new named graph (server picks the IRI, returned in Location header)
curl -X POST -H "Content-Type: text/turtle" \
     --data-binary @data.ttl \
     http://localhost:3030/rdf-graph-store

# Delete a named graph
curl -X DELETE \
     "http://localhost:3030/rdf-graph-store?graph=http%3A//example.org/mygraph"

# Check whether a named graph exists (no body in response)
curl -I "http://localhost:3030/rdf-graph-store?graph=http%3A//example.org/mygraph"
```

### Implementation notes

- New crate file `sparql_endpoint/src/graph_store.rs`.
- All mutating operations require a write lock on `Arc<RwLock<Datastore>>`.
- PUT semantics are `DROP SILENT GRAPH <g>; INSERT DATA { GRAPH <g> { … } }` — requires
  a `Datastore::remove_graph(graph_id)` method in `dag_rdf` (not yet implemented).
- POST to `/rdf-graph-store` (create) must generate a unique graph IRI (UUID or counter)
  and return it in the `Location` response header with `201 Created`.
- Content negotiation for GET: `text/turtle` (primary), `application/n-triples`.
  Return `406 Not Acceptable` for unsupported `Accept` types.
- Validate that `?graph=<iri>` is an absolute IRI (§4.2); return `400 Bad Request` if not.
- The `/upload` stopgap endpoint can remain for backward compat once GSP is live,
  or redirect: `POST /upload → POST /rdf-graph-store?default`.
- Add the GSP route to `server.rs` after the existing `/sparql` routes.

---

## 2. Authentication

The server currently has no access control.  Two realistic tiers:

### Tier 1 — API key (simple, ship first)

- Add an `api_key: Option<String>` field to `Config`.
- Middleware (`tower_http::validate_request::ValidateRequestHeaderLayer` or a
  custom `axum::middleware`) checks `Authorization: Bearer <key>` on all
  mutating requests (POST `/upload`, PUT/POST/DELETE `/rdf-graph-store`).
- Read endpoints remain unauthenticated unless a `require_auth_for_reads: bool`
  flag is set.
- Key is configured via `--api-key <KEY>` CLI flag or `DAGALOG_API_KEY` env var.

### Tier 2 — OAuth2 / OIDC (for multi-user deployments)

- Use `axum-oidc` or `tower-oidc` to validate JWT bearer tokens.
- Claims map to read / write / admin roles.
- `Config` gains `oidc_issuer: Option<Url>` and `oidc_audience: Option<String>`.
- Roles can be scoped per dataset when datasets are implemented (see §4).

### Considerations

- SPARQL 1.1 Protocol defines no auth mechanism — auth is a transport concern.
  Returning `401 Unauthorized` with `WWW-Authenticate: Bearer` is standard.
- The browser UI needs to store and send the token; add an input field in the
  frontend once auth is active.

---

## 3. Datasets and Separate Instances

The current architecture has one `Arc<RwLock<Datastore>>` for the whole server.

### Multiple named datasets (Fuseki-style)

Add a dataset registry:

```rust
pub struct DatasetRegistry {
    datasets: HashMap<String, Arc<RwLock<Datastore>>>,
}
```

URL scheme mirrors Apache Jena Fuseki:

```
GET  /dataset/{name}/sparql          — query
POST /dataset/{name}/sparql          — query / update
GET  /dataset/{name}/rdf-graph-store — GSP
POST /dataset/                       — create new dataset (admin)
DELETE /dataset/{name}               — drop dataset (admin)
```

`AppState` becomes `Arc<DatasetRegistry>` and handlers resolve the dataset
by path parameter before acquiring the per-dataset lock.

A `default` dataset (currently the single store) is pre-created on startup;
`--data` / `--ontology` / `--rules` flags populate it.

### Completely separate instances

For full isolation — separate memory, separate OWL reasoning, separate ports —
run multiple `dagalog --serve` processes with different `--port` values.
Docker Compose makes this straightforward (see §4).

### Persistence

The current in-memory `Datastore` is lost on restart. Long-term options:
- Snapshot serialisation (Turtle dump on shutdown, reload on startup). Low
  complexity, fine for modest sizes.
- Memory-mapped storage (sled, redb, or custom). Required for large datasets.
  Defer until benchmarks indicate a need.

---

## 4. Docker

### Dockerfile (multi-stage)

```dockerfile
# ── builder ──────────────────────────────────────────────────────────────────
FROM rust:1.87-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p dagalog

# ── runtime ──────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/dagalog /usr/local/bin/dagalog
EXPOSE 3030
ENTRYPOINT ["dagalog"]
CMD ["--serve"]
```

### docker-compose.yml (single instance with persistent data)

```yaml
services:
  dagalog:
    build: .
    ports:
      - "3030:3030"
    volumes:
      - ./data:/data
    command: ["--serve", "--port", "3030", "--data", "/data/dataset.ttl"]
    environment:
      - DAGALOG_API_KEY=${DAGALOG_API_KEY:-}
```

### Multiple instances

```yaml
services:
  dagalog-a:
    build: .
    ports: ["3031:3030"]
    volumes: ["./data/a:/data"]
    command: ["--serve", "--data", "/data/dataset.ttl"]

  dagalog-b:
    build: .
    ports: ["3032:3030"]
    volumes: ["./data/b:/data"]
    command: ["--serve", "--data", "/data/dataset.ttl"]
```

### Configuration via environment variables

Add an `env_config()` constructor to `Config` that reads:

| Env var                   | Config field              | Default       |
|---------------------------|---------------------------|---------------|
| `DAGALOG_PORT`            | `bind_addr` port          | `3030`        |
| `DAGALOG_BASE_IRI`        | `base_iri`                | auto          |
| `DAGALOG_READ_ONLY`       | `read_only`               | `false`       |
| `DAGALOG_API_KEY`         | `api_key`                 | none          |
| `DAGALOG_QUERY_TIMEOUT`   | `max_query_timeout_secs`  | `30`          |

CLI flags take precedence over env vars.

---

## 5. Publishing a Pre-built Docker Image

### Why it matters for usability

Without a published image, anyone who wants to try dagalog via Docker must
first clone the repository and wait for a full Rust compile (several minutes
on a fresh machine).  A pre-built image removes that friction entirely:

```sh
docker run -p 3030:3030 ghcr.io/daghovland/rdf-datalog --serve
```

This is the difference between "requires a Rust toolchain" and "runs anywhere
Docker is installed" — relevant for data engineers, ontology authors, and CI
pipelines that just want a SPARQL endpoint without a build step.

### Recommended registry: GitHub Container Registry (ghcr.io)

Since the source is already at `github.com/daghovland/rdf-datalog`, the
natural home is `ghcr.io/daghovland/rdf-datalog`.  GitHub Actions can publish
it automatically on every push to `main` or on version tags.

**Sample workflow** (`.github/workflows/docker.yml`):

```yaml
name: Docker

on:
  push:
    branches: [main]
    tags: ["v*"]

jobs:
  build-and-push:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@v4
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
      - uses: docker/metadata-action@v5
        id: meta
        with:
          images: ghcr.io/daghovland/rdf-datalog
          tags: |
            type=ref,event=branch
            type=semver,pattern={{version}}
            type=semver,pattern={{major}}.{{minor}}
      - uses: docker/build-push-action@v6
        with:
          context: .
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
```

This produces:
- `ghcr.io/daghovland/rdf-datalog:main` — latest from the main branch
- `ghcr.io/daghovland/rdf-datalog:1.2.3` — exact version tag
- `ghcr.io/daghovland/rdf-datalog:1.2` — minor-version alias

### Docker Hub as an alternative

Docker Hub (`daghovland/rdf-datalog`) has broader default discoverability
(it is the default registry for `docker pull`), but requires separate
credentials. Reasonable to add later if adoption warrants it; ghcr.io is
lower friction to set up and sufficient for most users who find the project
via GitHub.

### Build time

A cold Rust build compiles all workspace crates and takes ~5–10 minutes on
GitHub-hosted runners.  Strategies to reduce it:

- **Layer caching** — use `docker/build-push-action` with `cache-from:
  type=gha` and `cache-to: type=gha,mode=max`. GitHub Actions caches the
  Rust layer between runs; subsequent builds take 1–2 minutes for small
  changes.
- **`cargo-chef`** — pre-compile dependencies in a separate layer (the
  `lukemathwalker/cargo-chef` image). Dependencies change rarely; only the
  application layer rebuilds on code changes.

### Multi-platform images

GitHub Actions runners support `linux/amd64` and `linux/arm64` via QEMU.
Add `platforms: linux/amd64,linux/arm64` to the `build-push-action` step to
produce images that run natively on both x86-64 servers and Apple Silicon.

---

## Suggested implementation order

1. **GSP** (`graph_store.rs`) — most immediately useful; replaces `/upload`.
2. **Env-var config + Dockerfile** — enables reproducible deployments.
3. **Published Docker image** (§5) — wire up the GitHub Actions workflow once the Dockerfile is stable.
4. **Fuseki-compatible routing** (§6 Phase F1) — per-dataset URL paths.
5. **Admin API** (§6 Phase F2) — dataset create/delete/list.
6. **SPARQL Update** (§6 Phase F3) — INSERT/DELETE DATA, CLEAR, DROP.
7. **API-key auth** — minimal security for public deployments.
8. **Dataset registry** (§6 Phase F5) — multi-tenancy; requires Phase F1 + F2 first.
9. **OIDC auth** — only needed when dataset-level access control matters.
10. **Persistence / snapshots** — when data must survive restarts.

---

## 6. Fuseki Drop-in Compatibility

The goal is for any HTTP client that works against an Apache Jena Fuseki
in-memory instance to work unmodified against dagalog.  This includes standard
clients such as Apache Jena's own `UpdateExecutionHTTP`, rdflib's
`SPARQLUpdateStore`, and Comunica.

**Fuseki documentation:** <https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html>

**Scope:** in-memory datasets (`dbType=mem`) only.  TDB2 on-disk persistence
is out of scope.

**Integration tests:** all Fuseki compatibility tests live in
[`sparql_endpoint/tests/fuseki_compat.rs`](sparql_endpoint/tests/fuseki_compat.rs).
All 50 tests pass; no `#[ignore]` attributes remain.

---

### 6.1 Fuseki URL structure

Fuseki exposes every dataset under a configurable path prefix.  A default
single-dataset in-memory server is typically started as:

```sh
fuseki-server --mem /ds
```

which gives the dataset the name `/ds` and exposes these service endpoints:

| Service | URL | Methods |
|---------|-----|---------|
| Query | `/{name}/sparql` | GET, POST |
| Query (alias) | `/{name}/query` | GET, POST |
| Update | `/{name}/update` | POST |
| GSP read-write | `/{name}/data` | GET, PUT, POST, DELETE, HEAD |
| GSP read-only | `/{name}/get` | GET, HEAD |

dagalog must support exactly these paths.  The current `/sparql` and
`/rdf-graph-store` paths can be kept as aliases for backward compatibility.

---

### 6.2 Admin API (`/$/...`)

Fuseki exposes a management API under the special prefix `/$/ `.  All admin
endpoints return JSON.

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/$/ping` | GET, POST | Liveness check — returns `"OK"` |
| `/$/server` | GET | Server info (version, uptime, datasets) |
| `/$/datasets` | GET | List all datasets |
| `/$/datasets` | POST | Create a new dataset |
| `/$/datasets/{name}` | GET | Info for one dataset |
| `/$/datasets/{name}` | DELETE | Remove a dataset |

#### Creating a dataset (`POST /$/datasets`)

Form-encoded body with two parameters:

| Parameter | Required | Values |
|-----------|----------|--------|
| `dbName` | yes | URL path name, e.g. `/ds` or `mydata` |
| `dbType` | yes | `mem` (in-memory, supported); `tdb2` (out of scope) |

Returns `200 OK` on success (not 201).

#### Dataset list response (`GET /$/datasets`)

```json
{
  "datasets": [
    {
      "ds.name": "/ds",
      "ds.state": "active",
      "ds.services": [
        { "srv.type": "query",  "srv.description": "SPARQL 1.1 Query",
          "srv.endpoints": ["query", "sparql"] },
        { "srv.type": "update", "srv.description": "SPARQL 1.1 Update",
          "srv.endpoints": ["update"] },
        { "srv.type": "gsp-rw", "srv.description": "Graph Store Protocol (Read-Write)",
          "srv.endpoints": ["data"] },
        { "srv.type": "gsp-r",  "srv.description": "Graph Store Protocol (Read only)",
          "srv.endpoints": ["get"] }
      ]
    }
  ]
}
```

#### Dataset info response (`GET /$/datasets/{name}`)

Returns a single object matching the element shape above (no wrapping `datasets` array).

---

### 6.3 SPARQL Update (`/{name}/update`)

The SPARQL 1.1 Update language must be parsed and executed.  Required
operations for a drop-in replacement:

| Operation | Example |
|-----------|---------|
| `INSERT DATA` | `INSERT DATA { <s> <p> <o> }` |
| `DELETE DATA` | `DELETE DATA { <s> <p> <o> }` |
| `INSERT/DELETE WHERE` | `DELETE { ?s ?p ?o } WHERE { ?s ?p ?o }` |
| `CLEAR` | `CLEAR DEFAULT`, `CLEAR GRAPH <g>`, `CLEAR ALL` |
| `DROP` | `DROP GRAPH <g>`, `DROP ALL` |
| `CREATE` | `CREATE GRAPH <g>` |
| `LOAD` | `LOAD <url> INTO GRAPH <g>` (HTTP URL loading; skip for in-memory-only) |
| `COPY`, `MOVE`, `ADD` | graph-to-graph copy/move/merge |

**Parser note:** The current `sparql_parser` crate is SELECT-only.  SPARQL
Update is a separate grammar.  The simplest path is to add an `update` module
alongside the existing `parse_query` entry point rather than extending the
SELECT parser.

**Endpoint behaviour:**

| Content-Type | Body |
|---|---|
| `application/sparql-update` | Raw SPARQL Update string |
| `application/x-www-form-urlencoded` | `update=<percent-encoded string>` |

Returns `200 OK` on success, `400 Bad Request` for parse errors, `500` for
execution errors.

---

### 6.4 GSP content negotiation on `/{name}/data`

The Fuseki `/data` endpoint accepts and produces more formats than the current
`/rdf-graph-store` implementation.  Required additions:

**GET (Accept):**

| MIME type | Format | Status |
|-----------|--------|--------|
| `text/turtle` | Turtle | ✓ Done |
| `application/n-triples` | N-Triples | ✓ Done |
| `application/n-quads` | N-Quads | ❌ Needed |
| `application/trig` | TriG | ❌ Needed |

**PUT/POST (Content-Type accepted):**

| MIME type | Format | Status |
|-----------|--------|--------|
| `text/turtle` | Turtle | ✓ Done |
| `application/n-triples` | N-Triples | ❌ Needed |
| `application/n-quads` | N-Quads | ❌ Needed |
| `application/trig` | TriG | ❌ Needed |

The `turtle` crate now exports `parse_ntriples`, `parse_nquads`, and
`parse_trig` — they just need to be wired into the GSP upload path alongside
the existing `parse_turtle` call.

---

### 6.5 Dataset registry (multi-dataset)

Today dagalog holds a single `Arc<RwLock<Datastore>>`.  Fuseki is
multi-dataset.  The architecture change:

```rust
pub struct DatasetRegistry {
    datasets: HashMap<String, Arc<RwLock<Datastore>>>,
}
```

Routes become `/{name}/sparql`, `/{name}/data`, etc., where `{name}` is
matched dynamically and looked up in the registry.  404 when the dataset does
not exist.

The single-dataset `Config::default()` starts a registry with one entry named
`/ds`, preserving backward-compatible behaviour for the Docker image and CLI.

**Prerequisite:** `Datastore::drop_all` or a graph-level clear is needed so
`DELETE /$/datasets/{name}` can release memory.  The existing
`Datastore::remove_graph` covers named graphs; the default graph and
reified-triples table also need clearing.

---

### 6.6 Implementation phases and test groups

| Phase | Description | Test group | Status |
|-------|-------------|------------|--------|
| F1 | Per-dataset URL routing (`/{name}/sparql`, `/{name}/data`, etc.) | A, B | ✓ Done |
| F2 | Admin API: ping + server info | C | ✓ Done |
| F3 | Admin API: list + info datasets | D | ✓ Done |
| F4 | Admin API: create + delete datasets | E | ✓ Done |
| F5 | SPARQL Update (`/{name}/update`) | F | ✓ Done |
| F6 | GSP content negotiation (N-Quads, TriG upload + download) | G | ✓ Done |
| F7 | Dataset registry (multi-dataset, dynamic routing) | H | ✓ Done |
| F8 | Full lifecycle (create → upload → query → delete) | I | ✓ Done |

All 50 tests in `fuseki_compat.rs` pass — no `#[ignore]` attributes remain in this file.

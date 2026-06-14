# Dagalog HTTP Server — Remaining Work

This document covers the remaining implementation areas for the HTTP server.
Completed work (GSP, Fuseki-compatible routing, admin API, SPARQL Update,
multi-dataset registry, Dockerfile) is documented in [`README.md`](README.md).

Authorization is covered in detail in [`AUTH.md`](AUTH.md).

---

## 1. Persistence

The current in-memory `Datastore` is lost on restart. The persistence goal is:

> **When a write transaction returns `200 OK` (or `204 No Content`), the server
> guarantees that the written data will survive a process crash or restart.**

This is the standard durability guarantee (the D in ACID). It means every
mutating endpoint — SPARQL Update (`POST /{ds}/update`), GSP PUT/POST/DELETE,
graph-store admin — must complete a durable commit before responding.

See [`PERSISTENCE_PLAN.md`](PERSISTENCE_PLAN.md) for the phased implementation
roadmap.

### Design options

| Approach | Durability | Complexity | Notes |
|---|---|---|---|
| **Snapshot on shutdown** | None (crash loses data) | Low | Not acceptable for the durability goal |
| **WAL (Write-Ahead Log)** | Full — fsync before 200 OK | Medium | Recommended first step; pure Rust, no external deps |
| **Embedded ACID store** (`redb` / `sled`) | Full | Medium | Delegates durability to a proven library; easier to get right |
| **Memory-mapped + WAL** (custom) | Full | High | Needed eventually for datasets that exceed RAM; defer |

**Recommended approach:** WAL on top of a `redb` embedded database. `redb`
provides ACID transactions with a pure-Rust implementation and zero external
dependencies. The `Datastore` quad tables are stored as `redb` tables;
committing a `redb` write transaction is the durability boundary.

### Transaction model

Each HTTP mutating request maps to exactly one `redb` write transaction:

1. Open a write transaction on request arrival.
2. Apply all quad insertions / deletions to the `redb` tables.
3. Commit (fsync). If this returns `Ok`, respond `200`/`204`.
4. If the commit errors, respond `500` and the transaction is automatically
   rolled back.

Read requests (SPARQL SELECT, GSP GET) use `redb` read transactions, which
are snapshot-isolated — they see a consistent view of the store even while a
concurrent write transaction is in progress.

### Configuration

Two storage modes — selected at startup, cannot be changed at runtime:

| Mode | How to select | Data survives restart? |
|---|---|---|
| **In-memory** (default) | omit `--data-dir` | No — current behaviour |
| **Persistent** | `--data-dir <PATH>` or `DAGALOG_DATA_DIR` | Yes — durable commit before 200 OK |

| CLI flag | Env var | Description | Default |
|---|---|---|---|
| `--data-dir <PATH>` | `DAGALOG_DATA_DIR` | Directory for the `redb` changelog file | *(in-memory)* |
| `--no-persist` | `DAGALOG_NO_PERSIST=1` | Force in-memory even if `DAGALOG_DATA_DIR` is set | `false` |

Storage locations (local disk, Docker volumes, Kubernetes PVCs) and caveats
(NFS, cloud object storage) are documented in
[`PERSISTENCE_PLAN.md`](PERSISTENCE_PLAN.md).

### Prerequisites

- `Datastore::drop_all` (clear all graphs and the reified-triples table) — needed
  for dataset deletion.
- A thin `PersistentDatastore` wrapper (or refactor of `Datastore`) that holds a
  `redb::Database` handle alongside the in-memory interning table
  (`GraphElementManager`). The interning table itself must also be persisted so
  `GraphElementId`s remain stable across restarts.

---

## 3. Publishing a Pre-built Docker Image

Without a published image, anyone who wants to try dagalog via Docker must
clone the repository and wait for a full Rust compile (several minutes on a
fresh machine).  A pre-built image removes that friction entirely:

```sh
docker run -p 3030:3030 ghcr.io/daghovland/rdf-datalog --serve
```

**Recommended registry:** GitHub Container Registry (`ghcr.io/daghovland/rdf-datalog`).

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
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

### Build time

A cold Rust build takes ~5–10 minutes on GitHub-hosted runners. Use
`cache-from: type=gha` (shown above) to bring subsequent builds down to
1–2 minutes for small changes.  `cargo-chef` can also pre-compile dependencies
in a separate layer to further reduce cache misses.

### Multi-platform images

Add `platforms: linux/amd64,linux/arm64` to the `build-push-action` step to
produce images that run natively on both x86-64 servers and Apple Silicon.

---

## Suggested implementation order

| Step | Description | Status |
|------|-------------|--------|
| 1 | GSP (`graph_store.rs`) | ✓ Done |
| 2 | Env-var config (`DAGALOG_PORT`, `DAGALOG_READ_ONLY`, …) | ✓ Done |
| 3 | Published Docker image (GitHub Actions `docker.yml`) | ✓ Done |
| 4 | Fuseki-compatible routing (`/{name}/sparql`, `/{name}/data`, …) | ✓ Done |
| 5 | Admin API (`/$/ping`, `/$/datasets`, …) | ✓ Done |
| 6 | SPARQL Update (`/{name}/update`) | ✓ Done |
| 7 | API-key auth (see [`AUTH.md`](AUTH.md) steps A–D) | ❌ Remaining |
| 8 | Dataset registry (multi-dataset, dynamic routing) | ✓ Done |
| 9 | Entra ID / RBAC auth (see [`AUTH.md`](AUTH.md) steps E–H) | ❌ Remaining |
| 10 | Durable transactional persistence (`redb`-backed, see [`PERSISTENCE_PLAN.md`](PERSISTENCE_PLAN.md)) | ❌ Remaining |

# Dagalog HTTP Server — Remaining Work

This document covers the remaining implementation areas for the HTTP server.
Completed work (GSP, Fuseki-compatible routing, admin API, SPARQL Update,
multi-dataset registry, Dockerfile) is documented in [`README.md`](README.md).

Authorization is covered in detail in [`AUTH.md`](AUTH.md).

---

## 1. Persistence

The current in-memory `Datastore` is lost on restart. Long-term options:

- **Snapshot serialisation** — Turtle dump on shutdown, reload on startup. Low
  complexity, fine for modest sizes.
- **Memory-mapped storage** (`sled`, `redb`, or custom). Required for large
  datasets. Defer until benchmarks indicate a need.

`Datastore::drop_all` (clear all graphs and the reified-triples table) is a
prerequisite for dataset deletion to release memory.

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
| 10 | Persistence / snapshots | ❌ Remaining |

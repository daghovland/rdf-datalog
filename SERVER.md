# Dagalog HTTP Server — Remaining Work

This document covers the remaining implementation areas for the HTTP server.
Completed work (GSP, Fuseki-compatible routing, admin API, SPARQL Update,
multi-dataset registry, Dockerfile) is documented in [`README.md`](README.md).

---

## 1. Authentication

The server currently has no access control.  Two realistic tiers:

### Tier 1 — API key (simple, ship first)

- Add an `api_key: Option<String>` field to `Config`.
- Middleware (`tower_http::validate_request::ValidateRequestHeaderLayer` or a
  custom `axum::middleware`) checks `Authorization: Bearer <key>` on all
  mutating requests (PUT/POST/DELETE `/rdf-graph-store`, `/{name}/update`, etc.).
- Read endpoints remain unauthenticated unless a `require_auth_for_reads: bool`
  flag is set.
- Key is configured via `--api-key <KEY>` CLI flag or `DAGALOG_API_KEY` env var.

### Tier 2 — OAuth2 / OIDC (for multi-user deployments)

- Use `axum-oidc` or `tower-oidc` to validate JWT bearer tokens.
- Claims map to read / write / admin roles.
- `Config` gains `oidc_issuer: Option<Url>` and `oidc_audience: Option<String>`.
- Roles can be scoped per dataset when datasets are implemented.

### Considerations

- SPARQL 1.1 Protocol defines no auth mechanism — auth is a transport concern.
  Returning `401 Unauthorized` with `WWW-Authenticate: Bearer` is standard.
- The browser UI needs to store and send the token; add an input field in the
  frontend once auth is active.

---

## 2. Persistence

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
| 3 | Published Docker image (GitHub Actions `docker.yml`) | ❌ Remaining |
| 4 | Fuseki-compatible routing (`/{name}/sparql`, `/{name}/data`, …) | ✓ Done |
| 5 | Admin API (`/$/ping`, `/$/datasets`, …) | ✓ Done |
| 6 | SPARQL Update (`/{name}/update`) | ✓ Done |
| 7 | API-key auth | ❌ Remaining |
| 8 | Dataset registry (multi-dataset, dynamic routing) | ✓ Done |
| 9 | OIDC auth | ❌ Remaining |
| 10 | Persistence / snapshots | ❌ Remaining |

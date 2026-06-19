# Deployment

dagalog can serve a SPARQL 1.1 HTTP endpoint. This page covers running it as a server,
configuration options, authentication, and Docker deployment.

---

## Starting the server

```sh
# Empty store
dagalog --serve

# Load data at startup
dagalog --serve --data data.ttl --ontology schema.ttl

# Custom port
dagalog --serve --data data.ttl --port 8080
```

The server listens on `http://localhost:3030` by default.

**SPARQL endpoint:** `http://localhost:3030/sparql`  
**Web UI:** `http://localhost:3030`  
**Service Description:** `http://localhost:3030/sparql` (with `Accept: text/turtle`)

---

## Docker

### Quick start

```sh
# Empty store
docker run -p 3030:3030 ghcr.io/daghovland/dagalog

# Load a local file
docker run -p 3030:3030 -v ./data:/data ghcr.io/daghovland/dagalog --serve --data /data/my.ttl
```

### Build locally

```sh
git clone https://github.com/daghovland/rdf-datalog
cd rdf-datalog
docker build -t dagalog .
docker run -p 3030:3030 dagalog
```

### docker-compose

The included `docker-compose.yml` mounts `./data/` and loads `./data/dataset.ttl`:

```sh
docker compose up
```

To start with an empty store:

```sh
docker compose run --rm -p 3030:3030 dagalog --serve
```

---

## Environment variables

All CLI flags have environment variable equivalents. Environment variables are useful in
Docker, Kubernetes, and cloud deployments where you want to avoid putting secrets on the
command line.

CLI flags take precedence over environment variables.

| Variable | CLI flag | Default | Description |
|---|---|---|---|
| `DAGALOG_PORT` | `--port` | `3030` | Port to listen on |
| `DAGALOG_BASE_IRI` | `--base-iri` | `http://localhost:PORT` | Base IRI for Service Description |
| `DAGALOG_READ_ONLY` | `--read-only` | `false` | Disable all mutating endpoints |
| `DAGALOG_QUERY_TIMEOUT` | `--query-timeout` | `30` | Maximum query time in seconds |
| `DAGALOG_DATA_DIR` | `--data-dir` | *(in-memory)* | Directory for durable storage |
| `DAGALOG_NO_PERSIST` | `--no-persist` | `false` | Force in-memory mode |
| `DAGALOG_API_KEY` | `--api-key` | *(none)* | Static Bearer token |
| `DAGALOG_AUTH_READS` | `--require-auth-for-reads` | `false` | Protect reads with API key |
| `DAGALOG_OIDC_ISSUER` | `--oidc-issuer` | *(none)* | OIDC provider base URL |
| `DAGALOG_OIDC_AUDIENCE` | `--oidc-audience` | *(none)* | Expected `aud` JWT claim |

See the [full environment variable table in the README](../../README.md#environment-variables)
for all OIDC configuration variables.

---

## Authentication

The server supports three authentication tiers, selected at startup:

| Tier | When to use |
|---|---|
| 0 — None (default) | Local / trusted-network deployments |
| 1 — API key | Single-tenant, simple deployments |
| 2 — OIDC / JWT | Multi-user deployments (Azure Entra ID, Google, Keycloak, …) |

### No authentication (Tier 0)

The default. All endpoints are open. Suitable for local development and trusted networks.

```sh
dagalog --serve --data data.ttl
```

### API key (Tier 1)

Protects write endpoints with a shared Bearer token. Reads remain open by default.

```sh
dagalog --serve --data data.ttl --api-key "my-secret-key"
```

To protect reads too:

```sh
dagalog --serve --data data.ttl --api-key "my-secret-key" --require-auth-for-reads
```

Clients send the key in the `Authorization` header:

```sh
curl -H "Authorization: Bearer my-secret-key" http://localhost:3030/ds/update \
     --data "INSERT DATA { <urn:s> <urn:p> <urn:o> }" \
     -H "Content-Type: application/sparql-update"
```

### OIDC / JWT (Tier 2)

For multi-user deployments. dagalog validates incoming JWTs locally using the provider's
public keys. Supported providers include Azure Entra ID, Google, Keycloak, and Auth0.

```sh
dagalog --serve --data data.ttl \
  --oidc-issuer "https://login.microsoftonline.com/<tenant-id>/v2.0" \
  --oidc-audience "api://dagalog"
```

See the [Authentication section in the README](../../README.md#authentication) for
step-by-step setup guides for Azure Entra ID, Google, Keycloak, and Auth0.

---

## Read-only mode

Start dagalog with `--read-only` to disable all mutating endpoints (SPARQL Update,
Graph Store Protocol writes, admin API). Useful when serving a static dataset:

```sh
dagalog --serve --data data.ttl --read-only
```

---

## Multi-dataset server

dagalog exposes a Fuseki-compatible routing API. Each dataset has its own SPARQL endpoint:

| Endpoint | Description |
|---|---|
| `GET /sparql` | Default dataset query |
| `GET /{name}/sparql` | Named dataset query |
| `POST /$/datasets` | Create a new dataset |
| `DELETE /$/datasets/{name}` | Drop a dataset |

See the [SPARQL HTTP endpoint section in the README](../../README.md#sparql-http-endpoint)
for a full protocol reference.

---

## Query timeout

Prevent runaway queries from hanging the server:

```sh
dagalog --serve --data data.ttl --query-timeout 10   # 10-second limit
```

Default is 30 seconds. Set to `0` to disable the timeout (not recommended for public endpoints).

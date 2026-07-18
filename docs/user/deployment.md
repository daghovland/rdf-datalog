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
| `DAGALOG_OIDC_JWKS_URI` | `--oidc-jwks-uri` | *(auto-discovered)* | Explicit JWKS URI (skips OIDC discovery) |
| `DAGALOG_OIDC_ROLES_CLAIM` | `--oidc-roles-claim` | `roles` | JWT claim path holding the roles array |
| `DAGALOG_OIDC_READ_ROLE` | `--oidc-read-role` | `dagalog.Read` | Role name that grants read access |
| `DAGALOG_OIDC_WRITE_ROLE` | `--oidc-write-role` | `dagalog.Write` | Role name that grants write access |
| `DAGALOG_OIDC_ADMIN_ROLE` | `--oidc-admin-role` | `dagalog.Admin` | Role name that grants admin access |
| `DAGALOG_OIDC_BROWSER_CLIENT_ID` | `--oidc-browser-client-id` | *(none)* | App client ID for the MSAL.js sign-in button in the browser UI |

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

### Permission model

Every request is classified into one of three permissions before the auth check runs:

| Permission | Operations |
|------------|-----------|
| `Read` | `GET /sparql`, `GET /{name}/sparql`, `GET /{name}/data`, GSP GET, admin reads |
| `Write` | `POST /{name}/update`, `POST /{name}/rml`, `PUT`/`POST`/`DELETE` on data and GSP endpoints |
| `Admin` | `POST /$/datasets` (create), `DELETE /$/datasets/{name}` (drop), `POST /$/compact` |

`Write` implies `Read`. `Admin` implies both. `--read-only` forces `Read` regardless of
credentials — no token can unlock a mutating endpoint.

### OIDC / JWT (Tier 2)

For multi-user deployments. dagalog acts as a pure resource server: it validates incoming
JWTs locally using the provider's public JWKS keys, and reads a `roles` claim to decide
`Read`/`Write`/`Admin`. No OIDC library or redirect flow runs on the dagalog side.

```sh
dagalog --serve --data data.ttl \
  --oidc-issuer "https://login.microsoftonline.com/<tenant-id>/v2.0" \
  --oidc-audience "api://dagalog"
```

#### Azure Entra ID

**Step 1 — Register the app.** Entra ID → App registrations → New registration. Name:
`dagalog`. Under *Expose an API*, set the Application ID URI (e.g. `api://dagalog`).

**Step 2 — Create app roles.** In the app registration → *App roles* → Create, with
*Allowed member types* set to **Applications + Users**:

| Display name | Value |
|---|---|
| Dagalog Read | `dagalog.Read` |
| Dagalog Write | `dagalog.Write` |
| Dagalog Admin | `dagalog.Admin` |

**Step 3 — Assign roles.** In *Enterprise applications → dagalog → Users and groups*,
assign users, security groups, or service principals to the roles above. For a service
principal calling dagalog, use *API permissions → Add permission → My APIs → dagalog →
Application permissions*, then grant admin consent.

**Step 4 — Start dagalog:**

```sh
dagalog --serve --data data.ttl \
  --oidc-issuer "https://login.microsoftonline.com/<tenant-id>/v2.0" \
  --oidc-audience "api://dagalog"
```

**Calling the API** — a service principal acquires a token with the client-credentials flow:

```sh
TOKEN=$(curl -s -X POST \
  "https://login.microsoftonline.com/<tenant-id>/oauth2/v2.0/token" \
  -d "grant_type=client_credentials" \
  -d "client_id=<client-id>" \
  -d "client_secret=<secret>" \
  -d "scope=api://dagalog/.default" \
  | jq -r .access_token)

curl -H "Authorization: Bearer $TOKEN" \
     "http://localhost:3030/sparql?query=SELECT+*+WHERE+%7B%7D"
```

**Browser sign-in (MSAL.js)** — set `--oidc-browser-client-id` to the app registration's
client ID to enable a *Sign in* button in the browser UI; MSAL.js then handles the
interactive popup flow and token refresh automatically.

#### Google

Google issues standard RS256 JWTs for service accounts and Identity Platform users. Google
JWTs carry no application roles by default, so map a custom claim (Workspace custom
attribute, or an IAP policy) into a top-level claim such as `dagalog_roles`:

```sh
dagalog --serve --data data.ttl \
  --oidc-issuer "https://accounts.google.com" \
  --oidc-audience "https://dagalog.example.com" \
  --oidc-roles-claim "dagalog_roles"
```

```sh
curl -H "Authorization: Bearer $(gcloud auth print-identity-token \
       --audiences=https://dagalog.example.com)" \
     "https://dagalog.example.com/sparql?query=SELECT+*+WHERE+%7B%7D"
```

#### Keycloak, Auth0, and other providers

Any standard OIDC provider works the same way; role claim path and role names are
configurable so you can reuse names already defined in your identity provider:

```sh
dagalog --serve \
  --oidc-issuer "https://keycloak.example.com/realms/myrealm" \
  --oidc-audience "dagalog" \
  --oidc-roles-claim "realm_access.roles" \
  --oidc-read-role  "my-read-role" \
  --oidc-write-role "my-write-role" \
  --oidc-admin-role "my-admin-role"
```

Keycloak nests realm roles inside `realm_access.roles`; the roles-claim path supports
dot-separated nesting for providers that structure claims this way.

#### Auth config endpoint

`GET /auth/config` is always public and returns the active auth mode, which the browser
UI uses to decide what sign-in option to show:

```sh
curl http://localhost:3030/auth/config
# {"mode":"oidc","oidc":{"issuer":"https://…","audience":"api://dagalog"}}
```

#### Library usage

```rust
use sparql_endpoint::{AuthConfig, Config, OidcConfig, serve};

// Azure Entra ID convenience constructor:
let config = Config {
    auth: AuthConfig::Oidc(OidcConfig::azure("<tenant-id>", "api://dagalog")),
    ..Config::default()
};

// Generic OIDC (Google, Keycloak, Auth0, …):
let config = Config {
    auth: AuthConfig::Oidc(OidcConfig {
        issuer:      "https://accounts.google.com".to_owned(),
        jwks_uri:    None,                       // auto-discovered
        audience:    "https://dagalog.example.com".to_owned(),
        roles_claim: "dagalog_roles".to_owned(),
        read_role:   "dagalog.Read".to_owned(),
        write_role:  "dagalog.Write".to_owned(),
        admin_role:  "dagalog.Admin".to_owned(),
        browser_client_id: None,
    }),
    ..Config::default()
};
```

---

## Read-only mode

Start dagalog with `--read-only` to disable all mutating endpoints (SPARQL Update,
Graph Store Protocol writes, admin API). Useful when serving a static dataset:

```sh
dagalog --serve --data data.ttl --read-only
```

---

## HTTP API reference

### Root endpoints

| Route | Description |
|---|---|
| `GET /` | Browser UI (query + upload) |
| `GET /sparql?query=<encoded>` | SPARQL 1.1 SELECT / ASK / CONSTRUCT |
| `POST /sparql` (`application/sparql-query`) | SPARQL 1.1 query (direct body) |
| `POST /sparql` (`application/x-www-form-urlencoded`) | SPARQL 1.1 query or update (form body) |
| `POST /sparql` (`application/sparql-update`) | SPARQL 1.1 Update (direct body) |
| `GET /sparql` (no `query=`) | SPARQL 1.1 Service Description (Turtle) |
| `GET /.well-known/void`, `GET /void` | VoID dataset description |
| `POST /upload` | Load Turtle data into the default graph (legacy alias) |

Response format for SELECT/ASK is negotiated via `Accept`: `application/sparql-results+json`
(default), `application/sparql-results+xml`, or `text/csv`; unrecognised formats get
`406 Not Acceptable`. All responses carry an `ETag` based on the dataset's write generation
counter, so conditional `If-None-Match` requests work for caching.

### Graph Store Protocol (GSP)

| Route | Description |
|---|---|
| `GET`/`PUT`/`POST`/`DELETE`/`HEAD` `/rdf-graph-store?default` or `?graph=<iri>` | Retrieve / replace / merge into / delete / check a graph |
| `POST /rdf-graph-store` | Create a new graph (server-assigned IRI, returned in `Location`) |
| `GET`/`PUT` `/rdf-graphs/{name}` | Direct graph identification (§4.1) |

Output format is negotiated via `Accept`: Turtle (default), N-Triples, N-Quads, TriG, or
JSON-LD.

### Fuseki-compatible per-dataset routes

The server exposes a `default` dataset at `/ds`, plus any created via the admin API:

| Route | Description |
|---|---|
| `GET`/`POST` `/{name}/sparql` or `/{name}/query` | SPARQL SELECT |
| `POST /{name}/update` | SPARQL Update |
| `POST /{name}/rml` | Apply an RML mapping (`multipart/form-data`), merge into the dataset |
| `GET\|PUT\|POST\|DELETE\|HEAD /{name}/data` | GSP read-write |
| `GET\|HEAD /{name}/get` | GSP read-only |

`POST /rml/map` (root-level, not dataset-scoped) applies an RML mapping and returns the
generated RDF directly, touching no dataset — see the
[RML mapping guide](rml-mapping.md#applying-mappings-over-http).

### Admin API (`/$/…`)

| Route | Description |
|---|---|
| `GET /$/ping` | Liveness check |
| `GET /$/server` | Server info (version, dataset list) |
| `GET /$/datasets` | List all datasets |
| `POST /$/datasets` | Create a dataset (form body: `dbName=…&dbType=mem`) |
| `GET /$/datasets/{name}` | Dataset info |
| `DELETE /$/datasets/{name}` | Drop a dataset |
| `POST /$/compact` | Rewrite the persistence changelog |

### Library usage

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use dag_rdf::Datastore;
use sparql_endpoint::{AuthConfig, Config, serve};

#[tokio::main]
async fn main() {
    let mut store = Datastore::new(1_000_000);
    // load data, apply reasoning, apply rules …

    let config = Config::default(); // 0.0.0.0:3030, no auth

    serve(Arc::new(RwLock::new(store)), config).await.unwrap();
}
```

---

## Query timeout

Prevent runaway queries from hanging the server:

```sh
dagalog --serve --data data.ttl --query-timeout 10   # 10-second limit
```

Default is 30 seconds. Set to `0` to disable the timeout (not recommended for public endpoints).

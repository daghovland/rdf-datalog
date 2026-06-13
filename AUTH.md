# Dagalog Authorization Plan

This document covers access-control for the HTTP server.  Three tiers are
planned, in order of complexity:

| Tier | Mechanism | When to use |
|------|-----------|-------------|
| 0 | None (current) | Local / trusted-network deployments |
| 1 | Static API key | Single-tenant, simple deployments |
| 2 | Azure Entra ID + App Roles | Multi-user, Azure-hosted deployments |
| 3 | Managed Identity | Service-to-service inside Azure (no credentials) |

---

## Authorization model

Three logical permissions map to every operation:

| Permission | Operations |
|------------|-----------|
| `Read` | `GET /sparql`, `GET /{name}/sparql`, `GET /{name}/data`, `GET /rdf-graph-store`, `GET /$/…` |
| `Write` | `POST /{name}/update` (SPARQL Update), `PUT/POST/DELETE /{name}/data`, `PUT/POST/DELETE /rdf-graph-store` |
| `Admin` | `POST /$/datasets` (create), `DELETE /$/datasets/{name}` (drop) |

`Write` implies `Read`.  `Admin` implies both.

When `--read-only` is active the server enforces `Read`-only regardless of
auth credentials — no token can unlock mutating endpoints.

---

## Tier 1 — Static API key

### Design

- `Config` gains `api_key: Option<String>` and `require_auth_for_reads: bool`.
- When `api_key` is set, all `Write` + `Admin` operations require
  `Authorization: Bearer <key>`.  Reads stay open unless
  `require_auth_for_reads` is also set.
- The comparison must use constant-time equality (the `subtle` crate or
  `crypto_common::constant_time_eq`) to prevent timing attacks.

### Config changes (`sparql_endpoint/src/lib.rs`)

```rust
pub struct Config {
    // … existing fields …
    pub api_key: Option<String>,
    pub require_auth_for_reads: bool,
}
```

### CLI / env vars

| CLI flag | Env var | Description |
|----------|---------|-------------|
| `--api-key <KEY>` | `DAGALOG_API_KEY` | Shared secret; omit to disable auth |
| `--require-auth-for-reads` | `DAGALOG_AUTH_READS` | Protect GET endpoints too |

### Middleware sketch (`sparql_endpoint/src/auth.rs`)

```rust
pub async fn require_write_auth<B>(
    State(state): State<AppState>,
    request: Request<B>,
    next: Next<B>,
) -> Response {
    let Some(ref key) = state.config.api_key else {
        return next.run(request).await;   // auth disabled
    };
    match extract_bearer(request.headers()) {
        Some(token) if constant_eq(token.as_bytes(), key.as_bytes()) => {
            next.run(request).await
        }
        _ => unauthorized_response(),
    }
}
```

Apply via `.route_layer(axum::middleware::from_fn_with_state(state, require_write_auth))`
on all mutating routes in `server.rs`.

---

## Tier 2 — Azure Entra ID (RBAC)

### Overview

Clients authenticate against **Azure Entra ID** (formerly Azure AD) and present
a short-lived JWT Bearer token.  dagalog validates the token locally (signature,
expiry, audience) and reads the `roles` claim to authorise the request.

No OIDC library is needed — only JWT validation with RS256.  The full OIDC
flow runs on the *client* side; the server is a pure **resource server**.

### Azure setup

**Step 1 — Register the app**

1. Azure portal → Entra ID → App registrations → New registration.
2. Name: `dagalog` (or `dagalog-{env}`).  No redirect URI needed for a pure
   API (headless) deployment.
3. Under *Expose an API*, set the **Application ID URI** (e.g.
   `api://dagalog`).

**Step 2 — Define app roles**

In the app registration → *App roles* → Create:

| Display name | Value | Description |
|---|---|---|
| Dagalog Read | `dagalog.Read` | Execute SELECT queries, fetch graphs |
| Dagalog Write | `dagalog.Write` | SPARQL Update, PUT/POST/DELETE graphs |
| Dagalog Admin | `dagalog.Admin` | Create and drop datasets |

Set *Allowed member types* to **Applications + Users** for each role.

**Step 3 — Assign roles**

In Entra ID → Enterprise applications → `dagalog` → Users and groups:
assign individual users, security groups, or service principals to roles.

For a service principal (another Azure app calling dagalog), use:
*App registrations → (the calling app) → API permissions → Add permission →
My APIs → dagalog → Application permissions → dagalog.Write*.
Then grant admin consent.

### Token flow

```
Client                             Entra ID                dagalog
  │                                    │                       │
  │── POST /oauth2/v2.0/token ────────>│                       │
  │   grant_type=client_credentials   │                       │
  │   scope=api://dagalog/.default    │                       │
  │<── access_token (JWT) ────────────│                       │
  │                                    │                       │
  │── GET /{name}/sparql?query=… ─────────────────────────────>│
  │   Authorization: Bearer <token>   │                       │
  │                                    │── validate JWT        │
  │                                    │   check roles claim   │
  │<── 200 OK ─────────────────────────────────────────────────│
```

The JWT contains (among other claims):

```json
{
  "iss": "https://login.microsoftonline.com/{tenant_id}/v2.0",
  "aud": "api://dagalog",
  "roles": ["dagalog.Read"],
  "exp": 1700000000
}
```

### Server implementation

**New config fields:**

```rust
pub struct EntraConfig {
    pub tenant_id:   String,          // AAD tenant UUID
    pub client_id:   String,          // app's client_id / Application ID URI
    pub read_role:   String,          // default "dagalog.Read"
    pub write_role:  String,          // default "dagalog.Write"
    pub admin_role:  String,          // default "dagalog.Admin"
}

pub enum AuthConfig {
    None,
    ApiKey { key: String, require_for_reads: bool },
    Entra(EntraConfig),
}

pub struct Config {
    // … existing fields …
    pub auth: AuthConfig,
}
```

**CLI / env vars for Entra:**

| CLI flag | Env var | Description |
|----------|---------|-------------|
| `--entra-tenant <UUID>` | `DAGALOG_ENTRA_TENANT` | Tenant ID |
| `--entra-audience <URI>` | `DAGALOG_ENTRA_AUDIENCE` | App ID URI, e.g. `api://dagalog` |
| `--entra-read-role <NAME>` | `DAGALOG_ENTRA_READ_ROLE` | default `dagalog.Read` |
| `--entra-write-role <NAME>` | `DAGALOG_ENTRA_WRITE_ROLE` | default `dagalog.Write` |
| `--entra-admin-role <NAME>` | `DAGALOG_ENTRA_ADMIN_ROLE` | default `dagalog.Admin` |

**Required crates:**

```toml
jsonwebtoken = "9"    # JWT decode + RS256/ES256 validation
serde = "1"           # deserialise JWT claims
# reqwest is already a dependency for SPARQL requests
```

**JWKS caching (`sparql_endpoint/src/auth.rs`):**

```rust
pub struct JwksCache {
    keys: Arc<RwLock<Option<(JwkSet, Instant)>>>,
}

impl JwksCache {
    async fn fetch_or_refresh(&self, tenant_id: &str) -> Result<DecodingKey, AuthError> {
        // Cache TTL: 1 hour.  On miss or expiry, fetch from:
        // https://login.microsoftonline.com/{tenant_id}/discovery/v2.0/keys
        // Parse the JWK set, find the key matching the JWT's `kid` header.
    }
}
```

**Middleware:**

```rust
pub async fn entra_auth<B>(
    State(state): State<AppState>,
    mut request: Request<B>,
    next: Next<B>,
) -> Response {
    let AuthConfig::Entra(ref cfg) = state.config.auth else {
        return next.run(request).await;
    };
    let token = match extract_bearer(request.headers()) {
        Some(t) => t,
        None => return unauthorized_response(),
    };
    let claims = match validate_jwt(token, cfg, &state.jwks_cache).await {
        Ok(c) => c,
        Err(_) => return unauthorized_response(),
    };
    // Inject claims for handlers to inspect
    request.extensions_mut().insert(claims);
    next.run(request).await
}
```

Each handler (or a per-permission wrapper) extracts `Extension<Claims>` and
calls `claims.has_role(&cfg.write_role)` before proceeding.

**Validation steps inside `validate_jwt`:**

1. Decode the JWT header to get `kid` (key ID) and `alg` (must be `RS256`).
2. Fetch the matching public key from the JWKS cache.
3. Validate: signature, `exp`, `nbf`, `aud == cfg.client_id`,
   `iss == https://login.microsoftonline.com/{tenant_id}/v2.0`.
4. Return the decoded `Claims` struct.

### Per-dataset role scoping (future)

Phase 2 can add dataset-scoped roles:

```
dagalog.dataset.{name}.Read
dagalog.dataset.{name}.Write
```

The app-role names are registered in Entra ID for each dataset, or a
single parameterised claim convention is used with a custom claim
(`resource: "dataset:{name}"`). Defer until multi-tenant dataset isolation
is required.

---

## Tier 3 — Managed Identity (service-to-service)

Managed Identity is relevant in two directions:

### Incoming: dagalog as a resource server

A calling service (another Azure Container App, a Function, an AKS pod)
authenticates with its own Managed Identity and obtains a token for
`api://dagalog`.  On the dagalog side this is **identical to Tier 2** — the
token is a standard Entra ID JWT and is validated the same way.

The difference is only on the *caller* side: the caller uses the Azure
Instance Metadata Service (IMDS) endpoint instead of a client secret:

```sh
# Inside an Azure-hosted service:
curl "http://169.254.169.254/metadata/identity/oauth2/token?\
      api-version=2018-02-01&resource=api://dagalog" \
     -H Metadata:true
```

No code changes needed in dagalog beyond Tier 2.

### Outgoing: dagalog calling Azure services

When dagalog itself needs to reach an Azure service (e.g., load initial data
from **Azure Blob Storage** on startup, or write persistence snapshots), it can
use its own Managed Identity rather than storing credentials.

```rust
// Pseudocode — requires azure_identity + azure_storage_blobs crates
let credential = DefaultAzureCredential::default();
let blob_client = BlobClient::new(account, container, blob, credential);
let data = blob_client.get().await?;
```

`DefaultAzureCredential` tries (in order): environment variables,
workload-identity federation, IMDS managed identity, CLI credentials.

This is most relevant once **Tier 2 (Persistence)** in `SERVER.md` is
implemented — snapshots can be read/written to Blob Storage instead of local
disk.

---

## Browser UI changes

When authentication is active the browser UI needs minimal additions:

**API key (Tier 1):**
- A settings panel with a single "API key" text input.
- Store in `sessionStorage` (not `localStorage` — don't persist across tabs).
- Attach as `Authorization: Bearer <key>` on every mutating fetch.

**Entra ID (Tier 2):**
- Integrate [MSAL.js](https://github.com/AzureAD/microsoft-authentication-library-for-js)
  (loaded from CDN).
- Add a "Sign in" button; on click open the Entra ID popup/redirect flow.
- Store the access token in memory (MSAL handles refresh automatically).
- Pass `scopes: ["api://dagalog/dagalog.Write"]` to acquire a token with the
  right audience and role.

Both modes should degrade gracefully: unauthenticated users still see
read-only query results; only mutating actions prompt for credentials.

---

## Implementation order

| Step | Description |
|------|-------------|
| A | Add `AuthConfig` enum + `api_key` to `Config`; extend CLI with `--api-key` |
| B | Implement `require_write_auth` middleware; wire into `server.rs` |
| C | Add `require_auth_for_reads` flag; protect GET endpoints when set |
| D | Add API key input to browser UI |
| E | Add `EntraConfig` fields to `Config`; extend CLI with `--entra-*` flags |
| F | Implement JWKS cache (`JwksCache`) and `validate_jwt` |
| G | Implement `entra_auth` middleware; inject `Claims` extension |
| H | Add MSAL.js sign-in flow to browser UI |
| I | Document Managed Identity setup for Azure Container Apps / AKS |

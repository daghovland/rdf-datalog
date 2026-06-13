# Dagalog Authorization Plan

This document covers access-control for the HTTP server.  Three tiers are
planned, in order of complexity:

| Tier | Mechanism | When to use |
|------|-----------|-------------|
| 0 | None (current) | Local / trusted-network deployments |
| 1 | Static API key | Single-tenant, simple deployments |
| 2 | Generic OIDC (Azure Entra ID, Google, Keycloak, Auth0, …) | Multi-user, any cloud or on-prem |
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

## Tier 2b — Generic OIDC providers (Google, Keycloak, Auth0, …)

The Entra ID design is a thin specialisation of standard OIDC JWT validation.
Generalising it to support any OIDC provider requires only:

1. Replacing the Azure-specific JWKS URL with an auto-discovered one.
2. Making `issuer` and `audience` configurable instead of derived from a
   tenant ID.

### Generalised config

```rust
pub struct OidcConfig {
    /// Base URL of the identity provider, e.g.
    ///   "https://accounts.google.com"
    ///   "https://login.microsoftonline.com/{tenant}/v2.0"
    ///   "https://keycloak.example.com/realms/myrealm"
    pub issuer: String,

    /// Optional override for the JWKS URI.  When `None` the server fetches
    /// `{issuer}/.well-known/openid-configuration` and reads `jwks_uri`.
    pub jwks_uri: Option<String>,

    /// Expected value of the `aud` claim (client_id or resource URI).
    pub audience: String,

    /// JWT claim that holds the role list.  Entra ID uses `"roles"`;
    /// Keycloak puts realm roles under `"realm_access.roles"`.
    /// Google tokens do not carry roles — use `groups_claim` instead.
    pub roles_claim: String,            // default "roles"

    pub read_role:  String,             // default "dagalog.Read"
    pub write_role: String,             // default "dagalog.Write"
    pub admin_role: String,             // default "dagalog.Admin"
}

pub enum AuthConfig {
    None,
    ApiKey { key: String, require_for_reads: bool },
    Oidc(OidcConfig),
}
```

`EntraConfig` becomes a builder helper that populates `OidcConfig` with the
Azure-specific issuer URL and `roles_claim`.

### OIDC discovery

On startup (or first request), fetch and cache the provider metadata:

```rust
let meta_url = format!("{}/.well-known/openid-configuration", cfg.issuer);
let meta: OidcMetadata = reqwest::get(&meta_url).await?.json().await?;
// Validate: meta.issuer == cfg.issuer (reject lookalike URLs)
// Cache: meta.jwks_uri — used by JwksCache::fetch_or_refresh
```

### CLI / env vars (generic OIDC)

| CLI flag | Env var | Description |
|----------|---------|-------------|
| `--oidc-issuer <URL>` | `DAGALOG_OIDC_ISSUER` | Provider base URL |
| `--oidc-audience <STR>` | `DAGALOG_OIDC_AUDIENCE` | Expected `aud` value |
| `--oidc-jwks-uri <URL>` | `DAGALOG_OIDC_JWKS_URI` | Override JWKS URL |
| `--oidc-roles-claim <STR>` | `DAGALOG_OIDC_ROLES_CLAIM` | Claim with roles (default `roles`) |
| `--oidc-read-role <NAME>` | `DAGALOG_OIDC_READ_ROLE` | default `dagalog.Read` |
| `--oidc-write-role <NAME>` | `DAGALOG_OIDC_WRITE_ROLE` | default `dagalog.Write` |
| `--oidc-admin-role <NAME>` | `DAGALOG_OIDC_ADMIN_ROLE` | default `dagalog.Admin` |

### Google Identity example

Google issues standard RS256 JWTs for service accounts and for the
Identity-Aware Proxy (IAP).  Two usage patterns:

**Service account (server-to-server)**

```sh
# Acquire a token scoped to your dagalog deployment
gcloud auth print-identity-token --audiences="https://dagalog.example.com"
```

Start dagalog with:

```sh
dagalog \
  --oidc-issuer https://accounts.google.com \
  --oidc-audience https://dagalog.example.com \
  --oidc-roles-claim "dagalog_roles" \
  --oidc-write-role writer
```

Google tokens do not carry application roles by default.  Either:
- Add a custom claim via a Google Workspace custom attribute or a Cloud IAP
  policy, or
- Map the `email` / `sub` claim to a role in dagalog's own config (future).

**Google Cloud Identity Platform / Firebase Auth (user-facing)**

Register dagalog as an OAuth2 resource in the [Google Cloud Console](https://console.cloud.google.com/):
1. *APIs & Services → Credentials → Create OAuth client ID* (Web application).
2. Set **Authorized redirect URIs** if the browser UI will perform the PKCE
   flow; leave empty for a pure resource server.
3. The issuer is `https://accounts.google.com`; the audience is your
   **client_id** from step 1.

### Keycloak example

Keycloak is the easiest way to test OIDC locally:

```sh
docker run -p 8080:8080 \
  -e KEYCLOAK_ADMIN=admin -e KEYCLOAK_ADMIN_PASSWORD=admin \
  quay.io/keycloak/keycloak:latest start-dev
```

1. Create a realm (e.g. `dagalog-dev`).
2. Create a client with *Access Type: bearer-only*, ID `dagalog`.
3. Add realm roles: `dagalog.Read`, `dagalog.Write`, `dagalog.Admin`.
4. Create a test user and assign roles.

Start dagalog:

```sh
dagalog \
  --oidc-issuer http://localhost:8080/realms/dagalog-dev \
  --oidc-audience dagalog \
  --oidc-roles-claim realm_access.roles
```

Keycloak puts realm roles inside a nested object; dagalog's JWT extraction
must handle dot-separated claim paths.

---

## Testing

### Tier 1 — API key tests

**Unit tests (`sparql_endpoint/src/auth.rs` or `tests/api_key.rs`)**

Use `axum::test::TestClient` (or `tower::ServiceExt::oneshot`) to drive
the router without binding a port.

| Test | Setup | Expected |
|------|-------|----------|
| No auth configured — write allowed | `api_key: None` | 200 |
| Correct key on write endpoint | `Authorization: Bearer correct` | 200 |
| Wrong key on write endpoint | `Authorization: Bearer wrong` | 401 |
| Missing header on write endpoint | no `Authorization` header | 401 |
| Correct key on read endpoint, reads unprotected | `require_for_reads: false` | 200 without key |
| Correct key on read endpoint, reads protected | `require_for_reads: true` | 401 without key |
| Timing: key comparison is constant-time | measure many correct/wrong key timings | Δt < noise floor |

```rust
#[tokio::test]
async fn api_key_wrong_returns_401() {
    let app = build_test_app(Config {
        auth: AuthConfig::ApiKey {
            key: "secret".into(),
            require_for_reads: false,
        },
        ..Config::default()
    });
    let response = app
        .oneshot(
            Request::post("/sparql/update")
                .header("Authorization", "Bearer wrong")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
```

**Integration test (end-to-end with a real HTTP listener)**

Spawn a `tokio::task` with `sparql_endpoint::run(config)`, wait for it to
bind, then use `reqwest` to issue real HTTP requests:

```rust
let url = spawn_server_with_key("test-key").await;
let client = reqwest::Client::new();

// Unauthenticated write → 401
let r = client.post(format!("{url}/sparql/update")).send().await?;
assert_eq!(r.status(), 401);

// Authenticated write → 200 (or 400 if body is empty, not 401)
let r = client
    .post(format!("{url}/sparql/update"))
    .bearer_auth("test-key")
    .body("INSERT DATA { <s> <p> <o> }")
    .send()
    .await?;
assert_ne!(r.status(), 401);
```

### Tier 2 — OIDC / JWT tests

**Unit tests — JWT validation (`sparql_endpoint/src/auth.rs`)**

Generate real RS256 key pairs in the test harness so the full validation
path runs without mocking `jsonwebtoken`.

```rust
fn make_test_keypair() -> (EncodingKey, DecodingKey, String) {
    // Use `rsa` crate to generate a 2048-bit key at test time.
    // Return encoding key, decoding key, and a fake kid.
}

fn make_claims(issuer: &str, audience: &str, roles: &[&str]) -> Claims { … }
```

| Test | Variation | Expected |
|------|-----------|----------|
| Valid token | correct iss/aud/exp/roles | `Ok(claims)` |
| Expired token | `exp` in the past | `Err(AuthError::Expired)` |
| Wrong audience | `aud` ≠ configured | `Err(AuthError::InvalidAudience)` |
| Wrong issuer | `iss` ≠ configured | `Err(AuthError::InvalidIssuer)` |
| Wrong algorithm | HS256 instead of RS256 | `Err(AuthError::UnsupportedAlgorithm)` |
| Missing role | valid token, no write role | middleware returns 403 |
| `dagalog.Admin` has write access | Admin role on write endpoint | 200 |

**Unit tests — JWKS cache**

Mock the HTTP layer with the [`wiremock`](https://crates.io/crates/wiremock)
crate:

```toml
[dev-dependencies]
wiremock = "0.6"
```

```rust
#[tokio::test]
async fn jwks_cache_refreshes_after_ttl() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&jwks_response()))
        .expect(2)   // called twice: initial fetch + post-TTL refresh
        .mount(&mock_server)
        .await;

    let cache = JwksCache::new(mock_server.uri() + "/keys", Duration::from_millis(1));
    cache.fetch_or_refresh().await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;  // expire TTL
    cache.fetch_or_refresh().await.unwrap();

    mock_server.verify().await;
}
```

**Integration tests — full HTTP flow with a local OIDC mock**

Option A — embedded mock (no external process, CI-friendly):

Use `wiremock` to serve both `.well-known/openid-configuration` and the
JWKS endpoint, issue tokens signed by a known test key, and drive the full
middleware stack.

Option B — Keycloak (closer to production, slower):

Add a `docker-compose.test.yml` that brings up Keycloak and a configured
dagalog instance.  Mark tests `#[ignore]` so they run only when
`DAGALOG_INTEGRATION_OIDC=1` is set.

```sh
DAGALOG_INTEGRATION_OIDC=1 cargo test --test oidc_integration -- --ignored
```

**Testing with Azure Entra ID (CI/CD)**

For PR pipelines, prefer the wiremock approach above.  For staging/release
pipelines that test against a real Entra ID tenant:

1. Create a dedicated `dagalog-test` app registration.
2. Store `DAGALOG_TEST_TENANT_ID`, `DAGALOG_TEST_CLIENT_ID`,
   `DAGALOG_TEST_CLIENT_SECRET` as repository secrets.
3. In CI, acquire a token via the client-credentials flow and exercise the
   live server:

```sh
TOKEN=$(curl -s -X POST \
  "https://login.microsoftonline.com/$TENANT/oauth2/v2.0/token" \
  -d "grant_type=client_credentials&client_id=$CLIENT_ID&client_secret=$SECRET&scope=api://dagalog/.default" \
  | jq -r .access_token)

curl -H "Authorization: Bearer $TOKEN" \
     "https://dagalog-staging.example.com/sparql?query=SELECT+*+WHERE+%7B%7D"
```

**Testing with Google (local dev)**

Use a Google service account key file:

```sh
# Obtain an identity token scoped to the local dagalog instance
gcloud auth print-identity-token --audiences="http://localhost:3030"
```

Start dagalog locally with Google as the OIDC provider:

```sh
DAGALOG_OIDC_ISSUER=https://accounts.google.com \
DAGALOG_OIDC_AUDIENCE=http://localhost:3030 \
cargo run -- serve
```

Then `curl -H "Authorization: Bearer $(gcloud auth print-identity-token ...)"`.

---

## Implementation order

| Step | Description |
|------|-------------|
| A | Add `AuthConfig` enum + `api_key` to `Config`; extend CLI with `--api-key` |
| B | Implement `require_write_auth` middleware; wire into `server.rs` |
| C | Add `require_auth_for_reads` flag; protect GET endpoints when set |
| D | Add API key input to browser UI |
| E | Generalise `EntraConfig` → `OidcConfig`; implement OIDC discovery |
| F | Implement JWKS cache (`JwksCache`) and `validate_jwt` |
| G | Implement `oidc_auth` middleware; inject `Claims` extension |
| H | Write unit tests for API key middleware (tower `oneshot`) |
| I | Write unit tests for JWT validation (test RSA key pair + `wiremock`) |
| J | Write integration test suite with embedded OIDC mock |
| K | Add MSAL.js sign-in flow to browser UI (Azure / generic OIDC) |
| L | Document Managed Identity setup for Azure Container Apps / AKS |

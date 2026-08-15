/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SPARQL 1.1 HTTP endpoint + Fuseki-compatible admin API.

pub mod admin;
pub mod auth;
pub mod constraints;
pub mod dataset_routes;
pub mod frontend;
pub mod graph_store;
pub mod negotiate;
pub mod ottr_endpoint;
pub mod persistence;
pub mod query;
pub mod query_builder;
pub mod reasoner_delta;
pub mod registry;
pub mod rml_endpoint;
pub mod rules_endpoint;
pub mod serialize;
pub mod server;
pub mod service_desc;
pub mod shacl_endpoint;
pub mod sparql_update;
pub mod transaction_routes;
pub mod transactions;
pub mod upload;
pub mod void;
pub mod vqs_routes;

use dag_rdf::datastore::Datastore;
use datalog::{IncrementalReasoner, Rule};
use ingress::NetworkPolicy;
use persistence::QuadChangelog;
use registry::DatasetRegistry;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use transactions::TransactionRegistry;

/// Authentication/authorization mode for the HTTP server.
#[derive(Clone, Debug, Default)]
pub enum AuthConfig {
    /// No authentication required (default — suitable for local/trusted deployments).
    #[default]
    None,
    /// Static Bearer token.  All mutating operations require `Authorization: Bearer <key>`.
    ApiKey {
        /// The shared secret clients must supply.
        key: String,
        /// When `true`, read-only operations (GET /sparql, etc.) also require the key.
        require_for_reads: bool,
    },
    /// Generic OIDC JWT validation (Azure Entra ID, Google, Keycloak, Auth0, …).
    Oidc(OidcConfig),
}

/// OIDC resource-server configuration.
///
/// The server validates incoming Bearer JWTs by:
/// 1. Discovering the JWKS URI from `{issuer}/.well-known/openid-configuration`
///    (unless `jwks_uri` is set explicitly).
/// 2. Fetching and caching the public keys.
/// 3. Verifying the token's signature, expiry, issuer, and audience.
/// 4. Extracting roles from the claim at `roles_claim` and comparing against
///    `read_role`, `write_role`, and `admin_role`.
///
/// See AUTH.md §Tier 2 / §Tier 2b for detailed setup instructions and examples
/// for Azure Entra ID, Google, and Keycloak.
#[derive(Clone, Debug)]
pub struct OidcConfig {
    /// Base URL of the identity provider (issuer).
    ///
    /// Examples:
    /// - `"https://login.microsoftonline.com/{tenant}/v2.0"` (Azure Entra ID)
    /// - `"https://accounts.google.com"` (Google)
    /// - `"https://keycloak.example.com/realms/myrealm"` (Keycloak)
    pub issuer: String,

    /// Optional explicit JWKS URI.  When `None`, the server fetches
    /// `{issuer}/.well-known/openid-configuration` and reads `jwks_uri`.
    pub jwks_uri: Option<String>,

    /// Expected value of the `aud` JWT claim (client ID or resource URI).
    pub audience: String,

    /// Dot-separated path to the roles array inside the JWT payload.
    ///
    /// - Azure / Google: `"roles"` (flat array)
    /// - Keycloak realm roles: `"realm_access.roles"` (nested object)
    ///
    /// **Limitation:** only one level of nesting is supported (`"a.b"`).
    /// Deeper paths such as `"resource_access.myapp.roles"` will silently
    /// find nothing, causing all requests to fail with `insufficient role`.
    /// Use a custom claim mapper on the identity provider side to flatten
    /// the roles array to a top-level claim if deeper nesting is needed.
    pub roles_claim: String,

    /// Role value that grants read access (default: `"dagalog.Read"`).
    pub read_role: String,
    /// Role value that grants write access (default: `"dagalog.Write"`).
    pub write_role: String,
    /// Role value that grants admin access (default: `"dagalog.Admin"`).
    pub admin_role: String,

    /// Browser application client ID for MSAL.js / Google Identity Services.
    ///
    /// When set, the browser UI shows a provider sign-in button and acquires
    /// tokens automatically.  Leave `None` to fall back to a manual token
    /// input (paste the token obtained from e.g. `gcloud auth print-identity-token`).
    pub browser_client_id: Option<String>,
}

impl OidcConfig {
    /// Convenience constructor for Azure Entra ID.
    ///
    /// Builds the **v2.0** issuer form (`.../v2.0`). This matches tokens
    /// issued for `audience` only if that app's Entra ID manifest requests
    /// v2.0 access tokens — `api.requestedAccessTokenVersion: 2` in the
    /// current Microsoft Graph manifest format, or the legacy
    /// `accessTokenAcceptedVersion: 2` on older portal views (same
    /// property, set on the *resource* app, not the caller). Without that
    /// setting — the default for most app registrations — tokens obtained
    /// via the classic
    /// Managed Identity / IMDS flow (`resource=` query param, not `scope=`)
    /// carry the **v1.0** issuer `https://sts.windows.net/{tenant_id}/`
    /// instead, and validation against this config's issuer will fail with
    /// `AuthError::InvalidIssuer`. If you can't change the manifest, build
    /// `OidcConfig` manually with `issuer:
    /// format!("https://sts.windows.net/{tenant_id}/")` instead of calling
    /// this constructor. See `docs/plans/AUTH.md` §Tier 3 for the full
    /// explanation and the Managed-Identity app-role-assignment caveat.
    pub fn azure(tenant_id: &str, audience: &str) -> Self {
        Self {
            issuer: format!("https://login.microsoftonline.com/{}/v2.0", tenant_id),
            jwks_uri: None,
            audience: audience.to_owned(),
            roles_claim: "roles".to_owned(),
            read_role: "dagalog.Read".to_owned(),
            write_role: "dagalog.Write".to_owned(),
            admin_role: "dagalog.Admin".to_owned(),
            browser_client_id: None,
        }
    }

    /// Convenience constructor for Google.
    pub fn google(audience: &str) -> Self {
        Self {
            issuer: "https://accounts.google.com".to_owned(),
            jwks_uri: None,
            audience: audience.to_owned(),
            roles_claim: "roles".to_owned(),
            read_role: "dagalog.Read".to_owned(),
            write_role: "dagalog.Write".to_owned(),
            admin_role: "dagalog.Admin".to_owned(),
            browser_client_id: None,
        }
    }
}

#[cfg(test)]
mod oidc_config_tests {
    use super::OidcConfig;

    /// `OidcConfig::azure()` must build the v2.0 issuer form, not the
    /// `sts.windows.net` (v1.0) form — see the doc comment on `azure()` and
    /// `docs/plans/AUTH.md` §Tier 3 for why this specific string matters:
    /// classic Managed-Identity/IMDS tokens only carry this issuer when the
    /// target app's manifest sets `accessTokenAcceptedVersion: 2`.
    #[test]
    fn azure_v2_issuer_format_is_pinned() {
        let cfg = OidcConfig::azure("11111111-2222-3333-4444-555555555555", "api://dagalog");
        assert_eq!(
            cfg.issuer,
            "https://login.microsoftonline.com/11111111-2222-3333-4444-555555555555/v2.0"
        );
        assert_eq!(cfg.audience, "api://dagalog");
        assert_eq!(cfg.roles_claim, "roles");
        assert_eq!(cfg.read_role, "dagalog.Read");
        assert_eq!(cfg.write_role, "dagalog.Write");
        assert_eq!(cfg.admin_role, "dagalog.Admin");
    }
}

/// Runtime configuration for the SPARQL endpoint.
#[derive(Clone, Debug)]
pub struct Config {
    /// Address to bind to (default: 0.0.0.0:3030).
    pub bind_addr: SocketAddr,
    /// Base IRI of this endpoint, used in Service Description.
    pub base_iri: String,
    /// If true, the update endpoint is disabled.
    pub read_only: bool,
    /// Maximum query execution time in seconds (default: 30).
    ///
    /// Enforced via cooperative cancellation inside the SPARQL evaluator
    /// (`sparql_parser::deadline::Deadline`, threaded through
    /// `execute_with_base`'s `timeout` parameter): the evaluator itself
    /// periodically checks an absolute deadline at loop-iteration
    /// boundaries and aborts once it has elapsed. `0` disables the timeout
    /// entirely (no deadline is constructed at all — a true no-op, not a
    /// zero-length one). A timed-out query gets a `503 Service Unavailable`
    /// response, distinct from the generic `500` used for other execution
    /// errors. See [#372](https://github.com/daghovland/rdf-datalog/issues/372).
    pub max_query_timeout_secs: u64,
    /// Authentication mode (default: none).
    pub auth: AuthConfig,
    /// Directory for durable persistence (redb changelog).
    ///
    /// `None` (default) → in-memory only; data is lost on restart.
    /// `Some(path)` → a redb changelog is created at `<path>/dagalog.redb`;
    ///   committed writes survive crash and restart.
    pub data_dir: Option<PathBuf>,
    /// Maximum request body size for the RML mapping endpoints
    /// (`POST /{name}/rml`, `POST /rml/map`), in bytes.
    ///
    /// These routes accept arbitrary CSV/XML/JSON source files as multipart
    /// parts, which routinely exceed axum's server-wide 2 MB
    /// `DefaultBodyLimit`. This field overrides the limit for just those two
    /// routes; every other route keeps the 2 MB default.
    ///
    /// Configurable via `--max-rml-upload-bytes` / `DAGALOG_MAX_RML_UPLOAD_BYTES`.
    /// See [#257](https://github.com/daghovland/rdf-datalog/issues/257).
    pub max_rml_upload_bytes: usize,
    /// Maximum request body size for the RDF write routes
    /// (`POST`/`PUT /{name}/data`, `POST`/`PUT /rdf-graph-store`,
    /// `POST`/`PUT`/`DELETE /rdf-graphs/{*path}`, `POST /upload`,
    /// `POST /{name}/shacl`), in bytes.
    ///
    /// These routes accept whole RDF graphs (Turtle/TriG/JSON-LD/N-Quads) or
    /// SHACL shapes graphs as a raw request body, which routinely exceed
    /// axum's server-wide 2 MB `DefaultBodyLimit` for realistic datasets.
    /// This field overrides the limit for just those routes; every other
    /// route keeps the 2 MB default.
    ///
    /// Configurable via `--max-rdf-upload-bytes` / `DAGALOG_MAX_RDF_UPLOAD_BYTES`.
    /// See [#274](https://github.com/daghovland/rdf-datalog/issues/274).
    pub max_rdf_upload_bytes: usize,
    /// Maximum wall-clock time (in seconds) any single HTTP request may
    /// occupy a connection before the server responds `408 Request Timeout`
    /// and frees the connection.
    ///
    /// This bounds *connection occupancy*, not server-side work: when it
    /// fires, the client gets a 408 and the connection is released, but the
    /// handler future (parse / lock / mutation) keeps running to completion
    /// in the background. It exists to stop a stalled or adversarially slow
    /// client from holding a connection (and, transitively, a request slot)
    /// open indefinitely.
    ///
    /// Distinct from `max_query_timeout_secs`, which bounds SPARQL
    /// evaluator work itself (cooperative cancellation inside the
    /// evaluator) rather than connection occupancy — see
    /// [#372](https://github.com/daghovland/rdf-datalog/issues/372).
    ///
    /// Configurable via `--request-timeout` / `DAGALOG_REQUEST_TIMEOUT`.
    /// See [#367](https://github.com/daghovland/rdf-datalog/issues/367).
    pub request_timeout_secs: u64,
    /// Datalog rules for incremental reasoning.
    ///
    /// When non-empty, an [`IncrementalReasoner`] is created from these rules
    /// during server startup (running full initial materialisation once).
    /// Subsequent INSERT DATA / DELETE DATA / GSP mutations then trigger
    /// semi-naive incremental re-materialisation instead of no inference at all.
    ///
    /// When empty (the default), no reasoner is created and SPARQL Update
    /// mutations affect only the explicitly stated triples.
    ///
    /// Related: [#110](https://github.com/daghovland/rdf-datalog/issues/110)
    pub initial_rules: Vec<Rule>,
    /// Network access policy for SPARQL `LOAD`, `SERVICE`, and JSON-LD external `@context` URLs.
    ///
    /// Default: [`NetworkPolicy::Deny`] — remote fetches return an error pointing
    /// the user to `--network=allow`.
    ///
    /// Related: [#118](https://github.com/daghovland/rdf-datalog/issues/118)
    pub network_policy: NetworkPolicy,
    /// Explicit origin allow-list for CORS on **state-changing** requests
    /// (any method other than `GET`/`HEAD`), e.g. `POST /update`,
    /// `PUT /{name}/data`, `DELETE /$/datasets/{name}`.
    ///
    /// Default: empty — no cross-origin state-changing request gets a
    /// CORS-approving preflight response, so browsers refuse to send the
    /// real request. Safe/read-only methods (`GET`, `HEAD`) remain permissive
    /// (`Access-Control-Allow-Origin: *`) regardless of this setting, since
    /// `allow_credentials` is never set and cross-origin reads are a
    /// legitimate use case (a web UI hosted on a different origin).
    ///
    /// Configurable via `--cors-allow-origin` (repeatable) /
    /// `DAGALOG_CORS_ALLOW_ORIGIN` (comma-separated).
    /// See [#362](https://github.com/daghovland/rdf-datalog/issues/362).
    pub cors_allowed_origins: Vec<String>,
    /// **Test-only** escape hatch for the `LOAD` SSRF preflight: when `true`,
    /// IPv4/IPv6 loopback addresses (`127.0.0.0/8`, `::1`) are exempted from
    /// the SSRF block that otherwise applies whenever `network_policy` is
    /// `Allow`/`AllowList`.
    ///
    /// Default: `false` — loopback is blocked like every other
    /// private/reserved range (see
    /// [#365](https://github.com/daghovland/rdf-datalog/issues/365)).
    /// Deliberately **not** exposed via any CLI flag or environment variable:
    /// the only intended caller is the wiremock-based test harness in
    /// `sparql_endpoint/tests/common/mod.rs`, which needs `LOAD` to be able to
    /// reach a mock HTTP server bound to `127.0.0.1`. A real deployment must
    /// never set this — loopback-bound services (admin UIs, metadata
    /// sidecars, etc.) are exactly what SSRF protection exists to block.
    pub allow_loopback_for_ssrf_tests: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_addr: "0.0.0.0:3030".parse().unwrap(),
            base_iri: "http://localhost:3030".to_string(),
            read_only: true,
            max_query_timeout_secs: 30,
            auth: AuthConfig::None,
            data_dir: None,
            max_rml_upload_bytes: 64 * 1024 * 1024,
            max_rdf_upload_bytes: 64 * 1024 * 1024,
            request_timeout_secs: 30,
            initial_rules: Vec::new(),
            network_policy: NetworkPolicy::Deny,
            cors_allowed_origins: Vec::new(),
            allow_loopback_for_ssrf_tests: false,
        }
    }
}

/// Shared application state threaded through all handlers.
#[derive(Clone)]
pub struct AppState {
    /// The default dataset store (the `"ds"` entry in the registry).
    /// Kept for backward compatibility with existing `/sparql` and `/rdf-graph-store` routes.
    pub store: Arc<RwLock<Datastore>>,
    /// All named datasets, including `"ds"` which aliases `store`.
    pub registry: Arc<RwLock<DatasetRegistry>>,
    pub config: Config,
    /// Cache for OIDC JWKS (public keys).  Always present; no-op when auth is not OIDC.
    pub jwks_cache: auth::JwksCache,
    /// Durable changelog.  `None` when the server runs in in-memory mode (no `data_dir`).
    pub changelog: Option<Arc<Mutex<QuadChangelog>>>,
    /// Cached VQS productive-extension index (navigation graph + Wld configuration
    /// set), rebuilt lazily whenever the underlying `Datastore` generation changes.
    pub vqs_cache: Arc<RwLock<Option<vqs_routes::VqsCache>>>,
    /// Incremental Datalog reasoner slot for the default (`"ds"`) dataset.
    /// The inner `Option` is `Some` when a ruleset is currently loaded —
    /// initially populated when `Config::initial_rules` is non-empty, and
    /// mutable afterwards via `POST /{dataset}/rules` (see
    /// [#390](https://github.com/daghovland/rdf-datalog/issues/390)), which
    /// is why this is wrapped in an outer `RwLock` rather than a bare
    /// `Option`: a request can flip it from `None` to `Some` (or replace an
    /// existing reasoner's ruleset) after server startup, and that must be
    /// visible to subsequent requests, including this crate's own root
    /// `/sparql` route which reads this field directly rather than through
    /// `DatasetRegistry`.
    ///
    /// This is the *same* cell instance as the `"ds"` entry's reasoner slot
    /// in `AppState.registry` (see `DatasetRegistry::new_with_default`), so
    /// the two never drift out of sync — see
    /// [#457](https://github.com/daghovland/rdf-datalog/issues/457).
    ///
    /// Handlers that mutate the store take a read lock on this cell, then
    /// (if `Some`) lock the inner mutex and call
    /// [`IncrementalReasoner::apply_insertions`] / [`IncrementalReasoner::apply_deletions`]
    /// so that inferred triples are updated after every write.
    ///
    /// Related: [#110](https://github.com/daghovland/rdf-datalog/issues/110)
    pub reasoner: registry::ReasonerCell,
    /// Network access policy for SPARQL `LOAD`, `SERVICE`, and JSON-LD external `@context` URLs.
    ///
    /// Related: [#118](https://github.com/daghovland/rdf-datalog/issues/118)
    pub network_policy: NetworkPolicy,
    /// See [`Config::allow_loopback_for_ssrf_tests`]. Copied from `config` at
    /// startup so handlers don't need to reach into `config` for it.
    pub allow_loopback_for_ssrf_tests: bool,
    /// Open transaction registry for the proprietary BEGIN / COMMIT / ROLLBACK API.
    ///
    /// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
    pub transactions: Arc<Mutex<TransactionRegistry>>,
}

/// Start the SPARQL endpoint server.
///
/// If `config.data_dir` is set, opens a `redb` changelog there, replays it into
/// the initial `Datastore`, and wires all mutating handlers to commit to the
/// changelog before updating the in-memory store.
pub async fn serve(store: Arc<RwLock<Datastore>>, config: Config) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    serve_on_listener(store, config, listener).await
}

/// Start the server on an already-bound listener (useful for tests).
///
/// `store` is the initial in-memory `Datastore`.  When `config.data_dir` is set,
/// the changelog is opened and its contents are replayed **into** `store` before
/// any requests are accepted, layering changelog mutations on top of any data
/// already present (e.g. pre-loaded from files via `--data`).
pub async fn serve_on_listener(
    store: Arc<RwLock<Datastore>>,
    config: Config,
    listener: tokio::net::TcpListener,
) -> Result<(), std::io::Error> {
    // Open the changelog (if configured) and replay it into the existing store.
    // replay_into() layers changelog mutations ON TOP of any pre-loaded data
    // (e.g. from --data files), so both sources are visible.
    // See: https://github.com/daghovland/rdf-datalog/issues/66
    let changelog: Option<Arc<Mutex<QuadChangelog>>> = if let Some(ref dir) = config.data_dir {
        std::fs::create_dir_all(dir).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("cannot create data-dir {}: {e}", dir.display()),
            )
        })?;
        let db_path = dir.join("dagalog.redb");
        let cl = QuadChangelog::open(&db_path).map_err(std::io::Error::other)?;
        cl.replay_into(&mut *store.write().await)
            .map_err(std::io::Error::other)?;
        Some(Arc::new(Mutex::new(cl)))
    } else {
        None
    };

    // Build the incremental reasoner (if rules are configured) BEFORE handing
    // the store to axum.  `IncrementalReasoner::new` runs full initial
    // materialisation, so the store is fully derived before the first request.
    let initial_reasoner: Option<Arc<Mutex<IncrementalReasoner>>> =
        if config.initial_rules.is_empty() {
            None
        } else {
            let rules = config.initial_rules.clone();
            let reasoner =
                IncrementalReasoner::new(rules, &mut *store.write().await).map_err(|e| {
                    std::io::Error::other(format!(
                        "initial rules + data are contradictory, refusing to start: {e}"
                    ))
                })?;
            Some(Arc::new(Mutex::new(reasoner)))
        };
    // Shared, runtime-mutable cell — see the doc comment on `AppState::reasoner`
    // and `registry::ReasonerCell` for why this is `Arc<RwLock<Option<..>>>`
    // rather than a bare `Option`. The same `Arc` is registered under `"ds"`
    // in the `DatasetRegistry` below, so the two stay aliased.
    let reasoner: registry::ReasonerCell = Arc::new(RwLock::new(initial_reasoner));

    let network_policy = config.network_policy.clone();
    let allow_loopback_for_ssrf_tests = config.allow_loopback_for_ssrf_tests;
    let registry = DatasetRegistry::new_with_default(store.clone(), reasoner.clone());
    let state = AppState {
        store,
        registry: Arc::new(RwLock::new(registry)),
        jwks_cache: auth::JwksCache::new(std::time::Duration::from_secs(3600)),
        changelog,
        config,
        vqs_cache: Arc::new(RwLock::new(None)),
        reasoner,
        network_policy,
        allow_loopback_for_ssrf_tests,
        transactions: Arc::new(Mutex::new(TransactionRegistry::new())),
    };
    let app = server::build_router(state);
    axum::serve(listener, app).await
}

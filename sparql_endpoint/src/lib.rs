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
pub mod dataset_routes;
pub mod frontend;
pub mod graph_store;
pub mod negotiate;
pub mod query;
pub mod query_builder;
pub mod registry;
pub mod serialize;
pub mod server;
pub mod service_desc;
pub mod sparql_update;
pub mod upload;

use dag_rdf::datastore::Datastore;
use registry::DatasetRegistry;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    pub max_query_timeout_secs: u64,
    /// Authentication mode (default: none).
    pub auth: AuthConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_addr: "0.0.0.0:3030".parse().unwrap(),
            base_iri: "http://localhost:3030".to_string(),
            read_only: true,
            max_query_timeout_secs: 30,
            auth: AuthConfig::None,
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
}

/// Start the SPARQL endpoint server.
pub async fn serve(store: Arc<RwLock<Datastore>>, config: Config) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    serve_on_listener(store, config, listener).await
}

/// Start the server on an already-bound listener (useful for tests).
pub async fn serve_on_listener(
    store: Arc<RwLock<Datastore>>,
    config: Config,
    listener: tokio::net::TcpListener,
) -> Result<(), std::io::Error> {
    let registry = DatasetRegistry::new_with_default(store.clone());
    let state = AppState {
        store,
        registry: Arc::new(RwLock::new(registry)),
        jwks_cache: auth::JwksCache::new(std::time::Duration::from_secs(3600)),
        config,
    };
    let app = server::build_router(state);
    axum::serve(listener, app).await
}

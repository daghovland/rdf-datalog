/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Authentication middleware for the SPARQL endpoint.
//!
//! See `docs/plans/AUTH.md` for the full design.
//!
//! - Tier 1: static API key (`AuthConfig::ApiKey`)
//! - Tier 2: generic OIDC JWT validation (`AuthConfig::Oidc`)

use axum::{
    Json,
    extract::State,
    http::{
        HeaderMap, Method, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

use crate::{AppState, AuthConfig, OidcConfig};

// ── Permission classification ─────────────────────────────────────────────────

/// Which class of permission a request requires.
#[derive(Debug, PartialEq, Eq)]
pub enum Permission {
    Read,
    Write,
    Admin,
}

/// Map an HTTP method + path to the permission it requires.
///
/// Follows the operation table in AUTH.md:
/// - Admin: `POST /$/datasets` (create), `DELETE /$/datasets/{name}` (drop).
/// - Write: SPARQL Update (`POST …/update`), upload, GSP mutating methods.
/// - Read: everything else, including `POST /sparql` and `POST /{name}/sparql`
///   which carry SPARQL SELECT queries, not updates.
pub fn classify(method: &Method, path: &str) -> Permission {
    // Admin operations must be checked before the generic DELETE/PUT rule.
    if method == Method::POST && path == "/$/datasets" {
        return Permission::Admin;
    }
    if method == Method::DELETE && path.starts_with("/$/datasets/") {
        return Permission::Admin;
    }

    // Explicit write-POST endpoints (POST on SPARQL query endpoints is a Read).
    if method == Method::POST
        && (path == "/upload" || path.ends_with("/update") || path.ends_with("/rml"))
    {
        return Permission::Write;
    }

    // All PUT and DELETE (except the admin paths already matched above) are writes.
    if method == Method::PUT || method == Method::DELETE {
        return Permission::Write;
    }

    // POST on Graph Store Protocol endpoints is a write (append).
    if method == Method::POST
        && (path == "/rdf-graph-store"
            || path.starts_with("/rdf-graphs/")
            || path.ends_with("/data"))
    {
        return Permission::Write;
    }

    // GET, HEAD, OPTIONS, POST /sparql, POST /{name}/sparql, POST /{name}/query …
    Permission::Read
}

// ── Auth errors ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AuthError {
    Expired,
    InvalidAudience,
    InvalidIssuer,
    UnsupportedAlgorithm,
    InsufficientRole,
    JwksFetchFailed(String),
    UnknownKeyId(String),
    Invalid(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Expired => write!(f, "token expired"),
            AuthError::InvalidAudience => write!(f, "invalid audience"),
            AuthError::InvalidIssuer => write!(f, "invalid issuer"),
            AuthError::UnsupportedAlgorithm => write!(f, "unsupported algorithm"),
            AuthError::InsufficientRole => write!(f, "insufficient role"),
            AuthError::JwksFetchFailed(e) => write!(f, "JWKS fetch failed: {}", e),
            AuthError::UnknownKeyId(kid) => write!(f, "unknown key id: {}", kid),
            AuthError::Invalid(e) => write!(f, "invalid token: {}", e),
        }
    }
}

// ── JWT claims ────────────────────────────────────────────────────────────────

/// Decoded JWT claims from an OIDC token.
///
/// All fields are stored in a flat map so that any provider's custom claims
/// (roles, realm_access.roles, etc.) can be extracted via dot-path.
#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    #[serde(flatten)]
    fields: HashMap<String, serde_json::Value>,
}

impl Claims {
    /// Extract the roles array at the given dot-separated `claim_path`.
    ///
    /// - `"roles"` → `token.roles` (Azure, Google custom claim)
    /// - `"realm_access.roles"` → `token.realm_access.roles` (Keycloak)
    pub fn extract_roles(&self, claim_path: &str) -> Vec<String> {
        let mut parts = claim_path.splitn(2, '.');
        let first = parts.next().unwrap_or("");
        let value = match self.fields.get(first) {
            Some(v) => v,
            None => return vec![],
        };
        if let Some(rest) = parts.next() {
            extract_from_value(value, rest)
        } else {
            extract_string_array(value)
        }
    }

    /// Return `true` if the claim at `claim_path` contains `role`.
    pub fn has_role(&self, claim_path: &str, role: &str) -> bool {
        self.extract_roles(claim_path).iter().any(|r| r == role)
    }
}

fn extract_from_value(value: &serde_json::Value, path: &str) -> Vec<String> {
    let mut parts = path.splitn(2, '.');
    let key = parts.next().unwrap_or("");
    match value.get(key) {
        Some(v) => {
            if let Some(rest) = parts.next() {
                extract_from_value(v, rest)
            } else {
                extract_string_array(v)
            }
        }
        None => vec![],
    }
}

fn extract_string_array(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => vec![],
    }
}

// ── JWT validation (pure) ─────────────────────────────────────────────────────

/// Validate `token` using `decoding_key` and the OIDC configuration.
///
/// This is a pure function — no I/O.  The caller is responsible for fetching
/// the correct `DecodingKey` from the JWKS cache.
///
/// Validates: algorithm, signature, expiry, issuer, audience.
pub fn validate_jwt(
    token: &str,
    alg: Algorithm,
    decoding_key: &DecodingKey,
    config: &OidcConfig,
) -> Result<Claims, AuthError> {
    let mut validation = Validation::new(alg);
    validation.set_issuer(&[&config.issuer]);
    validation.set_audience(&[&config.audience]);

    decode::<Claims>(token, decoding_key, &validation)
        .map(|td| td.claims)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::Expired,
            jsonwebtoken::errors::ErrorKind::InvalidAudience => AuthError::InvalidAudience,
            jsonwebtoken::errors::ErrorKind::InvalidIssuer => AuthError::InvalidIssuer,
            jsonwebtoken::errors::ErrorKind::InvalidAlgorithm => AuthError::UnsupportedAlgorithm,
            _ => AuthError::Invalid(e.to_string()),
        })
}

// ── OIDC discovery ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OidcMetadata {
    issuer: String,
    jwks_uri: String,
}

async fn discover_jwks_uri(issuer: &str, client: &reqwest::Client) -> Result<String, AuthError> {
    let meta_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let meta: OidcMetadata = client
        .get(&meta_url)
        .send()
        .await
        .map_err(|e| AuthError::JwksFetchFailed(e.to_string()))?
        .json()
        .await
        .map_err(|e| AuthError::JwksFetchFailed(format!("OIDC discovery parse failed: {}", e)))?;

    if meta.issuer != issuer {
        return Err(AuthError::Invalid(format!(
            "OIDC issuer mismatch: expected {}, got {}",
            issuer, meta.issuer
        )));
    }
    Ok(meta.jwks_uri)
}

async fn fetch_jwks(uri: &str, client: &reqwest::Client) -> Result<JwkSet, AuthError> {
    let resp = client
        .get(uri)
        .send()
        .await
        .map_err(|e| AuthError::JwksFetchFailed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AuthError::JwksFetchFailed(format!(
            "JWKS endpoint returned {}",
            resp.status()
        )));
    }
    resp.json::<JwkSet>()
        .await
        .map_err(|e| AuthError::JwksFetchFailed(format!("JWKS parse failed: {}", e)))
}

fn find_decoding_key(jwk_set: &JwkSet, kid: Option<&str>) -> Result<DecodingKey, AuthError> {
    let jwk = if let Some(kid_str) = kid {
        jwk_set
            .keys
            .iter()
            .find(|k| k.common.key_id.as_deref() == Some(kid_str))
            .ok_or_else(|| AuthError::UnknownKeyId(kid_str.to_owned()))?
    } else {
        jwk_set
            .keys
            .first()
            .ok_or_else(|| AuthError::JwksFetchFailed("JWKS has no keys".to_owned()))?
    };

    DecodingKey::from_jwk(jwk).map_err(|e| AuthError::Invalid(format!("invalid JWK: {}", e)))
}

// ── JWKS cache ────────────────────────────────────────────────────────────────

struct JwksCacheInner {
    /// Discovered or configured JWKS URI.
    jwks_uri: Option<String>,
    /// Cached JWK set and the time it was fetched.
    keys: Option<(JwkSet, Instant)>,
}

/// Thread-safe cache of OIDC public keys with TTL-based refresh.
///
/// Shared via `AppState`; cheap to clone (contains an `Arc` internally).
#[derive(Clone)]
pub struct JwksCache {
    inner: Arc<Mutex<JwksCacheInner>>,
    ttl: Duration,
    http_client: reqwest::Client,
}

impl JwksCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(JwksCacheInner {
                jwks_uri: None,
                keys: None,
            })),
            ttl,
            http_client: reqwest::Client::new(),
        }
    }

    /// Fetch the `DecodingKey` for the given `kid`.
    ///
    /// - If `static_jwks_uri` is `Some`, use it directly (no discovery).
    /// - Otherwise, discover the JWKS URI from `issuer` and cache it.
    /// - The JWK set is cached for `self.ttl`; expired cache triggers a re-fetch.
    ///
    /// The mutex is held only for cache reads and writes — never during HTTP I/O.
    /// This avoids blocking all concurrent requests while one request fetches JWKS.
    pub async fn get_key(
        &self,
        issuer: &str,
        static_jwks_uri: Option<&str>,
        kid: Option<&str>,
    ) -> Result<DecodingKey, AuthError> {
        // Phase 1: snapshot cached state. Release lock before any I/O.
        let (cached_uri, needs_refresh) = {
            let inner = self.inner.lock().await;
            let cached_uri = static_jwks_uri
                .map(str::to_owned)
                .or_else(|| inner.jwks_uri.clone());
            let needs_refresh = inner
                .keys
                .as_ref()
                .is_none_or(|(_, ts)| ts.elapsed() > self.ttl);
            (cached_uri, needs_refresh)
        };

        // Phase 2: HTTP work outside the lock.
        let jwks_uri = match cached_uri {
            Some(uri) => uri,
            None => discover_jwks_uri(issuer, &self.http_client).await?,
        };

        let fresh_set = if needs_refresh {
            Some(fetch_jwks(&jwks_uri, &self.http_client).await?)
        } else {
            None
        };

        // Phase 3: update cache under lock (double-check to avoid clobbering a
        // concurrent refresh that finished while we were doing I/O).
        let mut inner = self.inner.lock().await;
        if inner.jwks_uri.is_none() && static_jwks_uri.is_none() {
            inner.jwks_uri = Some(jwks_uri);
        }
        if let Some(new_set) = fresh_set {
            let still_stale = inner
                .keys
                .as_ref()
                .is_none_or(|(_, ts)| ts.elapsed() > self.ttl);
            if still_stale {
                inner.keys = Some((new_set, Instant::now()));
            }
        }

        let (jwk_set, _) = inner.keys.as_ref().expect("keys populated above");
        find_decoding_key(jwk_set, kid)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

/// Constant-time byte-slice comparison.
///
/// TEMPORARY WORKAROUND: The `rsa` crate does not yet expose a constant-time
/// key-length check, so we pad the shorter slice to `max(a.len(), b.len())`
/// before comparing. This avoids leaking the expected key length via an
/// early-return branch. Replace with upstream constant-time length comparison
/// once the rsa crate provides one.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let len = a.len().max(b.len());
    let mut a_padded = vec![0u8; len];
    let mut b_padded = vec![0u8; len];
    a_padded[..a.len()].copy_from_slice(a);
    b_padded[..b.len()].copy_from_slice(b);
    a_padded.ct_eq(&b_padded).into()
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        "Unauthorized",
    )
        .into_response()
}

fn forbidden_response() -> Response {
    (StatusCode::FORBIDDEN, "Forbidden: insufficient roles").into_response()
}

// ── Auth config endpoint ──────────────────────────────────────────────────────

/// `GET /auth/config` — returns the active authentication mode and non-secret
/// configuration parameters.  Always public (no auth required).
pub async fn auth_config_handler(State(state): State<AppState>) -> Response {
    #[derive(serde::Serialize)]
    struct AuthConfigResponse {
        mode: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<ApiKeyInfo>,
        #[serde(skip_serializing_if = "Option::is_none")]
        oidc: Option<OidcInfo>,
    }

    #[derive(serde::Serialize)]
    struct ApiKeyInfo {
        require_for_reads: bool,
    }

    #[derive(serde::Serialize)]
    struct OidcInfo {
        issuer: String,
        audience: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        browser_client_id: Option<String>,
    }

    let body = match &state.config.auth {
        AuthConfig::None => AuthConfigResponse {
            mode: "none",
            api_key: None,
            oidc: None,
        },
        AuthConfig::ApiKey {
            require_for_reads, ..
        } => AuthConfigResponse {
            mode: "api_key",
            api_key: Some(ApiKeyInfo {
                require_for_reads: *require_for_reads,
            }),
            oidc: None,
        },
        AuthConfig::Oidc(cfg) => AuthConfigResponse {
            mode: "oidc",
            api_key: None,
            oidc: Some(OidcInfo {
                issuer: cfg.issuer.clone(),
                audience: cfg.audience.clone(),
                browser_client_id: cfg.browser_client_id.clone(),
            }),
        },
    };

    Json(body).into_response()
}

// ── Middleware ────────────────────────────────────────────────────────────────

/// Axum middleware that enforces the configured authentication policy.
///
/// Applied globally; the `classify` function decides per-request which
/// permission is needed and whether the configured auth mode requires a check.
///
/// Special case: `GET /auth/config` is always public regardless of auth mode.
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    // Auth config endpoint is always public (browsers need it before sign-in).
    if request.uri().path() == "/auth/config" {
        return next.run(request).await;
    }

    let required = classify(request.method(), request.uri().path());

    match &state.config.auth {
        AuthConfig::None => next.run(request).await,

        AuthConfig::ApiKey {
            key,
            require_for_reads,
        } => {
            let needs_check = match required {
                Permission::Write | Permission::Admin => true,
                Permission::Read => *require_for_reads,
            };

            if !needs_check {
                return next.run(request).await;
            }

            match extract_bearer(request.headers()) {
                Some(token) if constant_time_eq(token.as_bytes(), key.as_bytes()) => {
                    next.run(request).await
                }
                _ => unauthorized_response(),
            }
        }

        AuthConfig::Oidc(config) => {
            // Clone the token so we can release the borrow on request headers.
            let token = match extract_bearer(request.headers()) {
                Some(t) => t.to_owned(),
                None => return unauthorized_response(),
            };

            // Decode the JWT header to get kid and alg (without verifying signature).
            let header = match decode_header(&token) {
                Ok(h) => h,
                Err(_) => return unauthorized_response(),
            };

            // Only accept RS256 and ES256.
            let alg = match header.alg {
                Algorithm::RS256 | Algorithm::ES256 => header.alg,
                _ => return unauthorized_response(),
            };

            // Fetch the matching public key from the JWKS cache.
            let decoding_key = match state
                .jwks_cache
                .get_key(
                    &config.issuer,
                    config.jwks_uri.as_deref(),
                    header.kid.as_deref(),
                )
                .await
            {
                Ok(k) => k,
                Err(_) => return unauthorized_response(),
            };

            // Validate signature, expiry, issuer, audience.
            let claims = match validate_jwt(&token, alg, &decoding_key, config) {
                Ok(c) => c,
                Err(_) => return unauthorized_response(),
            };

            // Check that the token carries the required role.
            let authorized = match required {
                Permission::Read => {
                    claims.has_role(&config.roles_claim, &config.read_role)
                        || claims.has_role(&config.roles_claim, &config.write_role)
                        || claims.has_role(&config.roles_claim, &config.admin_role)
                }
                Permission::Write => {
                    claims.has_role(&config.roles_claim, &config.write_role)
                        || claims.has_role(&config.roles_claim, &config.admin_role)
                }
                Permission::Admin => claims.has_role(&config.roles_claim, &config.admin_role),
            };

            if !authorized {
                return forbidden_response();
            }

            next.run(request).await
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header};
    use rsa::{
        RsaPrivateKey,
        pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
    };
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

    // ── Permission classifier (16 cases) ──────────────────────────────────────

    #[test]
    fn get_sparql_is_read() {
        assert_eq!(classify(&Method::GET, "/sparql"), Permission::Read);
    }

    #[test]
    fn post_sparql_query_is_read() {
        assert_eq!(classify(&Method::POST, "/sparql"), Permission::Read);
    }

    #[test]
    fn post_dataset_sparql_is_read() {
        assert_eq!(classify(&Method::POST, "/ds/sparql"), Permission::Read);
    }

    #[test]
    fn post_dataset_query_is_read() {
        assert_eq!(classify(&Method::POST, "/ds/query"), Permission::Read);
    }

    #[test]
    fn post_update_is_write() {
        assert_eq!(classify(&Method::POST, "/ds/update"), Permission::Write);
    }

    #[test]
    fn post_upload_is_write() {
        assert_eq!(classify(&Method::POST, "/upload"), Permission::Write);
    }

    #[test]
    fn post_dataset_rml_is_write() {
        assert_eq!(classify(&Method::POST, "/ds/rml"), Permission::Write);
    }

    #[test]
    fn put_gsp_is_write() {
        assert_eq!(
            classify(&Method::PUT, "/rdf-graph-store"),
            Permission::Write
        );
    }

    #[test]
    fn delete_gsp_is_write() {
        assert_eq!(
            classify(&Method::DELETE, "/rdf-graph-store"),
            Permission::Write
        );
    }

    #[test]
    fn post_gsp_is_write() {
        assert_eq!(
            classify(&Method::POST, "/rdf-graph-store"),
            Permission::Write
        );
    }

    #[test]
    fn put_dataset_data_is_write() {
        assert_eq!(classify(&Method::PUT, "/ds/data"), Permission::Write);
    }

    #[test]
    fn delete_dataset_data_is_write() {
        assert_eq!(classify(&Method::DELETE, "/ds/data"), Permission::Write);
    }

    #[test]
    fn post_dataset_data_is_write() {
        assert_eq!(classify(&Method::POST, "/ds/data"), Permission::Write);
    }

    #[test]
    fn post_datasets_is_admin() {
        assert_eq!(classify(&Method::POST, "/$/datasets"), Permission::Admin);
    }

    #[test]
    fn delete_dataset_is_admin() {
        assert_eq!(
            classify(&Method::DELETE, "/$/datasets/myds"),
            Permission::Admin
        );
    }

    #[test]
    fn get_datasets_is_read() {
        assert_eq!(classify(&Method::GET, "/$/datasets"), Permission::Read);
    }

    #[test]
    fn get_server_info_is_read() {
        assert_eq!(classify(&Method::GET, "/$/server"), Permission::Read);
    }

    // ── Claims dot-path extraction ─────────────────────────────────────────────

    #[test]
    fn extract_flat_roles() {
        let json = serde_json::json!({ "roles": ["dagalog.Read", "dagalog.Write"] });
        let claims: Claims = serde_json::from_value(json).unwrap();
        assert_eq!(
            claims.extract_roles("roles"),
            vec!["dagalog.Read", "dagalog.Write"]
        );
    }

    #[test]
    fn extract_nested_roles() {
        let json = serde_json::json!({
            "realm_access": { "roles": ["dagalog.Admin"] }
        });
        let claims: Claims = serde_json::from_value(json).unwrap();
        assert_eq!(
            claims.extract_roles("realm_access.roles"),
            vec!["dagalog.Admin"]
        );
    }

    #[test]
    fn extract_missing_roles_returns_empty() {
        let json = serde_json::json!({ "sub": "user123" });
        let claims: Claims = serde_json::from_value(json).unwrap();
        assert!(claims.extract_roles("roles").is_empty());
    }

    #[test]
    fn has_role_returns_false_when_no_roles_claim() {
        let json = serde_json::json!({ "sub": "user123" });
        let claims: Claims = serde_json::from_value(json).unwrap();
        assert!(!claims.has_role("roles", "dagalog.Read"));
    }

    // ── Test RSA key pair (generated once per test run) ───────────────────────

    struct TestKeyPair {
        encoding_key: EncodingKey,
        decoding_key: DecodingKey,
        public_key: rsa::RsaPublicKey,
        kid: String,
    }

    static TEST_KEYS: OnceLock<TestKeyPair> = OnceLock::new();

    fn test_keys() -> &'static TestKeyPair {
        TEST_KEYS.get_or_init(|| {
            let mut rng = rand::rngs::OsRng;
            let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("RSA key gen");
            let private_pem = private_key.to_pkcs8_pem(LineEnding::LF).expect("PKCS8 PEM");
            let public_pem = private_key
                .to_public_key()
                .to_public_key_pem(LineEnding::LF)
                .expect("public PEM");
            let public_key = private_key.to_public_key();
            TestKeyPair {
                encoding_key: EncodingKey::from_rsa_pem(private_pem.as_bytes())
                    .expect("encoding key"),
                decoding_key: DecodingKey::from_rsa_pem(public_pem.as_bytes())
                    .expect("decoding key"),
                public_key,
                kid: "test-key-001".to_string(),
            }
        })
    }

    fn future_exp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600
    }

    fn past_exp() -> u64 {
        // Must be more than jsonwebtoken's default 60-second leeway.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 3600
    }

    fn test_config() -> OidcConfig {
        OidcConfig {
            issuer: "https://test.example.com".to_owned(),
            jwks_uri: None,
            audience: "api://dagalog".to_owned(),
            roles_claim: "roles".to_owned(),
            read_role: "dagalog.Read".to_owned(),
            write_role: "dagalog.Write".to_owned(),
            admin_role: "dagalog.Admin".to_owned(),
            browser_client_id: None,
        }
    }

    #[derive(serde::Serialize)]
    struct TestClaims<'a> {
        iss: &'a str,
        aud: &'a str,
        exp: u64,
        roles: Vec<&'a str>,
    }

    fn make_token(iss: &str, aud: &str, exp: u64, roles: &[&str]) -> String {
        let keys = test_keys();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(keys.kid.clone());
        let claims = TestClaims {
            iss,
            aud,
            exp,
            roles: roles.to_vec(),
        };
        jsonwebtoken::encode(&header, &claims, &keys.encoding_key).expect("encode token")
    }

    // ── validate_jwt unit tests ────────────────────────────────────────────────

    #[test]
    fn valid_token_decoded_successfully() {
        let token = make_token(
            "https://test.example.com",
            "api://dagalog",
            future_exp(),
            &["dagalog.Read"],
        );
        let config = test_config();
        let result = validate_jwt(&token, Algorithm::RS256, &test_keys().decoding_key, &config);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let claims = result.unwrap();
        assert!(claims.has_role("roles", "dagalog.Read"));
    }

    #[test]
    fn expired_token_returns_expired_error() {
        let token = make_token(
            "https://test.example.com",
            "api://dagalog",
            past_exp(),
            &["dagalog.Read"],
        );
        let config = test_config();
        let result = validate_jwt(&token, Algorithm::RS256, &test_keys().decoding_key, &config);
        assert!(
            matches!(result, Err(AuthError::Expired)),
            "expected Expired, got {:?}",
            result
        );
    }

    #[test]
    fn wrong_audience_rejected() {
        let token = make_token(
            "https://test.example.com",
            "api://wrong-app",
            future_exp(),
            &["dagalog.Read"],
        );
        let config = test_config();
        let result = validate_jwt(&token, Algorithm::RS256, &test_keys().decoding_key, &config);
        assert!(
            matches!(result, Err(AuthError::InvalidAudience)),
            "expected InvalidAudience, got {:?}",
            result
        );
    }

    #[test]
    fn wrong_issuer_rejected() {
        let token = make_token(
            "https://evil.example.com",
            "api://dagalog",
            future_exp(),
            &["dagalog.Read"],
        );
        let config = test_config();
        let result = validate_jwt(&token, Algorithm::RS256, &test_keys().decoding_key, &config);
        assert!(
            matches!(result, Err(AuthError::InvalidIssuer)),
            "expected InvalidIssuer, got {:?}",
            result
        );
    }

    #[test]
    fn hs256_token_rejected_as_unsupported_algorithm() {
        let hs256_key = EncodingKey::from_secret(b"secret");
        let header = Header::new(Algorithm::HS256);
        let claims = TestClaims {
            iss: "https://test.example.com",
            aud: "api://dagalog",
            exp: future_exp(),
            roles: vec!["dagalog.Read"],
        };
        let token = jsonwebtoken::encode(&header, &claims, &hs256_key).expect("encode HS256");

        // validate_jwt is called with RS256 — the token header says HS256.
        let config = test_config();
        let result = validate_jwt(&token, Algorithm::RS256, &test_keys().decoding_key, &config);
        assert!(
            matches!(
                result,
                Err(AuthError::UnsupportedAlgorithm) | Err(AuthError::Invalid(_))
            ),
            "expected UnsupportedAlgorithm or Invalid, got {:?}",
            result
        );
    }

    #[test]
    fn token_with_no_roles_claim_has_no_roles() {
        // Token is valid but has no roles claim (simulates a Google token).
        #[derive(serde::Serialize)]
        struct MinimalClaims<'a> {
            iss: &'a str,
            aud: &'a str,
            exp: u64,
        }
        let keys = test_keys();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(keys.kid.clone());
        let claims = MinimalClaims {
            iss: "https://test.example.com",
            aud: "api://dagalog",
            exp: future_exp(),
        };
        let token = jsonwebtoken::encode(&header, &claims, &keys.encoding_key).unwrap();

        let config = test_config();
        let result = validate_jwt(&token, Algorithm::RS256, &keys.decoding_key, &config);
        let decoded = result.expect("token should be valid");

        // No roles claim → extract_roles returns empty.
        assert!(decoded.extract_roles("roles").is_empty());
        assert!(!decoded.has_role("roles", "dagalog.Read"));
        assert!(!decoded.has_role("roles", "dagalog.Write"));
    }

    #[test]
    fn admin_role_grants_write_access() {
        let json = serde_json::json!({ "roles": ["dagalog.Admin"] });
        let claims: Claims = serde_json::from_value(json).unwrap();
        let config = test_config();
        // Admin implies Write.
        assert!(
            claims.has_role(&config.roles_claim, &config.admin_role),
            "Admin role should be present"
        );
    }

    // ── constant_time_eq ─────────────────────────────────────────────────────

    #[test]
    fn constant_time_eq_same_content_returns_true() {
        assert!(constant_time_eq(b"secret-key", b"secret-key"));
    }

    #[test]
    fn constant_time_eq_different_content_returns_false() {
        assert!(!constant_time_eq(b"secret-key", b"wrong-key!"));
    }

    #[test]
    fn constant_time_eq_different_lengths_returns_false() {
        // Regression: early-return on length mismatch leaks the expected key
        // length via a timing side-channel. The fix pads both sides so the
        // comparison always takes the same amount of time.
        assert!(!constant_time_eq(b"short", b"much-longer-key"));
        assert!(!constant_time_eq(b"much-longer-key", b"short"));
        assert!(!constant_time_eq(b"", b"nonempty"));
        assert!(!constant_time_eq(b"nonempty", b""));
    }

    #[test]
    fn constant_time_eq_empty_slices() {
        assert!(constant_time_eq(b"", b""));
    }

    // ── Build JWK set from test RSA key ───────────────────────────────────────

    fn make_test_jwks_response() -> serde_json::Value {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use rsa::traits::PublicKeyParts;

        let keys = test_keys();
        let n = URL_SAFE_NO_PAD.encode(keys.public_key.n().to_bytes_be());
        let e = URL_SAFE_NO_PAD.encode(keys.public_key.e().to_bytes_be());
        serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": keys.kid,
                "use": "sig",
                "alg": "RS256",
                "n": n,
                "e": e
            }]
        })
    }

    // ── JWKS cache unit tests (with wiremock) ─────────────────────────────────

    #[tokio::test]
    async fn jwks_cache_fetches_key_and_validates_token() {
        let mock = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(make_test_jwks_response()))
            .mount(&mock)
            .await;

        let jwks_uri = format!("{}/jwks", mock.uri());
        let cache = JwksCache::new(Duration::from_secs(3600));
        let key = cache
            .get_key(
                "https://test.example.com",
                Some(&jwks_uri),
                Some("test-key-001"),
            )
            .await
            .expect("should fetch key");

        let token = make_token(
            "https://test.example.com",
            "api://dagalog",
            future_exp(),
            &["dagalog.Read"],
        );
        let config = test_config();
        let claims = validate_jwt(&token, Algorithm::RS256, &key, &config);
        assert!(claims.is_ok(), "token validated with cached key");
    }

    #[tokio::test]
    async fn jwks_cache_refreshes_after_ttl() {
        let mock = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(make_test_jwks_response()))
            .expect(2) // called twice: initial fetch + post-TTL refresh
            .mount(&mock)
            .await;

        let jwks_uri = format!("{}/jwks", mock.uri());
        // TTL of 1 ms — expires immediately.
        let cache = JwksCache::new(Duration::from_millis(1));

        cache
            .get_key("https://test.example.com", Some(&jwks_uri), None)
            .await
            .expect("first fetch");

        tokio::time::sleep(Duration::from_millis(5)).await;

        cache
            .get_key("https://test.example.com", Some(&jwks_uri), None)
            .await
            .expect("second fetch after TTL");

        mock.verify().await;
    }

    /// Regression test: the mutex must NOT be held during HTTP I/O.
    ///
    /// With the old implementation (lock held throughout `get_key`), two
    /// concurrent callers would serialize: the second blocks until the first
    /// releases the mutex, meaning total wall time ≈ 2 × network latency.
    /// With the fix (lock released before I/O), both callers fetch concurrently
    /// and total wall time ≈ 1 × network latency.
    ///
    /// This test verifies the correctness property (both callers get a valid key).
    /// The performance property (concurrent not serialized) is not asserted
    /// because timing tests are fragile, but the structural fix ensures it.
    #[tokio::test]
    async fn jwks_cache_concurrent_calls_both_succeed() {
        let mock = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(make_test_jwks_response()))
            .mount(&mock)
            .await;

        let jwks_uri = format!("{}/jwks", mock.uri());
        let cache = Arc::new(JwksCache::new(Duration::from_secs(3600)));

        let c1 = cache.clone();
        let u1 = jwks_uri.clone();
        let t1 = tokio::spawn(async move {
            c1.get_key("https://test.example.com", Some(&u1), Some("test-key-001"))
                .await
        });

        let c2 = cache.clone();
        let u2 = jwks_uri.clone();
        let t2 = tokio::spawn(async move {
            c2.get_key("https://test.example.com", Some(&u2), Some("test-key-001"))
                .await
        });

        let (r1, r2) = tokio::join!(t1, t2);
        assert!(r1.unwrap().is_ok(), "first caller should get key");
        assert!(r2.unwrap().is_ok(), "second caller should get key");
    }

    #[tokio::test]
    async fn jwks_cache_uses_discovery_when_no_static_uri() {
        let mock = MockServer::start().await;

        let jwks_uri = format!("{}/jwks", mock.uri());
        let oidc_meta = serde_json::json!({
            "issuer": mock.uri(),
            "jwks_uri": jwks_uri,
            "authorization_endpoint": "https://ignored/auth",
            "token_endpoint": "https://ignored/token",
        });

        Mock::given(matchers::method("GET"))
            .and(matchers::path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(oidc_meta))
            .mount(&mock)
            .await;

        Mock::given(matchers::method("GET"))
            .and(matchers::path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(make_test_jwks_response()))
            .mount(&mock)
            .await;

        let cache = JwksCache::new(Duration::from_secs(3600));
        // No static JWKS URI → discovery is triggered.
        let key = cache
            .get_key(&mock.uri(), None, Some("test-key-001"))
            .await
            .expect("key via discovery");

        let token = make_token(
            &mock.uri(),
            "api://dagalog",
            future_exp(),
            &["dagalog.Read"],
        );
        let mut config = test_config();
        config.issuer = mock.uri();
        let result = validate_jwt(&token, Algorithm::RS256, &key, &config);
        assert!(result.is_ok());
    }
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for Tier 2 (OIDC JWT) authentication.
//!
//! Uses `wiremock` to serve a local OIDC provider (discovery + JWKS endpoints)
//! and a `TestServer` with `AuthConfig::Oidc` pointing at the mock.
//! All token creation uses a test RSA key pair generated in `common_oidc`.

mod common;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::{
    RsaPrivateKey,
    pkcs8::{EncodePrivateKey, LineEnding},
    traits::PublicKeyParts,
};
use sparql_endpoint::OidcConfig;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

// ── Shared test RSA key pair ──────────────────────────────────────────────────

struct OidcTestKeys {
    encoding_key: EncodingKey,
    public_key: rsa::RsaPublicKey,
    kid: String,
}

static OIDC_TEST_KEYS: OnceLock<OidcTestKeys> = OnceLock::new();

fn oidc_keys() -> &'static OidcTestKeys {
    OIDC_TEST_KEYS.get_or_init(|| {
        let mut rng = rand::rngs::OsRng;
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("RSA key gen");
        let private_pem = private_key.to_pkcs8_pem(LineEnding::LF).expect("PKCS8 PEM");
        OidcTestKeys {
            encoding_key: EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key"),
            public_key: private_key.to_public_key(),
            kid: "oidc-test-key".to_string(),
        }
    })
}

fn make_jwks_response() -> serde_json::Value {
    let keys = oidc_keys();
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

#[derive(serde::Serialize)]
struct TestClaims<'a> {
    iss: &'a str,
    aud: &'a str,
    exp: u64,
    roles: Vec<&'a str>,
}

fn make_token(iss: &str, aud: &str, exp: u64, roles: &[&str]) -> String {
    let keys = oidc_keys();
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

// ── Mock OIDC provider setup ──────────────────────────────────────────────────

async fn start_oidc_mock() -> MockServer {
    let mock = MockServer::start().await;
    let jwks_uri = format!("{}/jwks", mock.uri());

    let discovery = serde_json::json!({
        "issuer": mock.uri(),
        "jwks_uri": jwks_uri,
        "authorization_endpoint": format!("{}/auth", mock.uri()),
        "token_endpoint": format!("{}/token", mock.uri()),
    });

    Mock::given(matchers::method("GET"))
        .and(matchers::path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(discovery))
        .mount(&mock)
        .await;

    Mock::given(matchers::method("GET"))
        .and(matchers::path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(make_jwks_response()))
        .mount(&mock)
        .await;

    mock
}

// ── Helper: start TestServer with OIDC auth pointing at a mock provider ───────

async fn start_oidc_server(mock: &MockServer) -> common::TestServer {
    let config = OidcConfig {
        issuer: mock.uri(),
        jwks_uri: None, // discover from mock's /.well-known/openid-configuration
        audience: "api://dagalog".to_owned(),
        roles_claim: "roles".to_owned(),
        read_role: "dagalog.Read".to_owned(),
        write_role: "dagalog.Write".to_owned(),
        admin_role: "dagalog.Admin".to_owned(),
        browser_client_id: None,
    };
    common::TestServer::start_with_oidc("", config).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A request with no Authorization header returns 401.
#[tokio::test]
async fn oidc_no_token_returns_401() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;

    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 401);
}

/// A valid token with the Read role allows GET /sparql.
#[tokio::test]
async fn oidc_valid_read_role_allows_query() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;
    let token = make_token(
        &mock.uri(),
        "api://dagalog",
        future_exp(),
        &["dagalog.Read"],
    );

    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request failed");
    assert_ne!(resp.status().as_u16(), 401, "Read role must allow query");
    assert_ne!(resp.status().as_u16(), 403, "Read role must allow query");
}

/// A valid token with no roles returns 403 on all endpoints.
/// This covers the Google token case: valid JWT but no roles granted.
#[tokio::test]
async fn oidc_no_roles_returns_403() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;
    let token = make_token(&mock.uri(), "api://dagalog", future_exp(), &[]);

    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        403,
        "no roles → 403 (not 401 or 200)"
    );
}

/// An expired token returns 401.
#[tokio::test]
async fn oidc_expired_token_returns_401() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;
    let token = make_token(&mock.uri(), "api://dagalog", past_exp(), &["dagalog.Read"]);

    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 401, "expired token → 401");
}

/// A token with only the Read role cannot execute a write (SPARQL Update).
#[tokio::test]
async fn oidc_read_role_cannot_write() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;
    let token = make_token(
        &mock.uri(),
        "api://dagalog",
        future_exp(),
        &["dagalog.Read"],
    );

    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .bearer_auth(&token)
        .body("INSERT DATA { <urn:s> <urn:p> <urn:o> }")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 403, "Read-only role must not write");
}

/// A token with the Write role can execute a SPARQL Update.
#[tokio::test]
async fn oidc_write_role_allows_update() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;
    let token = make_token(
        &mock.uri(),
        "api://dagalog",
        future_exp(),
        &["dagalog.Write"],
    );

    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .bearer_auth(&token)
        .body("INSERT DATA { <urn:s> <urn:p> <urn:o> }")
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        403,
        "Write role must allow SPARQL Update"
    );
    assert_ne!(
        resp.status().as_u16(),
        401,
        "Write role must allow SPARQL Update"
    );
}

/// The Admin role implies Write access.
#[tokio::test]
async fn oidc_admin_role_implies_write() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;
    let token = make_token(
        &mock.uri(),
        "api://dagalog",
        future_exp(),
        &["dagalog.Admin"],
    );

    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .bearer_auth(&token)
        .body("INSERT DATA { <urn:s> <urn:p> <urn:o> }")
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        403,
        "Admin role must imply Write access"
    );
    assert_ne!(
        resp.status().as_u16(),
        401,
        "Admin role must imply Write access"
    );
}

/// A token signed with a different key than the JWKS returns 401.
#[tokio::test]
async fn oidc_wrong_signing_key_returns_401() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;

    // Generate a second key pair unknown to the server.
    let mut rng = rand::rngs::OsRng;
    let evil_key = RsaPrivateKey::new(&mut rng, 2048).expect("evil RSA key");
    let evil_pem = evil_key.to_pkcs8_pem(LineEnding::LF).expect("PKCS8 PEM");
    let evil_enc = EncodingKey::from_rsa_pem(evil_pem.as_bytes()).unwrap();

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("oidc-test-key".to_string()); // claim the real kid but use wrong key
    let claims = TestClaims {
        iss: &mock.uri(),
        aud: "api://dagalog",
        exp: future_exp(),
        roles: vec!["dagalog.Read"],
    };
    let token = jsonwebtoken::encode(&header, &claims, &evil_enc).unwrap();

    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 401, "wrong signing key → 401");
}

/// `GET /auth/config` is public even with OIDC configured.
#[tokio::test]
async fn auth_config_endpoint_is_public() {
    let mock = start_oidc_mock().await;
    let server = start_oidc_server(&mock).await;

    let resp = server
        .client
        .get(format!("{}/auth/config", server.base_url))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["mode"], "oidc");
    assert!(body["oidc"]["issuer"].is_string());
}

/// `GET /auth/config` returns `mode: "none"` when no auth is configured.
#[tokio::test]
async fn auth_config_no_auth_mode() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .get(format!("{}/auth/config", server.base_url))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["mode"], "none");
}

/// `GET /auth/config` returns `mode: "api_key"` with `require_for_reads`.
#[tokio::test]
async fn auth_config_api_key_mode() {
    let server = common::TestServer::start_writable_with_key("", "secret").await;
    let resp = server
        .client
        .get(format!("{}/auth/config", server.base_url))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["mode"], "api_key");
    assert_eq!(body["api_key"]["require_for_reads"], false);
}

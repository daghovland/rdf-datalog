/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for Tier 3 (Azure Managed Identity, service-to-service).
//!
//! Tier 3-incoming reuses Tier 2's generic OIDC validation unmodified (see
//! `docs/plans/AUTH.md` §Tier 3) — there is no separate "Managed Identity"
//! code path. What's genuinely Tier-3-specific, and what these tests pin,
//! is the token *shape* a Managed Identity actually produces:
//!
//! - It carries extra claims a delegated user token wouldn't (`idtyp`,
//!   `appid`, `oid`, no `upn`/`unique_name`) — proving the flattened `Claims`
//!   map tolerates them without special-casing.
//! - Its `iss` claim format (v1.0 `sts.windows.net` vs v2.0
//!   `login.microsoftonline.com/.../v2.0`) depends on the *resource* app's
//!   manifest, not the caller — the classic IMDS flow issues v1.0 unless
//!   `accessTokenAcceptedVersion: 2` is set. A server configured with
//!   `OidcConfig::azure()` (v2.0) must reject a v1.0-issuer token, and must
//!   accept it once configured for the v1.0 issuer instead.
//! - A structurally valid token with no assigned app role must 403, not
//!   silently pass or 401 — this is the "forgot to assign the role via
//!   Microsoft Graph" failure mode (Managed Identities have no App
//!   registration entry, so the usual portal role-assignment UI doesn't
//!   apply to them).

mod common;

use jsonwebtoken::{Algorithm, Header};
use sparql_endpoint::OidcConfig;
use std::time::{SystemTime, UNIX_EPOCH};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

const TEST_TENANT: &str = "test-tenant-id";
const AUDIENCE: &str = "api://dagalog";

fn v2_issuer() -> String {
    format!("https://login.microsoftonline.com/{TEST_TENANT}/v2.0")
}

fn v1_issuer() -> String {
    format!("https://sts.windows.net/{TEST_TENANT}/")
}

fn future_exp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600
}

/// Claims shaped like a real Azure Managed Identity service-to-service
/// token: `idtyp: "app"` and `appid` mark it as an application (not
/// delegated-user) token, and there is no `upn`/`unique_name`/`name` — all
/// present on interactive user tokens but absent from Managed Identity ones.
#[derive(serde::Serialize)]
struct MiClaims<'a> {
    iss: &'a str,
    aud: &'a str,
    exp: u64,
    roles: Vec<&'a str>,
    idtyp: &'a str,
    appid: &'a str,
    oid: &'a str,
}

fn make_mi_token(iss: &str, aud: &str, exp: u64, roles: &[&str]) -> String {
    let keys = common::oidc_test_keys();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(keys.kid.clone());
    let claims = MiClaims {
        iss,
        aud,
        exp,
        roles: roles.to_vec(),
        idtyp: "app",
        appid: "22222222-3333-4444-5555-666666666666",
        oid: "33333333-4444-5555-6666-777777777777",
    };
    jsonwebtoken::encode(&header, &claims, &keys.encoding_key).expect("encode token")
}

/// Mock server exposing only a JWKS endpoint (no discovery route) — matches
/// how these tests set `OidcConfig.jwks_uri` explicitly, which skips OIDC
/// discovery entirely (`discover_jwks_uri` is never called).
async fn start_jwks_mock() -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::shared_jwks_response()))
        .mount(&mock)
        .await;
    mock
}

fn oidc_config_with_issuer(issuer: &str, jwks_uri: &str) -> OidcConfig {
    OidcConfig {
        issuer: issuer.to_owned(),
        jwks_uri: Some(jwks_uri.to_owned()),
        audience: AUDIENCE.to_owned(),
        roles_claim: "roles".to_owned(),
        read_role: "dagalog.Read".to_owned(),
        write_role: "dagalog.Write".to_owned(),
        admin_role: "dagalog.Admin".to_owned(),
        browser_client_id: None,
    }
}

/// A Managed Identity token with the v2.0 issuer (i.e. the resource app's
/// manifest has `accessTokenAcceptedVersion: 2`) is accepted by a server
/// configured via `OidcConfig::azure()`.
#[tokio::test]
async fn entra_mi_token_with_v2_issuer_and_role_is_accepted() {
    let mock = start_jwks_mock().await;
    let jwks_uri = format!("{}/jwks", mock.uri());
    let mut config = OidcConfig::azure(TEST_TENANT, AUDIENCE);
    config.jwks_uri = Some(jwks_uri);
    let server = common::TestServer::start_with_oidc("", config).await;

    let token = make_mi_token(&v2_issuer(), AUDIENCE, future_exp(), &["dagalog.Write"]);
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
        401,
        "v2.0-issuer MI token must authenticate against OidcConfig::azure()"
    );
    assert_ne!(
        resp.status().as_u16(),
        403,
        "dagalog.Write role must authorize the update"
    );
}

/// A Managed Identity token with the *v1.0* issuer (what classic IMDS
/// actually issues absent the manifest change) is rejected by a server
/// configured with `OidcConfig::azure()`'s v2.0 issuer. This is the
/// documented Tier 3 footgun, made executable.
#[tokio::test]
async fn entra_mi_token_with_v1_issuer_is_rejected_by_v2_config() {
    let mock = start_jwks_mock().await;
    let jwks_uri = format!("{}/jwks", mock.uri());
    let mut config = OidcConfig::azure(TEST_TENANT, AUDIENCE);
    config.jwks_uri = Some(jwks_uri);
    let server = common::TestServer::start_with_oidc("", config).await;

    let token = make_mi_token(&v1_issuer(), AUDIENCE, future_exp(), &["dagalog.Read"]);
    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "v1.0-issuer MI token must be rejected by a v2.0-configured server"
    );
}

/// The documented workaround: configuring `--oidc-issuer` for the v1.0
/// form accepts the classic-IMDS token shape.
#[tokio::test]
async fn entra_mi_token_with_v1_issuer_accepted_when_configured_for_v1() {
    let mock = start_jwks_mock().await;
    let jwks_uri = format!("{}/jwks", mock.uri());
    let config = oidc_config_with_issuer(&v1_issuer(), &jwks_uri);
    let server = common::TestServer::start_with_oidc("", config).await;

    let token = make_mi_token(&v1_issuer(), AUDIENCE, future_exp(), &["dagalog.Read"]);
    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        401,
        "v1.0-issuer MI token must authenticate when the server is configured for the v1.0 issuer"
    );
    assert_ne!(resp.status().as_u16(), 403);
}

/// A structurally valid Managed Identity token with no assigned app role
/// (the "forgot to run the Graph app-role-assignment call" failure mode,
/// since MIs have no App registration entry for the usual portal UI) 403s
/// rather than silently passing or 401ing.
#[tokio::test]
async fn entra_mi_token_without_roles_claim_returns_403() {
    let mock = start_jwks_mock().await;
    let jwks_uri = format!("{}/jwks", mock.uri());
    let mut config = OidcConfig::azure(TEST_TENANT, AUDIENCE);
    config.jwks_uri = Some(jwks_uri);
    let server = common::TestServer::start_with_oidc("", config).await;

    let token = make_mi_token(&v2_issuer(), AUDIENCE, future_exp(), &[]);
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
        "valid-but-roleless MI token must 403, not 401 or 200"
    );
}

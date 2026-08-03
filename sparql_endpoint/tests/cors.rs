/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! CORS tests for state-changing vs. safe HTTP methods.
//!
//! With the documented default configuration (`AuthConfig::None`, no explicit
//! CORS allow-list), a browser page on a different origin must not be able to
//! obtain a CORS preflight approval for state-changing requests (`POST`,
//! `PUT`, `DELETE`) against a dagalog instance — that would let any web page
//! the victim's browser visits issue blind cross-origin writes against an
//! unauthenticated, reachable instance (LAN, container network, or via SSRF
//! from another app). Read-only cross-origin `GET` remains permissive since
//! `allow_credentials` is never set (no cookies/session leak) and this is a
//! legitimate use case (a web UI hosted on a different origin querying the
//! endpoint).
//!
//! Related: <https://github.com/daghovland/rdf-datalog/issues/362>

mod common;

use common::TestServer;

const EVIL_ORIGIN: &str = "https://evil.example";
const TRUSTED_ORIGIN: &str = "https://trusted-ui.example";

/// Send a CORS preflight (`OPTIONS` with `Origin` + `Access-Control-Request-Method`)
/// and return the `Access-Control-Allow-Origin` response header, if any.
async fn preflight(server: &TestServer, url: &str, origin: &str, method: &str) -> Option<String> {
    let resp = server
        .client
        .request(reqwest::Method::OPTIONS, url)
        .header("Origin", origin)
        .header("Access-Control-Request-Method", method)
        .send()
        .await
        .expect("preflight request failed");
    resp.headers()
        .get("access-control-allow-origin")
        .map(|v| v.to_str().unwrap().to_string())
}

/// A cross-origin preflight for `PUT /{name}/data` (Graph Store Protocol write)
/// must not be CORS-approved when no allow-list is configured.
#[tokio::test]
async fn cross_origin_preflight_rejected_for_gsp_put_by_default() {
    let server = TestServer::start_writable("").await;
    let url = server.dataset_data_default_url("ds");
    let allow_origin = preflight(&server, &url, EVIL_ORIGIN, "PUT").await;
    assert_eq!(
        allow_origin, None,
        "expected no Access-Control-Allow-Origin header for a cross-origin PUT preflight"
    );
}

/// A cross-origin preflight for `POST /{name}/update` (SPARQL Update) must not
/// be CORS-approved when no allow-list is configured.
#[tokio::test]
async fn cross_origin_preflight_rejected_for_sparql_update_by_default() {
    let server = TestServer::start_writable("").await;
    let url = server.dataset_update_url("ds");
    let allow_origin = preflight(&server, &url, EVIL_ORIGIN, "POST").await;
    assert_eq!(
        allow_origin, None,
        "expected no Access-Control-Allow-Origin header for a cross-origin POST preflight"
    );
}

/// A cross-origin preflight for `DELETE /$/datasets/{name}` (admin API) must
/// not be CORS-approved when no allow-list is configured.
#[tokio::test]
async fn cross_origin_preflight_rejected_for_admin_delete_by_default() {
    let server = TestServer::start_writable("").await;
    let url = server.admin_dataset_url("ds");
    let allow_origin = preflight(&server, &url, EVIL_ORIGIN, "DELETE").await;
    assert_eq!(
        allow_origin, None,
        "expected no Access-Control-Allow-Origin header for a cross-origin DELETE preflight"
    );
}

/// When the request's origin is present in the explicit allow-list, a
/// state-changing cross-origin preflight is approved and the origin is
/// echoed back (required for the browser to accept a non-`*` response when
/// credentials could later be added).
#[tokio::test]
async fn cross_origin_preflight_allowed_for_allowlisted_origin() {
    let server =
        TestServer::start_writable_with_cors_allowed_origins("", vec![TRUSTED_ORIGIN.to_string()])
            .await;
    let url = server.dataset_update_url("ds");
    let allow_origin = preflight(&server, &url, TRUSTED_ORIGIN, "POST").await;
    assert_eq!(allow_origin.as_deref(), Some(TRUSTED_ORIGIN));
}

/// An origin *not* in the allow-list is still rejected, even when the
/// allow-list is non-empty (i.e. this isn't a global bypass once any origin
/// is configured).
#[tokio::test]
async fn cross_origin_preflight_rejected_for_non_allowlisted_origin() {
    let server =
        TestServer::start_writable_with_cors_allowed_origins("", vec![TRUSTED_ORIGIN.to_string()])
            .await;
    let url = server.dataset_update_url("ds");
    let allow_origin = preflight(&server, &url, EVIL_ORIGIN, "POST").await;
    assert_eq!(allow_origin, None);
}

/// Cross-origin `GET /sparql` (safe, read-only, no credentials sent) remains
/// permissive by default — this preserves the legitimate use case of a web UI
/// hosted on a different origin querying the endpoint.
///
/// The origin is mirrored back rather than a literal `*`, since safe-method
/// approval is now decided per-request (predicate-based) alongside the
/// state-changing-method restriction on the same `CorsLayer` — but this is
/// equivalent in practice: `allow_credentials` is never set, so mirroring the
/// origin carries the same (lack of) risk as a wildcard.
#[tokio::test]
async fn cross_origin_get_still_permissive_by_default() {
    let server = TestServer::start("").await;
    let url = server.sparql_url();
    let allow_origin = preflight(&server, &url, EVIL_ORIGIN, "GET").await;
    assert_eq!(allow_origin.as_deref(), Some(EVIL_ORIGIN));
}

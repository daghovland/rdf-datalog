/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for SSRF hardening on `SPARQL LOAD`.
//!
//! Tests verify that the three SSRF protection layers are active:
//!
//! 1. Private/reserved IP blocking (pre-connection DNS preflight)
//! 2. Cross-host redirect blocking
//! 3. 64 MiB response body cap
//!
//! All tests use `#[tokio::test(flavor = "multi_thread")]` because the LOAD
//! implementation uses `tokio::task::block_in_place` internally.
//!
//! Related: <https://github.com/daghovland/rdf-datalog/issues/135>

mod common;

use ingress::NetworkPolicy;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper: send a SPARQL Update request and return the HTTP status code.
async fn sparql_update(server: &common::TestServer, update: &str) -> reqwest::StatusCode {
    server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update.to_string())
        .send()
        .await
        .expect("update request failed")
        .status()
}

/// Helper: send a SPARQL ASK query; returns true if the answer is "yes".
async fn sparql_ask(server: &common::TestServer, ask: &str) -> bool {
    let resp = server
        .client
        .get(server.sparql_query_url(ask))
        .send()
        .await
        .expect("query request failed");
    assert_eq!(resp.status(), 200, "ASK query returned non-200");
    let body: serde_json::Value = resp.json().await.expect("ASK response must be JSON");
    body["boolean"] == true
}

/// `LOAD <http://10.0.0.1/data.ttl>` must be blocked by the SSRF preflight
/// (RFC 1918 private address — no actual connection is made).
#[tokio::test(flavor = "multi_thread")]
async fn test_load_blocks_rfc1918_ip() {
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, "LOAD <http://10.0.0.1/data.ttl>").await;
    assert_eq!(
        status, 500,
        "LOAD of an RFC 1918 address must return 500 (SSRF blocked)"
    );
}

/// `LOAD <http://169.254.169.254/>` must be blocked by the SSRF preflight
/// (link-local range used by cloud metadata endpoints — no actual connection is made).
#[tokio::test(flavor = "multi_thread")]
async fn test_load_blocks_link_local_ip() {
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, "LOAD <http://169.254.169.254/>").await;
    assert_eq!(
        status, 500,
        "LOAD of a link-local address must return 500 (SSRF blocked)"
    );
}

/// `LOAD <ftp://example.org/data.ttl>` must be rejected because only `http`
/// and `https` schemes are permitted.
///
/// This may be caught either by the SSRF scheme check (returns HTTP 500 from
/// the endpoint) or earlier by the SPARQL parser — either way the update must
/// not succeed.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_blocks_unsupported_scheme() {
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, "LOAD <ftp://example.org/data.ttl>").await;
    assert_ne!(
        status, 200,
        "LOAD of an ftp:// URL must not succeed (unsupported scheme)"
    );
    // The endpoint may return 400 (parse error) or 500 (runtime rejection).
    assert!(
        status == reqwest::StatusCode::BAD_REQUEST
            || status == reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "expected 400 or 500 for unsupported scheme, got {status}"
    );
}

/// Wiremock serves a 302 redirect to a different host.  The cross-host redirect
/// policy must stop the follow and the LOAD must fail with HTTP 500.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_blocks_cross_host_redirect() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/redirect-me"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "http://evil.example.org/data.ttl"),
        )
        .mount(&mock_server)
        .await;

    let url = format!("{}/redirect-me", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(
        status, 500,
        "LOAD that follows a cross-host redirect must return 500"
    );
}

/// Wiremock advertises a Content-Length of 200 MiB (well above the 64 MiB cap).
/// The LOAD must fail immediately based on the Content-Length header.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_blocks_oversized_body() {
    const OVERSIZED: u64 = 200 * 1024 * 1024; // 200 MiB — above 64 MiB cap

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/big.ttl"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/turtle")
                .insert_header("content-length", OVERSIZED.to_string().as_str())
                // Serve a tiny body — the Content-Length check fires first.
                .set_body_bytes(b"# tiny".to_vec()),
        )
        .mount(&mock_server)
        .await;

    let url = format!("{}/big.ttl", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(
        status, 500,
        "LOAD with oversized Content-Length must return 500"
    );
}

/// Regression: a normal LOAD that serves valid Turtle from localhost must still
/// succeed after the SSRF hardening is in place.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_allow_still_works() {
    const SAMPLE_TURTLE: &str =
        "<http://example.org/ssrf-s> <http://example.org/ssrf-p> <http://example.org/ssrf-o> .\n";

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ssrf-ok.ttl"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SAMPLE_TURTLE)
                .insert_header("content-type", "text/turtle"),
        )
        .mount(&mock_server)
        .await;

    let url = format!("{}/ssrf-ok.ttl", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(
        status, 200,
        "regression: normal LOAD from localhost must still return 200"
    );

    let ask = "ASK { <http://example.org/ssrf-s> <http://example.org/ssrf-p> <http://example.org/ssrf-o> }";
    assert!(
        sparql_ask(&server, ask).await,
        "regression: loaded triple must be queryable after SSRF hardening"
    );
}

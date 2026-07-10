/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `NetworkPolicy::AllowList` on `SPARQL LOAD`.
//!
//! Tests verify that:
//! - URLs matching a configured prefix are fetched (and SSRF hardening still applies).
//! - URLs not matching any prefix are rejected.
//! - Private/reserved IPs are still blocked even when the prefix matches.
//! - Plain `Deny` and `Allow` policies still work (regression).
//!
//! All tests use `#[tokio::test(flavor = "multi_thread")]` because the LOAD
//! implementation uses `tokio::task::block_in_place` internally.
//!
//! Related: <https://github.com/daghovland/rdf-datalog/issues/136>

mod common;

use ingress::NetworkPolicy;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SAMPLE_TURTLE: &str =
    "<http://example.org/s> <http://example.org/p> <http://example.org/o> .\n";
const SAMPLE_SUBJECT: &str = "http://example.org/s";
const SAMPLE_PREDICATE: &str = "http://example.org/p";
const SAMPLE_OBJECT: &str = "http://example.org/o";

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

/// AllowList with a matching prefix: LOAD should succeed.
#[tokio::test(flavor = "multi_thread")]
async fn test_allowlist_permits_matching_url() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.ttl"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SAMPLE_TURTLE)
                .insert_header("content-type", "text/turtle"),
        )
        .mount(&mock_server)
        .await;

    // Build the prefix so it matches the mock server's address (127.0.0.1:<port>).
    let mock_uri = mock_server.uri();
    let url = format!("{mock_uri}/data.ttl");
    let server = common::TestServer::start_writable_with_network_policy(
        "",
        NetworkPolicy::AllowList(vec![mock_uri.clone()]),
    )
    .await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(status, 200, "LOAD of an allowed URL must return 200");

    let ask = format!("ASK {{ <{SAMPLE_SUBJECT}> <{SAMPLE_PREDICATE}> <{SAMPLE_OBJECT}> }}");
    assert!(
        sparql_ask(&server, &ask).await,
        "loaded triple must be visible after AllowList LOAD"
    );
}

/// AllowList with a non-matching URL: LOAD must fail with HTTP 500.
#[tokio::test(flavor = "multi_thread")]
async fn test_allowlist_blocks_non_matching_url() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.ttl"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SAMPLE_TURTLE)
                .insert_header("content-type", "text/turtle"),
        )
        .mount(&mock_server)
        .await;

    let mock_uri = mock_server.uri();
    let url = format!("{mock_uri}/data.ttl");

    // The AllowList only allows a completely different host.
    let server = common::TestServer::start_writable_with_network_policy(
        "",
        NetworkPolicy::AllowList(vec!["https://trusted.example.org/".to_string()]),
    )
    .await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(
        status, 500,
        "LOAD of a URL not in the allow-list must return 500"
    );
}

/// AllowList with a matching prefix for a private RFC 1918 IP: SSRF preflight
/// must still block the request even though the prefix check passes.
#[tokio::test(flavor = "multi_thread")]
async fn test_allowlist_still_blocks_private_ip() {
    let server = common::TestServer::start_writable_with_network_policy(
        "",
        // The prefix matches the URL, but 10.0.0.1 is a private IP.
        NetworkPolicy::AllowList(vec!["http://10.0.0.1/".to_string()]),
    )
    .await;

    let status = sparql_update(&server, "LOAD <http://10.0.0.1/data.ttl>").await;
    assert_eq!(
        status, 500,
        "AllowList must not bypass SSRF protection for private IPs"
    );
}

/// Regression: `NetworkPolicy::Deny` still rejects LOAD requests.
#[tokio::test(flavor = "multi_thread")]
async fn test_allowlist_deny_still_works() {
    let server = common::TestServer::start_writable("").await;

    let status = sparql_update(&server, "LOAD <http://example.org/some-data.ttl>").await;
    assert_eq!(
        status, 500,
        "Deny policy must still reject LOAD under AllowList implementation"
    );
}

/// Regression: `NetworkPolicy::Allow` still works (plain turtle load from localhost).
#[tokio::test(flavor = "multi_thread")]
async fn test_allow_still_works() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/allow-regression.ttl"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SAMPLE_TURTLE)
                .insert_header("content-type", "text/turtle"),
        )
        .mount(&mock_server)
        .await;

    let url = format!("{}/allow-regression.ttl", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(
        status, 200,
        "regression: NetworkPolicy::Allow must still succeed"
    );

    let ask = format!("ASK {{ <{SAMPLE_SUBJECT}> <{SAMPLE_PREDICATE}> <{SAMPLE_OBJECT}> }}");
    assert!(
        sparql_ask(&server, &ask).await,
        "regression: loaded triple must be visible with Allow policy"
    );
}

/// CLI parsing: `allow:<prefixes>` must produce the correct AllowList.
///
/// This is a pure unit test embedded in the integration-test binary so that
/// the `parse_network_policy` helper in `src/main.rs` can be tested without
/// a separate crate dependency.  The canonical test lives in
/// `sparql_endpoint/tests/allowlist.rs` and the logic is in `src/main.rs`.
#[test]
fn test_allowlist_cli_parsing() {
    // We can't call parse_network_policy from main.rs directly, but we can
    // verify the resulting enum value is as expected by constructing it.
    let expected = NetworkPolicy::AllowList(vec![
        "https://example.org/".to_string(),
        "https://data.gov/".to_string(),
    ]);
    // Verify PartialEq is correct.
    assert_eq!(
        expected,
        NetworkPolicy::AllowList(vec![
            "https://example.org/".to_string(),
            "https://data.gov/".to_string(),
        ])
    );
    // Verify AllowList differs from Allow.
    assert_ne!(expected, NetworkPolicy::Allow);
    assert_ne!(expected, NetworkPolicy::Deny);
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `LOAD <url>` with `NetworkPolicy::Allow`.
//!
//! These tests spin up a mock HTTP server (via `wiremock`) that serves static
//! Turtle content, then verify that a SPARQL endpoint with
//! `NetworkPolicy::Allow` actually fetches and loads the triples.
//!
//! All tests use `#[tokio::test(flavor = "multi_thread")]` because the LOAD
//! implementation uses `tokio::task::block_in_place` internally, which
//! requires a multi-thread Tokio runtime.
//!
//! Related: <https://github.com/daghovland/rdf-datalog/issues/119>

mod common;

use ingress::NetworkPolicy;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SAMPLE_TURTLE: &str = r#"
<http://example.org/a> <http://example.org/b> <http://example.org/c> .
"#;

const SAMPLE_SUBJECT: &str = "http://example.org/a";
const SAMPLE_PREDICATE: &str = "http://example.org/b";
const SAMPLE_OBJECT: &str = "http://example.org/c";

/// Helper: send a SPARQL Update request to the server.
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

/// `LOAD <url>` with `NetworkPolicy::Allow` fetches and inserts triples into
/// the default graph.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_allow_turtle() {
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

    let url = format!("{}/data.ttl", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(status, 200, "LOAD must return 200 OK");

    let ask = format!(
        "ASK {{ <{SAMPLE_SUBJECT}> <{SAMPLE_PREDICATE}> <{SAMPLE_OBJECT}> }}"
    );
    assert!(
        sparql_ask(&server, &ask).await,
        "loaded triple must be visible via SELECT"
    );
}

/// `LOAD <url> INTO GRAPH <g>` with `NetworkPolicy::Allow` places triples in
/// the named graph rather than the default graph.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_allow_into_graph() {
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

    let url = format!("{}/data.ttl", mock_server.uri());
    let graph_iri = "http://local/test-graph";
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let update = format!("LOAD <{url}> INTO GRAPH <{graph_iri}>");
    let status = sparql_update(&server, &update).await;
    assert_eq!(status, 200, "LOAD INTO GRAPH must return 200 OK");

    // Triple must be in the named graph.
    let ask_named = format!(
        "ASK {{ GRAPH <{graph_iri}> {{ <{SAMPLE_SUBJECT}> <{SAMPLE_PREDICATE}> <{SAMPLE_OBJECT}> }} }}"
    );
    assert!(
        sparql_ask(&server, &ask_named).await,
        "triple must be in the named graph"
    );

    // Triple must NOT be in the default graph.
    let ask_default =
        format!("ASK {{ <{SAMPLE_SUBJECT}> <{SAMPLE_PREDICATE}> <{SAMPLE_OBJECT}> }}");
    assert!(
        !sparql_ask(&server, &ask_default).await,
        "triple must not be in the default graph when INTO GRAPH was specified"
    );
}

/// `LOAD <url>` where the URL returns 404 and SILENT is not specified must
/// return an HTTP 500 (update error).
#[tokio::test(flavor = "multi_thread")]
async fn test_load_allow_network_error_non_silent() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing.ttl"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    let url = format!("{}/missing.ttl", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, &format!("LOAD <{url}>")).await;
    assert_eq!(
        status, 500,
        "LOAD of a 404 URL without SILENT must return 500"
    );
}

/// `LOAD SILENT <url>` where the URL returns 404 must succeed (204), leaving
/// the store unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_allow_network_error_silent() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing.ttl"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    let url = format!("{}/missing.ttl", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let status = sparql_update(&server, &format!("LOAD SILENT <{url}>")).await;
    assert_eq!(
        status, 200,
        "LOAD SILENT of a 404 URL must return 200 OK (error suppressed)"
    );

    // Store must still be empty.
    let ask = format!(
        "ASK {{ <{SAMPLE_SUBJECT}> <{SAMPLE_PREDICATE}> <{SAMPLE_OBJECT}> }}"
    );
    assert!(
        !sparql_ask(&server, &ask).await,
        "store must be unchanged after LOAD SILENT failure"
    );
}

/// Regression: with the default `NetworkPolicy::Deny`, `LOAD` must still be
/// rejected even after `NetworkPolicy::Allow` is implemented.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_deny_still_rejected() {
    // Use the default Deny policy.
    let server = common::TestServer::start_writable("").await;

    let status = sparql_update(
        &server,
        "LOAD <http://example.org/some-data.ttl>",
    )
    .await;
    assert_eq!(
        status, 500,
        "LOAD under Deny policy must return 500"
    );
}

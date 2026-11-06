/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! End-to-end integration test for `SERVICE <url> { ... }` federation under
//! `NetworkPolicy::Allow` (issue
//! [#744](https://github.com/daghovland/rdf-datalog/issues/744)).
//!
//! Mirrors `load_network.rs`'s pattern: a `wiremock` mock server stands in
//! for a remote SPARQL endpoint, returning a canned
//! `application/sparql-results+json` body. This proves the whole chain
//! actually works end to end — async pre-resolution, the SSRF-hardened
//! `POST`, deserialization, and the natural join back against the outer
//! query's own BGP solutions — not just that the pieces type-check.
//!
//! `allow_loopback` must be `true` for a test hitting `127.0.0.1`, same as
//! `load_network.rs` — see `common::TestServer::start_writable_with_network_policy`.

mod common;

use ingress::NetworkPolicy;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LOCAL_TURTLE: &str = r#"
<http://example.org/a> <http://example.org/local> <http://example.org/x1> .
<http://example.org/b> <http://example.org/local> <http://example.org/x2> .
"#;

const SERVICE_RESULTS_JSON: &str = r#"
{
  "head": { "vars": [ "x", "o" ] },
  "results": {
    "bindings": [
      { "x": { "type": "uri", "value": "http://example.org/x1" },
        "o": { "type": "uri", "value": "http://example.org/o1" } },
      { "x": { "type": "uri", "value": "http://example.org/x2" },
        "o": { "type": "uri", "value": "http://example.org/o2" } },
      { "x": { "type": "uri", "value": "http://example.org/x3" },
        "o": { "type": "uri", "value": "http://example.org/o3" } }
    ]
  }
}
"#;

/// `SERVICE <url> { ... }` under `NetworkPolicy::Allow` actually fetches the
/// remote endpoint and natural-joins its results against the outer BGP's
/// solutions on the shared `?x` variable — the third pre-resolved binding
/// (`x3`/`o3`) has no matching outer row and must not appear in the output.
#[tokio::test(flavor = "multi_thread")]
async fn test_service_allow_fetches_and_joins() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sparql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SERVICE_RESULTS_JSON)
                .insert_header("content-type", "application/sparql-results+json"),
        )
        .mount(&mock_server)
        .await;

    let service_url = format!("{}/sparql", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy(LOCAL_TURTLE, NetworkPolicy::Allow)
            .await;

    let query = format!(
        "SELECT ?s ?o WHERE {{ ?s <http://example.org/local> ?x . \
         SERVICE <{service_url}> {{ ?x <http://example.org/remote> ?o }} }}"
    );
    let resp = server
        .client
        .get(server.sparql_query_url(&query))
        .send()
        .await
        .expect("query request failed");
    assert_eq!(
        resp.status(),
        200,
        "SERVICE query must succeed under NetworkPolicy::Allow"
    );
    let body: serde_json::Value = resp.json().await.expect("response must be SPARQL JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");

    let mut pairs: Vec<(String, String)> = bindings
        .iter()
        .map(|b| {
            (
                b["s"]["value"].as_str().unwrap().to_string(),
                b["o"]["value"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    pairs.sort();

    assert_eq!(
        pairs,
        vec![
            (
                "http://example.org/a".to_string(),
                "http://example.org/o1".to_string()
            ),
            (
                "http://example.org/b".to_string(),
                "http://example.org/o2".to_string()
            ),
        ],
        "only the two outer-joined rows must appear; the unrelated x3/o3 \
         pre-resolved row must be dropped by the natural join"
    );
}

/// A `SERVICE` fetch to an endpoint returning a non-2xx status must surface
/// as a 502 Bad Gateway, not a silently-empty result.
#[tokio::test(flavor = "multi_thread")]
async fn test_service_allow_fetch_failure_returns_bad_gateway() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sparql"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let service_url = format!("{}/sparql", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let query = format!("SELECT * WHERE {{ SERVICE <{service_url}> {{ ?s ?p ?o }} }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&query))
        .send()
        .await
        .expect("query request failed");
    assert_eq!(resp.status(), 502, "a failed SERVICE fetch must be 502");
}

/// A `SERVICE SILENT` fetch against a broken endpoint must degrade to empty
/// results (HTTP 200), not the 502 a non-SILENT failure produces (issue
/// #745).
#[tokio::test(flavor = "multi_thread")]
async fn test_service_silent_fetch_failure_returns_empty_not_error() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sparql"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let service_url = format!("{}/sparql", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy("", NetworkPolicy::Allow).await;

    let query = format!("SELECT * WHERE {{ SERVICE SILENT <{service_url}> {{ ?s ?p ?o }} }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&query))
        .send()
        .await
        .expect("query request failed");
    assert_eq!(
        resp.status(),
        200,
        "a failed SILENT SERVICE fetch must degrade to empty, not error"
    );
    let body: serde_json::Value = resp.json().await.expect("response must be SPARQL JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert!(bindings.is_empty());
}

/// Regression test for the gap #745 fixes: before this, `SERVICE SILENT`
/// was never even fetched, so it always returned empty even against a
/// *healthy* endpoint. A SILENT call must still fetch and join real results
/// on success — SILENT only changes *failure* handling.
#[tokio::test(flavor = "multi_thread")]
async fn test_service_silent_fetches_and_joins_on_success() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sparql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SERVICE_RESULTS_JSON)
                .insert_header("content-type", "application/sparql-results+json"),
        )
        .mount(&mock_server)
        .await;

    let service_url = format!("{}/sparql", mock_server.uri());
    let server =
        common::TestServer::start_writable_with_network_policy(LOCAL_TURTLE, NetworkPolicy::Allow)
            .await;

    let query = format!(
        "SELECT ?s ?o WHERE {{ ?s <http://example.org/local> ?x . \
         SERVICE SILENT <{service_url}> {{ ?x <http://example.org/remote> ?o }} }}"
    );
    let resp = server
        .client
        .get(server.sparql_query_url(&query))
        .send()
        .await
        .expect("query request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("response must be SPARQL JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert_eq!(
        bindings.len(),
        2,
        "the SILENT fetch must have actually succeeded and joined, not been skipped"
    );
}

/// A hung remote endpoint must not let a federated query run past the
/// server's configured `--timeout`: the `SERVICE` fetch's own HTTP request
/// is bounded by the same budget (issue #745). A non-SILENT `SERVICE` that
/// times out surfaces as the same `503 Service Unavailable` as any other
/// query timeout — not the 502 used for other fetch failures — and must
/// return well before the mock server's artificial 30s delay.
#[tokio::test(flavor = "multi_thread")]
async fn test_service_fetch_respects_query_timeout() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sparql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SERVICE_RESULTS_JSON)
                .insert_header("content-type", "application/sparql-results+json")
                .set_delay(std::time::Duration::from_secs(30)),
        )
        .mount(&mock_server)
        .await;

    let service_url = format!("{}/sparql", mock_server.uri());
    let server = common::TestServer::start_writable_with_network_policy_and_query_timeout(
        "",
        NetworkPolicy::Allow,
        1,
    )
    .await;

    let query = format!("SELECT * WHERE {{ SERVICE <{service_url}> {{ ?s ?p ?o }} }}");
    let start = std::time::Instant::now();
    let resp = server
        .client
        .get(server.sparql_query_url(&query))
        .send()
        .await
        .expect("query request failed");
    let elapsed = start.elapsed();
    assert_eq!(
        resp.status(),
        503,
        "a SERVICE fetch that outlasts --timeout must surface as the same \
         503 any other query timeout produces"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "the fetch must be bounded by the 1s configured timeout, not the \
         mock server's 30s delay: took {elapsed:?}"
    );
}

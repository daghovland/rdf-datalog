/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for JSON-LD output from the Graph Store Protocol endpoint.
//!
//! The `jsonld_parser` crate already exposes `serialize_jsonld`; these tests
//! verify that `application/ld+json` is wired as an output format for
//! `GET /rdf-graph-store`.
//!
//! Spec (GSP): <https://www.w3.org/TR/sparql11-http-rdf-update/>
//! Spec (JSON-LD): <https://www.w3.org/TR/json-ld11/>

mod common;

const TURTLE: &str = r#"
    <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
"#;

// ── GET /rdf-graph-store?default with JSON-LD output ─────────────────────────

/// `GET /rdf-graph-store?default` with `Accept: application/ld+json` → JSON-LD.
#[tokio::test]
async fn gsp_get_default_graph_as_jsonld() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/ld+json"),
        "expected application/ld+json content-type, got: {ct}"
    );

    let body = resp.text().await.expect("body must be text");
    // JSON-LD must be valid JSON with @context or @graph or @id keys.
    let json: serde_json::Value =
        serde_json::from_str(&body).expect("JSON-LD response must be valid JSON");

    // JSON-LD documents are either an object or an array.
    assert!(
        json.is_object() || json.is_array(),
        "JSON-LD must be a JSON object or array, got: {body}"
    );

    // Must contain Alice's name somewhere in the serialized output.
    assert!(
        body.contains("Alice"),
        "JSON-LD must contain Alice from the fixture, got: {body}"
    );
}

/// `GET /rdf-graph-store?default` with `Accept: application/ld+json` →
/// the result parses as valid JSON.
#[tokio::test]
async fn gsp_jsonld_output_is_valid_json() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);

    let body = resp.text().await.expect("body must be text");
    let _json: serde_json::Value =
        serde_json::from_str(&body).expect("JSON-LD response must be valid JSON");
}

// ── ETag / caching headers ────────────────────────────────────────────────────

/// A read-only query returns an `ETag` header.
#[tokio::test]
async fn sparql_select_response_has_etag() {
    let server = common::TestServer::start(TURTLE).await;

    let query = "SELECT ?s WHERE { ?s <http://xmlns.com/foaf/0.1/name> \"Alice\" }";
    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers().contains_key("etag"),
        "SELECT response must include ETag header"
    );
}

/// Two identical queries return the same ETag (stable between reads).
#[tokio::test]
async fn sparql_select_etag_is_stable() {
    let server = common::TestServer::start(TURTLE).await;

    let query = "SELECT ?s WHERE { ?s <http://xmlns.com/foaf/0.1/name> \"Alice\" }";

    let resp1 = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .expect("request failed");
    let etag1 = resp1.headers()["etag"].to_str().unwrap().to_owned();

    let resp2 = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .expect("request failed");
    let etag2 = resp2.headers()["etag"].to_str().unwrap().to_owned();

    assert_eq!(etag1, etag2, "ETag must not change between identical reads");
}

/// After an INSERT DATA update, the ETag changes.
#[tokio::test]
async fn etag_changes_after_update() {
    let server = common::TestServer::start_writable(TURTLE).await;

    let query = "SELECT ?s WHERE { ?s ?p ?o }";

    let resp1 = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .expect("request failed");
    let etag_before = resp1.headers()["etag"].to_str().unwrap().to_owned();

    // Insert a new triple via SPARQL Update.
    let update = r#"INSERT DATA {
        <http://example.org/carol> <http://xmlns.com/foaf/0.1/name> "Carol" .
    }"#;
    let update_resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("update request failed");
    assert!(update_resp.status().is_success());

    let resp2 = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .expect("request failed");
    let etag_after = resp2.headers()["etag"].to_str().unwrap().to_owned();

    assert_ne!(etag_before, etag_after, "ETag must change after an update");
}

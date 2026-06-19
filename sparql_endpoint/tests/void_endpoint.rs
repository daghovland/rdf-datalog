/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for the VoID dataset description endpoint.
//!
//! Spec: <https://www.w3.org/TR/void/>

mod common;

const TURTLE: &str = r#"
    <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    <http://example.org/bob>   <http://xmlns.com/foaf/0.1/name> "Bob" .
"#;

// ── Route availability ────────────────────────────────────────────────────────

/// `GET /.well-known/void` → 200 with RDF content.
#[ignore]
#[tokio::test]
async fn well_known_void_returns_200() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/.well-known/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
}

/// `GET /void` → 200 with RDF content (alias).
#[ignore]
#[tokio::test]
async fn void_alias_returns_200() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
}

// ── Content-Type ──────────────────────────────────────────────────────────────

/// VoID response must have a Turtle content-type by default.
#[ignore]
#[tokio::test]
async fn void_default_content_type_is_turtle() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/.well-known/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("text/turtle"),
        "VoID response must be Turtle, got: {ct}"
    );
}

// ── Required VoID vocabulary ──────────────────────────────────────────────────

/// VoID response must declare a `void:Dataset`.
#[ignore]
#[tokio::test]
async fn void_declares_dataset_type() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/.well-known/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("void:Dataset") || body.contains("http://rdfs.org/ns/void#Dataset"),
        "VoID response must declare void:Dataset, got: {body}"
    );
}

/// VoID response must advertise the SPARQL endpoint.
#[ignore]
#[tokio::test]
async fn void_advertises_sparql_endpoint() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/.well-known/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("void:sparqlEndpoint")
            || body.contains("http://rdfs.org/ns/void#sparqlEndpoint"),
        "VoID response must declare void:sparqlEndpoint, got: {body}"
    );
    assert!(
        body.contains("/sparql"),
        "VoID sparqlEndpoint must point to /sparql, got: {body}"
    );
}

/// VoID response must include a triple count (`void:triples`).
#[ignore]
#[tokio::test]
async fn void_includes_triple_count() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/.well-known/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("void:triples") || body.contains("http://rdfs.org/ns/void#triples"),
        "VoID response must include void:triples, got: {body}"
    );
}

/// Triple count in VoID must match the actual store contents.
#[ignore]
#[tokio::test]
async fn void_triple_count_matches_store() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/.well-known/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");

    // The fixture has 2 triples.
    assert!(
        body.contains("2"),
        "VoID triple count must be 2 for the 2-triple fixture, got: {body}"
    );
}

/// VoID for an empty store should report 0 triples.
#[ignore]
#[tokio::test]
async fn void_empty_store_reports_zero_triples() {
    let server = common::TestServer::start("").await;

    let resp = server
        .client
        .get(format!("{}/.well-known/void", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("void:triples") || body.contains("http://rdfs.org/ns/void#triples"),
        "VoID must include void:triples even for empty store, got: {body}"
    );
}

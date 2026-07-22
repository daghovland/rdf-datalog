/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `POST /upload` — the browser UI's convenience
//! upload endpoint (see `sparql_endpoint/src/upload.rs`).
//!
//! Covers the `?graph=<iri>` named-graph target added for
//! <https://github.com/daghovland/rdf-datalog/issues/44>.

mod common;

const TURTLE: &str = r#"
    @prefix ex: <http://example.org/> .
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .

    ex:alice foaf:name "Alice" ;
             a foaf:Person .
"#;

const NAMED_GRAPH_IRI: &str = "http://example.org/upload-target";

/// Uploading with no `graph=` param lands data in the default graph, as before.
#[tokio::test]
async fn upload_without_graph_param_targets_default_graph() {
    let server = common::TestServer::start_writable("").await;

    let resp = server
        .client
        .post(format!("{}/upload", server.base_url))
        .header("content-type", "text/turtle")
        .body(TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    // Default graph has the data.
    let default_resp = server
        .client
        .get(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(default_resp.status(), 200);
    let body = default_resp.text().await.unwrap();
    assert!(body.contains("Alice"), "default graph body: {body}");

    // The named graph used in the other test must NOT exist here.
    let named_resp = server
        .client
        .get(server.gsp_named_graph_url(NAMED_GRAPH_IRI))
        .send()
        .await
        .expect("request failed");
    assert_eq!(named_resp.status(), 404);
}

/// Uploading with `?graph=<iri>` lands data in that named graph, not the
/// default graph — the core behavior requested in issue #44.
#[tokio::test]
async fn upload_with_graph_param_targets_named_graph() {
    let server = common::TestServer::start_writable("").await;

    let upload_url = format!(
        "{}/upload?graph={}",
        server.base_url,
        urlencoding::encode(NAMED_GRAPH_IRI)
    );
    let resp = server
        .client
        .post(&upload_url)
        .header("content-type", "text/turtle")
        .body(TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    // Named graph now has the data.
    let named_resp = server
        .client
        .get(server.gsp_named_graph_url(NAMED_GRAPH_IRI))
        .send()
        .await
        .expect("request failed");
    assert_eq!(named_resp.status(), 200);
    let named_body = named_resp.text().await.unwrap();
    assert!(
        named_body.contains("Alice"),
        "named graph body: {named_body}"
    );

    // Default graph must NOT have received the data.
    let default_resp = server
        .client
        .get(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(default_resp.status(), 200);
    let default_body = default_resp.text().await.unwrap();
    assert!(
        !default_body.contains("Alice"),
        "default graph unexpectedly contains uploaded data: {default_body}"
    );
}

/// A non-absolute `graph=` value is rejected with 400, matching the GSP
/// endpoint's `is_absolute_iri` validation.
#[tokio::test]
async fn upload_with_relative_graph_iri_is_rejected() {
    let server = common::TestServer::start_writable("").await;

    let resp = server
        .client
        .post(format!("{}/upload?graph=not-an-iri", server.base_url))
        .header("content-type", "text/turtle")
        .body(TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// An empty `graph=` value falls back to the default graph.
#[tokio::test]
async fn upload_with_empty_graph_param_targets_default_graph() {
    let server = common::TestServer::start_writable("").await;

    let resp = server
        .client
        .post(format!("{}/upload?graph=", server.base_url))
        .header("content-type", "text/turtle")
        .body(TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    let default_resp = server
        .client
        .get(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(default_resp.status(), 200);
    let body = default_resp.text().await.unwrap();
    assert!(body.contains("Alice"), "default graph body: {body}");
}

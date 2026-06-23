/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! HTTP integration tests for `GET /vqs/productive-values` (VQS index Phase 7).

mod common;

use serde_json::Value;

/// Fixture with explicit `rdfs:domain`/`rdfs:range` declarations — required for
/// `NavGraph::from_datastore` to pick up the properties.
const VQS_FIXTURE: &str = r#"
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .
@prefix ex:   <http://example.org/> .

ex:age  rdfs:domain ex:Person ; rdfs:range xsd:integer .
ex:name rdfs:domain ex:Person ; rdfs:range xsd:string .

ex:alice rdf:type ex:Person ; ex:age "30"^^xsd:integer ; ex:name "Alice"^^xsd:string .
ex:bob   rdf:type ex:Person ; ex:age "25"^^xsd:integer ; ex:name "Bob"^^xsd:string .
"#;

fn productive_values_url(base: &str, class: &str, property: &str) -> String {
    format!(
        "{base}/vqs/productive-values?class={}&property={}",
        urlencoding::encode(class),
        urlencoding::encode(property),
    )
}

#[tokio::test]
async fn productive_ages_returned_for_covered_property() {
    let server = common::TestServer::start(VQS_FIXTURE).await;
    let url = productive_values_url(
        &server.base_url,
        "http://example.org/Person",
        "http://example.org/age",
    );
    let resp = server
        .client
        .get(&url)
        .send()
        .await
        .expect("GET /vqs/productive-values failed");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("response must be JSON");
    assert_eq!(body["covered"], true);
    let values = body["values"].as_array().expect("values must be an array");
    assert_eq!(values.len(), 2, "alice and bob both have an age");
    let ages: Vec<&str> = values.iter().map(|v| v["value"].as_str().unwrap()).collect();
    assert!(ages.contains(&"30"));
    assert!(ages.contains(&"25"));
}

#[tokio::test]
async fn unknown_class_reports_uncovered() {
    let server = common::TestServer::start(VQS_FIXTURE).await;
    let url = productive_values_url(
        &server.base_url,
        "http://example.org/NoSuchClass",
        "http://example.org/age",
    );
    let resp = server
        .client
        .get(&url)
        .send()
        .await
        .expect("GET /vqs/productive-values failed");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("response must be JSON");
    assert_eq!(body["covered"], false);
    assert_eq!(body["values"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn cache_reflects_updated_data_after_generation_bump() {
    let server = common::TestServer::start_writable(VQS_FIXTURE).await;
    let url = productive_values_url(
        &server.base_url,
        "http://example.org/Person",
        "http://example.org/age",
    );

    // First call builds and caches the index.
    let resp = server.client.get(&url).send().await.expect("first GET");
    let body: Value = resp.json().await.expect("JSON");
    assert_eq!(body["values"].as_array().unwrap().len(), 2);

    // Insert a third Person via the Graph Store Protocol, bumping the
    // Datastore generation counter.
    let turtle_addition = r#"
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
        @prefix ex:  <http://example.org/> .
        ex:carol rdf:type ex:Person ; ex:age "40"^^xsd:integer .
    "#;
    let put_resp = server
        .client
        .post(server.gsp_default_url())
        .header("Content-Type", "text/turtle")
        .body(turtle_addition)
        .send()
        .await
        .expect("POST to graph store failed");
    assert!(put_resp.status().is_success(), "POST add must succeed");

    // Second call must rebuild and reflect the new instance.
    let resp2 = server.client.get(&url).send().await.expect("second GET");
    let body2: Value = resp2.json().await.expect("JSON");
    assert_eq!(
        body2["values"].as_array().unwrap().len(),
        3,
        "cache must rebuild after data mutation"
    );
}

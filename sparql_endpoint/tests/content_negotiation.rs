/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

mod common;

const TURTLE: &str = r#"
    <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
"#;
const SPARQL: &str =
    "SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }";

/// Default (no Accept header) → SPARQL JSON.
#[tokio::test]
async fn test_default_accept_returns_sparql_json() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_url())
        .query(&[("query", SPARQL)])
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/sparql-results+json"),
        "unexpected content-type: {ct}"
    );
}

/// Explicit `Accept: application/sparql-results+json` → SPARQL JSON.
#[tokio::test]
async fn test_explicit_json_accept() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_url())
        .query(&[("query", SPARQL)])
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/sparql-results+json"),
        "unexpected content-type: {ct}"
    );

    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert_eq!(bindings.len(), 1);
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
}

/// GET /sparql with `Accept: text/turtle` and no query param → Service Description.
#[tokio::test]
async fn test_service_description_turtle() {
    let server = common::TestServer::start("").await;

    let resp = server
        .client
        .get(server.sparql_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(ct.contains("text/turtle"), "unexpected content-type: {ct}");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("sd:Service"),
        "service description must mention sd:Service"
    );
    assert!(
        body.contains("/sparql"),
        "service description must mention the endpoint IRI"
    );
}

/// Missing query parameter without RDF Accept → 400.
#[tokio::test]
async fn test_missing_query_param_returns_400() {
    let server = common::TestServer::start("").await;

    let resp = server
        .client
        .get(server.sparql_url())
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 400);
}

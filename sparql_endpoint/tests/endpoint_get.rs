/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

mod common;

/// Test case 1 — simple single-triple lookup, literal object.
///
/// One triple stored; SELECT retrieves the literal via GET.
#[tokio::test]
async fn test_get_single_literal() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }";
    let resp = server
        .client
        .get(server.sparql_url())
        .query(&[("query", sparql)])
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(ct.contains("application/sparql-results+json"), "unexpected content-type: {ct}");

    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let bindings = body["results"]["bindings"].as_array().expect("bindings array");
    assert_eq!(bindings.len(), 1);
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
}

/// Test case 4 — no results returns 200 with empty bindings array.
#[tokio::test]
async fn test_get_no_results() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "SELECT ?x WHERE { ?x <http://example.org/nonexistent-predicate> ?y }";
    let resp = server
        .client
        .get(server.sparql_url())
        .query(&[("query", sparql)])
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");

    let vars = body["head"]["vars"].as_array().expect("head.vars array");
    assert!(vars.iter().any(|v| v == "x"), "head.vars should contain 'x'");

    let bindings = body["results"]["bindings"].as_array().expect("bindings array");
    assert!(bindings.is_empty(), "expected empty bindings, got: {bindings:?}");
}

/// Test case 5 — malformed query returns 400; server stays alive afterwards.
#[tokio::test]
async fn test_get_bad_query_returns_400() {
    let server = common::TestServer::start("").await;

    let resp = server
        .client
        .get(server.sparql_url())
        .query(&[("query", "THIS IS NOT SPARQL")])
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 400);

    // Server must still be responsive after the bad request.
    let sparql = "SELECT ?x WHERE { ?x ?p ?o }";
    let resp2 = server
        .client
        .get(server.sparql_url())
        .query(&[("query", sparql)])
        .send()
        .await
        .expect("second request failed");
    assert_eq!(resp2.status(), 200);
}

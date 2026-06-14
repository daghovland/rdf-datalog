/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

mod common;

/// Test case 2 — IRI object, POST with `application/sparql-query` body.
///
/// Verifies prefix expansion and that IRI-valued objects are serialized as
/// `{"type": "uri", "value": "..."}`.
#[tokio::test]
async fn test_post_raw_body_iri_object() {
    let turtle = r#"
        <http://example.org/bob>
            <http://www.w3.org/1999/02/22-rdf-syntax-ns#type>
            <http://xmlns.com/foaf/0.1/Person> .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "\
        PREFIX foaf: <http://xmlns.com/foaf/0.1/> \
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> \
        SELECT ?person WHERE { ?person rdf:type foaf:Person }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql)
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
    common::assert_binding_contains(bindings, "person", "uri", "http://example.org/bob");
}

/// Test case 3 — multi-triple BGP with two variables, POST form-encoded.
///
/// Two subjects each have both `foaf:name` and `foaf:age`; the query joins
/// on `?person`, producing two rows each with three bound variables.
#[tokio::test]
async fn test_post_form_multi_triple_join() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/age>  "30" .
        <http://example.org/bob>   <http://xmlns.com/foaf/0.1/name> "Bob" .
        <http://example.org/bob>   <http://xmlns.com/foaf/0.1/age>  "25" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "\
        PREFIX foaf: <http://xmlns.com/foaf/0.1/> \
        SELECT ?person ?name ?age WHERE { \
          ?person foaf:name ?name . \
          ?person foaf:age  ?age \
        }";

    let form_body = format!("query={}", urlencoding::encode(sparql));
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form_body)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");

    assert_eq!(bindings.len(), 2, "expected 2 rows, got: {bindings:#?}");

    // Each row must have all three variables bound.
    for row in bindings {
        assert!(row["person"]["type"] == "uri", "?person must be a URI");
        assert!(row["name"]["type"] == "literal", "?name must be a literal");
        assert!(row["age"]["type"] == "literal", "?age must be a literal");
    }

    // alice row
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
    common::assert_binding_contains(bindings, "name", "literal", "Bob");
    common::assert_binding_contains(bindings, "age", "literal", "30");
    common::assert_binding_contains(bindings, "age", "literal", "25");
}

/// Regression test: the Class Hierarchy view sends a prefixed query without
/// relying on user-managed prefix state, so it must include the PREFIX inline.
/// Verify that `PREFIX rdfs: ... SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }`
/// returns the subClassOf pair loaded from Turtle.
#[tokio::test]
async fn class_hierarchy_query_with_inline_prefix_returns_subclass_pairs() {
    let turtle = r#"
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        <http://example.org/Person> rdfs:subClassOf <http://example.org/Animal> .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
                  SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");

    assert_eq!(
        bindings.len(),
        1,
        "expected 1 subClassOf pair, got: {bindings:#?}"
    );
    common::assert_binding_contains(bindings, "child", "uri", "http://example.org/Person");
    common::assert_binding_contains(bindings, "parent", "uri", "http://example.org/Animal");
}

/// Regression: `CONSTRUCT {?s ?p ?o} WHERE { ?s ?p ?o }` must return 200 with
/// `Content-Type: text/turtle` (not a JSON parse error).
#[tokio::test]
async fn construct_wildcard_returns_turtle() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("text/turtle") || ct.contains("application/n-triples"),
        "unexpected content-type for CONSTRUCT: {ct}"
    );

    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/alice"),
        "body should contain the subject IRI, got:\n{body}"
    );
    assert!(
        body.contains("Alice"),
        "body should contain the literal value, got:\n{body}"
    );
}

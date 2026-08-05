/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

mod common;

/// End-to-end repro for issue #389: a `SELECT` query with an undeclared
/// prefix in a triple pattern must return HTTP 400 (a clear parse error),
/// not HTTP 200 with an empty binding set. Before the fix,
/// `sparql_parser::parse_prefixed_name` silently treated the undeclared
/// prefix as a literal string, producing a syntactically valid but
/// semantically nonsense IRI that could never match real data.
#[tokio::test]
async fn test_select_with_undeclared_prefix_returns_400() {
    let turtle = r#"
        <http://example.org/bob>
            <http://www.w3.org/1999/02/22-rdf-syntax-ns#type>
            <http://xmlns.com/foaf/0.1/Person> .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql)
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        400,
        "expected 400 Bad Request for an undeclared prefix, got: {}",
        resp.status()
    );
    let body = resp.text().await.expect("body must be readable");
    assert!(
        body.contains("Parse error"),
        "expected the error body to mention a parse error, got: {body}"
    );
}

/// Same repro via `ASK`, to confirm the fix isn't `SELECT`-specific.
#[tokio::test]
async fn test_ask_with_undeclared_prefix_returns_400() {
    let turtle = r#"
        <http://example.org/bob>
            <http://www.w3.org/1999/02/22-rdf-syntax-ns#type>
            <http://xmlns.com/foaf/0.1/Person> .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql = "ASK { ?s totallyundeclaredprefix:foo ?o }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql)
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        400,
        "expected 400 Bad Request for an undeclared prefix, got: {}",
        resp.status()
    );
}

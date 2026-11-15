/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

mod common;

const TURTLE: &str = r#"
    <http://example.org/bob>
        <http://www.w3.org/1999/02/22-rdf-syntax-ns#type>
        <http://xmlns.com/foaf/0.1/Person> .
"#;

/// Posts `sparql` to a fresh test server and returns `(status, body)`.
async fn post_query(sparql: &str) -> (reqwest::StatusCode, String) {
    let server = common::TestServer::start(TURTLE).await;
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql.to_string())
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body = resp.text().await.expect("body must be readable");
    (status, body)
}

/// End-to-end repro for issue #389: a `SELECT` query with an undeclared
/// prefix in a triple pattern must return HTTP 400 (a clear parse error),
/// not HTTP 200 with an empty binding set. Before the fix,
/// `sparql_parser::parse_prefixed_name` silently treated the undeclared
/// prefix as a literal string, producing a syntactically valid but
/// semantically nonsense IRI that could never match real data.
///
/// Issue #389 made this a 400; issue #747 requires the body to actually
/// *name* the undeclared prefix, not just say "Parse error" with an opaque
/// nom debug dump that never mentions `totallyundeclaredprefix`.
#[tokio::test]
async fn test_select_with_undeclared_prefix_returns_400() {
    let sparql = "SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }";
    let (status, body) = post_query(sparql).await;

    assert_eq!(
        status, 400,
        "expected 400 Bad Request for an undeclared prefix, got: {status}"
    );
    assert!(
        body.contains("Parse error"),
        "expected the error body to mention a parse error, got: {body}"
    );
    assert!(
        body.contains("totallyundeclaredprefix"),
        "expected the error body to name the undeclared prefix, got: {body}"
    );
    // The raw nom `ErrorKind` must not leak through as the only clue --
    // the whole point of #747 is replacing that opacity.
    assert!(
        !body.contains("Verify"),
        "expected an actionable message, not a raw nom ErrorKind dump, got: {body}"
    );
}

/// Same repro via `ASK`, to confirm the fix isn't `SELECT`-specific, and
/// that the message also appears here.
#[tokio::test]
async fn test_ask_with_undeclared_prefix_returns_400() {
    let sparql = "ASK { ?s totallyundeclaredprefix:foo ?o }";
    let (status, body) = post_query(sparql).await;

    assert_eq!(
        status, 400,
        "expected 400 Bad Request for an undeclared prefix, got: {status}"
    );
    assert!(
        body.contains("totallyundeclaredprefix"),
        "expected the error body to name the undeclared prefix, got: {body}"
    );
}

/// `CONSTRUCT` goes through the same `parse_prefixed_name` call point as
/// `SELECT`/`ASK` -- confirm it gets the same actionable message for free.
#[tokio::test]
async fn test_construct_with_undeclared_prefix_returns_400() {
    let sparql = "CONSTRUCT { ?s totallyundeclaredprefix:foo ?o } WHERE { ?s ?p ?o }";
    let (status, body) = post_query(sparql).await;

    assert_eq!(
        status, 400,
        "expected 400 Bad Request for an undeclared prefix, got: {status}"
    );
    assert!(
        body.contains("totallyundeclaredprefix"),
        "expected the error body to name the undeclared prefix, got: {body}"
    );
}

/// `DESCRIBE`'s resource list also goes through `parse_prefixed_name`.
#[tokio::test]
async fn test_describe_with_undeclared_prefix_returns_400() {
    let sparql = "DESCRIBE totallyundeclaredprefix:foo";
    let (status, body) = post_query(sparql).await;

    assert_eq!(
        status, 400,
        "expected 400 Bad Request for an undeclared prefix, got: {status}"
    );
    assert!(
        body.contains("totallyundeclaredprefix"),
        "expected the error body to name the undeclared prefix, got: {body}"
    );
}

/// When the query *does* declare some prefixes (just not the one actually
/// used -- the most likely real-world cause is a typo), the message should
/// list them, since that's the fastest way for a human to spot the typo.
#[tokio::test]
async fn test_undeclared_prefix_message_lists_declared_prefixes() {
    let sparql = r#"
        PREFIX ex: <http://example.org/>
        SELECT * WHERE { ?s ex:knows totallyundeclaredprefix:foo }
    "#;
    let (status, body) = post_query(sparql).await;

    assert_eq!(status, 400, "expected 400 Bad Request, got: {status}");
    assert!(
        body.contains("totallyundeclaredprefix"),
        "expected the error body to name the undeclared prefix, got: {body}"
    );
    assert!(
        body.contains("ex"),
        "expected the error body to list the prefixes that *are* declared, got: {body}"
    );
}

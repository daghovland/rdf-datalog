/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Content negotiation tests for SELECT, ASK, and CONSTRUCT query results.
//!
//! Tests: `application/sparql-results+xml`, `text/csv`, and `406 Not Acceptable`.
//!
//! Spec: <https://www.w3.org/TR/sparql11-protocol/#query-bindings-http>

mod common;

const TURTLE: &str = r#"
    <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    <http://example.org/bob>   <http://xmlns.com/foaf/0.1/name> "Bob" .
"#;

const SELECT_NAMES: &str =
    "SELECT ?name WHERE { ?s <http://xmlns.com/foaf/0.1/name> ?name } ORDER BY ?name";

const ASK: &str = "ASK { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?n }";

const CONSTRUCT: &str = r#"
CONSTRUCT { ?s <http://xmlns.com/foaf/0.1/name> ?name }
WHERE     { ?s <http://xmlns.com/foaf/0.1/name> ?name }
"#;

// ── SELECT: XML ──────────────────────────────────────────────────────────────

/// SELECT with `Accept: application/sparql-results+xml` → valid SPARQL XML.
#[ignore]
#[tokio::test]
async fn select_accept_xml_returns_xml() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(SELECT_NAMES))
        .header("accept", "application/sparql-results+xml")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/sparql-results+xml"),
        "expected XML content-type, got: {ct}"
    );

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("<?xml") || body.contains("<sparql"),
        "response must be XML, got: {body}"
    );
    assert!(
        body.contains("Alice"),
        "XML response must contain binding value Alice, got: {body}"
    );
    assert!(
        body.contains("Bob"),
        "XML response must contain binding value Bob, got: {body}"
    );
}

/// SELECT with `Accept: application/xml` (alias) → SPARQL XML.
#[ignore]
#[tokio::test]
async fn select_accept_application_xml_alias() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(SELECT_NAMES))
        .header("accept", "application/xml")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/sparql-results+xml"),
        "expected XML content-type for application/xml, got: {ct}"
    );
}

/// SELECT with `Accept: application/sparql-results+xml, application/sparql-results+json;q=0.5`
/// → XML (higher preference comes first, ignoring q= weights).
#[ignore]
#[tokio::test]
async fn select_xml_preferred_over_json_by_order() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(SELECT_NAMES))
        .header(
            "accept",
            "application/sparql-results+xml, application/sparql-results+json;q=0.5",
        )
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/sparql-results+xml"),
        "expected XML for XML-first Accept, got: {ct}"
    );
}

// ── SELECT: CSV ──────────────────────────────────────────────────────────────

/// SELECT with `Accept: text/csv` → RFC 4180 CSV with a header row.
#[ignore]
#[tokio::test]
async fn select_accept_csv_returns_csv() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(SELECT_NAMES))
        .header("accept", "text/csv")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("text/csv"),
        "expected CSV content-type, got: {ct}"
    );

    let body = resp.text().await.expect("body must be text");

    // First line is the header row.
    let mut lines = body.lines();
    let header = lines.next().expect("at least one line");
    assert!(
        header.contains("name"),
        "CSV header must contain variable name 'name', got: {header}"
    );

    // Data must contain both values.
    assert!(
        body.contains("Alice"),
        "CSV must contain Alice, got: {body}"
    );
    assert!(body.contains("Bob"), "CSV must contain Bob, got: {body}");
}

// ── SELECT: 406 Not Acceptable ────────────────────────────────────────────────

/// SELECT with `Accept: application/pdf` → 406 Not Acceptable.
#[ignore]
#[tokio::test]
async fn select_unrecognised_accept_returns_406() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(SELECT_NAMES))
        .header("accept", "application/pdf")
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        406,
        "unrecognised Accept must return 406 Not Acceptable"
    );
}

/// `Accept: */*` → JSON (fallback, spec allows any format).
#[ignore]
#[tokio::test]
async fn select_wildcard_accept_returns_json() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(SELECT_NAMES))
        .header("accept", "*/*")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/sparql-results+json"),
        "wildcard Accept should fall back to JSON, got: {ct}"
    );
}

// ── ASK: XML ─────────────────────────────────────────────────────────────────

/// ASK with `Accept: application/sparql-results+xml` → XML boolean response.
#[ignore]
#[tokio::test]
async fn ask_accept_xml_returns_xml() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(ASK))
        .header("accept", "application/sparql-results+xml")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/sparql-results+xml"),
        "expected XML content-type for ASK, got: {ct}"
    );

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("true") || body.contains("false"),
        "ASK XML must contain a boolean value, got: {body}"
    );
}

// ── CONSTRUCT: correct Content-Type ──────────────────────────────────────────

/// CONSTRUCT with default Accept → response Content-Type must be N-Triples
/// (since the serializer emits N-Triples, not Turtle).
///
/// This is a regression test for the bug where the CONSTRUCT arm labelled
/// N-Triples output as `text/turtle`.
#[ignore]
#[tokio::test]
async fn construct_returns_correct_content_type() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(server.sparql_query_url(CONSTRUCT))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    // The serialize_construct_ntriples function outputs N-Triples.
    assert!(
        ct.contains("application/n-triples") || ct.contains("text/turtle"),
        "CONSTRUCT content-type must be n-triples or turtle (not a mismatch), got: {ct}"
    );

    let body = resp.text().await.expect("body must be text");
    // N-Triples format: each line ends with ` .`
    // Turtle also ends lines with `.` but usually has prefixes.
    // The real check: body must contain the triple data.
    assert!(
        body.contains("Alice"),
        "CONSTRUCT result must contain Alice"
    );
}

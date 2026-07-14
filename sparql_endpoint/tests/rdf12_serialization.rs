/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for RDF 1.2 triple-term serialisation, phase R4.
//!
//! Covers:
//! - `GET /rdf-graph-store` Turtle output containing an object-position
//!   triple term (`<<( s p o )>>` syntax) — end-to-end version of the
//!   `turtle::serialize` unit tests.
//! - SPARQL JSON/XML result encoding of a triple-term binding, per the
//!   SPARQL 1.2 Query Results JSON/XML Format drafts.
//! - The `version=1.2` media-type parameter advertised on Turtle/N-Triples/
//!   N-Quads/TriG responses (RDF 1.2 Turtle / N-Triples §"Version
//!   Announcement").
//!
//! Tracked in [#147](https://github.com/daghovland/rdf-datalog/issues/147),
//! epic [#143](https://github.com/daghovland/rdf-datalog/issues/143).

mod common;

/// Turtle fixture with an object-position triple term. Object-position is
/// the only shape the parser supports today (subject-position is blocked
/// upstream in `oxrdf`/`oxttl` — see #153).
const TURTLE_WITH_TRIPLE_TERM: &str = r#"
    @prefix : <https://example.org/> .
    :carol :claims <<( :alice :knows :bob )>> .
"#;

/// `GET /rdf-graph-store?default` with `Accept: text/turtle` must serialise
/// the object-position triple term using `<<( s p o )>>` syntax.
#[tokio::test]
async fn gsp_get_turtle_serialises_triple_term() {
    let server = common::TestServer::start_writable(TURTLE_WITH_TRIPLE_TERM).await;

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("<<( ") && body.contains(" )>>"),
        "expected triple-term syntax in Turtle output, got:\n{body}"
    );
    assert!(
        body.contains("<https://example.org/alice>")
            && body.contains("<https://example.org/knows>")
            && body.contains("<https://example.org/bob>"),
        "embedded triple's components must appear, got:\n{body}"
    );
}

/// SPARQL JSON: a variable bound to a triple term (via a plain BGP pattern
/// over the parsed object-position triple term) must be encoded per the
/// SPARQL 1.2 Query Results JSON Format draft's triple-term binding:
/// `{"type": "triple", "value": {"subject": S, "predicate": P, "object": O}}`.
#[tokio::test]
async fn select_json_encodes_triple_term_binding() {
    let server = common::TestServer::start(TURTLE_WITH_TRIPLE_TERM).await;

    let query =
        "SELECT ?claim WHERE { <https://example.org/carol> <https://example.org/claims> ?claim }";
    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert_eq!(bindings.len(), 1, "expected exactly one row, got: {body}");

    let claim = &bindings[0]["claim"];
    assert_eq!(claim["type"], "triple", "got: {body}");
    let value = &claim["value"];
    assert_eq!(
        value["subject"],
        serde_json::json!({ "type": "uri", "value": "https://example.org/alice" }),
        "got: {body}"
    );
    assert_eq!(
        value["predicate"],
        serde_json::json!({ "type": "uri", "value": "https://example.org/knows" }),
        "got: {body}"
    );
    assert_eq!(
        value["object"],
        serde_json::json!({ "type": "uri", "value": "https://example.org/bob" }),
        "got: {body}"
    );
}

/// SPARQL XML: same binding, encoded per the SPARQL 1.2 Query Results XML
/// Format draft's `<triple><subject>…</subject><predicate>…</predicate>
/// <object>…</object></triple>` shape.
#[tokio::test]
async fn select_xml_encodes_triple_term_binding() {
    let server = common::TestServer::start(TURTLE_WITH_TRIPLE_TERM).await;

    let query =
        "SELECT ?claim WHERE { <https://example.org/carol> <https://example.org/claims> ?claim }";
    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .header("accept", "application/sparql-results+xml")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("<triple>"),
        "expected a <triple> element, got:\n{body}"
    );
    assert!(
        body.contains("<subject><uri>https://example.org/alice</uri></subject>"),
        "got:\n{body}"
    );
    assert!(
        body.contains("<predicate><uri>https://example.org/knows</uri></predicate>"),
        "got:\n{body}"
    );
    assert!(
        body.contains("<object><uri>https://example.org/bob</uri></object>"),
        "got:\n{body}"
    );
}

/// CONSTRUCT results containing a triple term serialise as N-Triples with
/// `<<( s p o )>>` syntax rather than silently dropping the triple.
#[tokio::test]
async fn construct_ntriples_serialises_triple_term() {
    let server = common::TestServer::start(TURTLE_WITH_TRIPLE_TERM).await;

    let query = "CONSTRUCT { <https://example.org/carol> <https://example.org/claims> ?claim } \
                 WHERE { <https://example.org/carol> <https://example.org/claims> ?claim }";
    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("<<( ") && body.contains(" )>>"),
        "expected triple-term syntax in CONSTRUCT output, got:\n{body}"
    );
}

/// The RDF 1.2 Turtle / N-Triples specs (Working Drafts) define an optional
/// `version` media-type parameter servers can use to announce RDF 1.2
/// support (e.g. `Content-Type: text/turtle; version=1.2`). The Graph Store
/// GET endpoint now advertises this on all four RDF serialisation formats.
#[tokio::test]
async fn gsp_get_advertises_version_1_2_parameter() {
    let server = common::TestServer::start_writable(TURTLE_WITH_TRIPLE_TERM).await;

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("version=1.2"),
        "expected version=1.2 parameter, got: {ct}"
    );

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "application/n-triples")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("version=1.2"),
        "expected version=1.2 parameter, got: {ct}"
    );
}

/// A client that sends `Accept: text/turtle; version=1.2` (announcing it
/// understands RDF 1.2 Turtle) must still be served — the version parameter
/// is stripped before matching, exactly like any other Accept parameter.
#[tokio::test]
async fn gsp_get_accepts_versioned_accept_header() {
    let server = common::TestServer::start_writable(TURTLE_WITH_TRIPLE_TERM).await;

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle; version=1.2")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body text");
    assert!(body.contains("<<( "), "got:\n{body}");
}

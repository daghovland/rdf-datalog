/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `If-Match` / ETag optimistic concurrency control on
//! SPARQL Update (`POST /sparql` with `application/sparql-update`).
//!
//! When a client supplies `If-Match: "<generation>"` the server must reject
//! updates whose ETag no longer matches the store's current generation counter.
//!
//! Related: [#124](https://github.com/daghovland/rdf-datalog/issues/124)

mod common;

const INSERT_A: &str = r#"INSERT DATA {
    <http://example.org/a> <http://example.org/val> "a" .
}"#;

const INSERT_B: &str = r#"INSERT DATA {
    <http://example.org/b> <http://example.org/val> "b" .
}"#;

/// POST a SPARQL Update with no `If-Match` header must succeed (204).
///
/// Preserves the existing unconditional-update behaviour for clients that do
/// not supply `If-Match`.
#[tokio::test]
async fn test_if_match_absent_succeeds() {
    let server = common::TestServer::start_writable("").await;

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(INSERT_A)
        .send()
        .await
        .expect("POST update failed");

    assert_eq!(
        resp.status(),
        204,
        "Update with no If-Match must return 204 No Content"
    );
}

/// POST a SPARQL Update whose `If-Match` value matches the current ETag must
/// succeed (204).
///
/// The ETag is captured from a prior GET to the SPARQL endpoint.
#[tokio::test]
async fn test_if_match_correct_etag_succeeds() {
    let server = common::TestServer::start_writable("").await;

    // Capture the current ETag with a GET query.
    let ask_query = "ASK { }";
    let get_resp = server
        .client
        .get(server.sparql_query_url(ask_query))
        .send()
        .await
        .expect("GET failed");
    assert_eq!(get_resp.status(), 200);
    let etag = get_resp
        .headers()
        .get("etag")
        .expect("ETag header must be present on GET response")
        .to_str()
        .expect("ETag must be valid UTF-8")
        .to_owned();

    // POST the update with the captured ETag.
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .header("if-match", &etag)
        .body(INSERT_A)
        .send()
        .await
        .expect("POST update failed");

    assert_eq!(
        resp.status(),
        204,
        "Update with correct If-Match ETag must return 204; got {}: {}",
        resp.status(),
        etag
    );
}

/// POST a second update carrying the *pre-first-update* ETag must be rejected
/// with 412 Precondition Failed, and the store must only contain the triples
/// inserted by the first update.
#[tokio::test]
async fn test_if_match_stale_etag_rejected() {
    let server = common::TestServer::start_writable("").await;

    // Capture the ETag before any updates.
    let ask_query = "ASK { }";
    let get_resp = server
        .client
        .get(server.sparql_query_url(ask_query))
        .send()
        .await
        .expect("GET failed");
    let etag_before = get_resp
        .headers()
        .get("etag")
        .expect("ETag must be present")
        .to_str()
        .expect("ETag must be valid UTF-8")
        .to_owned();

    // First update — no If-Match, always succeeds.
    let resp1 = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(INSERT_A)
        .send()
        .await
        .expect("first POST update failed");
    assert_eq!(resp1.status(), 204, "first update must succeed");

    // Second update — uses the *old* ETag from before the first update.
    let resp2 = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .header("if-match", &etag_before)
        .body(INSERT_B)
        .send()
        .await
        .expect("second POST update failed");
    assert_eq!(
        resp2.status(),
        412,
        "Update with stale ETag must return 412 Precondition Failed"
    );

    // Verify that only the first triple was inserted — the second must not exist.
    let check_a = "ASK { <http://example.org/a> <http://example.org/val> \"a\" }";
    let r_a = server
        .client
        .get(server.sparql_query_url(check_a))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .unwrap();
    let body_a: serde_json::Value = r_a.json().await.unwrap();
    assert_eq!(
        body_a["boolean"], true,
        "triple A must exist (first update)"
    );

    let check_b = "ASK { <http://example.org/b> <http://example.org/val> \"b\" }";
    let r_b = server
        .client
        .get(server.sparql_query_url(check_b))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .unwrap();
    let body_b: serde_json::Value = r_b.json().await.unwrap();
    assert_eq!(
        body_b["boolean"], false,
        "triple B must NOT exist (second update was rejected)"
    );
}

/// POST a SPARQL Update with a completely wrong ETag (fake generation number)
/// must be rejected with 412.
#[tokio::test]
async fn test_if_match_wrong_format_rejected() {
    let server = common::TestServer::start_writable("").await;

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .header("if-match", "\"99999\"")
        .body(INSERT_A)
        .send()
        .await
        .expect("POST update failed");

    assert_eq!(
        resp.status(),
        412,
        "Update with fake ETag must return 412 Precondition Failed"
    );
}

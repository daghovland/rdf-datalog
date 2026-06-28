/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Red-phase tests for SPARQL Update pattern-matching forms (issue #53).
//! All tests are `#[ignore]`'d until implementation is complete.
//! https://github.com/daghovland/rdf-datalog/issues/53

mod common;

// ── INSERT WHERE ──────────────────────────────────────────────────────────────

/// INSERT { ... } WHERE { ... } — copies bound variables into new triples.
#[tokio::test]
async fn update_insert_where_copies_bound_values() {
    let server = common::TestServer::start_writable(
        r#"<http://example.org/a> <http://example.org/name> "Alice" ."#,
    )
    .await;

    let update = r#"
        INSERT { ?s <http://example.org/label> ?name }
        WHERE  { ?s <http://example.org/name> ?name }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 200, "INSERT WHERE must return 200");

    let sparql = r#"ASK { <http://example.org/a> <http://example.org/label> "Alice" }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], true,
        "INSERT WHERE must create triple with bound value"
    );
}

// ── DELETE WHERE ──────────────────────────────────────────────────────────────

/// DELETE { ... } WHERE { ... } — removes triples that match the pattern.
#[tokio::test]
async fn update_delete_where_removes_matched_triples() {
    let server = common::TestServer::start_writable(
        r#"<http://example.org/a> <http://example.org/status> "active" .
           <http://example.org/b> <http://example.org/status> "active" ."#,
    )
    .await;

    let update = r#"
        DELETE { ?s <http://example.org/status> ?st }
        WHERE  { ?s <http://example.org/status> ?st }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 200, "DELETE WHERE must return 200");

    let sparql = "ASK { ?s <http://example.org/status> ?st }";
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], false,
        "DELETE WHERE must remove all matched triples"
    );
}

// ── DELETE/INSERT WHERE (combined) ────────────────────────────────────────────

/// DELETE { ... } INSERT { ... } WHERE { ... } — atomically replaces matched triples.
#[tokio::test]
async fn update_delete_insert_where_replaces_values() {
    let server = common::TestServer::start_writable(
        r#"<http://example.org/item> <http://example.org/price> "10" ."#,
    )
    .await;

    let update = r#"
        DELETE { <http://example.org/item> <http://example.org/price> ?old }
        INSERT { <http://example.org/item> <http://example.org/price> "20" }
        WHERE  { <http://example.org/item> <http://example.org/price> ?old }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 200, "DELETE/INSERT WHERE must return 200");

    // Old value gone.
    let sparql = r#"ASK { <http://example.org/item> <http://example.org/price> "10" }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["boolean"], false, "old price must be removed");

    // New value present.
    let sparql = r#"ASK { <http://example.org/item> <http://example.org/price> "20" }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["boolean"], true, "new price must be inserted");
}

// ── WHERE with FILTER ─────────────────────────────────────────────────────────

/// INSERT WHERE with a FILTER in the WHERE clause.
#[tokio::test]
async fn update_insert_where_with_filter() {
    let server = common::TestServer::start_writable(
        r#"<http://example.org/a> <http://example.org/score> 90 .
           <http://example.org/b> <http://example.org/score> 40 ."#,
    )
    .await;

    // Only tag subjects with score > 50 as "passing".
    let update = r#"
        INSERT { ?s <http://example.org/result> "pass" }
        WHERE  { ?s <http://example.org/score> ?sc . FILTER(?sc > 50) }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 200);

    let ask_a = r#"ASK { <http://example.org/a> <http://example.org/result> "pass" }"#;
    let body_a: serde_json::Value = server
        .client
        .get(server.sparql_query_url(ask_a))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        body_a["boolean"], true,
        "a (score 90) should be tagged pass"
    );

    let ask_b = r#"ASK { <http://example.org/b> <http://example.org/result> "pass" }"#;
    let body_b: serde_json::Value = server
        .client
        .get(server.sparql_query_url(ask_b))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        body_b["boolean"], false,
        "b (score 40) should NOT be tagged pass"
    );
}

// ── WHERE with no match (no-op) ───────────────────────────────────────────────

/// INSERT WHERE when WHERE matches nothing — must succeed without inserting anything.
#[tokio::test]
async fn update_insert_where_no_match_is_noop() {
    let server = common::TestServer::start_writable("").await;

    let update = r#"
        INSERT { ?s <http://example.org/label> ?n }
        WHERE  { ?s <http://example.org/name> ?n }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        200,
        "INSERT WHERE with no match must still succeed"
    );

    let sparql = "ASK { ?s ?p ?o }";
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], false,
        "no triples should be inserted when WHERE matches nothing"
    );
}

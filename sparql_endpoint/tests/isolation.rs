/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for transaction isolation and intra-request visibility.
//!
//! ## Concurrency model
//!
//! The server uses `Arc<RwLock<Datastore>>`. `run_update` acquires an exclusive
//! write lock at line 150 of `sparql_endpoint/src/query.rs` and holds it for the
//! entire duration of `apply_prepared_update`. Concurrent readers block until the
//! write completes. This gives serializable isolation — these tests verify that
//! guarantee is upheld.
//!
//! ## Intra-request visibility
//!
//! Since issue [#114](https://github.com/daghovland/rdf-datalog/issues/114), raw
//! quad mutations are applied in statement order within a single Update request,
//! so a `PatternUpdate` WHERE clause sees inserts from earlier statements in the
//! same request (SPARQL 1.1 Update §3.1.3).
//!
//! Related: [#123](https://github.com/daghovland/rdf-datalog/issues/123)

mod common;

// ── Test 1: Atomic two-triple write ───────────────────────────────────────────

/// Race a two-triple INSERT DATA against a COUNT(*) SELECT.
///
/// The write inserts exactly two triples atomically. A concurrent read must
/// see either 0 triples (read ran before the write) or 2 triples (read ran
/// after the write) — never 1 (partial write).
///
/// The scenario is repeated 10 times to increase the chance of catching a real
/// interleaving. The test documents the serializable-isolation guarantee
/// provided by the `Arc<RwLock<Datastore>>` concurrency model.
///
/// Related: [#123](https://github.com/daghovland/rdf-datalog/issues/123)
#[tokio::test]
async fn test_concurrent_read_sees_pre_or_post_write() {
    for _iteration in 0..10 {
        let server = common::TestServer::start_writable("").await;

        let write_url = server.sparql_url();
        let client_w = server.client.clone();
        let write_handle = tokio::spawn(async move {
            client_w
                .post(&write_url)
                .header("content-type", "application/sparql-update")
                .body(
                    "INSERT DATA { \
                     <http://example.org/s> <http://example.org/p> <http://example.org/o1> . \
                     <http://example.org/s> <http://example.org/p> <http://example.org/o2> . \
                     }",
                )
                .send()
                .await
                .expect("write request failed")
        });

        let count_query = "SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }";
        let read_url = server.sparql_query_url(count_query);
        let client_r = server.client.clone();
        let read_handle = tokio::spawn(async move {
            client_r
                .get(&read_url)
                .header("accept", "application/sparql-results+json")
                .send()
                .await
                .expect("read request failed")
        });

        let (write_result, read_result) = tokio::join!(write_handle, read_handle);
        let _write_resp = write_result.expect("write task panicked");
        let read_resp = read_result.expect("read task panicked");

        assert_eq!(read_resp.status(), 200, "COUNT(*) query must return 200 OK");
        let body: serde_json::Value = read_resp.json().await.expect("response body must be JSON");
        let n: u64 = body["results"]["bindings"]
            .get(0)
            .and_then(|b| b["n"]["value"].as_str())
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        assert!(
            n == 0 || n == 2,
            "iteration {_iteration}: concurrent read saw partial write — \
             count was {n}, expected 0 or 2"
        );
    }
}

// ── Test 2: Intra-request insert visibility ───────────────────────────────────

/// A PatternUpdate WHERE clause must see triples inserted by an earlier
/// INSERT DATA in the same request.
///
/// The combined request is:
/// ```sparql
/// INSERT DATA { <a> <p> <b> . } ;
/// INSERT { <b> <q> <c> . } WHERE { <a> <p> ?x }
/// ```
///
/// After the first statement, `<a> <p> <b>` is in the store.  The WHERE clause
/// of the second statement evaluates against the updated store, finds the triple,
/// and fires the INSERT.  After the request, `<b> <q> <c>` must be present.
///
/// Implements SPARQL 1.1 Update §3.1.3 "Semantics of SPARQL Update Sequences".
///
/// Related: [#114](https://github.com/daghovland/rdf-datalog/issues/114),
///          [#123](https://github.com/daghovland/rdf-datalog/issues/123)
#[tokio::test]
async fn test_intra_request_pattern_update_sees_preceding_insert() {
    let server = common::TestServer::start_writable("").await;

    let update = "\
        INSERT DATA { <http://example.org/a> <http://example.org/p> <http://example.org/b> . } ;\
        INSERT { <http://example.org/b> <http://example.org/q> <http://example.org/c> . } \
        WHERE  { <http://example.org/a> <http://example.org/p> ?x }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        204,
        "combined INSERT DATA ; INSERT WHERE must return 204 No Content"
    );

    // The derived triple must be present after the request.
    let ask = "ASK { <http://example.org/b> <http://example.org/q> <http://example.org/c> }";
    let resp = server
        .client
        .get(server.sparql_query_url(ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("response body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "<b> <q> <c> must be present: PatternUpdate WHERE clause must see \
         the preceding INSERT DATA result"
    );
}

// ── Test 3: Intra-request delete visibility ───────────────────────────────────

/// A DELETE WHERE in the same request must see and erase a triple that was
/// inserted by an earlier INSERT DATA in the same request.
///
/// The combined request is:
/// ```sparql
/// INSERT DATA { <s> <p> <o> . } ;
/// DELETE { <s> <p> ?o } WHERE { <s> <p> ?o }
/// ```
///
/// The WHERE clause of the DELETE fires against the post-INSERT state of the
/// store, binds `?o = <o>`, and deletes the triple.  After the request, the
/// triple must not be present.
///
/// Implements SPARQL 1.1 Update §3.1.3.
///
/// Related: [#114](https://github.com/daghovland/rdf-datalog/issues/114),
///          [#123](https://github.com/daghovland/rdf-datalog/issues/123)
#[tokio::test]
async fn test_intra_request_delete_hides_triple_from_where() {
    let server = common::TestServer::start_writable("").await;

    let update = "\
        INSERT DATA { <http://example.org/s> <http://example.org/p> <http://example.org/o> . } ;\
        DELETE { <http://example.org/s> <http://example.org/p> ?o } \
        WHERE  { <http://example.org/s> <http://example.org/p> ?o }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        204,
        "combined INSERT DATA ; DELETE WHERE must return 204 No Content"
    );

    // The triple must have been deleted.
    let ask = "ASK { <http://example.org/s> <http://example.org/p> <http://example.org/o> }";
    let resp = server
        .client
        .get(server.sparql_query_url(ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("response body must be JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "<s> <p> <o> must not be present: DELETE WHERE must have matched \
         and removed the triple inserted by the preceding INSERT DATA"
    );
}

// ── Test 4: Failed update must roll back (currently unimplemented) ────────────

/// A partially-executed Update request must be fully rolled back on failure.
///
/// The request first INSERTs a triple, then issues a non-SILENT LOAD of a
/// nonexistent URL.  The LOAD should fail, making the whole request fail with a
/// 4xx or 5xx response.  After the failure, the store must be empty — the
/// INSERT DATA must have been rolled back.
///
/// **This test is currently ignored** because rollback is not yet implemented:
/// the INSERT DATA is applied immediately and is not reverted when the
/// subsequent LOAD fails (issue [#126](https://github.com/daghovland/rdf-datalog/issues/126)).
/// Additionally, the current LOAD implementation is a no-op (returns 204), so
/// the request does not fail as expected.
///
/// Un-ignore this test once [#126](https://github.com/daghovland/rdf-datalog/issues/126)
/// is resolved.
///
/// Related: [#123](https://github.com/daghovland/rdf-datalog/issues/123),
///          [#126](https://github.com/daghovland/rdf-datalog/issues/126)
#[tokio::test]
#[ignore = "requires #126 rollback fix"]
async fn test_failed_update_leaves_store_unchanged() {
    let server = common::TestServer::start_writable("").await;

    // First statement succeeds (INSERT DATA), second statement should fail
    // (non-SILENT LOAD of a nonexistent URL with the default NetworkPolicy::Deny).
    // The entire request should fail with 4xx or 5xx.
    let update = "\
        INSERT DATA { <http://example.org/s> <http://example.org/p> <http://example.org/o> . } ;\
        LOAD <http://example.org/nonexistent.ttl>";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");

    assert!(
        resp.status().is_client_error() || resp.status().is_server_error(),
        "a LOAD of a nonexistent URL must fail with 4xx or 5xx, got {}",
        resp.status()
    );

    // After the failed request, the store must be empty — the INSERT DATA
    // must have been rolled back.
    let count_query = "SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }";
    let resp = server
        .client
        .get(server.sparql_query_url(count_query))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET COUNT failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("response body must be JSON");
    let n: u64 = body["results"]["bindings"]
        .get(0)
        .and_then(|b| b["n"]["value"].as_str())
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    assert_eq!(
        n, 0,
        "store must be empty after a rolled-back update, found {n} triple(s)"
    );
}

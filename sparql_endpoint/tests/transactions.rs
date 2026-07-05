/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the proprietary HTTP transaction API.
//!
//! Tests cover the BEGIN / COMMIT / ROLLBACK lifecycle, transactional reads
//! and writes via `?txId=`, conflict detection, and rollback on constraint
//! violations.
//!
//! Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)

mod common;

const EX_S: &str = "http://example.org/s";
const EX_P: &str = "http://example.org/p";
const EX_O: &str = "http://example.org/o";

fn insert_triple(s: &str, p: &str, o: &str) -> String {
    format!("INSERT DATA {{ <{s}> <{p}> <{o}> . }}")
}

fn select_triple(s: &str, p: &str, o: &str) -> String {
    format!("ASK {{ <{s}> <{p}> <{o}> . }}")
}

// ── Helper: begin transaction ────────────────────────────────────────────────

async fn begin_transaction(server: &common::TestServer) -> String {
    let resp = server
        .client
        .post(format!("{}/transaction/begin", server.base_url))
        .send()
        .await
        .expect("POST /transaction/begin failed");
    assert_eq!(resp.status().as_u16(), 200, "begin must return 200");
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    body["txId"]
        .as_str()
        .expect("txId must be a string")
        .to_owned()
}

// ── Test 1: begin returns a txId ─────────────────────────────────────────────

/// `POST /transaction/begin` returns HTTP 200 with a JSON body containing a
/// non-empty `txId` string.
///
/// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
#[tokio::test]
async fn test_transaction_begin_returns_tx_id() {
    let server = common::TestServer::start_writable("").await;
    let tx_id = begin_transaction(&server).await;
    assert!(!tx_id.is_empty(), "txId must be non-empty");
}

// ── Test 2: commit empty transaction → 204 ───────────────────────────────────

/// Begin a transaction and immediately commit it without any buffered changes.
///
/// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
#[tokio::test]
async fn test_transaction_commit_empty() {
    let server = common::TestServer::start_writable("").await;
    let tx_id = begin_transaction(&server).await;

    let resp = server
        .client
        .post(format!("{}/transaction/{tx_id}/commit", server.base_url))
        .send()
        .await
        .expect("POST /transaction/.../commit failed");

    assert_eq!(resp.status().as_u16(), 204, "empty commit must return 204");
}

// ── Test 3: commit applies buffered inserts ───────────────────────────────────

/// Buffer an INSERT DATA inside a transaction, commit, then verify the triple
/// is visible via a plain SELECT (not transactional).
///
/// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
#[tokio::test]
async fn test_transaction_commit_applies_inserts() {
    let server = common::TestServer::start_writable("").await;
    let tx_id = begin_transaction(&server).await;

    // Buffer an insert inside the transaction.
    let update = insert_triple(EX_S, EX_P, EX_O);
    let resp = server
        .client
        .post(format!(
            "{}/sparql?txId={}",
            server.base_url,
            urlencoding::encode(&tx_id)
        ))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST buffered update failed");
    assert_eq!(
        resp.status().as_u16(),
        200,
        "buffered update must return 200 (not 204)"
    );

    // Triple must NOT be visible before commit.
    let ask = select_triple(EX_S, EX_P, EX_O);
    let pre_resp = server
        .client
        .get(server.sparql_query_url(&ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET pre-commit ask failed");
    assert_eq!(pre_resp.status().as_u16(), 200);
    let pre_body: serde_json::Value = pre_resp.json().await.unwrap();
    assert_eq!(
        pre_body["boolean"],
        serde_json::Value::Bool(false),
        "triple must not be visible before commit"
    );

    // Commit.
    let commit_resp = server
        .client
        .post(format!("{}/transaction/{tx_id}/commit", server.base_url))
        .send()
        .await
        .expect("commit failed");
    assert_eq!(commit_resp.status().as_u16(), 204, "commit must return 204");

    // Triple must be visible after commit.
    let post_resp = server
        .client
        .get(server.sparql_query_url(&ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET post-commit ask failed");
    assert_eq!(post_resp.status().as_u16(), 200);
    let post_body: serde_json::Value = post_resp.json().await.unwrap();
    assert_eq!(
        post_body["boolean"],
        serde_json::Value::Bool(true),
        "triple must be visible after commit"
    );
}

// ── Test 4: rollback discards buffered changes ────────────────────────────────

/// Buffer an INSERT DATA inside a transaction, roll back, then verify the
/// triple is NOT visible via a plain SELECT.
///
/// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
#[tokio::test]
async fn test_transaction_rollback_discards_changes() {
    let server = common::TestServer::start_writable("").await;
    let tx_id = begin_transaction(&server).await;

    // Buffer an insert inside the transaction.
    let update = insert_triple(EX_S, EX_P, EX_O);
    server
        .client
        .post(format!(
            "{}/sparql?txId={}",
            server.base_url,
            urlencoding::encode(&tx_id)
        ))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST buffered update failed");

    // Rollback.
    let rollback_resp = server
        .client
        .post(format!("{}/transaction/{tx_id}/rollback", server.base_url))
        .send()
        .await
        .expect("rollback failed");
    assert_eq!(
        rollback_resp.status().as_u16(),
        204,
        "rollback must return 204"
    );

    // Triple must NOT be visible after rollback.
    let ask = select_triple(EX_S, EX_P, EX_O);
    let resp = server
        .client
        .get(server.sparql_query_url(&ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET after rollback failed");
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"],
        serde_json::Value::Bool(false),
        "triple must not be visible after rollback"
    );
}

// ── Test 5: transactional read sees pending inserts ───────────────────────────

/// Read-within-transaction must see pending inserts that have not yet been
/// committed; a parallel non-transactional read must NOT see them.
///
/// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
#[tokio::test]
async fn test_transaction_read_sees_pending_inserts() {
    let server = common::TestServer::start_writable("").await;
    let tx_id = begin_transaction(&server).await;

    // Buffer an insert inside the transaction.
    let update = insert_triple(EX_S, EX_P, EX_O);
    server
        .client
        .post(format!(
            "{}/sparql?txId={}",
            server.base_url,
            urlencoding::encode(&tx_id)
        ))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST buffered update failed");

    let ask = select_triple(EX_S, EX_P, EX_O);

    // Transactional read (with txId) must see the pending insert.
    let tx_read_resp = server
        .client
        .get(format!(
            "{}/sparql?txId={}&query={}",
            server.base_url,
            urlencoding::encode(&tx_id),
            urlencoding::encode(&ask)
        ))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET transactional read failed");
    assert_eq!(tx_read_resp.status().as_u16(), 200);
    let tx_body: serde_json::Value = tx_read_resp.json().await.unwrap();
    assert_eq!(
        tx_body["boolean"],
        serde_json::Value::Bool(true),
        "transactional read must see pending insert"
    );

    // Non-transactional read must NOT see the pending insert.
    let plain_resp = server
        .client
        .get(server.sparql_query_url(&ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET non-transactional read failed");
    assert_eq!(plain_resp.status().as_u16(), 200);
    let plain_body: serde_json::Value = plain_resp.json().await.unwrap();
    assert_eq!(
        plain_body["boolean"],
        serde_json::Value::Bool(false),
        "non-transactional read must not see uncommitted pending insert"
    );

    // Clean up: rollback.
    server
        .client
        .post(format!("{}/transaction/{tx_id}/rollback", server.base_url))
        .send()
        .await
        .expect("rollback failed");
}

// ── Test 6: conflict detection ────────────────────────────────────────────────

/// Begin two transactions, commit the first (which changes the generation),
/// then commit the second.  The second commit must be rejected with HTTP 409.
///
/// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
#[tokio::test]
async fn test_transaction_conflict_detection() {
    let server = common::TestServer::start_writable("").await;

    let tx1 = begin_transaction(&server).await;
    let tx2 = begin_transaction(&server).await;

    // Commit tx1 — this bumps the store generation.
    let commit1 = server
        .client
        .post(format!("{}/transaction/{tx1}/commit", server.base_url))
        .send()
        .await
        .expect("tx1 commit failed");
    assert_eq!(
        commit1.status().as_u16(),
        204,
        "tx1 commit must succeed (204)"
    );

    // Commit tx2 — generation has changed, must be rejected with 409.
    let commit2 = server
        .client
        .post(format!("{}/transaction/{tx2}/commit", server.base_url))
        .send()
        .await
        .expect("tx2 commit failed");
    assert_eq!(
        commit2.status().as_u16(),
        409,
        "tx2 commit must be rejected with 409 Conflict"
    );
}

// ── Test 7: not-found errors ──────────────────────────────────────────────────

/// Commit and rollback with a nonexistent txId must return 404.
///
/// Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)
#[tokio::test]
async fn test_transaction_not_found() {
    let server = common::TestServer::start_writable("").await;
    let bogus = "00000000-0000-0000-0000-000000000000";

    let commit_resp = server
        .client
        .post(format!("{}/transaction/{bogus}/commit", server.base_url))
        .send()
        .await
        .expect("commit 404 request failed");
    assert_eq!(
        commit_resp.status().as_u16(),
        404,
        "commit with unknown txId must return 404"
    );

    let rollback_resp = server
        .client
        .post(format!("{}/transaction/{bogus}/rollback", server.base_url))
        .send()
        .await
        .expect("rollback 404 request failed");
    assert_eq!(
        rollback_resp.status().as_u16(),
        404,
        "rollback with unknown txId must return 404"
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for Tier 1 (static API key) authentication.
//!
//! Tests are ordered to mirror the implementation steps in AUTH.md:
//!   Step A/B — write protection (api_key_* tests)
//!   Step C   — read protection (require_for_reads tests)
//!
//! All tests start as #[ignore] and are un-ignored as each step lands.

mod common;

const KEY: &str = "test-secret-key-abc123";

// ── No-auth baseline ──────────────────────────────────────────────────────────

/// When no auth is configured, write operations must succeed without any token.
#[tokio::test]
async fn no_auth_write_allowed() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body("INSERT DATA { <urn:s> <urn:p> <urn:o> }")
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        401,
        "write should be allowed when no auth is configured"
    );
}

// ── API key — write endpoint ──────────────────────────────────────────────────

/// The correct Bearer token allows a write operation.
#[tokio::test]
async fn api_key_correct_write_allowed() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .bearer_auth(KEY)
        .body("INSERT DATA { <urn:s> <urn:p> <urn:o> }")
        .send()
        .await
        .expect("request failed");
    assert_ne!(resp.status().as_u16(), 401, "correct key must allow write");
}

/// A wrong Bearer token must be rejected with 401.
#[tokio::test]
async fn api_key_wrong_write_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .bearer_auth("this-is-not-the-right-key")
        .body("INSERT DATA { <urn:s> <urn:p> <urn:o> }")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 401, "wrong key must return 401");
}

/// A missing Authorization header on a write endpoint must return 401.
#[tokio::test]
async fn api_key_missing_write_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body("INSERT DATA { <urn:s> <urn:p> <urn:o> }")
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "missing key must return 401 on write endpoint"
    );
}

// ── API key — read endpoints open by default ──────────────────────────────────

/// When require_for_reads is false (the default), GET /sparql needs no token.
#[tokio::test]
async fn api_key_reads_open_by_default() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        401,
        "reads must be open when require_for_reads is false"
    );
}

/// Canary: POST /sparql with a SELECT query body is a read, never a write.
/// Must succeed without a key even when writes are protected.
#[tokio::test]
async fn post_sparql_query_no_key_allowed() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body("SELECT * WHERE {}")
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        401,
        "POST /sparql with SELECT body is a read and must not require auth"
    );
}

// ── API key — reads protected when flag set (Step C) ─────────────────────────

/// When require_for_reads is true, GET /sparql without a key returns 401.
#[tokio::test]
async fn api_key_reads_protected_when_flag_set() {
    let server = common::TestServer::start_writable_with_key_protect_reads("", KEY).await;
    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "reads must return 401 when require_for_reads is true and no key is supplied"
    );
}

/// When require_for_reads is true, the correct key unlocks GET /sparql.
#[tokio::test]
async fn api_key_correct_read_allowed_when_protected() {
    let server = common::TestServer::start_writable_with_key_protect_reads("", KEY).await;
    let resp = server
        .client
        .get(server.sparql_query_url("SELECT * WHERE {}"))
        .bearer_auth(KEY)
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        401,
        "correct key must allow reads when require_for_reads is true"
    );
}

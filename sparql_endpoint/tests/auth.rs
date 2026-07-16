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

// ── Issue #163 — SPARQL Update smuggled through Read-classified POST /sparql ──
//
// `classify()` maps every POST to `/sparql`, `/{name}/sparql`, `/{name}/query`
// to `Permission::Read`, but the handler dispatches update bodies (raw
// `application/sparql-update`, or form `update=`) to `run_update`, which
// mutates the store with no independent permission check. With
// `start_writable_with_key` (require_for_reads: false), Read needs no token
// at all, so an unauthenticated caller could smuggle a full write through
// these "read" endpoints. See https://github.com/daghovland/rdf-datalog/issues/163

const SMUGGLED_UPDATE: &str = "INSERT DATA { <urn:s163> <urn:p163> <urn:o163> }";

/// Confirm the store was NOT mutated by a rejected smuggled update, by
/// checking the triple is absent via an authenticated ASK query.
async fn assert_smuggled_triple_absent(server: &common::TestServer) {
    let resp = server
        .client
        .get(server.sparql_query_url("ASK { <urn:s163> <urn:p163> <urn:o163> }"))
        .bearer_auth(KEY)
        .send()
        .await
        .expect("ASK request failed");
    let body: serde_json::Value = resp.json().await.expect("ASK body must be JSON");
    assert_eq!(
        body["boolean"], false,
        "smuggled update must not have mutated the store"
    );
}

/// A raw `application/sparql-update` body POSTed to `/sparql` with no key
/// must be rejected with 401, not executed as a write.
#[tokio::test]
async fn post_sparql_update_raw_body_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(SMUGGLED_UPDATE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated SPARQL Update body on POST /sparql must be rejected"
    );
    assert_smuggled_triple_absent(&server).await;
}

/// A form-encoded `update=` body POSTed to `/sparql` with no key must be
/// rejected with 401, not executed as a write.
#[tokio::test]
async fn post_sparql_update_form_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("update={}", urlencoding::encode(SMUGGLED_UPDATE)))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated form-encoded SPARQL Update on POST /sparql must be rejected"
    );
    assert_smuggled_triple_absent(&server).await;
}

/// Same bug via the Fuseki-compatible `/{name}/sparql` alias.
#[tokio::test]
async fn post_dataset_sparql_update_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.dataset_sparql_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(SMUGGLED_UPDATE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated SPARQL Update body on POST /{{name}}/sparql must be rejected"
    );
}

/// Same bug via the Fuseki-compatible `/{name}/query` alias.
#[tokio::test]
async fn post_dataset_query_update_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.dataset_query_url("ds"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("update={}", urlencoding::encode(SMUGGLED_UPDATE)))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated form-encoded SPARQL Update on POST /{{name}}/query must be rejected"
    );
}

// ── Regression: the fix must not mangle the body for the read case ──────────

/// A form-encoded `query=` body (a genuine read) on POST /sparql must still
/// reach the handler intact and return results, with no key required.
///
/// This exercises the body-buffering/reconstruction path added to detect
/// smuggled updates — it must not swallow or corrupt the body for reads.
#[tokio::test]
async fn post_sparql_query_form_no_key_allowed_and_intact() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!(
            "query={}",
            urlencoding::encode("SELECT * WHERE {}")
        ))
        .send()
        .await
        .expect("request failed");
    assert_ne!(
        resp.status().as_u16(),
        401,
        "form-encoded SELECT body on POST /sparql is a read and must not require auth"
    );
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["results"]["bindings"].is_array(),
        "SELECT body must have reached the handler intact, got: {body:?}"
    );
}

/// A form-encoded `update=` body WITH the correct key must still reach
/// `run_update` and actually apply — the fix must not break the legitimate
/// authenticated write path while closing the unauthenticated one.
#[tokio::test]
async fn post_sparql_update_form_with_key_succeeds() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/x-www-form-urlencoded")
        .bearer_auth(KEY)
        .body(format!("update={}", urlencoding::encode(SMUGGLED_UPDATE)))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        204,
        "authenticated form-encoded update must be applied, got {}",
        resp.status()
    );

    let ask = server
        .client
        .get(server.sparql_query_url("ASK { <urn:s163> <urn:p163> <urn:o163> }"))
        .bearer_auth(KEY)
        .send()
        .await
        .expect("ASK request failed");
    let body: serde_json::Value = ask.json().await.expect("ASK body must be JSON");
    assert_eq!(
        body["boolean"], true,
        "authenticated update must have applied to the store"
    );
}

// ── Issue #163 — /transaction/* fail open to Read ────────────────────────────
//
// `classify()` has no branch for `/transaction/...`, so begin/commit/rollback
// all fall through to the default `Permission::Read`. These are writes (or
// write-adjacent) and must require Write permission.

#[tokio::test]
async fn post_transaction_begin_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.transaction_begin_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated POST /transaction/begin must be rejected"
    );
}

#[tokio::test]
async fn post_transaction_commit_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.transaction_commit_url("nonexistent-tx"))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated POST /transaction/{{txId}}/commit must be rejected"
    );
}

#[tokio::test]
async fn post_transaction_rollback_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.transaction_rollback_url("nonexistent-tx"))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated POST /transaction/{{txId}}/rollback must be rejected"
    );
}

// ── Issue #163 — POST /$/compact fails open to Read ──────────────────────────
//
// `admin_compact` rewrites the persistence changelog — an admin-tier
// operation with no matching `classify()` branch, so it falls through to Read.

#[tokio::test]
async fn post_compact_no_key_returns_401() {
    let server = common::TestServer::start_writable_with_key("", KEY).await;
    let resp = server
        .client
        .post(server.admin_compact_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated POST /$/compact must be rejected"
    );
}

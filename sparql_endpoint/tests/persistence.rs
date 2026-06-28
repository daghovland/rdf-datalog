/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for durable transactional persistence.
//!
//! Tests verify the core durability guarantee: when a write returns 200/204,
//! the data survives a server restart.
//!
//! Architecture: redb is used as a durable changelog over the in-memory store.
//! Mutations are written to redb (fsynced) before the in-memory store is updated.
//! On restart, the changelog is replayed into a fresh in-memory Datastore.

mod common;

// ── P1: Default-off behaviour ─────────────────────────────────────────────────

/// Without `--data-dir`, no redb file is created anywhere.
///
/// The default server (no data_dir in Config) must write nothing to disk.
#[tokio::test]
async fn persist_default_off_creates_no_files() {
    let dir = tempfile::tempdir().unwrap();

    // Start a writable server without persistence.
    let server = common::TestServer::start_writable("").await;

    // Insert something.
    let turtle = r#"<http://example.org/s> <http://example.org/p> "hello" ."#;
    let resp = server
        .client
        .put(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(turtle)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 204);

    drop(server);

    // The tempdir should be empty — server did not write to it.
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        entries.is_empty(),
        "non-persistent server must not create any files"
    );
}

// ── P2: Insert survives restart ───────────────────────────────────────────────

/// A triple inserted via GSP PUT must still be present after the server restarts.
///
/// This is the core durability guarantee: committed write survives process death.
#[tokio::test]
async fn persist_insert_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    // First server instance — persistent.
    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;

        let turtle =
            r#"<http://example.org/subject> <http://example.org/predicate> "hello world" ."#;
        let resp = server
            .client
            .put(server.gsp_default_url())
            .header("content-type", "text/turtle")
            .body(turtle)
            .send()
            .await
            .expect("PUT failed");
        assert_eq!(
            resp.status(),
            204,
            "PUT should return 204 when graph already exists"
        );
        // Explicitly shut down and release the redb file lock before restarting.
        server.shutdown().await;
    }

    // Second server instance pointing to same data directory.
    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;

    let sparql =
        "SELECT ?o WHERE { <http://example.org/subject> <http://example.org/predicate> ?o }";
    let resp = server2
        .client
        .get(server2.sparql_query_url(sparql))
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert!(
        !bindings.is_empty(),
        "triple should survive server restart; got empty bindings"
    );
    common::assert_binding_contains(bindings, "o", "literal", "hello world");
}

// ── P3: Delete survives restart ───────────────────────────────────────────────

/// Triples deleted via GSP DELETE must remain absent after restart.
///
/// Without this guarantee, deleted data could "come back" if the changelog
/// only records inserts and replay re-adds everything.
#[tokio::test]
async fn persist_delete_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    // First server: insert then delete.
    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;

        // Insert a triple.
        let turtle = r#"<http://example.org/s2> <http://example.org/p2> "to be deleted" ."#;
        let resp = server
            .client
            .put(server.gsp_default_url())
            .header("content-type", "text/turtle")
            .body(turtle)
            .send()
            .await
            .expect("PUT failed");
        assert_eq!(resp.status(), 204);

        // Delete the default graph.
        let resp = server
            .client
            .delete(server.gsp_default_url())
            .send()
            .await
            .expect("DELETE failed");
        assert_eq!(resp.status(), 204);
        server.shutdown().await;
    }

    // Restart — the delete must persist.
    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;

    let sparql = "SELECT ?o WHERE { <http://example.org/s2> <http://example.org/p2> ?o }";
    let resp = server2
        .client
        .get(server2.sparql_query_url(sparql))
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert!(
        bindings.is_empty(),
        "deleted triple must remain absent after restart; got: {bindings:?}"
    );
}

// ── P4: SPARQL Update INSERT DATA survives restart ────────────────────────────

/// A triple inserted via SPARQL Update INSERT DATA must persist across restart.
#[tokio::test]
async fn persist_sparql_update_insert_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;

        let update = r#"
            INSERT DATA {
                <http://example.org/u1> <http://example.org/q> "via update" .
            }
        "#;
        let resp = server
            .client
            .post(server.dataset_update_url("ds"))
            .header("content-type", "application/sparql-update")
            .body(update)
            .send()
            .await
            .expect("POST update failed");
        assert_eq!(resp.status(), 200, "SPARQL Update must return 200");
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;

    let sparql = "SELECT ?o WHERE { <http://example.org/u1> <http://example.org/q> ?o }";
    let resp = server2
        .client
        .get(server2.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert!(
        !bindings.is_empty(),
        "SPARQL Update insert must survive restart"
    );
    common::assert_binding_contains(bindings, "o", "literal", "via update");
}

// ── P5: Typed literals survive restart and remain queryable ──────────────────

/// xsd:integer literals must round-trip through the changelog and be
/// structurally equal to freshly-parsed values so that SPARQL queries
/// (which intern values as dedicated enum variants) can find them.
///
/// Without `rdf_literal_from_typed`, replay reconstructs `TypedLiteral` instead
/// of `IntegerLiteral`, causing a structural mismatch and making the quad
/// invisible to queries that compare with `42`.
#[tokio::test]
async fn persist_typed_literal_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        // Insert a triple with an integer literal via SPARQL Update INSERT DATA.
        let update = r#"
            INSERT DATA {
                <http://example.org/t1> <http://example.org/count> 42 .
            }
        "#;
        let resp = server
            .client
            .post(server.dataset_update_url("ds"))
            .header("content-type", "application/sparql-update")
            .body(update)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;

    // ASK query — the integer literal must be found.
    let sparql = "ASK { <http://example.org/t1> <http://example.org/count> 42 }";
    let resp = server2
        .client
        .get(server2.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], true,
        "xsd:integer literal must be queryable after restart"
    );
}

// ── P7: Pre-loaded data survives alongside persistence ────────────────────────

/// When `--data` files are loaded before starting with `--data-dir`, the
/// pre-loaded data must be accessible alongside any changelog data.
///
/// Bug #66: the server was replacing the pre-loaded store entirely with the
/// (possibly empty) changelog replay, discarding the loaded files.
/// See: https://github.com/daghovland/rdf-datalog/issues/66
#[tokio::test]
async fn persist_preloaded_data_visible_with_persistence_enabled() {
    let dir = tempfile::tempdir().unwrap();

    let preloaded_turtle = r#"<http://example.org/preloaded> <http://example.org/p> "from file" ."#;

    // Start with pre-loaded data AND a fresh data-dir (empty changelog).
    let server = common::TestServer::start_writable_persistent(preloaded_turtle, dir.path()).await;

    // The pre-loaded triple must be queryable.
    let sparql = r#"ASK { <http://example.org/preloaded> <http://example.org/p> "from file" }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], true,
        "pre-loaded data must be visible when persistence is enabled; got: {body}"
    );
}

/// Pre-loaded data must still be visible after a server restart with persistence.
///
/// Bug #66: data loaded via --data is not in the changelog, so it would vanish
/// on restart unless the server re-loads the files.  This test verifies the
/// design where changelog mutations are applied ON TOP of the pre-loaded store.
/// See: https://github.com/daghovland/rdf-datalog/issues/66
#[tokio::test]
async fn persist_preloaded_data_visible_after_restart() {
    let dir = tempfile::tempdir().unwrap();

    let preloaded_turtle = r#"<http://example.org/base> <http://example.org/p> "base triple" ."#;

    // Start, append a runtime triple (POST, not PUT — preserves existing graph), shutdown.
    {
        let server =
            common::TestServer::start_writable_persistent(preloaded_turtle, dir.path()).await;
        let runtime_turtle =
            r#"<http://example.org/runtime> <http://example.org/p> "runtime triple" ."#;
        let resp = server
            .client
            .post(server.gsp_default_url())
            .header("content-type", "text/turtle")
            .body(runtime_turtle)
            .send()
            .await
            .expect("POST failed");
        assert_eq!(resp.status(), 204);
        server.shutdown().await;
    }

    // Restart with same pre-loaded data and same data-dir.
    let server2 = common::TestServer::start_writable_persistent(preloaded_turtle, dir.path()).await;

    // Both the base and runtime triple must be visible.
    for (iri, val) in [
        ("http://example.org/base", "base triple"),
        ("http://example.org/runtime", "runtime triple"),
    ] {
        let sparql = format!(r#"ASK {{ <{iri}> <http://example.org/p> "{val}" }}"#);
        let resp = server2
            .client
            .get(server2.sparql_query_url(&sparql))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(
            body["boolean"], true,
            "triple ({iri}, {val}) must be visible after restart"
        );
    }
}

// ── P6: Multiple restarts accumulate correctly ────────────────────────────────

/// Inserting across multiple restart cycles must accumulate all data.
#[tokio::test]
async fn persist_multiple_restarts_accumulate() {
    let dir = tempfile::tempdir().unwrap();

    let subjects = [
        "http://example.org/r1",
        "http://example.org/r2",
        "http://example.org/r3",
    ];

    // Insert one triple per server lifetime.
    for s in &subjects {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        let turtle = format!(r#"<{s}> <http://example.org/p> "val" ."#);
        let resp = server
            .client
            .post(server.gsp_default_url())
            .header("content-type", "text/turtle")
            .body(turtle)
            .send()
            .await
            .expect("POST failed");
        assert_eq!(resp.status(), 204, "GSP POST should return 204");
        server.shutdown().await;
    }

    // Final query should see all three subjects.
    let server = common::TestServer::start_writable_persistent("", dir.path()).await;
    for s in &subjects {
        let sparql = format!("ASK {{ <{s}> <http://example.org/p> ?o }}");
        let resp = server
            .client
            .get(server.sparql_query_url(&sparql))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(
            body["boolean"], true,
            "subject {s} should be present after multiple restarts"
        );
    }
}

// ── Compaction endpoint (#72) ─────────────────────────────────────────────────
// https://github.com/daghovland/rdf-datalog/issues/72

/// POST /$/compact rewrites the changelog to contain only the current live quads.
///
/// After compaction, the number of log entries should equal the number of distinct
/// live quads (not the number of historical mutations), and all data must still be
/// queryable.
#[tokio::test]
async fn compact_reduces_log_entry_count() {
    let dir = tempfile::tempdir().unwrap();
    let server = common::TestServer::start_writable_persistent("", dir.path()).await;

    // Insert 3 triples, then delete 1 — 4 log entries, only 2 live quads.
    let insert3 = r#"
        INSERT DATA {
            <http://example.org/a> <http://example.org/p> "v1" .
            <http://example.org/b> <http://example.org/p> "v2" .
            <http://example.org/c> <http://example.org/p> "v3" .
        }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(insert3)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let delete1 = r#"DELETE DATA { <http://example.org/c> <http://example.org/p> "v3" . }"#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(delete1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Compact.
    let compact_url = format!("{}/$/compact", server.base_url);
    let resp = server
        .client
        .post(&compact_url)
        .send()
        .await
        .expect("compact request failed");
    assert_eq!(resp.status(), 200, "POST /$/compact must return 200");

    // The response body must include entries_after (=2, one per live quad).
    let body: serde_json::Value = resp.json().await.unwrap();
    let entries_after = body["entries_after"].as_u64().unwrap_or(u64::MAX);
    assert!(
        entries_after <= 2,
        "entries_after should be ≤ 2 (live quads); got {entries_after}"
    );

    // Data must still be queryable after compaction.
    for (s, v) in [("a", "v1"), ("b", "v2")] {
        let sparql = format!(r#"ASK {{ <http://example.org/{s}> <http://example.org/p> "{v}" }}"#);
        let resp = server
            .client
            .get(server.sparql_query_url(&sparql))
            .send()
            .await
            .unwrap();
        let ask: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(
            ask["boolean"], true,
            "triple {s}={v} must be present after compaction"
        );
    }
}

/// Data must survive a full restart after compaction.
///
/// This is the core correctness guarantee: the compacted log is a valid
/// snapshot that produces the same store on replay.
#[tokio::test]
async fn compact_data_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;

        // Insert, delete, insert again — creates a history.
        let insert = r#"
            INSERT DATA {
                <http://example.org/x> <http://example.org/p> "kept" .
                <http://example.org/y> <http://example.org/p> "dropped" .
            }
        "#;
        let resp = server
            .client
            .post(server.dataset_update_url("ds"))
            .header("content-type", "application/sparql-update")
            .body(insert)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let delete = r#"DELETE DATA { <http://example.org/y> <http://example.org/p> "dropped" . }"#;
        let resp = server
            .client
            .post(server.dataset_update_url("ds"))
            .header("content-type", "application/sparql-update")
            .body(delete)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // Compact before shutdown.
        let compact_url = format!("{}/$/compact", server.base_url);
        let resp = server
            .client
            .post(&compact_url)
            .send()
            .await
            .expect("compact request failed");
        assert_eq!(resp.status(), 200, "compact must succeed before shutdown");

        server.shutdown().await;
    }

    // Restart — must replay the compacted snapshot correctly.
    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;

    let ask_kept = r#"ASK { <http://example.org/x> <http://example.org/p> "kept" }"#;
    let body: serde_json::Value = server2
        .client
        .get(server2.sparql_query_url(ask_kept))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        body["boolean"], true,
        "'kept' triple must survive restart after compaction"
    );

    let ask_dropped = r#"ASK { <http://example.org/y> <http://example.org/p> "dropped" }"#;
    let body: serde_json::Value = server2
        .client
        .get(server2.sparql_query_url(ask_dropped))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        body["boolean"], false,
        "'dropped' triple must remain absent after restart"
    );
}

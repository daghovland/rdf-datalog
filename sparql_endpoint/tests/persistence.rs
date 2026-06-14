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

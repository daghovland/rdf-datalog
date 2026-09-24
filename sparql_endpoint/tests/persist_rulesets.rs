/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for persisting runtime-loaded rulesets across a server
//! restart, per [#475](https://github.com/daghovland/rdf-datalog/issues/475).
//!
//! See [`docs/plans/PERSIST_RULESETS_475_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/PERSIST_RULESETS_475_PLAN.md)
//! for the design.

mod common;

const MANAGER_IMPLIES_EMPLOYEE: &str = r#"
PREFIX ex: <http://ex/>
ex:Employee[?x] :- ex:Manager[?x] .
"#;

const MANAGER_IMPLIES_CONTRACTOR: &str = r#"
PREFIX ex: <http://ex/>
ex:Contractor[?x] :- ex:Manager[?x] .
"#;

async fn ask(server: &common::TestServer, dataset: &str, sparql: &str) -> bool {
    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(dataset),
        urlencoding::encode(sparql)
    );
    let resp = server
        .client
        .get(url)
        .header("Accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query request failed");
    assert!(
        resp.status().is_success(),
        "query failed: {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.expect("invalid JSON");
    body["boolean"].as_bool().expect("boolean field")
}

async fn seed_alice_manager(server: &common::TestServer) {
    let turtle = r#"<http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Manager> ."#;
    // Use GSP POST so we don't clobber other test data in the same dataset.
    let resp = server
        .client
        .post(server.dataset_data_default_url("ds"))
        .header("content-type", "text/turtle")
        .body(turtle)
        .send()
        .await
        .expect("seed POST failed");
    assert!(resp.status().is_success(), "seed failed: {}", resp.status());
}

// ── R1: full-replace ruleset survives restart ─────────────────────────────────

#[tokio::test]
async fn persist_replace_ruleset_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        seed_alice_manager(&server).await;

        let resp = server
            .client
            .post(server.dataset_rules_url("ds"))
            .header("content-type", "text/x-datalog")
            .body(MANAGER_IMPLIES_EMPLOYEE)
            .send()
            .await
            .expect("POST rules failed");
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Employee> }"
        )
        .await,
        "ruleset-derived fact must survive restart"
    );
}

// ── R2: second full-replace overrides the first, in order, across restart ────

#[tokio::test]
async fn persist_ruleset_replace_then_replace_again_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        seed_alice_manager(&server).await;

        let resp = server
            .client
            .post(server.dataset_rules_url("ds"))
            .header("content-type", "text/x-datalog")
            .body(MANAGER_IMPLIES_EMPLOYEE)
            .send()
            .await
            .expect("POST rules failed");
        assert_eq!(resp.status(), 200);

        // Replace with a different ruleset -- must win on replay.
        let resp = server
            .client
            .post(server.dataset_rules_url("ds"))
            .header("content-type", "text/x-datalog")
            .body(MANAGER_IMPLIES_CONTRACTOR)
            .send()
            .await
            .expect("POST rules failed");
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        !ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Employee> }"
        )
        .await,
        "first (overridden) ruleset's derivation must not survive"
    );
    assert!(
        ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Contractor> }"
        )
        .await,
        "second (final) ruleset's derivation must survive restart"
    );
}

// ── R3: named/scoped ruleset survives restart ─────────────────────────────────

#[tokio::test]
async fn persist_named_ruleset_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        seed_alice_manager(&server).await;

        let resp = server
            .client
            .post(server.dataset_rules_id_url("ds", "a"))
            .header("content-type", "text/x-datalog")
            .body(MANAGER_IMPLIES_EMPLOYEE)
            .send()
            .await
            .expect("POST rules/a failed");
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Employee> }"
        )
        .await,
        "named ruleset's derivation must survive restart"
    );
}

// ── R4: named ruleset delete survives restart ─────────────────────────────────

#[tokio::test]
async fn persist_named_ruleset_delete_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        seed_alice_manager(&server).await;

        for (id, body) in [
            ("a", MANAGER_IMPLIES_EMPLOYEE),
            ("b", MANAGER_IMPLIES_CONTRACTOR),
        ] {
            let resp = server
                .client
                .post(server.dataset_rules_id_url("ds", id))
                .header("content-type", "text/x-datalog")
                .body(body)
                .send()
                .await
                .expect("POST rules/{id} failed");
            assert_eq!(resp.status(), 200);
        }

        let resp = server
            .client
            .delete(server.dataset_rules_id_url("ds", "a"))
            .send()
            .await
            .expect("DELETE rules/a failed");
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        !ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Employee> }"
        )
        .await,
        "deleted named ruleset's derivation must not survive restart"
    );
    assert!(
        ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Contractor> }"
        )
        .await,
        "surviving named ruleset's derivation must survive restart"
    );
}

// ── R5: empty-body unload survives restart ────────────────────────────────────

#[tokio::test]
async fn persist_empty_ruleset_unload_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        seed_alice_manager(&server).await;

        let resp = server
            .client
            .post(server.dataset_rules_url("ds"))
            .header("content-type", "text/x-datalog")
            .body(MANAGER_IMPLIES_EMPLOYEE)
            .send()
            .await
            .expect("POST rules failed");
        assert_eq!(resp.status(), 200);

        // Unload: empty body.
        let resp = server
            .client
            .post(server.dataset_rules_url("ds"))
            .header("content-type", "text/x-datalog")
            .body("")
            .send()
            .await
            .expect("POST empty rules failed");
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        !ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Employee> }"
        )
        .await,
        "unloaded ruleset's derivation must not reappear after restart"
    );
    // Base fact must still be there -- unload only clears derived facts.
    assert!(
        ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Manager> }"
        )
        .await,
        "base fact must survive an unload + restart"
    );
}

// ── R6: no data-dir -> no file, in-memory only ────────────────────────────────

#[tokio::test]
async fn persist_ruleset_without_data_dir_is_memory_only() {
    let dir = tempfile::tempdir().unwrap();

    let server = common::TestServer::start_writable("").await;
    seed_alice_manager(&server).await;
    let resp = server
        .client
        .post(server.dataset_rules_url("ds"))
        .header("content-type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE)
        .send()
        .await
        .expect("POST rules failed");
    assert_eq!(resp.status(), 200);
    drop(server);

    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        entries.is_empty(),
        "non-persistent server must not create any files for a ruleset POST"
    );
}

// ── R7: compact must not discard ruleset persistence ──────────────────────────

#[tokio::test]
async fn persist_compact_preserves_ruleset() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        seed_alice_manager(&server).await;

        let resp = server
            .client
            .post(server.dataset_rules_url("ds"))
            .header("content-type", "text/x-datalog")
            .body(MANAGER_IMPLIES_EMPLOYEE)
            .send()
            .await
            .expect("POST rules failed");
        assert_eq!(resp.status(), 200);

        let compact_url = format!("{}/$/compact", server.base_url);
        let resp = server
            .client
            .post(&compact_url)
            .send()
            .await
            .expect("compact request failed");
        assert_eq!(resp.status(), 200, "compact must succeed");
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        ask(
            &server2,
            "ds",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Employee> }"
        )
        .await,
        "ruleset must survive restart even after a compaction ran beforehand"
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `--data-dir` persistence across *every* dataset in
//! the registry, not just the default `"ds"`, per
//! [#670](https://github.com/daghovland/rdf-datalog/issues/670).
//!
//! See [`docs/plans/DATADIR_MULTI_DATASET_670_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/DATADIR_MULTI_DATASET_670_PLAN.md)
//! for the design.

mod common;

const MANAGER_IMPLIES_EMPLOYEE: &str = r#"
PREFIX ex: <http://ex/>
ex:Employee[?x] :- ex:Manager[?x] .
"#;

async fn create_dataset(server: &common::TestServer, name: &str) {
    let resp = server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("dbName=/{name}&dbType=mem"))
        .send()
        .await
        .expect("create dataset request failed");
    assert_eq!(
        resp.status(),
        200,
        "create dataset {name} must return 200, got {}",
        resp.status()
    );
}

async fn delete_dataset(server: &common::TestServer, name: &str) {
    let resp = server
        .client
        .delete(server.admin_dataset_url(name))
        .send()
        .await
        .expect("delete dataset request failed");
    assert_eq!(
        resp.status(),
        200,
        "delete dataset {name} must return 200, got {}",
        resp.status()
    );
}

async fn seed_triple(server: &common::TestServer, dataset: &str, turtle: &str) {
    let resp = server
        .client
        .post(server.dataset_data_default_url(dataset))
        .header("content-type", "text/turtle")
        .body(turtle.to_owned())
        .send()
        .await
        .expect("seed POST failed");
    assert!(
        resp.status().is_success(),
        "seed into {dataset} failed: {}",
        resp.status()
    );
}

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
        "query on {dataset} failed: {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.expect("invalid JSON");
    body["boolean"].as_bool().expect("boolean field")
}

async fn dataset_exists(server: &common::TestServer, name: &str) -> bool {
    server
        .client
        .get(server.admin_dataset_url(name))
        .send()
        .await
        .expect("get dataset request failed")
        .status()
        == 200
}

const HAS_S: &str = "ASK { <http://ex/s> <http://ex/p> <http://ex/o> }";
const TRIPLE: &str = "<http://ex/s> <http://ex/p> <http://ex/o> .";

// ── M1: writes to a non-"ds" dataset must not leak into "ds" on replay ────────

#[tokio::test]
async fn non_ds_write_does_not_leak_into_ds_after_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        create_dataset(&server, "foo").await;
        seed_triple(&server, "foo", TRIPLE).await;
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        ask(&server2, "foo", HAS_S).await,
        "quad written to 'foo' must be present in 'foo' after restart"
    );
    assert!(
        !ask(&server2, "ds", HAS_S).await,
        "quad written to 'foo' must NOT leak into 'ds' after restart"
    );
}

// ── M2: non-"ds" dataset data survives restart ─────────────────────────────────

#[tokio::test]
async fn non_ds_dataset_data_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        create_dataset(&server, "bar").await;
        seed_triple(&server, "bar", TRIPLE).await;
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(dataset_exists(&server2, "bar").await, "'bar' must exist");
    assert!(
        ask(&server2, "bar", HAS_S).await,
        "'bar' data must survive restart"
    );
}

// ── M3: an empty created dataset survives restart ──────────────────────────────

#[tokio::test]
async fn empty_created_dataset_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        create_dataset(&server, "empty1").await;
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        dataset_exists(&server2, "empty1").await,
        "an empty created dataset must still exist after restart"
    );
}

// ── M4: a deleted dataset does not reappear after restart ──────────────────────

#[tokio::test]
async fn deleted_dataset_does_not_reappear_after_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        create_dataset(&server, "gone1").await;
        seed_triple(&server, "gone1", TRIPLE).await;
        delete_dataset(&server, "gone1").await;
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        !dataset_exists(&server2, "gone1").await,
        "a deleted dataset must not reappear after restart"
    );
}

// ── M5: delete then recreate comes back empty (no resurrected quads) ───────────

#[tokio::test]
async fn delete_then_recreate_is_empty_after_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        create_dataset(&server, "cycle1").await;
        seed_triple(&server, "cycle1", TRIPLE).await;
        delete_dataset(&server, "cycle1").await;
        create_dataset(&server, "cycle1").await;
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    assert!(
        dataset_exists(&server2, "cycle1").await,
        "recreated dataset must exist"
    );
    assert!(
        !ask(&server2, "cycle1", HAS_S).await,
        "recreated dataset must not resurrect quads from its deleted incarnation"
    );
}

// ── M6: a non-"ds" dataset's ruleset survives restart ───────────────────────────

#[tokio::test]
async fn non_ds_dataset_ruleset_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        create_dataset(&server, "ruled1").await;
        seed_triple(
            &server,
            "ruled1",
            "<http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Manager> .",
        )
        .await;
        let resp = server
            .client
            .post(server.dataset_rules_url("ruled1"))
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
            "ruled1",
            "ASK { <http://ex/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://ex/Employee> }"
        )
        .await,
        "ruleset-derived fact in a non-ds dataset must survive restart"
    );
}

// ── M7: compact must preserve every dataset, not just "ds" ─────────────────────

#[tokio::test]
async fn compact_preserves_all_datasets() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;
        seed_triple(&server, "ds", TRIPLE).await;
        create_dataset(&server, "compacted1").await;
        seed_triple(&server, "compacted1", TRIPLE).await;
        create_dataset(&server, "compacted_empty").await;

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
        ask(&server2, "ds", HAS_S).await,
        "'ds' data must survive a compact + restart"
    );
    assert!(
        dataset_exists(&server2, "compacted1").await,
        "'compacted1' must survive a compact + restart"
    );
    assert!(
        ask(&server2, "compacted1", HAS_S).await,
        "'compacted1' data must survive a compact + restart"
    );
    assert!(
        dataset_exists(&server2, "compacted_empty").await,
        "an empty dataset must survive a compact + restart"
    );
}

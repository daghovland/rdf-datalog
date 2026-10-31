/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! HTTP-level integration tests for the Azure Blob Storage snapshot backend
//! ([#504](https://github.com/daghovland/rdf-datalog/issues/504)).
//!
//! These exercise the real startup-restore and `POST /$/compact`
//! upload wiring end to end over HTTP, without touching a real Azure
//! account: `Config::snapshot_backend_override` (a test-only escape hatch
//! mirroring `allow_loopback_for_ssrf_tests`) injects an in-memory fake
//! implementation of `sparql_endpoint::azure_snapshot::SnapshotBackend`
//! instead of constructing a real `AzureBlobSnapshotBackend`. A separate
//! `#[ignore]`d test against a real account lives in
//! `sparql_endpoint/src/azure_snapshot.rs`.

mod common;

use async_trait::async_trait;
use dag_rdf::datastore::Datastore;
use sparql_endpoint::azure_snapshot::SnapshotBackend;
use sparql_endpoint::{Config, serve_on_listener};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

/// In-memory fake `SnapshotBackend`, local to this test file (the library's
/// own `#[cfg(test)]` fake in `azure_snapshot.rs` isn't visible from an
/// integration-test crate).
#[derive(Default)]
struct FakeSnapshotBackend {
    stored: Mutex<Option<Vec<u8>>>,
    download_calls: Mutex<u32>,
    upload_calls: Mutex<u32>,
}

impl FakeSnapshotBackend {
    fn new() -> Self {
        Self::default()
    }

    fn seeded(bytes: Vec<u8>) -> Self {
        Self {
            stored: Mutex::new(Some(bytes)),
            ..Self::default()
        }
    }
}

#[async_trait]
impl SnapshotBackend for FakeSnapshotBackend {
    async fn download(&self, dest: &Path) -> Result<bool, String> {
        *self.download_calls.lock().unwrap() += 1;
        let stored = self.stored.lock().unwrap().clone();
        match stored {
            Some(bytes) => {
                std::fs::write(dest, bytes).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn upload(&self, src: &Path) -> Result<(), String> {
        *self.upload_calls.lock().unwrap() += 1;
        let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
        *self.stored.lock().unwrap() = Some(bytes);
        Ok(())
    }
}

/// Start a writable, persistent server at `data_dir`, with `backend` (if
/// any) wired in as the snapshot backend. Returns the server plus a join
/// handle so the caller can `abort()` it (mirrors
/// `common::TestServer::shutdown`, duplicated here because that helper
/// doesn't expose a way to set `snapshot_backend_override`).
async fn start_persistent_with_backend(
    data_dir: &Path,
    backend: Option<Arc<dyn SnapshotBackend>>,
) -> (String, reqwest::Client, tokio::task::JoinHandle<()>) {
    let store = Arc::new(RwLock::new(Datastore::new(1024)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr");
    let base_url = format!("http://{}", addr);
    let config = Config {
        bind_addr: addr,
        base_iri: base_url.clone(),
        read_only: false,
        max_query_timeout_secs: 10,
        data_dir: Some(data_dir.to_path_buf()),
        snapshot_backend_override: backend,
        ..Default::default()
    };
    let handle = tokio::spawn(async move {
        serve_on_listener(store, config, listener)
            .await
            .expect("server error");
    });
    tokio::task::yield_now().await;
    (base_url, reqwest::Client::new(), handle)
}

async fn shutdown(handle: tokio::task::JoinHandle<()>) {
    handle.abort();
    let _ = handle.await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}

const INSERT_UPDATE: &str = "INSERT DATA { <http://example.org/s> <http://example.org/p> \"snapshot-test\" }";
const ASK_QUERY: &str = "ASK { <http://example.org/s> <http://example.org/p> \"snapshot-test\" }";

async fn ask(base_url: &str, client: &reqwest::Client) -> bool {
    let resp = client
        .get(format!(
            "{base_url}/sparql?query={}",
            urlencoding::encode(ASK_QUERY)
        ))
        .send()
        .await
        .expect("ask request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("ASK response must be JSON");
    body["boolean"] == true
}

/// A fresh replica with no local `redb` file yet, but a configured blob
/// snapshot backend that already holds a previously-uploaded snapshot, must
/// restore from it at startup -- the whole point of the feature for a
/// deployment with no mounted persistent volume.
#[tokio::test(flavor = "multi_thread")]
async fn startup_restores_from_snapshot_when_local_file_is_missing() {
    // First "replica": commit a quad, which the compaction step below will
    // turn into the minimal snapshot we seed the fake blob with.
    let dir_a = tempfile::tempdir().unwrap();
    let (base_a, client_a, handle_a) = start_persistent_with_backend(dir_a.path(), None).await;
    let resp = client_a
        .post(format!("{base_a}/ds/update"))
        .header("content-type", "application/sparql-update")
        .body(INSERT_UPDATE)
        .send()
        .await
        .expect("update failed");
    assert_eq!(resp.status(), 200);
    shutdown(handle_a).await;

    let snapshot_bytes = std::fs::read(dir_a.path().join("dagalog.redb")).unwrap();

    // Second "replica": brand-new, empty data_dir (simulating a fresh
    // container with no mounted volume), but the blob already has the
    // first replica's snapshot.
    let dir_b = tempfile::tempdir().unwrap();
    let fake: Arc<dyn SnapshotBackend> = Arc::new(FakeSnapshotBackend::seeded(snapshot_bytes));
    let (base_b, client_b, handle_b) =
        start_persistent_with_backend(dir_b.path(), Some(fake)).await;

    assert!(
        ask(&base_b, &client_b).await,
        "quad from the restored snapshot must be visible"
    );
    shutdown(handle_b).await;
}

/// When a local `redb` file already exists, startup must NOT overwrite it
/// from the blob snapshot -- a running instance's own locally-committed
/// data always takes priority over the disaster-recovery blob.
#[tokio::test(flavor = "multi_thread")]
async fn startup_does_not_restore_when_local_file_already_present() {
    let dir = tempfile::tempdir().unwrap();

    // Pre-populate a local file by starting and stopping a server once
    // WITHOUT inserting the marker quad.
    let (_base1, _client1, handle1) = start_persistent_with_backend(dir.path(), None).await;
    shutdown(handle1).await;
    assert!(dir.path().join("dagalog.redb").exists());

    // Seed a fake blob that DOES have the marker quad; if startup restored
    // from it, the ASK below would wrongly succeed.
    let dir_other = tempfile::tempdir().unwrap();
    let (base_other, client_other, handle_other) =
        start_persistent_with_backend(dir_other.path(), None).await;
    client_other
        .post(format!("{base_other}/ds/update"))
        .header("content-type", "application/sparql-update")
        .body(INSERT_UPDATE)
        .send()
        .await
        .expect("update failed");
    shutdown(handle_other).await;
    let other_snapshot = std::fs::read(dir_other.path().join("dagalog.redb")).unwrap();

    let fake: Arc<dyn SnapshotBackend> = Arc::new(FakeSnapshotBackend::seeded(other_snapshot));
    let (base2, client2, handle2) = start_persistent_with_backend(dir.path(), Some(fake)).await;

    assert!(
        !ask(&base2, &client2).await,
        "existing local file must not be overwritten by the blob snapshot"
    );
    shutdown(handle2).await;
}

/// `POST /$/compact` must upload the freshly-compacted file to the
/// configured blob backend, and report that in its JSON response.
#[tokio::test(flavor = "multi_thread")]
async fn compact_uploads_to_snapshot_backend() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeSnapshotBackend::new());
    let backend_for_config: Arc<dyn SnapshotBackend> = fake.clone();
    let (base, client, handle) =
        start_persistent_with_backend(dir.path(), Some(backend_for_config)).await;

    client
        .post(format!("{base}/ds/update"))
        .header("content-type", "application/sparql-update")
        .body(INSERT_UPDATE)
        .send()
        .await
        .expect("update failed");

    let resp = client
        .post(format!("{base}/$/compact"))
        .send()
        .await
        .expect("compact request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("compact response must be JSON");
    assert_eq!(body["blob_snapshot_uploaded"], serde_json::json!(true));

    assert_eq!(
        *fake.upload_calls.lock().unwrap(),
        1,
        "exactly one upload should have happened"
    );
    assert!(
        fake.stored.lock().unwrap().is_some(),
        "the backend must now hold the compacted snapshot bytes"
    );

    shutdown(handle).await;
}

/// `POST /$/compact` without any snapshot backend configured must behave
/// exactly as before #504: no `blob_snapshot_uploaded` field at all.
#[tokio::test(flavor = "multi_thread")]
async fn compact_without_backend_omits_upload_field() {
    let dir = tempfile::tempdir().unwrap();
    let (base, client, handle) = start_persistent_with_backend(dir.path(), None).await;

    let resp = client
        .post(format!("{base}/$/compact"))
        .send()
        .await
        .expect("compact request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("compact response must be JSON");
    assert!(
        body.get("blob_snapshot_uploaded").is_none(),
        "no backend configured -> no upload field"
    );

    shutdown(handle).await;
}

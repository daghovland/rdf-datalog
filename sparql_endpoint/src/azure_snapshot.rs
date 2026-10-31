/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Azure Blob Storage snapshot backend for the `redb`-backed persistence
//! layer ([#504](https://github.com/daghovland/rdf-datalog/issues/504)).
//!
//! ## Why this exists
//!
//! [`crate::persistence::QuadChangelog`] is a `redb` embedded database: it
//! requires random-write access and `fsync` on a local file, which rules out
//! pointing it directly at an Azure Blob Storage object (an HTTPS put/get
//! object store, not a mountable POSIX filesystem — see
//! `docs/plans/PERSISTENCE_PLAN.md`'s "Cloud object storage ... out of
//! scope" section). So this module does **not** make `redb` address a blob;
//! it instead moves the whole local `redb` *file* to and from a blob at two
//! points:
//!
//! - **Startup**: if a local database file doesn't exist yet (e.g. a fresh
//!   replica with no mounted volume) and a blob snapshot is configured, the
//!   blob is downloaded to the local path *before* `redb` ever opens it.
//! - **`POST /$/compact`**: after compaction rewrites the local changelog
//!   into a minimal snapshot (see
//!   [`crate::persistence::QuadChangelog::compact_multi`]), the now-compact
//!   file is uploaded to the blob.
//!
//! This is opt-in and additive: local-disk `--data-dir` keeps working
//! unchanged when no Azure Blob Storage snapshot is configured.
//!
//! ## Credential handling
//!
//! The issue's original design (see `docs/plans/AUTH.md`) assumed
//! `azure_identity::DefaultAzureCredential`, a single "try everything"
//! credential. **That type does not exist in the current stable
//! `azure_identity` 1.x generation** — it was removed before 1.0 in favor of
//! explicit credential types (see that crate's `CHANGELOG.md`, the `1.0.0`
//! and historical entries mentioning `DefaultAzureCredential`'s removal).
//! This module uses [`azure_identity::ManagedIdentityCredential`] directly,
//! which is exactly the credential this issue is named for (system-assigned
//! or, via `ManagedIdentityCredentialOptions`, user-assigned Managed
//! Identity) and authenticates via IMDS with no credential material handled
//! by this crate at all.

use async_trait::async_trait;
use azure_core::Bytes;
use azure_core::credentials::TokenCredential;
use azure_core::http::{NoFormat, RequestContent, Url};
use azure_identity::ManagedIdentityCredential;
use azure_storage_blob::BlobClient;
use azure_storage_blob::models::{BlobClientDownloadOptions, BlobClientUploadOptions};
use std::path::Path;
use std::sync::Arc;

/// Parsed `--azure-blob-snapshot` configuration: which blob holds the
/// durable snapshot of the local `redb` persistence file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AzureBlobSnapshotConfig {
    pub account: String,
    pub container: String,
    /// The blob name. May itself contain `/` (Blob Storage has no real
    /// directory hierarchy; `/` in a blob name is just a naming convention),
    /// which is why parsing below only splits the first two `/`-separated
    /// segments off and takes everything else verbatim as the blob name.
    pub blob: String,
}

impl AzureBlobSnapshotConfig {
    /// Parse the `--azure-blob-snapshot` / `DAGALOG_AZURE_BLOB_SNAPSHOT`
    /// value, of the form `<account>/<container>/<blob-name>`.
    ///
    /// `<blob-name>` may contain further `/` characters (e.g.
    /// `snapshots/dagalog.redb`); only the first two slashes are treated as
    /// field separators.
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut parts = s.splitn(3, '/');
        let account = parts.next().unwrap_or("");
        let container = parts.next().unwrap_or("");
        let blob = parts.next().unwrap_or("");
        if account.is_empty() || container.is_empty() || blob.is_empty() {
            return Err(format!(
                "invalid --azure-blob-snapshot value {s:?}: expected \
                 <account>/<container>/<blob-name>, all three non-empty"
            ));
        }
        Ok(Self {
            account: account.to_string(),
            container: container.to_string(),
            blob: blob.to_string(),
        })
    }

    /// The `https://<account>.blob.core.windows.net/<container>/<blob>` URL
    /// this config addresses, with the container and each `/`-separated
    /// segment of the blob name percent-encoded individually (preserving the
    /// blob name's own internal `/`s as path separators, not literal `%2F`).
    pub fn blob_url(&self) -> Result<Url, String> {
        let mut url = Url::parse(&format!("https://{}.blob.core.windows.net", self.account))
            .map_err(|e| format!("invalid storage account name {:?}: {e}", self.account))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|()| "blob endpoint URL cannot be a base".to_string())?;
            segments.push(&self.container);
            for segment in self.blob.split('/') {
                segments.push(segment);
            }
        }
        Ok(url)
    }
}

/// Abstraction over "where the redb snapshot file additionally lives on
/// durable storage beyond the local `--data-dir` path", so the core startup
/// / compaction wiring can be unit-tested without live Azure credentials —
/// see the fake in this module's tests and the integration test that
/// injects it via `Config::snapshot_backend_override`.
#[async_trait]
pub trait SnapshotBackend: Send + Sync {
    /// Download the latest snapshot to `dest`. Returns `Ok(true)` if a
    /// snapshot was found and written, `Ok(false)` if none exists yet
    /// (e.g. first-ever startup for this deployment) — a no-op, not an
    /// error. Returns `Err` only for genuine failures (auth, network,
    /// container missing, etc.) that the caller should treat as a hard
    /// startup failure rather than silently starting empty.
    async fn download(&self, dest: &Path) -> Result<bool, String>;

    /// Upload the file at `src`, overwriting any previous snapshot.
    async fn upload(&self, src: &Path) -> Result<(), String>;
}

/// Real [`SnapshotBackend`] backed by Azure Blob Storage, authenticated via
/// [`ManagedIdentityCredential`].
pub struct AzureBlobSnapshotBackend {
    client: BlobClient,
}

impl AzureBlobSnapshotBackend {
    pub fn new(config: &AzureBlobSnapshotConfig) -> Result<Self, String> {
        let url = config.blob_url()?;
        let credential: Arc<dyn TokenCredential> = ManagedIdentityCredential::new(None)
            .map_err(|e| format!("failed to construct ManagedIdentityCredential: {e}"))?;
        let client = BlobClient::new(url, Some(credential), None)
            .map_err(|e| format!("failed to construct Azure BlobClient: {e}"))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl SnapshotBackend for AzureBlobSnapshotBackend {
    async fn download(&self, dest: &Path) -> Result<bool, String> {
        if !self
            .client
            .exists()
            .await
            .map_err(|e| format!("checking blob existence failed: {e}"))?
        {
            return Ok(false);
        }
        let result = self
            .client
            .download(None::<BlobClientDownloadOptions<'_>>)
            .await
            .map_err(|e| format!("blob download failed: {e}"))?;
        let bytes = result
            .body
            .collect()
            .await
            .map_err(|e| format!("reading blob download body failed: {e}"))?;

        // Write to a sibling temp file then rename, so a crash mid-download
        // never leaves a partially-written file at `dest` for redb to open.
        let tmp_path = dest.with_extension("downloading-tmp");
        std::fs::write(&tmp_path, &bytes)
            .map_err(|e| format!("writing downloaded snapshot to {tmp_path:?} failed: {e}"))?;
        std::fs::rename(&tmp_path, dest)
            .map_err(|e| format!("renaming downloaded snapshot into place failed: {e}"))?;
        Ok(true)
    }

    async fn upload(&self, src: &Path) -> Result<(), String> {
        let bytes = std::fs::read(src)
            .map_err(|e| format!("reading local snapshot file {src:?} failed: {e}"))?;
        let content: RequestContent<Bytes, NoFormat> = RequestContent::from(bytes);
        self.client
            .upload(content, None::<BlobClientUploadOptions<'_>>)
            .await
            .map_err(|e| format!("blob upload failed: {e}"))?;
        Ok(())
    }
}

/// If `db_path` does not already exist locally, try to restore it from
/// `backend` (when one is configured). Returns `Ok(())` whether or not a
/// snapshot was actually found — "no snapshot yet" is a normal first-boot
/// state, not an error. Returns `Err` only when `backend.download` itself
/// errors (auth/network/etc.), which callers should treat as a hard startup
/// failure rather than silently starting from an empty store when a durable
/// snapshot was expected to exist.
pub async fn restore_if_missing(
    backend: Option<&dyn SnapshotBackend>,
    db_path: &Path,
) -> Result<(), String> {
    let Some(backend) = backend else {
        return Ok(());
    };
    if db_path.exists() {
        return Ok(());
    }
    backend.download(db_path).await.map(|_found| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ── config parsing ────────────────────────────────────────────────────

    #[test]
    fn parses_three_segment_value() {
        let cfg = AzureBlobSnapshotConfig::parse("myaccount/mycontainer/dagalog.redb").unwrap();
        assert_eq!(cfg.account, "myaccount");
        assert_eq!(cfg.container, "mycontainer");
        assert_eq!(cfg.blob, "dagalog.redb");
    }

    #[test]
    fn blob_name_may_contain_further_slashes() {
        let cfg =
            AzureBlobSnapshotConfig::parse("myaccount/mycontainer/snapshots/dagalog.redb").unwrap();
        assert_eq!(cfg.blob, "snapshots/dagalog.redb");
    }

    #[test]
    fn rejects_missing_segments() {
        assert!(AzureBlobSnapshotConfig::parse("myaccount/mycontainer").is_err());
        assert!(AzureBlobSnapshotConfig::parse("myaccount").is_err());
        assert!(AzureBlobSnapshotConfig::parse("").is_err());
    }

    #[test]
    fn rejects_empty_segment() {
        // Empty container between two slashes.
        assert!(AzureBlobSnapshotConfig::parse("myaccount//dagalog.redb").is_err());
    }

    #[test]
    fn blob_url_is_well_formed() {
        let cfg = AzureBlobSnapshotConfig::parse("myaccount/mycontainer/dagalog.redb").unwrap();
        let url = cfg.blob_url().unwrap();
        assert_eq!(
            url.as_str(),
            "https://myaccount.blob.core.windows.net/mycontainer/dagalog.redb"
        );
    }

    #[test]
    fn blob_url_preserves_internal_slashes_as_path_separators() {
        let cfg =
            AzureBlobSnapshotConfig::parse("myaccount/mycontainer/snapshots/dagalog.redb").unwrap();
        let url = cfg.blob_url().unwrap();
        assert_eq!(
            url.as_str(),
            "https://myaccount.blob.core.windows.net/mycontainer/snapshots/dagalog.redb"
        );
    }

    // ── fake backend + restore_if_missing wiring ──────────────────────────

    /// In-memory fake [`SnapshotBackend`] for tests that don't need (and, in
    /// this environment, can't reach) a real Azure Blob Storage account.
    /// Also used by `sparql_endpoint/tests/azure_snapshot.rs` via
    /// `Config::snapshot_backend_override` for true HTTP-level integration
    /// coverage of the startup-restore and post-compaction-upload wiring.
    #[derive(Default)]
    pub struct FakeSnapshotBackend {
        /// `None` = no snapshot has ever been uploaded ("blob does not exist").
        pub stored: Mutex<Option<Vec<u8>>>,
        pub download_calls: Mutex<u32>,
        pub upload_calls: Mutex<u32>,
    }

    impl FakeSnapshotBackend {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn seeded(bytes: Vec<u8>) -> Self {
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

    #[tokio::test]
    async fn restore_if_missing_is_noop_without_a_backend() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dagalog.redb");
        restore_if_missing(None, &dest).await.unwrap();
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn restore_if_missing_is_noop_when_local_file_already_present() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dagalog.redb");
        std::fs::write(&dest, b"local data").unwrap();

        let backend = FakeSnapshotBackend::seeded(b"blob data".to_vec());
        restore_if_missing(Some(&backend), &dest).await.unwrap();

        // The pre-existing local file must be left untouched: a running
        // instance's own locally-committed data takes priority over the
        // disaster-recovery blob snapshot.
        assert_eq!(std::fs::read(&dest).unwrap(), b"local data");
        assert_eq!(*backend.download_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn restore_if_missing_downloads_when_local_file_absent_and_blob_exists() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dagalog.redb");

        let backend = FakeSnapshotBackend::seeded(b"blob data".to_vec());
        restore_if_missing(Some(&backend), &dest).await.unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"blob data");
        assert_eq!(*backend.download_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn restore_if_missing_is_ok_when_no_snapshot_exists_yet() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dagalog.redb");

        let backend = FakeSnapshotBackend::new();
        restore_if_missing(Some(&backend), &dest).await.unwrap();

        // First-ever boot: no blob yet is not an error, and no local file
        // is created either.
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn restore_if_missing_propagates_backend_errors() {
        struct FailingBackend;
        #[async_trait]
        impl SnapshotBackend for FailingBackend {
            async fn download(&self, _dest: &Path) -> Result<bool, String> {
                Err("simulated auth failure".to_string())
            }
            async fn upload(&self, _src: &Path) -> Result<(), String> {
                unreachable!("not exercised by this test")
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dagalog.redb");
        let err = restore_if_missing(Some(&FailingBackend), &dest)
            .await
            .unwrap_err();
        assert!(err.contains("simulated auth failure"));
    }

    // ── real Azure integration test (requires live credentials) ──────────

    /// Exercises a real Azure Blob Storage account end-to-end: uploads a
    /// small snapshot, then downloads it back and checks round-trip
    /// equality. Authenticates via `ManagedIdentityCredential`, so it only
    /// passes when actually running on (or configured to impersonate) an
    /// Azure resource with a Managed Identity that has Storage Blob Data
    /// Contributor on the target container.
    ///
    /// Requires `DAGALOG_TEST_AZURE_BLOB_SNAPSHOT` to be set to
    /// `<account>/<container>/<blob-name>` of a real, writable container.
    /// Ignored by default — Dag runs this manually in a real Azure
    /// environment per [#504](https://github.com/daghovland/rdf-datalog/issues/504).
    #[tokio::test]
    #[ignore = "requires a real Azure Managed Identity + Blob Storage account; see #504"]
    async fn real_azure_blob_roundtrip() {
        let spec = std::env::var("DAGALOG_TEST_AZURE_BLOB_SNAPSHOT")
            .expect("set DAGALOG_TEST_AZURE_BLOB_SNAPSHOT=<account>/<container>/<blob-name>");
        let cfg = AzureBlobSnapshotConfig::parse(&spec).unwrap();
        let backend = AzureBlobSnapshotBackend::new(&cfg).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("upload.bin");
        let payload = b"dagalog azure blob snapshot roundtrip test";
        std::fs::write(&src, payload).unwrap();

        backend.upload(&src).await.expect("upload failed");

        let dest = dir.path().join("download.bin");
        let found = backend.download(&dest).await.expect("download failed");
        assert!(found, "just-uploaded blob should be found");
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
    }
}

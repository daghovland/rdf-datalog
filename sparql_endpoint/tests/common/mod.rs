/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Shared helpers for sparql_endpoint integration tests.

use dag_rdf::datastore::Datastore;
use datalog::Rule;
use ingress::NetworkPolicy;
use sparql_endpoint::{AuthConfig, Config, OidcConfig, serve_on_listener};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use turtle::{parse_trig, parse_turtle};

/// A running test server bound to a random loopback port.
///
/// Dropping this value cancels the background server task.
/// For tests that need to restart the server (persistence tests), call
/// `shutdown().await` to wait for the task to fully terminate and release
/// any file locks before starting a second instance.
pub struct TestServer {
    pub base_url: String,
    #[allow(dead_code)]
    pub client: reqwest::Client,
    // Kept alive so the server task runs for the duration of the test.
    _handle: tokio::task::JoinHandle<()>,
}

#[allow(dead_code)]
impl TestServer {
    /// Start a read-only server pre-loaded with Turtle data.
    ///
    /// Pass an empty string for an empty store.
    pub async fn start(turtle: &str) -> Self {
        Self::start_inner(turtle, false, true, AuthConfig::None).await
    }

    /// Start a writable server (read_only: false) pre-loaded with Turtle data.
    ///
    /// Required for any test that exercises PUT, POST, or DELETE on the graph store.
    pub async fn start_writable(turtle: &str) -> Self {
        Self::start_inner(turtle, false, false, AuthConfig::None).await
    }

    /// Start a writable server pre-loaded with Turtle data, with a custom
    /// `max_rdf_upload_bytes` override (see `Config::max_rdf_upload_bytes`).
    ///
    /// Use this to test the RDF write-route body-limit override at a small,
    /// fast-to-exceed size instead of the 64 MiB production default.
    pub async fn start_writable_with_rdf_limit(turtle: &str, max_rdf_upload_bytes: usize) -> Self {
        let mut ds = Datastore::new(1024);
        if !turtle.is_empty() {
            parse_turtle(&mut ds, std::io::BufReader::new(turtle.as_bytes()))
                .expect("test fixture turtle must parse");
        }
        let store = Arc::new(RwLock::new(ds));
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
            auth: AuthConfig::None,
            data_dir: None,
            max_rdf_upload_bytes,
            ..Default::default()
        };
        let handle = tokio::spawn(async move {
            serve_on_listener(store, config, listener)
                .await
                .expect("server error");
        });
        tokio::task::yield_now().await;
        TestServer {
            base_url,
            client: reqwest::Client::new(),
            _handle: handle,
        }
    }

    /// Start a writable server pre-loaded with Turtle data, with a custom
    /// `max_rml_upload_bytes` override (see `Config::max_rml_upload_bytes`).
    ///
    /// Use this to test the RML upload-route body-limit override at a small,
    /// fast-to-exceed size instead of the 64 MiB production default.
    pub async fn start_writable_with_rml_limit(turtle: &str, max_rml_upload_bytes: usize) -> Self {
        let mut ds = Datastore::new(1024);
        if !turtle.is_empty() {
            parse_turtle(&mut ds, std::io::BufReader::new(turtle.as_bytes()))
                .expect("test fixture turtle must parse");
        }
        let store = Arc::new(RwLock::new(ds));
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
            auth: AuthConfig::None,
            data_dir: None,
            max_rml_upload_bytes,
            ..Default::default()
        };
        let handle = tokio::spawn(async move {
            serve_on_listener(store, config, listener)
                .await
                .expect("server error");
        });
        tokio::task::yield_now().await;
        TestServer {
            base_url,
            client: reqwest::Client::new(),
            _handle: handle,
        }
    }

    /// Start a writable server pre-loaded with TriG data.
    ///
    /// Use this when test fixtures need named graphs. TriG extends Turtle with
    /// `<graph-iri> { ... }` blocks.
    pub async fn start_writable_trig(trig: &str) -> Self {
        Self::start_inner(trig, true, false, AuthConfig::None).await
    }

    /// Start a writable server with the given network policy.
    ///
    /// Use this to test `NetworkPolicy::Allow` (SPARQL LOAD from remote URLs).
    /// Tests using this method must run with a multi-thread Tokio runtime
    /// (`#[tokio::test(flavor = "multi_thread")]`) because `block_in_place`
    /// is used internally when fetching remote content.
    #[allow(dead_code)]
    pub async fn start_writable_with_network_policy(
        turtle: &str,
        network_policy: NetworkPolicy,
    ) -> Self {
        let mut ds = Datastore::new(1024);
        if !turtle.is_empty() {
            parse_turtle(&mut ds, std::io::BufReader::new(turtle.as_bytes()))
                .expect("test fixture turtle must parse");
        }
        let store = Arc::new(RwLock::new(ds));
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
            auth: AuthConfig::None,
            data_dir: None,
            network_policy,
            ..Default::default()
        };
        let handle = tokio::spawn(async move {
            serve_on_listener(store, config, listener)
                .await
                .expect("server error");
        });
        tokio::task::yield_now().await;
        TestServer {
            base_url,
            client: reqwest::Client::new(),
            _handle: handle,
        }
    }

    /// Start a writable server protected by a static API key (reads are open).
    pub async fn start_writable_with_key(turtle: &str, api_key: &str) -> Self {
        Self::start_inner(
            turtle,
            false,
            false,
            AuthConfig::ApiKey {
                key: api_key.to_string(),
                require_for_reads: false,
            },
        )
        .await
    }

    /// Start a writable server where both reads and writes require the API key.
    pub async fn start_writable_with_key_protect_reads(turtle: &str, api_key: &str) -> Self {
        Self::start_inner(
            turtle,
            false,
            false,
            AuthConfig::ApiKey {
                key: api_key.to_string(),
                require_for_reads: true,
            },
        )
        .await
    }

    /// Start a writable server with OIDC authentication.
    pub async fn start_with_oidc(turtle: &str, oidc_config: OidcConfig) -> Self {
        Self::start_inner(turtle, false, false, AuthConfig::Oidc(oidc_config)).await
    }

    /// Start a persistent writable server using the given data directory.
    ///
    /// The changelog is stored at `<data_dir>/dagalog.redb`.  On startup the
    /// changelog entries are applied on top of the pre-loaded `data` Turtle,
    /// so both sources are visible together.
    pub async fn start_writable_persistent(data: &str, data_dir: &Path) -> Self {
        Self::start_inner_with_data_dir(data, false, false, AuthConfig::None, Some(data_dir)).await
    }

    /// Start a writable server with an already-built store and incremental rules.
    ///
    /// The `store` must be the same `Datastore` that was used to intern the IRI
    /// resources referenced by `rules` (IDs must match).
    /// `IncrementalReasoner::new` will run initial materialisation inside
    /// `serve_on_listener`.
    #[allow(dead_code)]
    pub async fn start_with_store_and_rules(
        store: Arc<RwLock<Datastore>>,
        rules: Vec<Rule>,
        read_only: bool,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let base_url = format!("http://{}", addr);
        let config = Config {
            bind_addr: addr,
            base_iri: base_url.clone(),
            read_only,
            max_query_timeout_secs: 10,
            auth: AuthConfig::None,
            data_dir: None,
            initial_rules: rules,
            ..Default::default()
        };
        let handle = tokio::spawn(async move {
            serve_on_listener(store, config, listener)
                .await
                .expect("server error");
        });
        tokio::task::yield_now().await;
        TestServer {
            base_url,
            client: reqwest::Client::new(),
            _handle: handle,
        }
    }

    async fn start_inner(data: &str, use_trig: bool, read_only: bool, auth: AuthConfig) -> Self {
        Self::start_inner_with_data_dir(data, use_trig, read_only, auth, None).await
    }

    async fn start_inner_with_data_dir(
        data: &str,
        use_trig: bool,
        read_only: bool,
        auth: AuthConfig,
        data_dir: Option<&Path>,
    ) -> Self {
        let mut ds = Datastore::new(1024);
        if !data.is_empty() {
            if use_trig {
                parse_trig(&mut ds, std::io::BufReader::new(data.as_bytes()))
                    .expect("test fixture trig must parse");
            } else {
                parse_turtle(&mut ds, std::io::BufReader::new(data.as_bytes()))
                    .expect("test fixture turtle must parse");
            }
        }
        let store = Arc::new(RwLock::new(ds));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let base_url = format!("http://{}", addr);
        let config = Config {
            bind_addr: addr,
            base_iri: base_url.clone(),
            read_only,
            max_query_timeout_secs: 10,
            auth,
            data_dir: data_dir.map(Path::to_path_buf),
            ..Default::default()
        };
        let handle = tokio::spawn(async move {
            serve_on_listener(store, config, listener)
                .await
                .expect("server error");
        });
        tokio::task::yield_now().await;
        TestServer {
            base_url,
            client: reqwest::Client::new(),
            _handle: handle,
        }
    }

    /// Abort the server task and wait for it to fully terminate.
    ///
    /// Necessary before starting a second server that uses the same `data_dir`,
    /// because `redb` holds a file lock that is only released when the server
    /// task (and its `QuadChangelog`) is fully dropped.
    #[allow(dead_code)]
    pub async fn shutdown(self) {
        let handle = self._handle;
        handle.abort();
        // Wait for the abort to complete (returns JoinError::Cancelled, which is expected).
        let _ = handle.await;
        // Brief yield to let the OS fully release file locks.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    pub fn sparql_url(&self) -> String {
        format!("{}/sparql", self.base_url)
    }

    /// `/sparql?query=<url-encoded SPARQL>` — use instead of `.query(&[...])`.
    pub fn sparql_query_url(&self, sparql: &str) -> String {
        format!(
            "{}/sparql?query={}",
            self.base_url,
            urlencoding::encode(sparql)
        )
    }

    /// Base URL for the Graph Store endpoint: `<base>/rdf-graph-store`.
    ///
    /// Append `?default` or `?graph=<encoded-iri>` as needed.
    pub fn gsp_url(&self) -> String {
        format!("{}/rdf-graph-store", self.base_url)
    }

    /// `GET/PUT/POST/DELETE /rdf-graph-store?default` — targets the default graph.
    ///
    /// Spec §4.2: https://www.w3.org/TR/sparql11-http-rdf-update/#indirect-graph-identification
    pub fn gsp_default_url(&self) -> String {
        format!("{}/rdf-graph-store?default", self.base_url)
    }

    /// `GET/PUT/POST/DELETE /rdf-graph-store?graph=<encoded-iri>` — targets a named graph.
    ///
    /// `graph_iri` must be an absolute IRI; it is percent-encoded here.
    /// Spec §4.2: https://www.w3.org/TR/sparql11-http-rdf-update/#indirect-graph-identification
    pub fn gsp_named_graph_url(&self, graph_iri: &str) -> String {
        format!(
            "{}/rdf-graph-store?graph={}",
            self.base_url,
            urlencoding::encode(graph_iri)
        )
    }

    // ── Fuseki-compatible URL builders ───────────────────────────────────────
    //
    // These mirror Apache Jena Fuseki's per-dataset service URLs.
    // Spec: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html

    /// `GET/POST /{name}/sparql` — Fuseki per-dataset query endpoint.
    ///
    /// Also accessible as `/{name}/query` (alias).
    pub fn dataset_sparql_url(&self, dataset: &str) -> String {
        let name = dataset.trim_start_matches('/');
        format!("{}/{name}/sparql", self.base_url)
    }

    /// `GET/POST /{name}/query` — alias for the query endpoint.
    pub fn dataset_query_url(&self, dataset: &str) -> String {
        let name = dataset.trim_start_matches('/');
        format!("{}/{name}/query", self.base_url)
    }

    /// `POST /{name}/update` — Fuseki per-dataset SPARQL Update endpoint.
    pub fn dataset_update_url(&self, dataset: &str) -> String {
        let name = dataset.trim_start_matches('/');
        format!("{}/{name}/update", self.base_url)
    }

    /// `POST /{name}/rml` — apply an RML mapping (multipart/form-data) to a dataset.
    pub fn dataset_rml_url(&self, dataset: &str) -> String {
        let name = dataset.trim_start_matches('/');
        format!("{}/{name}/rml", self.base_url)
    }

    /// `POST /rml/map` — apply an RML mapping and return the generated RDF
    /// directly, without touching any dataset.
    pub fn rml_map_url(&self) -> String {
        format!("{}/rml/map", self.base_url)
    }

    /// `POST /{name}/ottr` — expand stOTTR templates/instances
    /// (multipart/form-data, one or more document parts) into a dataset.
    pub fn dataset_ottr_url(&self, dataset: &str) -> String {
        let name = dataset.trim_start_matches('/');
        format!("{}/{name}/ottr", self.base_url)
    }

    /// `GET/PUT/POST/DELETE/HEAD /{name}/data` — Fuseki GSP read-write endpoint.
    pub fn dataset_data_url(&self, dataset: &str) -> String {
        let name = dataset.trim_start_matches('/');
        format!("{}/{name}/data", self.base_url)
    }

    /// `GET/PUT/POST/DELETE/HEAD /{name}/data?default` — default graph on dataset.
    pub fn dataset_data_default_url(&self, dataset: &str) -> String {
        format!("{}?default", self.dataset_data_url(dataset))
    }

    /// `GET/PUT/POST/DELETE/HEAD /{name}/data?graph=<iri>` — named graph on dataset.
    pub fn dataset_data_graph_url(&self, dataset: &str, graph_iri: &str) -> String {
        format!(
            "{}?graph={}",
            self.dataset_data_url(dataset),
            urlencoding::encode(graph_iri)
        )
    }

    /// `GET/HEAD /{name}/get` — Fuseki GSP read-only endpoint.
    pub fn dataset_get_url(&self, dataset: &str) -> String {
        let name = dataset.trim_start_matches('/');
        format!("{}/{name}/get", self.base_url)
    }

    /// `GET /$/ping` — Fuseki liveness check.
    ///
    /// Returns `"OK"` with 200.
    /// Spec: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html#server-information
    pub fn admin_ping_url(&self) -> String {
        format!("{}/$/ping", self.base_url)
    }

    /// `GET /$/server` — Fuseki server info (version, uptime, datasets).
    pub fn admin_server_url(&self) -> String {
        format!("{}/$/server", self.base_url)
    }

    /// `GET|POST /$/datasets` — list or create datasets.
    ///
    /// POST body: `dbName=/{name}&dbType=mem` (form-encoded).
    /// Spec: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html#datasets-and-services
    pub fn admin_datasets_url(&self) -> String {
        format!("{}/$/datasets", self.base_url)
    }

    /// `GET|DELETE /$/datasets/{name}` — info or delete one dataset.
    ///
    /// `name` should not include a leading `/`.
    pub fn admin_dataset_url(&self, name: &str) -> String {
        let n = name.trim_start_matches('/');
        format!("{}/$/datasets/{n}", self.base_url)
    }

    /// `POST /$/compact` — rewrite the persistence changelog as a minimal snapshot.
    pub fn admin_compact_url(&self) -> String {
        format!("{}/$/compact", self.base_url)
    }

    /// `POST /transaction/begin` — start a new transaction, returns `{"txId": ...}`.
    pub fn transaction_begin_url(&self) -> String {
        format!("{}/transaction/begin", self.base_url)
    }

    /// `POST /transaction/{txId}/commit` — commit an open transaction.
    pub fn transaction_commit_url(&self, tx_id: &str) -> String {
        format!("{}/transaction/{tx_id}/commit", self.base_url)
    }

    /// `POST /transaction/{txId}/rollback` — discard an open transaction.
    pub fn transaction_rollback_url(&self, tx_id: &str) -> String {
        format!("{}/transaction/{tx_id}/rollback", self.base_url)
    }
}

// ── Shared OIDC test key infrastructure ─────────────────────────────────────
//
// Generated once per test-process via OnceLock and shared across all test
// files that use `mod common`.  This eliminates duplicate RSA key-gen in
// oidc_auth.rs and any future test file that needs OIDC tokens.

/// RSA key pair used by all OIDC integration tests.
#[allow(dead_code)]
pub struct OidcTestKeys {
    pub encoding_key: jsonwebtoken::EncodingKey,
    pub public_key: rsa::RsaPublicKey,
    pub kid: String,
}

static SHARED_OIDC_KEYS: std::sync::OnceLock<OidcTestKeys> = std::sync::OnceLock::new();

/// Returns the process-wide shared OIDC test key pair.
///
/// The RSA-2048 key is generated lazily on first call and reused for the
/// lifetime of the test process.
#[allow(dead_code)]
pub fn oidc_test_keys() -> &'static OidcTestKeys {
    SHARED_OIDC_KEYS.get_or_init(|| {
        use rsa::{
            RsaPrivateKey,
            pkcs8::{EncodePrivateKey, LineEnding},
        };
        let mut rng = rand::rngs::OsRng;
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("RSA key gen");
        let private_pem = private_key.to_pkcs8_pem(LineEnding::LF).expect("PKCS8 PEM");
        OidcTestKeys {
            encoding_key: jsonwebtoken::EncodingKey::from_rsa_pem(private_pem.as_bytes())
                .expect("encoding key"),
            public_key: private_key.to_public_key(),
            kid: "shared-oidc-test-key".to_string(),
        }
    })
}

/// Build a JWK Set JSON response for the shared OIDC test keys.
#[allow(dead_code)]
pub fn shared_jwks_response() -> serde_json::Value {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use rsa::traits::PublicKeyParts;
    let keys = oidc_test_keys();
    let n = URL_SAFE_NO_PAD.encode(keys.public_key.n().to_bytes_be());
    let e = URL_SAFE_NO_PAD.encode(keys.public_key.e().to_bytes_be());
    serde_json::json!({
        "keys": [{
            "kty": "RSA",
            "kid": keys.kid,
            "use": "sig",
            "alg": "RS256",
            "n": n,
            "e": e
        }]
    })
}

// ── Assertion helpers ────────────────────────────────────────────────────────

/// Assert that `bindings` contains at least one row where `var` has `expected_type`
/// and `expected_value`.
#[track_caller]
#[allow(dead_code)]
pub fn assert_binding_contains(
    bindings: &[serde_json::Value],
    var: &str,
    expected_type: &str,
    expected_value: &str,
) {
    let found = bindings.iter().any(|row| {
        let cell = &row[var];
        cell["type"] == expected_type && cell["value"] == expected_value
    });
    assert!(
        found,
        "Expected a binding for ?{var} = ({expected_type}, {expected_value}), got:\n{:#}",
        serde_json::Value::Array(bindings.to_vec())
    );
}

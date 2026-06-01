/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Shared helpers for sparql_endpoint integration tests.

use dag_rdf::datastore::Datastore;
use sparql_endpoint::{Config, serve_on_listener};
use std::sync::Arc;
use tokio::sync::RwLock;
use turtle::{parse_trig, parse_turtle};

/// A running test server bound to a random loopback port.
///
/// Dropping this value cancels the background server task.
pub struct TestServer {
    pub base_url: String,
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
        Self::start_inner(turtle, false, true).await
    }

    /// Start a writable server (read_only: false) pre-loaded with Turtle data.
    ///
    /// Required for any test that exercises PUT, POST, or DELETE on the graph store.
    pub async fn start_writable(turtle: &str) -> Self {
        Self::start_inner(turtle, false, false).await
    }

    /// Start a writable server pre-loaded with TriG data.
    ///
    /// Use this when test fixtures need named graphs. TriG extends Turtle with
    /// `<graph-iri> { ... }` blocks.
    pub async fn start_writable_trig(trig: &str) -> Self {
        Self::start_inner(trig, true, false).await
    }

    async fn start_inner(data: &str, use_trig: bool, read_only: bool) -> Self {
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

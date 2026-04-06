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
use turtle_parser::parse_turtle;

/// A running test server bound to a random loopback port.
///
/// Dropping this value cancels the background server task.
pub struct TestServer {
    pub base_url: String,
    pub client: reqwest::Client,
    // Kept alive so the server task runs for the duration of the test.
    _handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// Start a server pre-loaded with `turtle` RDF data.
    ///
    /// Pass an empty string for an empty store.
    pub async fn start(turtle: &str) -> Self {
        let mut ds = Datastore::new(1024);
        if !turtle.is_empty() {
            parse_turtle(&mut ds, std::io::BufReader::new(turtle.as_bytes()))
                .expect("test fixture turtle must parse");
        }
        let store = Arc::new(RwLock::new(ds));

        // Port 0 → OS picks a free port.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let base_url = format!("http://{}", addr);

        let config = Config {
            bind_addr: addr,
            base_iri: base_url.clone(),
            read_only: true,
            max_query_timeout_secs: 10,
        };

        let handle = tokio::spawn(async move {
            serve_on_listener(store, config, listener)
                .await
                .expect("server error");
        });

        // Yield once so axum reaches its accept loop before the test sends requests.
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
}

// ── Assertion helpers ────────────────────────────────────────────────────────

/// Assert that `bindings` contains at least one row where `var` has `expected_type`
/// and `expected_value`.
#[track_caller]
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

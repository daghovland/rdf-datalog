/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SPARQL 1.1 HTTP endpoint.
//!
//! Implements:
//! - P0: SPARQL 1.1 Protocol (GET/POST /sparql for SELECT/ASK/CONSTRUCT/DESCRIBE)
//! - P0: CORS headers
//! - P0: Content negotiation (SPARQL JSON, N-Triples, Turtle)
//! - P1: SPARQL 1.1 Service Description (GET /sparql with no query param)

pub mod frontend;
pub mod negotiate;
pub mod query;
pub mod serialize;
pub mod server;
pub mod service_desc;
pub mod upload;

use dag_rdf::datastore::Datastore;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Runtime configuration for the SPARQL endpoint.
#[derive(Clone, Debug)]
pub struct Config {
    /// Address to bind to (default: 0.0.0.0:3030).
    pub bind_addr: SocketAddr,
    /// Base IRI of this endpoint, used in Service Description.
    pub base_iri: String,
    /// If true, the update endpoint is disabled.
    pub read_only: bool,
    /// Maximum query execution time in seconds (default: 30).
    pub max_query_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_addr: "0.0.0.0:3030".parse().unwrap(),
            base_iri: "http://localhost:3030".to_string(),
            read_only: true,
            max_query_timeout_secs: 30,
        }
    }
}

/// Shared application state threaded through all handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<RwLock<Datastore>>,
    pub config: Config,
}

/// Start the SPARQL endpoint server.
///
/// Builds the axum router and binds to `config.bind_addr`.
/// This function does not return until the server shuts down.
pub async fn serve(store: Arc<RwLock<Datastore>>, config: Config) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    serve_on_listener(store, config, listener).await
}

/// Start the server on an already-bound listener.
///
/// Useful for tests: bind to port 0, read the OS-assigned port from
/// `listener.local_addr()`, then pass the listener here.
pub async fn serve_on_listener(
    store: Arc<RwLock<Datastore>>,
    config: Config,
    listener: tokio::net::TcpListener,
) -> Result<(), std::io::Error> {
    let state = AppState { store, config };
    let app = server::build_router(state);
    axum::serve(listener, app).await
}

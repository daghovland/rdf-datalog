/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! CLI entry point for `backlog_endpoint`. See `lib.rs` for what this crate
//! is and why it exists (issue #381 Stage 1 restructuring).

use clap::Parser;
use std::net::SocketAddr;

#[derive(Parser, Debug)]
#[command(
    name = "backlog-endpoint",
    about = "Standalone HTTP server for the dagalog backlog/provenance dashboard",
    long_about = "Serves the bl:/agp: backlog dashboard (see issue #381) as its own \
                  process, querying a separately running dagalog SPARQL endpoint over \
                  plain HTTP rather than being wired into dagalog's own HTTP server."
)]
struct Cli {
    /// Port to listen on
    #[arg(
        long = "port",
        value_name = "PORT",
        default_value = "3031",
        env = "BACKLOG_ENDPOINT_PORT"
    )]
    port: u16,

    /// URL of the dagalog SPARQL endpoint this dashboard queries, e.g. the
    /// `/sparql` route of a `dagalog --serve` instance
    #[arg(
        long = "sparql-endpoint",
        value_name = "URL",
        default_value = "http://localhost:3030/sparql",
        env = "BACKLOG_ENDPOINT_SPARQL_ENDPOINT"
    )]
    sparql_endpoint: String,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let addr = SocketAddr::from(([0, 0, 0, 0], cli.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));

    log::info!(
        "backlog_endpoint listening on http://{addr} (SPARQL endpoint: {})",
        cli.sparql_endpoint
    );

    let app = backlog_endpoint::build_router(cli.sparql_endpoint);
    axum::serve(listener, app)
        .await
        .expect("backlog_endpoint server error");
}

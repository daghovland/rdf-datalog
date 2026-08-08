/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `backlog_endpoint` — a standalone HTTP server for the read-only,
//! schema-*specific* dashboard over this repository's own `bl:`/`agp:`
//! backlog and provenance data.
//!
//! This crate is [#381](https://github.com/daghovland/rdf-datalog/issues/381)'s
//! "Stage 1" restructuring: the dashboard originally shipped as a `GET
//! /backlog` route inside `sparql_endpoint` itself, dagalog's own
//! domain-agnostic RDF/SPARQL engine binary. That coupled a GitHub-specific,
//! `bl:`/`agp:`-hardcoded application to the generic triplestore product.
//! `rdf-backlog` (this dashboard) uses dagalog purely as a backend over
//! plain HTTP — the way an app uses Postgres — so it now lives in its own
//! crate/binary within the same Cargo workspace, talking to a *running*
//! dagalog SPARQL endpoint over the network rather than in-process. See
//! `docs/plans/BACKLOG_PROVENANCE_DASHBOARD_PLAN.md` ("Decision: dogfood
//! tool, not a product feature") and
//! [#378](https://github.com/daghovland/rdf-datalog/issues/378) for the
//! full rationale. A full repository split ("Stage 2") remains available
//! later but isn't justified yet.
//!
//! Serves the dashboard page at both `/` (the primary route for this
//! standalone binary) and `/backlog` (kept for continuity with the
//! original route name). The configured `--sparql-endpoint` is injected
//! into the page as `window.SPARQL_ENDPOINT` so its JS — otherwise
//! unchanged from the original `sparql_endpoint`-hosted version — knows
//! where to send its queries instead of assuming same-origin `/sparql`.

use axum::{
    Router,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;

const BACKLOG_FRONTEND_HTML: &str = include_str!("backlog_frontend.html");

/// Serves [`BACKLOG_FRONTEND_HTML`] with a `<script>` prefix that sets
/// `window.SPARQL_ENDPOINT` to the configured target, so the page's
/// already-written JS (`sparqlFetch` in `backlog_frontend.html`) knows
/// where to send its queries. Plain string concatenation rather than a
/// templating dependency, since this is the only value ever injected.
async fn serve_backlog_frontend(sparql_endpoint: Arc<String>) -> Response {
    let injected = format!(
        "<script>window.SPARQL_ENDPOINT = {:?};</script>\n{}",
        sparql_endpoint.as_str(),
        BACKLOG_FRONTEND_HTML
    );
    (StatusCode::OK, Html(injected)).into_response()
}

/// Build the standalone dashboard `Router`, pointed at `sparql_endpoint`
/// (typically a locally running `dagalog --serve` instance's `/sparql`
/// route). Exposed separately from `main` so integration tests can spin up
/// the real router without going through the CLI/process boundary.
pub fn build_router(sparql_endpoint: String) -> Router {
    let sparql_endpoint = Arc::new(sparql_endpoint);
    let root_endpoint = sparql_endpoint.clone();
    let backlog_endpoint = sparql_endpoint;
    Router::new()
        .route(
            "/",
            get(move || {
                let sparql_endpoint = root_endpoint.clone();
                async move { serve_backlog_frontend(sparql_endpoint).await }
            }),
        )
        .route(
            "/backlog",
            get(move || {
                let sparql_endpoint = backlog_endpoint.clone();
                async move { serve_backlog_frontend(sparql_endpoint).await }
            }),
        )
}

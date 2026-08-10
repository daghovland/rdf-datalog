/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for the standalone backlog dashboard server (issue #381 Stage 1
//! restructuring — moved from `sparql_endpoint/tests/backlog_frontend.rs`,
//! which tested the same page as a `GET /backlog` route inside
//! `sparql_endpoint` before the dashboard became its own crate/binary).
//!
//! See [issue #381](https://github.com/daghovland/rdf-datalog/issues/381)
//! and `docs/plans/BACKLOG_PROVENANCE_DASHBOARD_PLAN.md`'s "Decision:
//! dogfood tool, not a product feature" section. As before, this is a
//! mostly-static HTML/JS page, so the practical test coverage is: assert
//! the routes exist, return the right content type, and the served body
//! contains markers proving the right file got served and that the
//! configured `--sparql-endpoint` was actually injected.

use std::net::SocketAddr;

/// A running `backlog_endpoint` test server bound to a random loopback port.
struct TestServer {
    base_url: String,
    client: reqwest::Client,
    _handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start(sparql_endpoint: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr: SocketAddr = listener.local_addr().expect("local_addr");
        let base_url = format!("http://{addr}");
        let app = backlog_endpoint::build_router(sparql_endpoint.to_string());
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server error");
        });
        tokio::task::yield_now().await;
        TestServer {
            base_url,
            client: reqwest::Client::new(),
            _handle: handle,
        }
    }
}

const TEST_SPARQL_ENDPOINT: &str = "http://localhost:3030/sparql";

/// `GET /` (the primary route for this standalone binary) must return
/// `200 OK`.
#[tokio::test]
async fn root_route_returns_200() {
    let server = TestServer::start(TEST_SPARQL_ENDPOINT).await;

    let resp = server
        .client
        .get(&server.base_url)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
}

/// `GET /backlog` is kept for continuity with the original route name and
/// must also return `200 OK`, serving the same page as `/`.
#[tokio::test]
async fn backlog_route_returns_200() {
    let server = TestServer::start(TEST_SPARQL_ENDPOINT).await;

    let resp = server
        .client
        .get(format!("{}/backlog", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
}

/// The response must be HTML (`Html(...)` sets `text/html; charset=utf-8`,
/// so this checks `contains`, not equality).
#[tokio::test]
async fn root_route_content_type_is_html() {
    let server = TestServer::start(TEST_SPARQL_ENDPOINT).await;

    let resp = server
        .client
        .get(&server.base_url)
        .send()
        .await
        .expect("request failed");

    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(ct.contains("text/html"), "expected text/html, got: {ct}");
}

/// The served body must be `backlog_frontend.html` -- distinguish by
/// markers specific to this page (its title and the hardcoded `bl:`/`agp:`
/// prefixes it queries with).
#[tokio::test]
async fn root_route_serves_dashboard_markers() {
    let server = TestServer::start(TEST_SPARQL_ENDPOINT).await;

    let resp = server
        .client
        .get(&server.base_url)
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body");
    assert!(
        body.contains("Backlog Dashboard"),
        "expected dashboard title marker in body"
    );
    assert!(
        body.contains("https://dagalog.no/ns/backlog#"),
        "expected bl: namespace hardcoded in body"
    );
    assert!(
        !body.contains("id=\"query-template\""),
        "backlog dashboard body should not be the generic query UI"
    );
}

/// The served body must contain the crates view (#382) -- its section id,
/// the Cytoscape.js CDN script URL it lazily loads for the dependency
/// graph, and the `bl:dependsOnCrate`/`bl:touchesCrate` queries it issues
/// -- proving the crate list + dependency graph + per-crate open-work-item
/// view actually shipped in this page rather than staying a stub.
#[tokio::test]
async fn root_route_serves_crates_view_markers() {
    let server = TestServer::start(TEST_SPARQL_ENDPOINT).await;

    let resp = server
        .client
        .get(&server.base_url)
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body");
    assert!(
        body.contains("id=\"view-crates\""),
        "expected crates view section in body"
    );
    assert!(
        body.contains("cytoscape@3.30.2"),
        "expected Cytoscape.js CDN script URL in body"
    );
    assert!(
        body.contains("bl:dependsOnCrate"),
        "expected a bl:dependsOnCrate query in body"
    );
    assert!(
        body.contains("bl:touchesCrate"),
        "expected a bl:touchesCrate query in body"
    );
    assert!(
        !body.contains("Crate list + dependency graph view is not built yet"),
        "crates view should no longer be a stub placeholder"
    );
}

/// The provenance timeline view (#383) must be present in the served body:
/// its section/list container and the `agp:` namespace it queries against.
/// This is the same "assert markers are in the static body" coverage level
/// the rest of this file uses for the #381 board/epics sections -- the page
/// is otherwise exercised by hand against the real dagalog/provenance
/// corpus (see #383's PR description for that manual verification).
#[tokio::test]
async fn root_route_serves_provenance_timeline_markers() {
    let server = TestServer::start(TEST_SPARQL_ENDPOINT).await;

    let resp = server
        .client
        .get(&server.base_url)
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body");
    assert!(
        body.contains("id=\"sessions-list\""),
        "expected provenance timeline's sessions-list container in body"
    );
    assert!(
        body.contains("https://dagalog.no/ns/agentprov#"),
        "expected agp: namespace hardcoded in body"
    );
    assert!(
        body.contains("agp:AgentSession"),
        "expected the provenance timeline's session query to reference agp:AgentSession"
    );
}

/// The "what's relevant" panel (#384) must be present in the served body:
/// its section id, an input control for the file-path/crate-name query,
/// and the two-hop query fragments (`bl:touchesFile`/`bl:touchesCrate`
/// joined through `agp:reasoningFor`) adapted from
/// `provenance/queries/related_to_file.sparql`/`related_to_crate.sparql`.
/// Same "assert markers in the static body" coverage level as the other
/// views in this file -- this panel has no live SPARQL endpoint to query
/// against in these tests, so it's verified by hand against the real
/// corpus (see #384's PR description for that manual verification).
#[tokio::test]
async fn root_route_serves_relevant_panel_markers() {
    let server = TestServer::start(TEST_SPARQL_ENDPOINT).await;

    let resp = server
        .client
        .get(&server.base_url)
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body");
    assert!(
        body.contains("id=\"view-relevant\""),
        "expected the relevant panel's section id in body"
    );
    assert!(
        body.contains("id=\"relevant-file-input\"") && body.contains("id=\"relevant-crate-input\""),
        "expected file-path and crate-name input controls in body"
    );
    assert!(
        body.contains("bl:touchesFile") && body.contains("bl:touchesCrate"),
        "expected both bl:touchesFile and bl:touchesCrate query fragments in body"
    );
    assert!(
        body.contains("agp:reasoningFor"),
        "expected agp:reasoningFor join fragment in body"
    );
}

/// The configured `--sparql-endpoint` must actually be injected into the
/// served page as `window.SPARQL_ENDPOINT`, proving the cross-process
/// wiring the #381 restructuring depends on (the page's JS reads this
/// global instead of assuming same-origin `/sparql`).
#[tokio::test]
async fn served_page_injects_configured_sparql_endpoint() {
    let custom_endpoint = "http://example-dagalog-host:9999/sparql";
    let server = TestServer::start(custom_endpoint).await;

    let resp = server
        .client
        .get(&server.base_url)
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body");
    assert!(
        body.contains(&format!("window.SPARQL_ENDPOINT = \"{custom_endpoint}\";")),
        "expected injected window.SPARQL_ENDPOINT = \"{custom_endpoint}\" in body, got prefix: {}",
        &body[..body.len().min(300)]
    );
}

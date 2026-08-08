/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for the backlog dashboard page shell (`GET /backlog`).
//!
//! See [issue #381](https://github.com/daghovland/rdf-datalog/issues/381)
//! and the epic-level plan
//! `docs/plans/BACKLOG_PROVENANCE_DASHBOARD_PLAN.md`. This page is
//! deliberately a separate route/file from the generic, schema-agnostic
//! `frontend.html` (see that plan's "Decision: dogfood tool, not a product
//! feature" section) -- it hardcodes `bl:`/`agp:` vocabulary and queries
//! the same dataset the generic UI does, over the same `/sparql` endpoint.
//!
//! This is a mostly-static HTML/JS page, so per this repo's TDD guidance
//! the practical test coverage is: assert the route exists, returns the
//! right content type, and the served body contains markers proving the
//! right file (not `frontend.html`) got served. The actual SPARQL queries
//! embedded in the page's JS were verified manually against a locally
//! served instance of the real backlog+provenance dataset (see PR
//! description) -- there is no in-process JS execution harness here.

mod common;

/// `GET /backlog` must return `200 OK`.
#[tokio::test]
async fn backlog_route_returns_200() {
    let server = common::TestServer::start("").await;

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
async fn backlog_route_content_type_is_html() {
    let server = common::TestServer::start("").await;

    let resp = server
        .client
        .get(format!("{}/backlog", server.base_url))
        .send()
        .await
        .expect("request failed");

    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(ct.contains("text/html"), "expected text/html, got: {ct}");
}

/// The served body must be `backlog_frontend.html`, not the generic
/// `frontend.html` -- distinguish by markers specific to this page (its
/// title and the hardcoded `bl:`/`agp:` prefixes it queries with).
#[tokio::test]
async fn backlog_route_serves_dashboard_markers() {
    let server = common::TestServer::start("").await;

    let resp = server
        .client
        .get(format!("{}/backlog", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body");
    assert!(
        body.contains("Backlog Dashboard"),
        "expected dashboard title marker in body"
    );
    assert!(
        body.contains("https://dagalog.dev/ns/backlog#"),
        "expected bl: namespace hardcoded in body"
    );
    assert!(
        !body.contains("id=\"query-template\""),
        "backlog dashboard body should not be the generic query UI"
    );
}

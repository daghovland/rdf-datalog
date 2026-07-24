/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the RDF write-route body-size limit
//! (`Config::max_rdf_upload_bytes`), covering
//! <https://github.com/daghovland/rdf-datalog/issues/274>.
//!
//! Before this fix, `POST`/`PUT /{name}/data`, `/rdf-graph-store`,
//! `/rdf-graphs/{*path}`, `/upload`, and `/{name}/shacl` all fell back to
//! axum's server-wide 2 MiB `DefaultBodyLimit`, so realistic RDF payloads
//! (which routinely exceed 2 MiB) were rejected with
//! `413 RequestEntityTooLarge` even though parsing was otherwise fine.

mod common;

const DS: &str = "ds";

/// Builds a syntactically valid Turtle document of at least `min_bytes` bytes:
/// one prefix declaration followed by many small triples.
fn large_turtle(min_bytes: usize) -> String {
    let mut turtle = String::from("@prefix ex: <http://example.org/> .\n");
    let mut i: u64 = 0;
    while turtle.len() < min_bytes {
        i += 1;
        turtle.push_str(&format!(
            "ex:subject{i} ex:predicate{i} \"value number {i} with some padding text to bulk up the line\" .\n"
        ));
    }
    turtle
}

// ── PUT /{name}/data — Fuseki-compatible per-dataset GSP ──────────────────────

/// A >2 MiB Turtle body PUT to `/{name}/data` must be accepted under the
/// default 64 MiB `max_rdf_upload_bytes`, proving the per-route override is
/// in effect (not just the route's existence).
#[tokio::test]
async fn dataset_data_put_accepts_upload_larger_than_2mb() {
    let server = common::TestServer::start_writable("").await;
    let big_turtle = large_turtle(3 * 1024 * 1024);

    let resp = server
        .client
        .put(server.dataset_data_default_url(DS))
        .header("content-type", "text/turtle")
        .body(big_turtle)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status().is_success(),
        "a >2MB body must be accepted under the raised per-route limit, got {}",
        resp.status()
    );
}

/// A body under the old 2 MiB default is unaffected by the change and still
/// succeeds (sanity check that small requests keep working).
#[tokio::test]
async fn dataset_data_put_accepts_small_upload() {
    let server = common::TestServer::start_writable("").await;
    let small_turtle = large_turtle(1024);

    let resp = server
        .client
        .put(server.dataset_data_default_url(DS))
        .header("content-type", "text/turtle")
        .body(small_turtle)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status().is_success(),
        "a small body must succeed as before, got {}",
        resp.status()
    );
}

/// With a small configured `max_rdf_upload_bytes`, a body exceeding *that*
/// limit is still correctly rejected — proving the override is a real,
/// enforced limit and not simply "no limit at all".
#[tokio::test]
async fn dataset_data_put_rejects_upload_over_configured_limit() {
    let server = common::TestServer::start_writable_with_rdf_limit("", 1024).await;
    let big_turtle = large_turtle(8 * 1024);

    let resp = server
        .client
        .put(server.dataset_data_default_url(DS))
        .header("content-type", "text/turtle")
        .body(big_turtle)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        413,
        "a body over the configured limit must be rejected with 413"
    );
}

// ── POST/PUT /rdf-graph-store — SPARQL Graph Store HTTP Protocol root ────────

#[tokio::test]
async fn gsp_put_accepts_upload_larger_than_2mb() {
    let server = common::TestServer::start_writable("").await;
    let big_turtle = large_turtle(3 * 1024 * 1024);

    let resp = server
        .client
        .put(format!("{}?default", server.gsp_url()))
        .header("content-type", "text/turtle")
        .body(big_turtle)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status().is_success(),
        "a >2MB body must be accepted under the raised per-route limit, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn gsp_put_rejects_upload_over_configured_limit() {
    let server = common::TestServer::start_writable_with_rdf_limit("", 1024).await;
    let big_turtle = large_turtle(8 * 1024);

    let resp = server
        .client
        .put(format!("{}?default", server.gsp_url()))
        .header("content-type", "text/turtle")
        .body(big_turtle)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        413,
        "a body over the configured limit must be rejected with 413"
    );
}

// ── POST /upload — browser UI convenience endpoint ───────────────────────────

#[tokio::test]
async fn upload_accepts_upload_larger_than_2mb() {
    let server = common::TestServer::start_writable("").await;
    let big_turtle = large_turtle(3 * 1024 * 1024);

    let resp = server
        .client
        .post(format!("{}/upload", server.base_url))
        .header("content-type", "text/turtle")
        .body(big_turtle)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        200,
        "a >2MB body must be accepted under the raised per-route limit"
    );
}

#[tokio::test]
async fn upload_rejects_upload_over_configured_limit() {
    let server = common::TestServer::start_writable_with_rdf_limit("", 1024).await;
    let big_turtle = large_turtle(8 * 1024);

    let resp = server
        .client
        .post(format!("{}/upload", server.base_url))
        .header("content-type", "text/turtle")
        .body(big_turtle)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        413,
        "a body over the configured limit must be rejected with 413"
    );
}

// ── POST /{name}/shacl — SHACL validation endpoint ───────────────────────────

#[tokio::test]
async fn shacl_post_accepts_upload_larger_than_2mb() {
    let server = common::TestServer::start_writable("").await;
    let mut big_shapes = String::from(
        "@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix ex: <http://example.org/> .\n",
    );
    let mut i: u64 = 0;
    while big_shapes.len() < 3 * 1024 * 1024 {
        i += 1;
        big_shapes.push_str(&format!(
            "ex:Shape{i} a sh:NodeShape ; sh:targetClass ex:Class{i} .\n"
        ));
    }

    let resp = server
        .client
        .post(format!("{}/{DS}/shacl", server.base_url))
        .header("content-type", "text/turtle")
        .body(big_shapes)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        200,
        "a >2MB shapes body must be accepted under the raised per-route limit"
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the RML REST endpoints:
//! - `POST /{name}/rml` — apply an RML mapping uploaded as
//!   `multipart/form-data` to a named dataset.
//! - `POST /rml/map` — apply a mapping and return the generated RDF
//!   directly, without touching any dataset.
//!
//! See `docs/plans/RML_REST_ENDPOINT_PLAN.md` for the full design.
//!
//! Spec: <https://www.w3.org/TR/rml/>

mod common;

use reqwest::multipart::{Form, Part};

const DS: &str = "ds";

const PEOPLE_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/PersonMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "people.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Person/{id}" ;
        rml:class ex:Person
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;

const PEOPLE_CSV: &str = "id,name\n1,Alice\n2,Bob\n";

const NAMED_GRAPH_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/PersonMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "people.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Person/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ] ;
        rml:graphMap [ rml:constant <http://example.com/MyGraph> ]
    ] .
"#;

/// 1. A multipart POST with a mapping + its CSV source must insert the mapped
/// triples into the dataset, visible via a subsequent SPARQL SELECT.
#[tokio::test]
async fn rml_post_csv_mapping_inserts_triples() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    let sparql =
        "SELECT ?name WHERE { <http://example.com/Person/1> <http://example.com/name> ?name }";
    let resp = server
        .client
        .get(server.dataset_sparql_url(DS) + "?query=" + &urlencoding::encode(sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
}

/// 2. A multipart POST without a `mapping` part must be rejected with 400.
#[tokio::test]
async fn rml_post_missing_mapping_part_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// 3. Posting to an unknown dataset must return 404.
#[tokio::test]
async fn rml_post_unknown_dataset_is_not_found() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part(
        "mapping",
        Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
    );

    let resp = server
        .client
        .post(server.dataset_rml_url("nonexistent"))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// 4. A read-only server must reject RML uploads with 403.
#[tokio::test]
async fn rml_post_read_only_server_is_forbidden() {
    let server = common::TestServer::start("").await;

    let form = Form::new().part(
        "mapping",
        Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
    );

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 403);
}

/// 5. Malformed Turtle in the `mapping` part must be rejected with 400 and an
/// error message in the body.
#[tokio::test]
async fn rml_post_invalid_mapping_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part(
        "mapping",
        Part::text("this is not valid turtle {{{").file_name("mapping.ttl"),
    );

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.expect("body");
    assert!(!body.is_empty(), "error body should describe the failure");
}

/// 6. A mapping using `rml:graphMap` must place triples in the named graph,
/// not the default graph.
#[tokio::test]
async fn rml_post_with_named_graph_inserts_into_named_graph() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(NAMED_GRAPH_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    let in_graph_sparql = "SELECT ?name WHERE { GRAPH <http://example.com/MyGraph> { <http://example.com/Person/1> <http://example.com/name> ?name } }";
    let resp = server
        .client
        .get(server.dataset_sparql_url(DS) + "?query=" + &urlencoding::encode(in_graph_sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    common::assert_binding_contains(bindings, "name", "literal", "Alice");

    let default_graph_sparql =
        "SELECT ?name WHERE { <http://example.com/Person/1> <http://example.com/name> ?name }";
    let resp = server
        .client
        .get(server.dataset_sparql_url(DS) + "?query=" + &urlencoding::encode(default_graph_sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    assert!(
        bindings.is_empty(),
        "triples with a graphMap must not appear in the default graph"
    );
}

/// 7. An RML mapping applied to a persistent server must survive a restart
/// (changelog replay).
#[tokio::test]
async fn rml_post_persists_to_changelog() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;

        let form = Form::new()
            .part(
                "mapping",
                Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
            )
            .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

        let resp = server
            .client
            .post(server.dataset_rml_url(DS))
            .multipart(form)
            .send()
            .await
            .expect("request failed");
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    let sparql =
        "SELECT ?name WHERE { <http://example.com/Person/1> <http://example.com/name> ?name }";
    let resp = server2
        .client
        .get(server2.sparql_query_url(sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
}

/// 8. On a server protected by an API key, an RML upload without the key must
/// be rejected with 401 (end-to-end check of the `Permission::Write`
/// classification, complementing the unit test in `auth.rs`).
#[tokio::test]
async fn rml_post_write_permission_required() {
    const KEY: &str = "test-key";
    let server = common::TestServer::start_writable_with_key("", KEY).await;

    let form = Form::new().part(
        "mapping",
        Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
    );

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 401, "missing key must return 401");
}

/// Builds a CSV with at least `min_bytes` bytes, to exercise the raised
/// per-route body limit (see `Config::max_rml_upload_bytes`).
fn large_people_csv(min_bytes: usize) -> String {
    let mut csv = String::from("id,name\n");
    let mut i = 0u32;
    while csv.len() < min_bytes {
        i += 1;
        csv.push_str(&format!("{i},Person{i}\n"));
    }
    csv
}

/// 9. A source file comfortably over axum's 2 MB `DefaultBodyLimit` must still
/// be accepted, proving the per-route override (`Config::max_rml_upload_bytes`,
/// default 64 MiB) is actually in effect and not just the route's existence.
#[tokio::test]
async fn rml_post_accepts_upload_larger_than_2mb() {
    let server = common::TestServer::start_writable("").await;
    let big_csv = large_people_csv(3 * 1024 * 1024);

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(big_csv).file_name("people.csv"));

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        200,
        "a >2MB source file must be accepted under the raised per-route limit"
    );
}

/// 10. With a small configured `max_rml_upload_bytes`, a source file exceeding
/// *that* limit is correctly rejected — proving `Config::max_rml_upload_bytes`
/// (wired to `--max-rml-upload-bytes` / `DAGALOG_MAX_RML_UPLOAD_BYTES`) is a real,
/// enforced limit and not simply "no limit at all". See #257.
///
/// Unlike the raw-body RDF write routes (which reject with 413 directly from
/// axum's `DefaultBodyLimit` layer), these routes use the `Multipart`
/// extractor: a body-limit violation surfaces as a `MultipartError` while
/// reading a field, which the handler maps to 400 with a descriptive message
/// (see `materialize_multipart` in `rml_endpoint.rs`). So the expected
/// rejection status here is 400, not 413.
#[tokio::test]
async fn rml_post_rejects_upload_over_configured_limit() {
    let server = common::TestServer::start_writable_with_rml_limit("", 1024).await;
    let big_csv = large_people_csv(8 * 1024);

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(big_csv).file_name("people.csv"));

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        400,
        "a source file over the configured limit must be rejected"
    );
    let body = resp.text().await.expect("body");
    assert!(!body.is_empty(), "error body should describe the failure");
}

/// 11. A normal-sized upload still succeeds under a small but sufficient
/// configured `max_rml_upload_bytes` (sanity check that the override is not
/// simply rejecting everything).
#[tokio::test]
async fn rml_post_accepts_upload_within_configured_limit() {
    let server = common::TestServer::start_writable_with_rml_limit("", 64 * 1024).await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        200,
        "a normal-sized upload must succeed under a sufficiently large configured limit"
    );
}

// ── POST /rml/map — stateless mapping endpoint ────────────────────────────────

/// 1. A multipart POST with a mapping + CSV source, no `Accept` header, must
/// return the generated RDF as Turtle (the default format).
#[tokio::test]
async fn rml_map_csv_mapping_returns_turtle_by_default() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.rml_map_url())
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.starts_with("text/turtle"),
        "expected text/turtle, got {content_type}"
    );
    let body = resp.text().await.expect("body");
    assert!(
        body.contains("Alice"),
        "expected generated Turtle to contain mapped data, got:\n{body}"
    );
}

/// 2. A multipart POST without a `mapping` part must be rejected with 400.
#[tokio::test]
async fn rml_map_missing_mapping_part_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.rml_map_url())
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// 3. Malformed Turtle in the `mapping` part must be rejected with 400 and an
/// error message in the body.
#[tokio::test]
async fn rml_map_invalid_mapping_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part(
        "mapping",
        Part::text("this is not valid turtle {{{").file_name("mapping.ttl"),
    );

    let resp = server
        .client
        .post(server.rml_map_url())
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.expect("body");
    assert!(!body.is_empty(), "error body should describe the failure");
}

/// 4. `/rml/map` must never write to any dataset — applying a mapping that
/// would generate a recognisable subject must leave `/ds` untouched.
#[tokio::test]
async fn rml_map_does_not_modify_any_dataset() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.rml_map_url())
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    let sparql =
        "SELECT ?name WHERE { <http://example.com/Person/1> <http://example.com/name> ?name }";
    let resp = server
        .client
        .get(server.dataset_sparql_url(DS) + "?query=" + &urlencoding::encode(sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    assert!(
        bindings.is_empty(),
        "/rml/map must not write into any dataset"
    );
}

/// 5. `Accept: application/ld+json` must return the generated RDF as JSON-LD.
#[tokio::test]
async fn rml_map_respects_accept_header_jsonld() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.rml_map_url())
        .header("accept", "application/ld+json")
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.starts_with("application/ld+json"),
        "expected application/ld+json, got {content_type}"
    );
    let body: serde_json::Value = resp.json().await.expect("json body");
    let body_str = body.to_string();
    assert!(
        body_str.contains("Alice"),
        "expected generated JSON-LD to contain mapped data, got:\n{body_str}"
    );
}

/// 6. A mapping using `rml:graphMap`, requested as `application/n-quads`,
/// must include the named graph's IRI as the quad's fourth term.
#[tokio::test]
async fn rml_map_with_named_graph_returns_nquads() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(NAMED_GRAPH_MAPPING).file_name("mapping.ttl"),
        )
        .part("people.csv", Part::text(PEOPLE_CSV).file_name("people.csv"));

    let resp = server
        .client
        .post(server.rml_map_url())
        .header("accept", "application/n-quads")
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.starts_with("application/n-quads"),
        "expected application/n-quads, got {content_type}"
    );
    let body = resp.text().await.expect("body");
    assert!(
        body.contains("http://example.com/MyGraph"),
        "expected the named graph IRI as the quad's 4th term, got:\n{body}"
    );
}

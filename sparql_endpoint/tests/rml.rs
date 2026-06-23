/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `POST /{name}/rml` — apply an RML mapping uploaded as
//! `multipart/form-data` to a named dataset.
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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
async fn rml_post_csv_mapping_inserts_triples() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
        )
        .part(
            "people.csv",
            Part::text(PEOPLE_CSV).file_name("people.csv"),
        );

    let resp = server
        .client
        .post(server.dataset_rml_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    let sparql = "SELECT ?name WHERE { <http://example.com/Person/1> <http://example.com/name> ?name }";
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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
async fn rml_post_missing_mapping_part_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part(
        "people.csv",
        Part::text(PEOPLE_CSV).file_name("people.csv"),
    );

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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
async fn rml_post_with_named_graph_inserts_into_named_graph() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part(
            "mapping",
            Part::text(NAMED_GRAPH_MAPPING).file_name("mapping.ttl"),
        )
        .part(
            "people.csv",
            Part::text(PEOPLE_CSV).file_name("people.csv"),
        );

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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
async fn rml_post_persists_to_changelog() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;

        let form = Form::new()
            .part(
                "mapping",
                Part::text(PEOPLE_MAPPING).file_name("mapping.ttl"),
            )
            .part(
                "people.csv",
                Part::text(PEOPLE_CSV).file_name("people.csv"),
            );

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
    let sparql = "SELECT ?name WHERE { <http://example.com/Person/1> <http://example.com/name> ?name }";
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
#[ignore = "red phase: dataset_rml_post is not yet implemented — see RML_REST_ENDPOINT_PLAN.md"]
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

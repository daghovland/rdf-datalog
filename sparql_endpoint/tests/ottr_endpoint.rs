/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the OTTR REST endpoint:
//! - `POST /{name}/ottr` — expand stOTTR templates/instances (uploaded as
//!   `multipart/form-data`, one or more self-contained stOTTR-document parts)
//!   into a named dataset.
//!
//! See `docs/plans/OTTR_HTTP_ENDPOINT_PLAN.md` for the full design.
//!
//! Spec: <https://spec.ottr.xyz/>

mod common;

use reqwest::multipart::{Form, Part};

const DS: &str = "ds";

/// A single self-contained stOTTR document: one template definition plus two
/// instance calls (mirrors `ottr/tests/fixtures/combined.stottr`).
const COMBINED: &str = r#"
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

ex:LabeledThing [ ottr:IRI ?thing, ottr:Literal ?label ] :: {
  ottr:Triple (?thing, rdf:type, ex:Thing),
  ottr:Triple (?thing, rdfs:label, ?label)
} .

ex:LabeledThing(<http://example.com/Widget>, "Widget") .
ex:LabeledThing(<http://example.com/Gadget>, "Gadget") .
"#;

/// Template-only document (no instances) — split-across-parts test.
const PERSON_TEMPLATE: &str = r#"
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

ex:Person [ ottr:IRI ?person, xsd:string ?name, ottr:IRI ?email ] :: {
  ottr:Triple (?person, rdf:type, foaf:Person),
  ottr:Triple (?person, foaf:name, ?name),
  ottr:Triple (?person, foaf:mbox, ?email)
} .
"#;

/// Instance-only document (no templates) — calls `ex:Person` from
/// `PERSON_TEMPLATE`, defined in a separate part.
const PERSON_INSTANCES: &str = r#"
@prefix ex:    <http://example.com/> .
@prefix foaf:  <http://xmlns.com/foaf/0.1/> .

ex:Person(<http://example.com/alice>, "Alice", <mailto:alice@example.com>) .
ex:Person(<http://example.com/bob>,   "Bob",   <mailto:bob@example.com>) .
"#;

const MALFORMED: &str = "this is not valid stOTTR syntax {{{";

/// An instance calling a template that is never defined in the request.
const UNKNOWN_TEMPLATE: &str = r#"
@prefix ex: <http://example.com/> .

ex:NoSuchTemplate(<http://example.com/thing>) .
"#;

/// 1. A single multipart part with a self-contained stOTTR document (template
/// + instances) must expand and insert the resulting triples into the
/// dataset, visible via a subsequent SPARQL SELECT.
#[tokio::test]
async fn ottr_post_single_part_inserts_triples() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part("document", Part::text(COMBINED));

    let resp = server
        .client
        .post(server.dataset_ottr_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    let sparql = "SELECT ?label WHERE { <http://example.com/Widget> \
                  <http://www.w3.org/2000/01/rdf-schema#label> ?label }";
    let resp = server
        .client
        .get(server.dataset_sparql_url(DS) + "?query=" + &urlencoding::encode(sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    common::assert_binding_contains(bindings, "label", "literal", "Widget");
}

/// 2. Templates and instances split across two separate multipart parts must
/// still expand correctly — proves `ottr::expand_documents` merges templates
/// across parsed documents before expanding instances.
#[tokio::test]
async fn ottr_post_templates_and_instances_split_across_parts() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new()
        .part("templates", Part::text(PERSON_TEMPLATE))
        .part("instances", Part::text(PERSON_INSTANCES));

    let resp = server
        .client
        .post(server.dataset_ottr_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);

    let sparql = "SELECT ?name WHERE { <http://example.com/alice> \
                  <http://xmlns.com/foaf/0.1/name> ?name }";
    let resp = server
        .client
        .get(server.dataset_sparql_url(DS) + "?query=" + &urlencoding::encode(sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
}

/// 3. A multipart body with zero parts must be rejected with `400`.
#[tokio::test]
async fn ottr_post_zero_parts_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new();

    let resp = server
        .client
        .post(server.dataset_ottr_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// 4. Malformed stOTTR syntax in a part must be rejected with `400` and the
/// parse error included in the body.
#[tokio::test]
async fn ottr_post_invalid_stottr_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part("document", Part::text(MALFORMED));

    let resp = server
        .client
        .post(server.dataset_ottr_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// 5. An instance referencing an undefined template must be rejected with
/// `400`.
#[tokio::test]
async fn ottr_post_unknown_template_is_bad_request() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part("document", Part::text(UNKNOWN_TEMPLATE));

    let resp = server
        .client
        .post(server.dataset_ottr_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// 6. POSTing to a dataset that doesn't exist must return `404`.
#[tokio::test]
async fn ottr_post_unknown_dataset_is_not_found() {
    let server = common::TestServer::start_writable("").await;

    let form = Form::new().part("document", Part::text(COMBINED));

    let resp = server
        .client
        .post(server.dataset_ottr_url("nonexistent"))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// 7. POSTing against a read-only server must return `403`.
#[tokio::test]
async fn ottr_post_read_only_server_is_forbidden() {
    let server = common::TestServer::start("").await;

    let form = Form::new().part("document", Part::text(COMBINED));

    let resp = server
        .client
        .post(server.dataset_ottr_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 403);
}

/// 8. An OTTR expansion applied to a persistent server must survive a restart
/// (changelog replay) — mirrors `rml_post_persists_to_changelog`.
#[tokio::test]
async fn ottr_post_persists_to_changelog() {
    let dir = tempfile::tempdir().unwrap();

    {
        let server = common::TestServer::start_writable_persistent("", dir.path()).await;

        let form = Form::new().part("document", Part::text(COMBINED));

        let resp = server
            .client
            .post(server.dataset_ottr_url(DS))
            .multipart(form)
            .send()
            .await
            .expect("request failed");
        assert_eq!(resp.status(), 200);
        server.shutdown().await;
    }

    let server2 = common::TestServer::start_writable_persistent("", dir.path()).await;
    let sparql = "SELECT ?label WHERE { <http://example.com/Widget> \
                  <http://www.w3.org/2000/01/rdf-schema#label> ?label }";
    let resp = server2
        .client
        .get(server2.sparql_query_url(sparql))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query failed");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    common::assert_binding_contains(bindings, "label", "literal", "Widget");
}

/// 9. On a server protected by an API key, an OTTR expansion without the key
/// must be rejected with `401` (end-to-end check of the `Permission::Write`
/// classification, complementing the unit test in `auth.rs`).
#[tokio::test]
async fn ottr_post_write_permission_required() {
    const KEY: &str = "test-key";
    let server = common::TestServer::start_writable_with_key("", KEY).await;

    let form = Form::new().part("document", Part::text(COMBINED));

    let resp = server
        .client
        .post(server.dataset_ottr_url(DS))
        .multipart(form)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status().as_u16(), 401, "missing key must return 401");
}

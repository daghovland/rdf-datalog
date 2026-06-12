#![allow(dead_code)]
/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Fuseki drop-in compatibility tests.
//!
//! These tests verify that dagalog accepts the same HTTP surface as Apache Jena
//! Fuseki operating in in-memory mode (`--mem /ds`).  A client that works
//! against Fuseki must work unmodified against dagalog.
//!
//! **Fuseki documentation:**
//! <https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html>
//!
//! All tests are `#[ignore]` — remove the attribute from a group after its
//! corresponding phase is implemented.  See `SERVER.md §6.6` for the phase
//! table and implementation order.
//!
//! ## Test organisation
//!
//! | Group | Phase | Description |
//! |-------|-------|-------------|
//! | A | F1 | Per-dataset query endpoint (`/{name}/sparql`, `/{name}/query`) |
//! | B | F1 | Per-dataset GSP (`/{name}/data`) |
//! | C | F2 | Admin: ping + server info |
//! | D | F3 | Admin: list + info datasets |
//! | E | F4 | Admin: create + delete datasets |
//! | F | F5 | SPARQL Update (`/{name}/update`) |
//! | G | F6 | GSP content negotiation (N-Quads, TriG upload + download) |
//! | H | F7 | Dynamic multi-dataset routing |
//! | I | F8 | Full lifecycle (create → upload → query → delete) |

mod common;

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// Default dataset name used throughout these tests, matching the Fuseki
/// convention of starting a single in-memory dataset as `--mem /ds`.
const DS: &str = "ds";

/// A small Turtle dataset loaded into the default graph at test startup.
const PEOPLE_TTL: &str = r#"
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .
    @prefix ex:   <http://example.org/> .

    ex:alice foaf:name "Alice" ;
             a foaf:Person .
    ex:bob   foaf:name "Bob" ;
             a foaf:Person .
"#;

/// Named graph IRI used in tests that exercise named-graph operations.
const GRAPH_IRI: &str = "http://example.org/graph1";

/// TriG document with one named graph and two triples.
const NAMED_GRAPH_TRIG: &str = r#"
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .
    @prefix ex:   <http://example.org/> .

    <http://example.org/graph1> {
        ex:charlie foaf:name "Charlie" ;
                   a foaf:Person .
    }
"#;

/// Turtle to upload in write tests.
const UPLOAD_TTL: &str = r#"
    @prefix ex:   <http://example.org/> .
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .

    ex:diana foaf:name "Diana" ;
             a foaf:Person .
"#;

/// N-Triples payload for format-negotiation tests.
const UPLOAD_NT: &str = "<http://example.org/eve> <http://xmlns.com/foaf/0.1/name> \"Eve\" .\n";

/// N-Quads payload placing one triple in a named graph.
const UPLOAD_NQ: &str = "<http://example.org/frank> <http://xmlns.com/foaf/0.1/name> \"Frank\" . <http://example.org/graph2> .\n";

/// Syntactically invalid Turtle used to test 400 responses.
const BAD_TTL: &str = "THIS IS NOT VALID TURTLE !!!";

/// A simple SPARQL SELECT that should return results for `PEOPLE_TTL`.
const SELECT_ALL_PEOPLE: &str =
    "SELECT ?person WHERE { ?person a <http://xmlns.com/foaf/0.1/Person> }";

/// A minimal SPARQL Update: insert one triple.
const INSERT_TRIPLE: &str = r#"
    INSERT DATA {
        <http://example.org/grace> <http://xmlns.com/foaf/0.1/name> "Grace" .
    }
"#;

/// Delete the triple added by INSERT_TRIPLE.
const DELETE_TRIPLE: &str = r#"
    DELETE DATA {
        <http://example.org/grace> <http://xmlns.com/foaf/0.1/name> "Grace" .
    }
"#;

/// Clear all triples in the default graph.
const CLEAR_DEFAULT: &str = "CLEAR DEFAULT";

/// SPARQL Update that drops a named graph entirely.
const DROP_GRAPH: &str = "DROP GRAPH <http://example.org/graph1>";

// ── A: Per-dataset query endpoint — Phase F1 ─────────────────────────────────
//
// Fuseki exposes `/{name}/sparql` and `/{name}/query` as aliases for the
// SPARQL 1.1 query endpoint.
//
// Spec: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html#datasets-and-services

/// A-1: GET `/{name}/sparql?query=<encoded>` — 200 with SPARQL JSON results.
///
/// Fuseki routes GET requests with a `query` parameter to the SPARQL query
/// engine and returns `application/sparql-results+json` by default.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_get_sparql_query_returns_results() {
    let server = common::TestServer::start(PEOPLE_TTL).await;
    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(DS),
        urlencoding::encode(SELECT_ALL_PEOPLE)
    );
    let resp = server.client.get(url).send().await.expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("application/sparql-results+json"),
        "unexpected content-type: {ct}"
    );
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert!(!bindings.is_empty(), "expected at least one Person result");
}

/// A-2: `/{name}/query` alias returns the same results as `/{name}/sparql`.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_query_alias_works() {
    let server = common::TestServer::start(PEOPLE_TTL).await;
    let url = format!(
        "{}?query={}",
        server.dataset_query_url(DS),
        urlencoding::encode(SELECT_ALL_PEOPLE)
    );
    let resp = server.client.get(url).send().await.expect("request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(body["results"]["bindings"].as_array().is_some());
}

/// A-3: POST `/{name}/sparql` with `application/sparql-query` body.
///
/// Fuseki also accepts raw query text as the POST body with this content-type.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_post_sparql_query_direct_body() {
    let server = common::TestServer::start(PEOPLE_TTL).await;
    let resp = server
        .client
        .post(server.dataset_sparql_url(DS))
        .header("Content-Type", "application/sparql-query")
        .body(SELECT_ALL_PEOPLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// A-4: POST `/{name}/sparql` with form-encoded `query=` parameter.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_post_sparql_query_form_encoded() {
    let server = common::TestServer::start(PEOPLE_TTL).await;
    let resp = server
        .client
        .post(server.dataset_sparql_url(DS))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("query={}", urlencoding::encode(SELECT_ALL_PEOPLE)))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// A-5: Query to non-existent dataset returns 404.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_query_nonexistent_dataset_returns_404() {
    let server = common::TestServer::start("").await;
    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url("nonexistent"),
        urlencoding::encode("SELECT * WHERE { ?s ?p ?o }")
    );
    let resp = server.client.get(url).send().await.expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// A-6: Malformed SPARQL on `/{name}/sparql` returns 400.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_sparql_malformed_query_returns_400() {
    let server = common::TestServer::start(PEOPLE_TTL).await;
    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(DS),
        urlencoding::encode("NOT VALID SPARQL")
    );
    let resp = server.client.get(url).send().await.expect("request failed");
    assert_eq!(resp.status(), 400);
}

// ── B: Per-dataset GSP — Phase F1 ────────────────────────────────────────────
//
// Fuseki exposes `/{name}/data` as the Graph Store Protocol endpoint for
// read-write operations.  It supports the same query parameters as the W3C
// GSP spec: `?default` and `?graph=<iri>`.
//
// Spec: https://www.w3.org/TR/sparql11-http-rdf-update/
// Fuseki: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html

/// B-1: GET `/{name}/data?default` — 200 with Turtle body.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_gsp_get_default_graph_returns_200() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let resp = server
        .client
        .get(server.dataset_data_default_url(DS))
        .header("Accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(ct.contains("text/turtle"), "unexpected content-type: {ct}");
}

/// B-2: PUT `/{name}/data?default` replaces the default graph.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_gsp_put_default_graph_returns_204() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let resp = server
        .client
        .put(server.dataset_data_default_url(DS))
        .header("Content-Type", "text/turtle")
        .body(UPLOAD_TTL)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 204 || resp.status() == 200,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// B-3: POST `/{name}/data?default` merges triples into the default graph.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_gsp_post_merge_returns_204() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let resp = server
        .client
        .post(server.dataset_data_default_url(DS))
        .header("Content-Type", "text/turtle")
        .body(UPLOAD_TTL)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 204 || resp.status() == 200,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// B-4: DELETE `/{name}/data?default` clears the default graph.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_gsp_delete_default_graph_returns_200_or_204() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let resp = server
        .client
        .delete(server.dataset_data_default_url(DS))
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// B-5: GET `/{name}/data?graph=<iri>` — returns a named graph as Turtle.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_gsp_get_named_graph_returns_200() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let resp = server
        .client
        .get(server.dataset_data_graph_url(DS, GRAPH_IRI))
        .header("Accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// B-6: GET `/{name}/data?graph=<iri>` for non-existent graph returns 404.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_gsp_get_nonexistent_named_graph_returns_404() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .get(server.dataset_data_graph_url(DS, "http://example.org/no-such-graph"))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// B-7: PUT `/{name}/data?graph=<iri>` creates a new named graph.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_gsp_put_named_graph_creates_graph() {
    let server = common::TestServer::start_writable("").await;
    let put = server
        .client
        .put(server.dataset_data_graph_url(DS, GRAPH_IRI))
        .header("Content-Type", "text/turtle")
        .body(UPLOAD_TTL)
        .send()
        .await
        .expect("PUT failed");
    assert!(
        put.status() == 201 || put.status() == 204 || put.status() == 200,
        "expected 201/204/200, got {}",
        put.status()
    );
    let get = server
        .client
        .get(server.dataset_data_graph_url(DS, GRAPH_IRI))
        .header("Accept", "text/turtle")
        .send()
        .await
        .expect("GET failed");
    assert_eq!(get.status(), 200);
}

/// B-8: `/{name}/get` read-only endpoint allows GET but rejects PUT.
///
/// Phase F1 — SERVER.md §6.1
#[ignore]
#[tokio::test]
async fn fuseki_get_readonly_endpoint_rejects_put() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let get = server
        .client
        .get(format!("{}?default", server.dataset_get_url(DS)))
        .send()
        .await
        .expect("GET failed");
    assert_eq!(get.status(), 200);

    let put = server
        .client
        .put(format!("{}?default", server.dataset_get_url(DS)))
        .header("Content-Type", "text/turtle")
        .body(UPLOAD_TTL)
        .send()
        .await
        .expect("PUT request failed");
    assert!(
        put.status() == 405 || put.status() == 403,
        "read-only /get endpoint must reject PUT with 405/403, got {}",
        put.status()
    );
}

// ── C: Admin — ping and server info — Phase F2 ───────────────────────────────
//
// Fuseki exposes a management API under `/$/`.  The simplest two endpoints are
// `/$/ping` (liveness) and `/$/server` (server metadata).
//
// Spec: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html#server-information

/// C-1: GET `/$/ping` — liveness check returns 200.
///
/// Fuseki returns `"OK"` as the body.  We only assert the status code.
///
/// Phase F2 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_ping_returns_200() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .get(server.admin_ping_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// C-2: POST `/$/ping` also returns 200.
///
/// Phase F2 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_ping_post_returns_200() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .post(server.admin_ping_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// C-3: GET `/$/server` returns JSON with at least a `version` key.
///
/// Phase F2 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_server_returns_json() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .get(server.admin_server_url())
        .header("Accept", "application/json")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("application/json"),
        "unexpected content-type: {ct}"
    );
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body.get("version").is_some(),
        "server info must include a 'version' field, got: {body}"
    );
}

/// C-4: `/$/server` JSON includes a `datasets` key listing active datasets.
///
/// Phase F2 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_server_includes_datasets() {
    let server = common::TestServer::start("").await;
    let body: serde_json::Value = server
        .client
        .get(server.admin_server_url())
        .send()
        .await
        .expect("request failed")
        .json()
        .await
        .expect("body must be JSON");
    assert!(
        body.get("datasets").is_some(),
        "server info must include a 'datasets' key, got: {body}"
    );
}

// ── D: Admin — dataset listing — Phase F3 ────────────────────────────────────
//
// `GET /$/datasets` returns a JSON document listing all registered datasets.
// The response shape mirrors Fuseki's `{"datasets":[{…}]}` structure.
//
// Spec: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html#datasets-and-services

/// D-1: GET `/$/datasets` returns 200 with JSON.
///
/// Phase F3 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_list_datasets_returns_200_json() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .get(server.admin_datasets_url())
        .header("Accept", "application/json")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("application/json"),
        "unexpected content-type: {ct}"
    );
}

/// D-2: The dataset list has a top-level `datasets` array.
///
/// Fuseki response shape: `{"datasets":[{"ds.name":…,"ds.state":…,"ds.services":[…]}]}`
///
/// Phase F3 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_list_datasets_has_datasets_array() {
    let server = common::TestServer::start("").await;
    let body: serde_json::Value = server
        .client
        .get(server.admin_datasets_url())
        .send()
        .await
        .expect("request failed")
        .json()
        .await
        .expect("body must be JSON");
    assert!(
        body["datasets"].is_array(),
        "response must have a top-level 'datasets' array, got: {body}"
    );
}

/// D-3: The default dataset `/ds` appears in the list with `ds.name` = `"/ds"`.
///
/// Phase F3 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_list_datasets_contains_default() {
    let server = common::TestServer::start("").await;
    let body: serde_json::Value = server
        .client
        .get(server.admin_datasets_url())
        .send()
        .await
        .expect("request failed")
        .json()
        .await
        .expect("body must be JSON");
    let datasets = body["datasets"].as_array().expect("datasets array");
    let found = datasets
        .iter()
        .any(|d| d["ds.name"] == "/ds" || d["ds.name"] == "ds");
    assert!(
        found,
        "default dataset '/ds' not found in list: {datasets:?}"
    );
}

/// D-4: GET `/$/datasets/{name}` returns info for a single dataset.
///
/// Phase F3 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_get_dataset_info_returns_200() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .get(server.admin_dataset_url(DS))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body.get("ds.name").is_some(),
        "dataset info must include 'ds.name', got: {body}"
    );
}

/// D-5: GET `/$/datasets/{name}` for non-existent dataset returns 404.
///
/// Phase F3 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_get_nonexistent_dataset_returns_404() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .get(server.admin_dataset_url("no-such-dataset"))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// D-6: Dataset info includes a `ds.services` array describing the endpoints.
///
/// Fuseki lists: query, update, gsp-rw, gsp-r service types.
/// We assert that at least one service entry exists.
///
/// Phase F3 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_dataset_info_has_services() {
    let server = common::TestServer::start("").await;
    let body: serde_json::Value = server
        .client
        .get(server.admin_dataset_url(DS))
        .send()
        .await
        .expect("request failed")
        .json()
        .await
        .expect("body must be JSON");
    let services = body["ds.services"].as_array();
    assert!(
        services.map(|s| !s.is_empty()).unwrap_or(false),
        "dataset info must include a non-empty 'ds.services' array, got: {body}"
    );
}

// ── E: Admin — create and delete datasets — Phase F4 ─────────────────────────
//
// `POST /$/datasets` creates a new in-memory dataset.
// `DELETE /$/datasets/{name}` removes it.
//
// Create request: form body `dbName=/{name}&dbType=mem`.
// Create response: 200 OK (Fuseki does not return 201).
// Delete response: 200 OK.
//
// Spec: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html#datasets-and-services

/// E-1: POST `/$/datasets` with `dbType=mem` creates a new in-memory dataset.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_create_dataset_returns_200() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/newds&dbType=mem")
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        200,
        "create dataset must return 200, got {}",
        resp.status()
    );
}

/// E-2: After creation, the new dataset appears in `GET /$/datasets`.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_create_dataset_appears_in_list() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/newds2&dbType=mem")
        .send()
        .await
        .expect("create request failed");

    let body: serde_json::Value = server
        .client
        .get(server.admin_datasets_url())
        .send()
        .await
        .expect("list request failed")
        .json()
        .await
        .expect("list must be JSON");
    let datasets = body["datasets"].as_array().expect("datasets array");
    let found = datasets
        .iter()
        .any(|d| d["ds.name"] == "/newds2" || d["ds.name"] == "newds2");
    assert!(
        found,
        "newly created dataset not found in list: {datasets:?}"
    );
}

/// E-3: After creation, the new dataset's query endpoint accepts queries.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_created_dataset_query_endpoint_works() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/freshds&dbType=mem")
        .send()
        .await
        .expect("create request failed");

    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url("freshds"),
        urlencoding::encode("SELECT * WHERE { ?s ?p ?o } LIMIT 1")
    );
    let resp = server
        .client
        .get(url)
        .send()
        .await
        .expect("query request failed");
    assert_eq!(
        resp.status(),
        200,
        "query on newly created dataset must return 200, got {}",
        resp.status()
    );
}

/// E-4: POST `/$/datasets` with an already-used name returns 409 Conflict.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_create_duplicate_dataset_returns_409() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/dupds&dbType=mem")
        .send()
        .await
        .expect("first create failed");

    let resp = server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/dupds&dbType=mem")
        .send()
        .await
        .expect("second create request failed");
    assert_eq!(
        resp.status(),
        409,
        "duplicate create must return 409, got {}",
        resp.status()
    );
}

/// E-5: DELETE `/$/datasets/{name}` removes a dataset.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_delete_dataset_returns_200() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/todelete&dbType=mem")
        .send()
        .await
        .expect("create failed");

    let resp = server
        .client
        .delete(server.admin_dataset_url("todelete"))
        .send()
        .await
        .expect("delete request failed");
    assert_eq!(
        resp.status(),
        200,
        "delete must return 200, got {}",
        resp.status()
    );
}

/// E-6: After deletion, the dataset no longer appears in the list.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_deleted_dataset_not_in_list() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/gone&dbType=mem")
        .send()
        .await
        .expect("create failed");
    server
        .client
        .delete(server.admin_dataset_url("gone"))
        .send()
        .await
        .expect("delete failed");

    let body: serde_json::Value = server
        .client
        .get(server.admin_datasets_url())
        .send()
        .await
        .expect("list request failed")
        .json()
        .await
        .expect("list must be JSON");
    let datasets = body["datasets"].as_array().expect("datasets array");
    let found = datasets
        .iter()
        .any(|d| d["ds.name"] == "/gone" || d["ds.name"] == "gone");
    assert!(
        !found,
        "deleted dataset still appears in list: {datasets:?}"
    );
}

/// E-7: After deletion, queries to the deleted dataset return 404.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_deleted_dataset_query_returns_404() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/vanished&dbType=mem")
        .send()
        .await
        .expect("create failed");
    server
        .client
        .delete(server.admin_dataset_url("vanished"))
        .send()
        .await
        .expect("delete failed");

    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url("vanished"),
        urlencoding::encode("SELECT * WHERE { ?s ?p ?o }")
    );
    let resp = server
        .client
        .get(url)
        .send()
        .await
        .expect("query request failed");
    assert_eq!(
        resp.status(),
        404,
        "query on deleted dataset must return 404, got {}",
        resp.status()
    );
}

/// E-8: DELETE on a non-existent dataset returns 404.
///
/// Phase F4 — SERVER.md §6.2
#[ignore]
#[tokio::test]
async fn fuseki_admin_delete_nonexistent_dataset_returns_404() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .delete(server.admin_dataset_url("does-not-exist"))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        404,
        "delete non-existent must return 404, got {}",
        resp.status()
    );
}

// ── F: SPARQL Update — Phase F5 ──────────────────────────────────────────────
//
// The `/{name}/update` endpoint accepts SPARQL 1.1 Update operations.
// Supported content-types: `application/sparql-update` (raw body) and
// `application/x-www-form-urlencoded` with `update=<encoded>`.
//
// Required operations for drop-in compatibility:
//   INSERT DATA, DELETE DATA, INSERT/DELETE WHERE, CLEAR, DROP, CREATE.
//
// Spec: https://www.w3.org/TR/sparql11-update/
// Fuseki: https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html

/// F-1: POST `/{name}/update` with `application/sparql-update` returns 200.
///
/// Phase F5 — SERVER.md §6.3
#[ignore]
#[tokio::test]
async fn fuseki_update_insert_data_returns_200() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/sparql-update")
        .body(INSERT_TRIPLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        200,
        "INSERT DATA must return 200, got {}",
        resp.status()
    );
}

/// F-2: After INSERT DATA, a SELECT query returns the inserted triple.
///
/// Phase F5 — SERVER.md §6.3
#[ignore]
#[tokio::test]
async fn fuseki_update_insert_data_is_queryable() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/sparql-update")
        .body(INSERT_TRIPLE)
        .send()
        .await
        .expect("INSERT failed");

    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(DS),
        urlencoding::encode(
            "SELECT ?name WHERE { <http://example.org/grace> <http://xmlns.com/foaf/0.1/name> ?name }"
        )
    );
    let body: serde_json::Value = server
        .client
        .get(url)
        .send()
        .await
        .expect("query failed")
        .json()
        .await
        .expect("body must be JSON");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    assert!(!bindings.is_empty(), "inserted triple must be queryable");
    common::assert_binding_contains(bindings, "name", "literal", "Grace");
}

/// F-3: DELETE DATA removes a triple that was previously inserted.
///
/// Phase F5 — SERVER.md §6.3
#[ignore]
#[tokio::test]
async fn fuseki_update_delete_data_removes_triple() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/sparql-update")
        .body(INSERT_TRIPLE)
        .send()
        .await
        .expect("INSERT failed");
    let del = server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/sparql-update")
        .body(DELETE_TRIPLE)
        .send()
        .await
        .expect("DELETE failed");
    assert_eq!(del.status(), 200);

    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(DS),
        urlencoding::encode(
            "ASK { <http://example.org/grace> <http://xmlns.com/foaf/0.1/name> \"Grace\" }"
        )
    );
    let body: serde_json::Value = server
        .client
        .get(url)
        .send()
        .await
        .expect("ASK failed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(
        body["boolean"], false,
        "deleted triple must no longer be present"
    );
}

/// F-4: CLEAR DEFAULT removes all triples from the default graph.
///
/// Phase F5 — SERVER.md §6.3
#[ignore]
#[tokio::test]
async fn fuseki_update_clear_default_empties_graph() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let clear = server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/sparql-update")
        .body(CLEAR_DEFAULT)
        .send()
        .await
        .expect("CLEAR failed");
    assert_eq!(clear.status(), 200);

    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(DS),
        urlencoding::encode("SELECT * WHERE { ?s ?p ?o } LIMIT 1")
    );
    let body: serde_json::Value = server
        .client
        .get(url)
        .send()
        .await
        .expect("SELECT failed")
        .json()
        .await
        .expect("body must be JSON");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    assert!(
        bindings.is_empty(),
        "CLEAR DEFAULT must leave no triples, got: {bindings:?}"
    );
}

/// F-5: Update via form-encoded `update=` parameter also works.
///
/// Phase F5 — SERVER.md §6.3
#[ignore]
#[tokio::test]
async fn fuseki_update_form_encoded_body_accepted() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("update={}", urlencoding::encode(INSERT_TRIPLE)))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        200,
        "form-encoded update must return 200, got {}",
        resp.status()
    );
}

/// F-6: Malformed SPARQL Update returns 400.
///
/// Phase F5 — SERVER.md §6.3
#[ignore]
#[tokio::test]
async fn fuseki_update_malformed_returns_400() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/sparql-update")
        .body("MANGLE DATA { <s> <p> <o> }")
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        400,
        "malformed update must return 400, got {}",
        resp.status()
    );
}

/// F-7: DROP GRAPH removes a named graph.
///
/// Phase F5 — SERVER.md §6.3
#[ignore]
#[tokio::test]
async fn fuseki_update_drop_graph_removes_named_graph() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let drop = server
        .client
        .post(server.dataset_update_url(DS))
        .header("Content-Type", "application/sparql-update")
        .body(DROP_GRAPH)
        .send()
        .await
        .expect("DROP failed");
    assert_eq!(drop.status(), 200);

    let get = server
        .client
        .get(server.dataset_data_graph_url(DS, GRAPH_IRI))
        .send()
        .await
        .expect("GET failed");
    assert_eq!(
        get.status(),
        404,
        "dropped named graph must return 404 on GET, got {}",
        get.status()
    );
}

// ── G: GSP content negotiation — Phase F6 ────────────────────────────────────
//
// Fuseki accepts and produces N-Triples, N-Quads, and TriG in addition to
// Turtle.  The `turtle` crate now exports `parse_ntriples`, `parse_nquads`,
// and `parse_trig`; they need to be wired into the GSP upload path.
//
// Spec: https://www.w3.org/TR/sparql11-http-rdf-update/#graph-management
// Content-types: text/turtle, application/n-triples, application/n-quads, application/trig

/// G-1: PUT `/{name}/data?default` with `application/n-triples` is accepted.
///
/// Phase F6 — SERVER.md §6.4
#[ignore]
#[tokio::test]
async fn fuseki_gsp_put_ntriples_accepted() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .put(server.dataset_data_default_url(DS))
        .header("Content-Type", "application/n-triples")
        .body(UPLOAD_NT)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "N-Triples PUT must return 200/204, got {}",
        resp.status()
    );
}

/// G-2: Triples uploaded as N-Triples are queryable.
///
/// Phase F6 — SERVER.md §6.4
#[ignore]
#[tokio::test]
async fn fuseki_gsp_put_ntriples_content_queryable() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .put(server.dataset_data_default_url(DS))
        .header("Content-Type", "application/n-triples")
        .body(UPLOAD_NT)
        .send()
        .await
        .expect("PUT failed");

    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(DS),
        urlencoding::encode(
            "ASK { <http://example.org/eve> <http://xmlns.com/foaf/0.1/name> \"Eve\" }"
        )
    );
    let body: serde_json::Value = server
        .client
        .get(url)
        .send()
        .await
        .expect("ASK failed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(body["boolean"], true, "N-Triples content must be queryable");
}

/// G-3: POST `/{name}/data?default` with `application/trig` is accepted.
///
/// TriG allows uploading into named graphs with a single request.
///
/// Phase F6 — SERVER.md §6.4
#[ignore]
#[tokio::test]
async fn fuseki_gsp_post_trig_accepted() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.dataset_data_default_url(DS))
        .header("Content-Type", "application/trig")
        .body(NAMED_GRAPH_TRIG)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "TriG POST must return 200/204, got {}",
        resp.status()
    );
}

/// G-4: GET `/{name}/data?default` with `Accept: application/n-triples` returns N-Triples.
///
/// Phase F6 — SERVER.md §6.4
#[ignore]
#[tokio::test]
async fn fuseki_gsp_get_accept_ntriples_returns_ntriples() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let resp = server
        .client
        .get(server.dataset_data_default_url(DS))
        .header("Accept", "application/n-triples")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("application/n-triples"),
        "expected application/n-triples content-type, got: {ct}"
    );
}

/// G-5: GET `/{name}/data?default` with `Accept: application/n-quads` returns N-Quads.
///
/// Phase F6 — SERVER.md §6.4
#[ignore]
#[tokio::test]
async fn fuseki_gsp_get_accept_nquads_returns_nquads() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let resp = server
        .client
        .get(server.dataset_data_default_url(DS))
        .header("Accept", "application/n-quads")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("application/n-quads"),
        "expected application/n-quads content-type, got: {ct}"
    );
}

/// G-6: GET `/{name}/data?default` with `Accept: application/trig` returns TriG.
///
/// Phase F6 — SERVER.md §6.4
#[ignore]
#[tokio::test]
async fn fuseki_gsp_get_accept_trig_returns_trig() {
    let server = common::TestServer::start_writable(PEOPLE_TTL).await;
    let resp = server
        .client
        .get(server.dataset_data_default_url(DS))
        .header("Accept", "application/trig")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("application/trig"),
        "expected application/trig content-type, got: {ct}"
    );
}

/// G-7: PUT with unsupported Content-Type returns 415 Unsupported Media Type.
///
/// Phase F6 — SERVER.md §6.4
#[ignore]
#[tokio::test]
async fn fuseki_gsp_put_unsupported_content_type_returns_415() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .put(server.dataset_data_default_url(DS))
        .header("Content-Type", "text/plain")
        .body("not rdf")
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        415,
        "unsupported Content-Type must return 415, got {}",
        resp.status()
    );
}

// ── H: Dynamic multi-dataset routing — Phase F7 ──────────────────────────────
//
// Once a DatasetRegistry is in place, dynamically created datasets must be
// fully routable.  Routes are resolved at request time from the registry, not
// hard-coded at startup.
//
// SERVER.md §6.5

/// H-1: Creating two datasets and querying each returns independent results.
///
/// Phase F7 — SERVER.md §6.5
#[ignore]
#[tokio::test]
async fn fuseki_two_datasets_are_independent() {
    let server = common::TestServer::start_writable("").await;
    // Create ds_a and ds_b.
    for name in &["/ds_a", "/ds_b"] {
        server
            .client
            .post(server.admin_datasets_url())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("dbName={name}&dbType=mem"))
            .send()
            .await
            .expect("create failed");
    }
    // Insert into ds_a only.
    server
        .client
        .post(server.dataset_update_url("ds_a"))
        .header("Content-Type", "application/sparql-update")
        .body(
            "INSERT DATA { <http://example.org/x> <http://example.org/in> <http://example.org/A> }",
        )
        .send()
        .await
        .expect("insert failed");

    // ds_a returns the triple; ds_b does not.
    let ask = |ds: &str| {
        let url = format!(
            "{}?query={}",
            server.dataset_sparql_url(ds),
            urlencoding::encode(
                "ASK { <http://example.org/x> <http://example.org/in> <http://example.org/A> }"
            )
        );
        let client = server.client.clone();
        async move {
            client
                .get(url)
                .send()
                .await
                .expect("ask failed")
                .json::<serde_json::Value>()
                .await
                .expect("json")
        }
    };
    assert_eq!(
        ask("ds_a").await["boolean"],
        true,
        "ds_a must contain the triple"
    );
    assert_eq!(
        ask("ds_b").await["boolean"],
        false,
        "ds_b must not contain the triple"
    );
}

/// H-2: Deleting a dataset from the registry makes its routes return 404.
///
/// Phase F7 — SERVER.md §6.5
#[ignore]
#[tokio::test]
async fn fuseki_deleted_dataset_routes_return_404() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/temp&dbType=mem")
        .send()
        .await
        .expect("create failed");
    server
        .client
        .delete(server.admin_dataset_url("temp"))
        .send()
        .await
        .expect("delete failed");

    let resp = server
        .client
        .get(format!(
            "{}?query={}",
            server.dataset_sparql_url("temp"),
            urlencoding::encode("SELECT * WHERE { ?s ?p ?o }")
        ))
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        404,
        "routes for deleted dataset must return 404, got {}",
        resp.status()
    );
}

// ── I: Full lifecycle — Phase F8 ──────────────────────────────────────────────
//
// End-to-end round-trip tests simulating typical client behaviour:
//   1. Create a new in-memory dataset via the Admin API.
//   2. Upload RDF data via the GSP endpoint.
//   3. Query the data with SPARQL SELECT.
//   4. Modify the data with SPARQL Update.
//   5. Delete the dataset.
//
// SERVER.md §6.6 Phase F8

/// I-1: Full lifecycle — create, upload Turtle, SELECT, DELETE.
///
/// This mirrors the typical workflow of a Fuseki client library:
/// ```sh
/// curl -X POST http://localhost:3030/$/datasets \
///      -d 'dbName=/myds&dbType=mem'
/// curl -X PUT  http://localhost:3030/myds/data?default \
///      -H 'Content-Type: text/turtle' --data-binary @data.ttl
/// curl http://localhost:3030/myds/sparql?query=SELECT+...
/// curl -X DELETE http://localhost:3030/$/datasets/myds
/// ```
///
/// Phase F8 — SERVER.md §6.6
#[ignore]
#[tokio::test]
async fn fuseki_full_lifecycle_create_upload_query_delete() {
    let server = common::TestServer::start_writable("").await;

    // Step 1: create dataset.
    let create = server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/lifecycle&dbType=mem")
        .send()
        .await
        .expect("create request failed");
    assert_eq!(create.status(), 200, "create: {}", create.status());

    // Step 2: upload Turtle into the default graph.
    let upload = server
        .client
        .put(server.dataset_data_default_url("lifecycle"))
        .header("Content-Type", "text/turtle")
        .body(PEOPLE_TTL)
        .send()
        .await
        .expect("upload request failed");
    assert!(
        upload.status() == 200 || upload.status() == 204,
        "upload: {}",
        upload.status()
    );

    // Step 3: query for Person instances.
    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url("lifecycle"),
        urlencoding::encode(SELECT_ALL_PEOPLE)
    );
    let body: serde_json::Value = server
        .client
        .get(url)
        .send()
        .await
        .expect("query failed")
        .json()
        .await
        .expect("query response must be JSON");
    let bindings = body["results"]["bindings"].as_array().expect("bindings");
    assert!(
        !bindings.is_empty(),
        "query must return results after upload"
    );

    // Step 4: delete the dataset.
    let del = server
        .client
        .delete(server.admin_dataset_url("lifecycle"))
        .send()
        .await
        .expect("delete request failed");
    assert_eq!(del.status(), 200, "delete: {}", del.status());

    // Step 5: query on deleted dataset returns 404.
    let gone = server
        .client
        .get(format!(
            "{}?query={}",
            server.dataset_sparql_url("lifecycle"),
            urlencoding::encode("SELECT * WHERE { ?s ?p ?o }")
        ))
        .send()
        .await
        .expect("post-delete query failed");
    assert_eq!(
        gone.status(),
        404,
        "deleted dataset must return 404, got {}",
        gone.status()
    );
}

/// I-2: Full lifecycle — create, upload via SPARQL Update, SELECT, delete.
///
/// Exercises the SPARQL Update path instead of GSP for data ingestion.
///
/// Phase F8 — SERVER.md §6.6
#[ignore]
#[tokio::test]
async fn fuseki_full_lifecycle_update_path() {
    let server = common::TestServer::start_writable("").await;

    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/updateds&dbType=mem")
        .send()
        .await
        .expect("create failed");

    server
        .client
        .post(server.dataset_update_url("updateds"))
        .header("Content-Type", "application/sparql-update")
        .body(INSERT_TRIPLE)
        .send()
        .await
        .expect("insert failed");

    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url("updateds"),
        urlencoding::encode(
            "ASK { <http://example.org/grace> <http://xmlns.com/foaf/0.1/name> \"Grace\" }"
        )
    );
    let body: serde_json::Value = server
        .client
        .get(url)
        .send()
        .await
        .expect("ASK failed")
        .json()
        .await
        .expect("ASK response must be JSON");
    assert_eq!(body["boolean"], true, "inserted triple must be queryable");

    server
        .client
        .delete(server.admin_dataset_url("updateds"))
        .send()
        .await
        .expect("delete failed");
}

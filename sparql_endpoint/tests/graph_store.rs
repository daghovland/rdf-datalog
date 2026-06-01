/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the SPARQL 1.1 Graph Store HTTP Protocol (GSP).
//!
//! All tests are marked `#[ignore]` — the `/rdf-graph-store` endpoint is not
//! yet implemented. Remove `#[ignore]` from a group as the corresponding
//! operation is implemented. See `SERVER.md` §1 for the implementation plan.
//!
//! **Specification:** W3C Recommendation 21 March 2013
//! <https://www.w3.org/TR/sparql11-http-rdf-update/>
//!
//! ## Test organisation
//!
//! | Group | Spec section | HTTP method |
//! |---|---|---|
//! | A | §5.2 `#http-get` | GET |
//! | B | §5.3 `#http-put` | PUT |
//! | C | §5.4 `#http-delete` | DELETE |
//! | D | §5.5 `#http-post` | POST (merge + create) |
//! | E | §5.6 `#http-head` | HEAD |
//! | F | §4.1 `#direct-graph-identification` | GET (optional feature) |
//! | G | §5.1 `#status-codes` | Read-only server rejects writes |
//! | H | §4.2 + §5 | Lifecycle round-trip |

mod common;

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// Turtle triples loaded into the default graph of the test store at startup.
const DEFAULT_GRAPH_TURTLE: &str = r#"
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .
    @prefix ex:   <http://example.org/> .

    ex:alice foaf:name "Alice" ;
             a foaf:Person .
"#;

/// IRI used as the pre-existing named graph in tests that need one.
const NAMED_GRAPH_IRI: &str = "http://example.org/graph1";

/// TriG document that pre-loads `NAMED_GRAPH_IRI` with two triples.
const NAMED_GRAPH_TRIG: &str = r#"
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .
    @prefix ex:   <http://example.org/> .

    <http://example.org/graph1> {
        ex:bob foaf:name "Bob" ;
               a foaf:Person .
    }
"#;

/// Turtle payload used when testing writes (PUT / POST body).
const WRITE_TURTLE: &str = r#"
    @prefix ex: <http://example.org/> .
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .

    ex:charlie foaf:name "Charlie" ;
               a foaf:Person .
"#;

/// A second Turtle payload for merge tests (POST).
const MERGE_TURTLE: &str = r#"
    @prefix ex: <http://example.org/> .
    @prefix foaf: <http://xmlns.com/foaf/0.1/> .

    ex:diana foaf:name "Diana" ;
             a foaf:Person .
"#;

/// Syntactically invalid Turtle used to test 400 responses.
const BAD_TURTLE: &str = "THIS IS NOT VALID TURTLE !!!";

// ── A: HTTP GET (§5.2) ───────────────────────────────────────────────────────

/// A-1: GET the default graph — 200 OK with an RDF payload.
///
/// Spec §5.2: "A request that uses the HTTP GET method MUST retrieve an RDF
/// payload that is a serialization of the named graph."
/// For the default graph via `?default` parameter, see §4.2.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-get>
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#indirect-graph-identification>
#[tokio::test]
async fn gsp_get_default_graph_returns_200() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .get(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// A-2: GET the default graph — Content-Type is text/turtle (or n-triples or rdf+xml).
///
/// Spec §5 preamble: "If the Accept header is not provided with a GET request,
/// the server MUST return one of RDF XML, Turtle, or N-Triples."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#graph-management>
#[tokio::test]
async fn gsp_get_default_graph_content_type_is_rdf() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .get(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    let is_rdf = ct.contains("text/turtle")
        || ct.contains("application/n-triples")
        || ct.contains("application/rdf+xml");
    assert!(is_rdf, "expected an RDF content-type, got: {ct}");
}

/// A-3: GET default graph with explicit Accept: text/turtle — 200 with Turtle body.
///
/// Spec §5.2: "The response … SHOULD be made cacheable … in any of the
/// preferred representation formats specified in the Accept request-header field."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-get>
#[tokio::test]
async fn gsp_get_default_graph_accept_turtle() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("text/turtle"),
        "expected text/turtle, got: {ct}"
    );
}

/// A-4: GET default graph — body contains the stored triples.
///
/// The response must serialise the graph content. We check that the IRI of a
/// known subject appears somewhere in the Turtle output.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-get>
#[tokio::test]
async fn gsp_get_default_graph_body_contains_triples() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/alice"),
        "expected alice IRI in response body, got:\n{body}"
    );
}

/// A-5: GET a named graph — 200 OK.
///
/// The graph identified by `?graph=<iri>` must exist and its triples be returned.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-get>
#[tokio::test]
async fn gsp_get_named_graph_returns_200() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);
    let resp = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// A-6: GET a named graph — body contains the graph's triples.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-get>
#[tokio::test]
async fn gsp_get_named_graph_body_contains_triples() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);
    let resp = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/bob"),
        "expected bob IRI in named-graph response, got:\n{body}"
    );
}

/// A-7: GET a non-existent named graph — 404 Not Found.
///
/// Spec §5.1: "If the RDF graph content identified in the request does not
/// exist in the server, and the operation requires that it does, a 404 Not
/// Found response code MUST be provided."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#status-codes>
#[tokio::test]
async fn gsp_get_named_graph_nonexistent_returns_404() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/no-such-graph");
    let resp = server
        .client
        .get(&url)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// A-8: GET with an Accept type the server cannot produce — 406 Not Acceptable.
///
/// Spec §5.2: "In the event that the specified representation format is not
/// supported, a 406 Not Acceptable response code SHOULD be returned."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-get>
#[tokio::test]
async fn gsp_get_unsupported_accept_returns_406() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "application/json") // not a valid RDF format
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 406);
}

// ── B: HTTP PUT (§5.3) ───────────────────────────────────────────────────────

/// B-1: PUT to a new named graph IRI — 201 Created.
///
/// Spec §5.3: "If new RDF graph content is created, the origin server MUST
/// inform the user agent via the 201 Created response."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-put>
#[tokio::test]
async fn gsp_put_creates_new_named_graph_returns_201() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/new-graph");
    let resp = server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 201);
}

/// B-2: PUT to a new graph — subsequent GET returns the stored triples.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-put>
#[tokio::test]
async fn gsp_put_new_graph_content_retrievable() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/new-graph");
    server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("PUT failed");

    let resp = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET failed");
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/charlie"),
        "expected charlie IRI after PUT, got:\n{body}"
    );
}

/// B-3: PUT to an existing named graph — 200 OK or 204 No Content.
///
/// Spec §5.3: "If existing RDF graph content is modified, either the 200 OK
/// or 204 No Content response codes MUST be sent."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-put>
#[tokio::test]
async fn gsp_put_replaces_existing_named_graph() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);
    let resp = server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// B-4: PUT replaces the full content — old triples are gone, new ones are present.
///
/// Spec §5.3 SPARQL equivalent: `DROP SILENT GRAPH <g>; INSERT DATA { GRAPH <g> { … } }`
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-put>
#[tokio::test]
async fn gsp_put_replace_removes_old_triples() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);

    // Replace the graph (which has bob) with charlie.
    server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("PUT failed");

    let resp = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET failed");
    let body = resp.text().await.expect("body text");

    assert!(
        body.contains("http://example.org/charlie"),
        "charlie should be present after PUT, got:\n{body}"
    );
    assert!(
        !body.contains("http://example.org/bob"),
        "bob should be gone after PUT replaced the graph, got:\n{body}"
    );
}

/// B-5: PUT to the default graph — 200 OK or 204 No Content.
///
/// Spec §5.3 default-graph equivalent: `DROP SILENT DEFAULT; INSERT DATA { … }`
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-put>
#[tokio::test]
async fn gsp_put_default_graph_success() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .put(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// B-6: PUT default graph replaces content — old triples gone, new ones present.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-put>
#[tokio::test]
async fn gsp_put_default_graph_replace_removes_old_triples() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;

    server
        .client
        .put(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("PUT failed");

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET failed");
    let body = resp.text().await.expect("body text");

    assert!(
        body.contains("http://example.org/charlie"),
        "charlie should be present after PUT, got:\n{body}"
    );
    assert!(
        !body.contains("http://example.org/alice"),
        "alice should be gone after default-graph PUT, got:\n{body}"
    );
}

/// B-7: PUT with invalid Turtle — 400 Bad Request.
///
/// Spec §5.1: "In response to operations involving an RDF payload, if the
/// attempt to parse the RDF payload according to the provided Content-Type
/// fails then the server MUST respond with a 400 Bad Request."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#status-codes>
#[tokio::test]
async fn gsp_put_bad_turtle_returns_400() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/g");
    let resp = server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(BAD_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// B-8: PUT with an unsupported Content-Type — 415 Unsupported Media Type.
///
/// Spec §5.1: "If a client issues a POST or PUT with a content type that is
/// not understood by the graph store, the implementation MUST respond with
/// 415 Unsupported Media Type."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#status-codes>
#[tokio::test]
async fn gsp_put_unsupported_content_type_returns_415() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/g");
    let resp = server
        .client
        .put(&url)
        .header("content-type", "text/plain")
        .body("some text")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 415);
}

/// B-9: PUT with a non-absolute graph IRI — 400 Bad Request.
///
/// Spec §4.2: "The query string IRI MUST be an absolute IRI and the server
/// MUST respond with a 400 Bad Request if it is not."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#indirect-graph-identification>
#[tokio::test]
async fn gsp_put_non_absolute_graph_iri_returns_400() {
    let server = common::TestServer::start_writable("").await;
    // Pass a relative IRI — percent-encoding preserves its relative form.
    let url = format!("{}/rdf-graph-store?graph=relative/path", server.base_url);
    let resp = server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

// ── C: HTTP DELETE (§5.4) ────────────────────────────────────────────────────

/// C-1: DELETE a named graph — 200 OK or 204 No Content.
///
/// Spec §5.4: "A response code of 200 OK or 204 No Content MUST be given in
/// the response if the operation succeeded."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-delete>
#[tokio::test]
async fn gsp_delete_named_graph_returns_success() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);
    let resp = server
        .client
        .delete(&url)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// C-2: DELETE a named graph — subsequent GET returns 404.
///
/// Spec §5.4 SPARQL equivalent: `DROP GRAPH <g>`.
/// After deletion the graph no longer exists, so GET MUST return 404.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-delete>
#[tokio::test]
async fn gsp_delete_named_graph_subsequent_get_returns_404() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);

    server
        .client
        .delete(&url)
        .send()
        .await
        .expect("DELETE failed");

    let resp = server.client.get(&url).send().await.expect("GET failed");
    assert_eq!(resp.status(), 404);
}

/// C-3: DELETE a graph that does not exist — 404 Not Found.
///
/// Spec §5.4: "If there is no such RDF graph content in the Graph Store, the
/// server MUST respond with a 404 Not Found response code."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-delete>
#[tokio::test]
async fn gsp_delete_nonexistent_named_graph_returns_404() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/no-such-graph");
    let resp = server
        .client
        .delete(&url)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// C-4: DELETE the default graph — 200 OK or 204 No Content.
///
/// Spec §5.4 SPARQL equivalent: `DROP DEFAULT`.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-delete>
#[tokio::test]
async fn gsp_delete_default_graph_returns_success() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .delete(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// C-5: DELETE the default graph — subsequent GET returns an empty graph (200 with empty body).
///
/// The default graph always exists (it may be empty); DELETE clears its content
/// but the graph itself remains. GET must return 200, not 404.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-delete>
#[tokio::test]
async fn gsp_delete_default_graph_clears_content() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;

    server
        .client
        .delete(server.gsp_default_url())
        .send()
        .await
        .expect("DELETE failed");

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET after DELETE failed");
    // Default graph always exists; 200 with empty or near-empty body.
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body text");
    assert!(
        !body.contains("http://example.org/alice"),
        "alice should be gone after DELETE, got:\n{body}"
    );
}

// ── D: HTTP POST (§5.5) ──────────────────────────────────────────────────────

/// D-1: POST to the default graph (merge) — 200 OK or 204 No Content.
///
/// Spec §5.5: "A request that uses the HTTP POST method and a request IRI
/// that identifies RDF graph content MUST be understood as a request that the
/// origin server perform an RDF merge of the enclosed RDF payload into the
/// RDF graph content identified by the … encoded IRI."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_merge_default_graph_returns_success() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .post(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(MERGE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// D-2: POST merge into default graph — new triples appear.
///
/// Spec §5.5 SPARQL equivalent: `INSERT DATA { … }`
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_merge_default_graph_adds_triples() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    server
        .client
        .post(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(MERGE_TURTLE)
        .send()
        .await
        .expect("POST failed");

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET failed");
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/diana"),
        "diana should appear after POST merge, got:\n{body}"
    );
}

/// D-3: POST merge into default graph — existing triples are preserved.
///
/// Unlike PUT, POST merges — it must not remove previously loaded data.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_merge_default_graph_preserves_existing_triples() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    server
        .client
        .post(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(MERGE_TURTLE)
        .send()
        .await
        .expect("POST failed");

    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET failed");
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/alice"),
        "alice must still be present after POST merge, got:\n{body}"
    );
}

/// D-4: POST merge into a named graph — 200 OK or 204 No Content.
///
/// Spec §5.5 SPARQL equivalent: `INSERT DATA { GRAPH <g> { … } }`
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_merge_named_graph_returns_success() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);
    let resp = server
        .client
        .post(&url)
        .header("content-type", "text/turtle")
        .body(MERGE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 200 || resp.status() == 204,
        "expected 200 or 204, got {}",
        resp.status()
    );
}

/// D-5: POST merge into a named graph — new triples appear, old ones preserved.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_merge_named_graph_adds_and_preserves_triples() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);
    server
        .client
        .post(&url)
        .header("content-type", "text/turtle")
        .body(MERGE_TURTLE)
        .send()
        .await
        .expect("POST failed");

    let resp = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET failed");
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/diana"),
        "diana should appear after POST merge, got:\n{body}"
    );
    assert!(
        body.contains("http://example.org/bob"),
        "bob must still be present after POST merge, got:\n{body}"
    );
}

/// D-6: POST to a non-existent named graph — 404 Not Found.
///
/// Spec §5.5: "If the graph IRI does not identify either a Graph Store or RDF
/// graph content, the origin server should respond with a 404 Not Found."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_nonexistent_named_graph_returns_404() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/no-such-graph");
    let resp = server
        .client
        .post(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// D-7: POST to the Graph Store (no `?graph` param) — creates a new graph, 201 Created.
///
/// Spec §5.5: "If the request IRI identifies the underlying Graph Store, the
/// origin server MUST create a new RDF graph comprised of the statements in
/// the RDF payload and return a designated graph IRI … along with a 201 Created code."
///
/// Example from the spec:
/// ```
/// POST /rdf-graphs HTTP/1.1
/// Host: example.com
/// Content-Type: application/rdf+xml
///
/// HTTP/1.1 201 Created
/// Location: http://example.com/rdf-graphs/newGraph1
/// ```
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_to_graph_store_creates_new_graph_returns_201() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.gsp_url()) // no ?graph or ?default
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 201);
}

/// D-8: POST to Graph Store — response includes a Location header.
///
/// Spec §5.5: "The new graph IRI should be specified in the Location HTTP
/// header along with a 201 Created code."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_to_graph_store_includes_location_header() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.gsp_url())
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 201);
    assert!(
        resp.headers().contains_key("location"),
        "201 response must include a Location header"
    );
}

/// D-9: POST to Graph Store — content is accessible at the Location URL.
///
/// Spec §5.5: the Location IRI "should be different from the request IRI" and
/// the content must be retrievable at that address.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_to_graph_store_content_accessible_at_location() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.gsp_url())
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 201);
    let location = resp
        .headers()
        .get("location")
        .expect("Location header missing")
        .to_str()
        .expect("Location must be a string")
        .to_owned();

    let get_resp = server
        .client
        .get(&location)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET at Location failed");
    assert_eq!(get_resp.status(), 200);
    let body = get_resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/charlie"),
        "charlie should be accessible at Location, got:\n{body}"
    );
}

/// D-10: POST with empty body — 204 No Content.
///
/// Spec §5.5: "If the request body is empty, the implementation SHOULD respond
/// with 204 No Content."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
#[tokio::test]
async fn gsp_post_empty_body_returns_204() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body("")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 204);
}

/// D-11: POST with invalid Turtle — 400 Bad Request.
///
/// Spec §5.1: parse failure MUST return 400.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#status-codes>
#[tokio::test]
async fn gsp_post_bad_turtle_returns_400() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(BAD_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 400);
}

/// D-12: POST with unsupported Content-Type — 415 Unsupported Media Type.
///
/// Spec §5.1: "If a client issues a POST or PUT with a content type that is
/// not understood by the graph store, the implementation MUST respond with
/// 415 Unsupported Media Type."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#status-codes>
#[tokio::test]
async fn gsp_post_unsupported_content_type_returns_415() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.gsp_default_url())
        .header("content-type", "text/plain")
        .body("some plain text")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 415);
}

// ── E: HTTP HEAD (§5.6) ──────────────────────────────────────────────────────

/// E-1: HEAD an existing named graph — 200 OK.
///
/// Spec §5.6: "The HTTP HEAD method is identical to GET except that the server
/// MUST NOT return a message-body in the response."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-head>
#[tokio::test]
async fn gsp_head_existing_named_graph_returns_200() {
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    let url = server.gsp_named_graph_url(NAMED_GRAPH_IRI);
    let resp = server
        .client
        .head(&url)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// E-2: HEAD the default graph — 200 OK.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-head>
#[tokio::test]
async fn gsp_head_default_graph_returns_200() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .head(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// E-3: HEAD a non-existent named graph — 404 Not Found.
///
/// HEAD is "identical to GET" for status codes; a missing graph must return 404.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-head>
#[tokio::test]
async fn gsp_head_nonexistent_named_graph_returns_404() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/no-such-graph");
    let resp = server
        .client
        .head(&url)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

/// E-4: HEAD response MUST NOT contain a message body.
///
/// Spec §5.6: "the server MUST NOT return a message-body in the response."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-head>
#[tokio::test]
async fn gsp_head_response_has_no_body() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let resp = server
        .client
        .head(server.gsp_default_url())
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.expect("body bytes");
    assert!(
        body.is_empty(),
        "HEAD response must have an empty body, got {} bytes",
        body.len()
    );
}

/// E-5: HEAD returns the same Content-Type header as GET.
///
/// Spec §5.6: "identical to GET" — headers must match.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-head>
#[tokio::test]
async fn gsp_head_content_type_matches_get() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;

    let get_ct = server
        .client
        .get(server.gsp_default_url())
        .send()
        .await
        .expect("GET failed")
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let head_ct = server
        .client
        .head(server.gsp_default_url())
        .send()
        .await
        .expect("HEAD failed")
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    assert_eq!(
        get_ct, head_ct,
        "HEAD Content-Type must match GET Content-Type"
    );
}

// ── F: Direct Graph Identification (§4.1, optional feature) ─────────────────
//
// §4.1 describes an *optional* mode where the request URI itself IS the graph
// IRI (e.g. `GET http://example.com/rdf-graphs/employees` rather than
// `GET /rdf-graph-store?graph=http://example.com/rdf-graphs/employees`).
//
// These tests are included for spec completeness. They are also `#[ignore]`
// on top of the `#[ignore]` that is already expected of all GSP tests — the
// feature may not be implemented at all if §4.2 indirect identification is
// sufficient.

/// F-1: Direct GET — the request URI identifies the named graph directly.
///
/// Spec §4.1 example:
/// ```
/// GET /rdf-graphs/employees HTTP/1.1
/// Host: example.com
/// Accept: text/turtle; charset=utf-8
/// ```
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#direct-graph-identification>
#[ignore]
#[tokio::test]
async fn gsp_direct_get_graph_returns_200() {
    // This test assumes the server routes /rdf-graphs/<name> to named graphs
    // where the full request IRI is used as the graph IRI — an optional feature.
    let server = common::TestServer::start_writable_trig(NAMED_GRAPH_TRIG).await;
    // Under direct identification, NAMED_GRAPH_IRI == the request URL, so we
    // construct it relative to the server's base.
    let url = format!("{}/rdf-graphs/graph1", server.base_url);
    let resp = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 200);
}

/// F-2: Direct PUT — store a new graph at a URL that becomes its IRI.
///
/// Spec §4.1: "the server would route operations onto a named graph in a
/// Graph Store via its Graph IRI."
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#direct-graph-identification>
#[ignore]
#[tokio::test]
async fn gsp_direct_put_graph_creates_201() {
    let server = common::TestServer::start_writable("").await;
    let url = format!("{}/rdf-graphs/new-direct-graph", server.base_url);
    let resp = server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 201);
}

// ── G: Read-only server rejects write operations (§5.1, §6) ─────────────────

/// G-1: PUT on a read-only server — 403 Forbidden or 405 Method Not Allowed.
///
/// Spec §5.1: security policies may restrict methods; §6 Security Considerations
/// notes that `401 Unauthorized` and `403 Forbidden` apply when access control
/// is in effect. 405 is appropriate when the method is disabled server-wide.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#status-codes>
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#security>
#[tokio::test]
async fn gsp_read_only_put_returns_403_or_405() {
    let server = common::TestServer::start("").await; // read_only: true
    let url = server.gsp_named_graph_url("http://example.org/g");
    let resp = server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 403 || resp.status() == 405,
        "expected 403 or 405 on read-only server, got {}",
        resp.status()
    );
}

/// G-2: DELETE on a read-only server — 403 or 405.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#security>
#[tokio::test]
async fn gsp_read_only_delete_returns_403_or_405() {
    let server = common::TestServer::start("").await;
    let url = server.gsp_named_graph_url("http://example.org/g");
    let resp = server
        .client
        .delete(&url)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 403 || resp.status() == 405,
        "expected 403 or 405 on read-only server, got {}",
        resp.status()
    );
}

/// G-3: POST on a read-only server — 403 or 405.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#security>
#[tokio::test]
async fn gsp_read_only_post_returns_403_or_405() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .post(server.gsp_default_url())
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status() == 403 || resp.status() == 405,
        "expected 403 or 405 on read-only server, got {}",
        resp.status()
    );
}

// ── H: Lifecycle round-trips (§4.2 + §5) ─────────────────────────────────────

/// H-1: Full named-graph lifecycle — PUT → GET → POST (merge) → GET → DELETE → GET (404).
///
/// Exercises every method in sequence against a single named graph to verify
/// that the state transitions are consistent.
///
/// Spec references: §5.3 (PUT), §5.2 (GET), §5.5 (POST merge), §5.4 (DELETE)
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#graph-management>
#[tokio::test]
async fn gsp_full_lifecycle_named_graph() {
    let server = common::TestServer::start_writable("").await;
    let url = server.gsp_named_graph_url("http://example.org/lifecycle-graph");

    // 1. PUT — create graph with charlie.
    let put = server
        .client
        .put(&url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("PUT failed");
    assert_eq!(put.status(), 201, "PUT should create with 201");

    // 2. GET — charlie is present.
    let get1 = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET 1 failed");
    let body1 = get1.text().await.expect("body 1");
    assert!(
        body1.contains("http://example.org/charlie"),
        "charlie must be present after PUT"
    );

    // 3. POST — merge diana in.
    let post = server
        .client
        .post(&url)
        .header("content-type", "text/turtle")
        .body(MERGE_TURTLE)
        .send()
        .await
        .expect("POST failed");
    assert!(
        post.status() == 200 || post.status() == 204,
        "POST merge should return 200/204, got {}",
        post.status()
    );

    // 4. GET — both charlie and diana are present.
    let get2 = server
        .client
        .get(&url)
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET 2 failed");
    let body2 = get2.text().await.expect("body 2");
    assert!(
        body2.contains("http://example.org/charlie"),
        "charlie must survive POST merge"
    );
    assert!(
        body2.contains("http://example.org/diana"),
        "diana must appear after POST merge"
    );

    // 5. DELETE — graph is removed.
    let delete = server
        .client
        .delete(&url)
        .send()
        .await
        .expect("DELETE failed");
    assert!(
        delete.status() == 200 || delete.status() == 204,
        "DELETE should return 200/204, got {}",
        delete.status()
    );

    // 6. GET — 404 after deletion.
    let get3 = server.client.get(&url).send().await.expect("GET 3 failed");
    assert_eq!(get3.status(), 404, "GET after DELETE should return 404");
}

/// H-2: Default graph is independent of named graphs.
///
/// PUT to a named graph must not affect the default graph, and vice versa.
/// This verifies the graph isolation requirement from §3 Protocol Model.
///
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#protocol-model>
#[tokio::test]
async fn gsp_named_and_default_graphs_are_independent() {
    let server = common::TestServer::start_writable(DEFAULT_GRAPH_TURTLE).await;
    let named_url = server.gsp_named_graph_url("http://example.org/isolated-graph");

    // PUT charlie into the named graph.
    server
        .client
        .put(&named_url)
        .header("content-type", "text/turtle")
        .body(WRITE_TURTLE)
        .send()
        .await
        .expect("PUT failed");

    // Default graph must still only have alice, not charlie.
    let resp = server
        .client
        .get(server.gsp_default_url())
        .header("accept", "text/turtle")
        .send()
        .await
        .expect("GET default failed");
    let body = resp.text().await.expect("body text");
    assert!(
        body.contains("http://example.org/alice"),
        "alice must remain in default graph, got:\n{body}"
    );
    assert!(
        !body.contains("http://example.org/charlie"),
        "charlie must NOT appear in default graph after named-graph PUT, got:\n{body}"
    );
}

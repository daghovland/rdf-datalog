/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for `GET /ns/agentprov/session` (#567): the hash-URI namespace
//! document `agp:AgentSession` individuals dereference to once their
//! `#fragment` is stripped client-side.
//!
//! See `docs/plans/AGENTPROV_SESSION_DEREF_567_PLAN.md` for the full design
//! (why this is outbound-only, unauthenticated, and 200s even when empty --
//! all different from `/describe`, #493's per-resource endpoint).
//!
//! The fixture mirrors the real shape emitted by
//! `scripts/new-provenance-summary.sh` / `provenance/summaries/*.ttl`:
//! an `agp:AgentSession` and its `agp:TranscriptSummary`, both hash-URIs
//! under `session:`, plus an unrelated `bl:PullRequest` in a different
//! namespace that must NOT leak into this document.

mod common;

const TURTLE: &str = r#"
    @prefix agp: <https://dagalog.no/ns/agentprov#> .
    @prefix bl: <https://dagalog.no/ns/backlog#> .
    @prefix ghpull: <https://github.com/daghovland/rdf-datalog/pull/> .
    @prefix ghissues: <https://github.com/daghovland/rdf-datalog/issues/> .
    @prefix prov: <http://www.w3.org/ns/prov#> .
    @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
    @prefix session: <https://dagalog.no/ns/agentprov/session#> .

    ghpull:567 a bl:PullRequest, bl:WorkItem ;
        rdfs:label "GET /ns/agentprov/session 404s" ;
        bl:closesIssue ghissues:567 .

    session:pr567 a agp:AgentSession ;
        prov:used ghissues:567 .

    session:pr567Summary a agp:TranscriptSummary ;
        agp:summaryText "A distilled summary of the reasoning behind PR #567." ;
        agp:reasoningFor ghpull:567 .
"#;

// ── route availability ──────────────────────────────────────────────────────

/// `GET /ns/agentprov/session` -> 200, even with data loaded.
#[tokio::test]
async fn agentprov_session_document_returns_200() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
}

/// An empty store -> still 200 (a namespace document describing zero known
/// sessions is a valid, minimal document, not an error) -- unlike
/// `/describe`'s 404 for a single unknown resource.
#[tokio::test]
async fn agentprov_session_document_empty_store_returns_200() {
    let server = common::TestServer::start("").await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
}

// ── outbound triples for session:* resources ────────────────────────────────

/// The document must include the `agp:AgentSession` individual's own
/// outbound triples.
#[tokio::test]
async fn agentprov_session_document_includes_session_triples() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("pr567"),
        "expected session:pr567 in body, got: {body}"
    );
    assert!(
        body.contains("AgentSession"),
        "expected agp:AgentSession rdf:type in body, got: {body}"
    );
}

/// The document must also include the `agp:TranscriptSummary` individual
/// (also a `session:*` hash-URI) and its `agp:summaryText`.
#[tokio::test]
async fn agentprov_session_document_includes_summary_triples() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("distilled summary"),
        "expected agp:summaryText in body, got: {body}"
    );
}

/// A `bl:PullRequest` in a different namespace must NOT appear -- this is a
/// namespace document for `session:*` resources only, outbound-only (unlike
/// `/describe`'s outbound+inbound merge).
#[tokio::test]
async fn agentprov_session_document_excludes_other_namespaces() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .send()
        .await
        .expect("request failed");

    let body = resp.text().await.expect("body must be text");
    assert!(
        !body.contains("404s"),
        "expected the bl:PullRequest's rdfs:label NOT to appear, got: {body}"
    );
}

// ── content negotiation ─────────────────────────────────────────────────────

/// Default (no Accept header) -> Turtle.
#[tokio::test]
async fn agentprov_session_document_default_content_type_is_turtle() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .send()
        .await
        .expect("request failed");

    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(ct.contains("turtle"), "expected Turtle, got: {ct}");
}

/// `Accept: application/ld+json` -> JSON-LD.
#[tokio::test]
async fn agentprov_session_document_jsonld_content_negotiation() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(ct.contains("application/ld+json"), "got: {ct}");
}

/// An `Accept` header naming no supported format -> 406.
#[tokio::test]
async fn agentprov_session_document_unsupported_accept_returns_406() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/ns/agentprov/session", server.base_url))
        .header("accept", "image/png")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 406);
}

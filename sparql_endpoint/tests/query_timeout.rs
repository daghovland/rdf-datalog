/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the cooperative SPARQL query-timeout `Deadline`
//! (`Config::max_query_timeout_secs`, issue #372, PR #423) firing end-to-end
//! through the real HTTP `/sparql` route and being mapped to a `503 Service
//! Unavailable` response, covering
//! <https://github.com/daghovland/rdf-datalog/issues/527>.
//!
//! `sparql_parser/tests/query_timeout_tests.rs` already covers the
//! `Deadline`/`execute_with_base` mechanics directly (with a 1ms timeout,
//! which is "already elapsed" by the time evaluation starts and so isn't a
//! timing race). This file instead covers the surface that #527 identified
//! as untested: `sparql_endpoint/src/query.rs`'s string-content match of
//! `Deadline::check`'s error message to a `503` HTTP status
//! (`query_execution_error_response`, around line 685-695) — if that string
//! match ever drifts out of sync with the error message `Deadline::check`
//! actually produces, timeouts would silently fall through to a generic
//! `500` instead, uncaught by any other test.
//!
//! Unlike the 1ms case above, a real HTTP round trip needs an actual elapsed
//! wall-clock budget (`Config::max_query_timeout_secs` is whole seconds, so
//! the shortest is 1s). To avoid a timing race against a 1s deadline, the
//! "slow" query here is sized so that, uncancelled, it deterministically
//! takes several seconds regardless of machine speed — a transitive-closure
//! property path (`ex:p+`) over a 3,000,000-node chain.
//!
//! **Chain size history, and why it's this large:** an earlier version of
//! this test used a 300,000-node chain, sized from a `cargo test` (debug
//! profile) benchmark that took ~5-6s. That was large enough in a *debug*
//! build but not in the CI `--release` build (see `test-release` in
//! `.github/workflows/ci.yml`), where the same 300,000-node query completed
//! in well under 1s and the test flaked to a false failure — release-mode
//! optimisation of the transitive-closure BFS loop is dramatically faster
//! than debug, so a size picked against a debug build gives no real
//! guarantee about the release build the CI job actually runs (or a faster
//! CI machine's release build specifically). The size here was instead
//! chosen against `cargo test --release`: a 1,000,000-node chain measured
//! ~3.6-6.6s end-to-end (including HTTP/parsing overhead) on a
//! resource-constrained local machine, well over the 1s budget; 3,000,000
//! is used for the actual test to leave a further safety margin for faster
//! CI hardware, while comfortably fitting a standard GitHub Actions runner's
//! memory (~16GB — this chain's `Datastore` measured well under 2GB
//! resident at 1,000,000 nodes, scaling roughly linearly).

mod common;

use std::fmt::Write as _;

/// Turtle for a long chain `ex:n0 ex:p ex:n1 ex:p ex:n2 ... ex:p ex:n{len}`,
/// sized so a transitive-closure property path over it (`ex:n0 ex:p* ?x`)
/// does real, measurable BFS work — large enough that a 1s configured
/// timeout reliably trips mid-evaluation rather than racing the query to
/// completion.
fn chain_turtle(len: usize) -> String {
    let mut out = String::with_capacity(len * 48);
    out.push_str("@prefix ex: <http://example.org/> .\n");
    for k in 0..len {
        let _ = writeln!(out, "ex:n{k} ex:p ex:n{} .", k + 1);
    }
    out
}

/// A transitive-closure property path query over a large synthetic graph,
/// run through the real HTTP `/sparql` endpoint with a 1s configured
/// `max_query_timeout_secs`, must be cut off and reported as `503 Service
/// Unavailable` with a body indicating a timeout — not hang until the query
/// finishes, and not fall through to a generic `500`.
#[tokio::test]
async fn slow_transitive_closure_query_times_out_with_503() {
    let turtle = chain_turtle(3_000_000);
    let server = common::TestServer::start_writable_with_query_timeout(&turtle, 1).await;

    let sparql = "\
        PREFIX ex: <http://example.org/> \
        SELECT ?x WHERE { ex:n0 ex:p+ ?x }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql)
        .send()
        .await
        .expect("request failed");

    let status = resp.status();
    let body = resp.text().await.expect("body must be readable");
    assert_eq!(
        status, 503,
        "expected 503 Service Unavailable for a query exceeding \
         max_query_timeout_secs, got {status}: {body}"
    );
    assert!(
        body.to_lowercase().contains("timeout"),
        "expected the 503 body to mention the timeout, got: {body:?}"
    );
}

/// An ordinary, fast query must still succeed normally under a short
/// configured `max_query_timeout_secs` — the timeout must not fire on
/// queries that finish well within budget. Guards against the timeout test
/// above passing only because every query is (accidentally) 503ing.
#[tokio::test]
async fn fast_query_succeeds_under_short_timeout() {
    let turtle = "\
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .\n\
        <http://example.org/alice> foaf:name \"Alice\" .\n";
    let server = common::TestServer::start_writable_with_query_timeout(turtle, 1).await;

    let sparql = "\
        PREFIX foaf: <http://xmlns.com/foaf/0.1/> \
        SELECT ?name WHERE { ?person foaf:name ?name }";

    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-query")
        .body(sparql)
        .send()
        .await
        .expect("request failed");

    let status = resp.status();
    let body = resp.text().await.expect("body must be readable");
    assert!(
        status.is_success(),
        "a fast query must succeed even under a 1s max_query_timeout_secs, \
         got {status}: {body}"
    );
    assert!(
        body.contains("Alice"),
        "expected the query result to contain the expected binding, got: {body:?}"
    );
}

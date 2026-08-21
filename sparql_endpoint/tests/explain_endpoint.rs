/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for the `?explain=true` EXPLAIN/profiling endpoint (issue #537).
//!
//! See `docs/plans/EXPLAIN_ENDPOINT_537_PLAN.md` for the design this
//! implements: a static query plan (join/component order, per-pattern
//! estimated cardinality and index used, reusing
//! `sparql_parser::join_ordering`/`component_ordering` rather than
//! recomputing them) plus total wall-clock timing, returned as JSON via
//! `?explain=true` on the existing `/sparql` endpoints — bypassing SPARQL
//! Results content negotiation entirely, since a plan isn't a result set.

mod common;

/// Test case 1 — single-pattern query's explain output.
///
/// A one-triple-pattern BGP: the plan must contain exactly one BGP node
/// with exactly one pattern entry, rendering the pattern's subject/
/// predicate/object as SPARQL-ish text.
#[tokio::test]
#[ignore = "not yet implemented, see docs/plans/EXPLAIN_ENDPOINT_537_PLAN.md"]
async fn test_explain_single_pattern() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let sparql =
        "SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server
        .client
        .get(url)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap();
    assert!(
        ct.contains("application/json"),
        "explain response must be JSON, got content-type: {ct}"
    );

    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["queryType"], "Select");

    let plan = body["plan"].as_array().expect("plan must be an array");
    assert_eq!(plan.len(), 1, "single top-level BGP: {plan:?}");
    assert_eq!(plan[0]["kind"], "BGP");

    let patterns = plan[0]["patterns"]
        .as_array()
        .expect("BGP node must have a patterns array");
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0]["position"], 0);
    let pattern_text = patterns[0]["pattern"].as_str().expect("pattern text");
    assert!(
        pattern_text.contains("http://example.org/alice"),
        "pattern text should mention the constant subject: {pattern_text}"
    );
    assert!(
        pattern_text.contains("?name"),
        "pattern text should mention the object variable: {pattern_text}"
    );
    assert!(
        patterns[0]["estimatedCardinality"].is_number(),
        "estimatedCardinality must be present: {patterns:?}"
    );
    assert!(
        patterns[0]["indexUsed"].is_string(),
        "indexUsed must be present: {patterns:?}"
    );
}

/// Test case 2 — a multi-pattern BGP's explain output shows the
/// selectivity-based join order `join_ordering::order_patterns` actually
/// chose, not the textual order the query was written in.
///
/// Mirrors the fixture in
/// `sparql_parser/src/join_ordering.rs`'s
/// `picks_pattern_with_smallest_predicate_cardinality_first` unit test:
/// `p1` has cardinality 1 (most selective), `p2` has cardinality 5. The
/// query is written with `p2` first (the deliberately worst order); the
/// explain plan must report `p1`'s pattern at position 0.
#[tokio::test]
#[ignore = "not yet implemented, see docs/plans/EXPLAIN_ENDPOINT_537_PLAN.md"]
async fn test_explain_multi_pattern_join_order() {
    let mut turtle = String::new();
    turtle.push_str("<http://example.org/s1> <http://example.org/p1> <http://example.org/o1> .\n");
    for i in 0..5 {
        turtle.push_str(&format!(
            "<http://example.org/s2_{i}> <http://example.org/p2> <http://example.org/o2_{i}> .\n"
        ));
    }
    let server = common::TestServer::start(&turtle).await;

    // Deliberately worst order: less-selective pattern (p2) written first.
    let sparql = "SELECT ?x ?y WHERE { \
        ?x <http://example.org/p2> ?y . \
        ?x <http://example.org/p1> ?y . \
    }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server
        .client
        .get(url)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");

    let plan = body["plan"].as_array().expect("plan must be an array");
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0]["kind"], "BGP");
    let patterns = plan[0]["patterns"]
        .as_array()
        .expect("BGP node must have a patterns array");
    assert_eq!(patterns.len(), 2);

    // p1 (cardinality 1) must be scheduled before p2 (cardinality 5),
    // regardless of the order they appear in the query text.
    let first_pattern = patterns[0]["pattern"].as_str().unwrap();
    let second_pattern = patterns[1]["pattern"].as_str().unwrap();
    assert!(
        first_pattern.contains("/p1"),
        "most selective pattern (p1) must be scheduled first: {patterns:?}"
    );
    assert!(
        second_pattern.contains("/p2"),
        "least selective pattern (p2) must be scheduled second: {patterns:?}"
    );
    assert_eq!(patterns[0]["estimatedCardinality"], 1);
    assert_eq!(patterns[1]["estimatedCardinality"], 5);
}

/// Test case 3 — normal (non-`explain`) query behavior is completely
/// unaffected: identical rows, status, and content-type whether or not the
/// `explain` code path exists, both with the parameter absent and with
/// `explain=false`. The zero-cost claim (explain support doesn't add any
/// branch/parameter to `eval_bgp`/`eval_component`'s hot path — see the
/// plan doc's Decision 2) isn't itself testable as a timing assertion
/// (flaky by construction); this test instead pins the observable
/// behavioral contract: normal queries produce exactly the same SPARQL
/// Results JSON they always did.
#[tokio::test]
async fn test_normal_query_unaffected_by_explain_param_presence() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;
    let sparql =
        "SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }";

    for url in [
        server.sparql_query_url(sparql),
        format!("{}&explain=false", server.sparql_query_url(sparql)),
    ] {
        let resp = server
            .client
            .get(url)
            .send()
            .await
            .expect("request failed");

        assert_eq!(resp.status(), 200);
        let ct = resp.headers()["content-type"].to_str().unwrap();
        assert!(
            ct.contains("application/sparql-results+json"),
            "unexpected content-type: {ct}"
        );

        let body: serde_json::Value = resp.json().await.expect("body must be JSON");
        let bindings = body["results"]["bindings"]
            .as_array()
            .expect("bindings array");
        assert_eq!(bindings.len(), 1);
        common::assert_binding_contains(bindings, "name", "literal", "Alice");
    }
}

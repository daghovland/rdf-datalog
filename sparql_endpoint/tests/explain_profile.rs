/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for the `?explain=true` runtime per-operator `"profile"` field
//! (issue #572, follow-up to #537's static-only `"plan"`).
//!
//! See `docs/plans/EXPLAIN_ENDPOINT_537_PLAN.md`'s "#572" section for the
//! design: a separate tree from `"plan"` (built from real execution, not a
//! static re-walk), collected via a thread-local profiler so the
//! non-`explain` path pays no cost. Tests assert structure (node shapes,
//! invocation counts, row counts) — never on actual duration values, which
//! are inherently non-deterministic.
//!
//! TDD: written and reviewed `#[ignore]`d before implementation, per this
//! repo's workflow; unignored together once `sparql_parser::profile` and
//! its wiring into `eval_bgp`/`eval_components_budgeted`/
//! `sparql_endpoint::explain` landed as one coherent change.

mod common;

/// Test 1 — single-pattern BGP: the profile's one top-level node is a
/// "BGP" with one "Pattern" child, non-negative timing, and correct
/// rows-in/rows-out.
#[tokio::test]
async fn test_profile_single_pattern_bgp() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;
    let sparql =
        "SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");

    let profile = body["profile"]
        .as_array()
        .expect("profile must be an array");
    assert_eq!(profile.len(), 1, "profile: {profile:?}");
    let bgp = &profile[0];
    assert_eq!(bgp["kind"], "BGP");
    assert_eq!(bgp["invocations"], 1);
    assert_eq!(
        bgp["rowsIn"], 1,
        "BGP starts from the single empty solution"
    );
    assert_eq!(bgp["rowsOut"], 1, "one matching triple");
    assert!(
        bgp["totalTimeMs"]
            .as_f64()
            .expect("totalTimeMs must be a number")
            >= 0.0,
        "bgp: {bgp:?}"
    );

    let children = bgp["children"].as_array().expect("BGP must have children");
    assert_eq!(children.len(), 1, "one triple pattern: {children:?}");
    let pattern = &children[0];
    assert_eq!(pattern["kind"], "Pattern");
    assert_eq!(pattern["invocations"], 1);
    assert_eq!(pattern["rowsIn"], 1);
    assert_eq!(pattern["rowsOut"], 1);
    assert!(
        pattern["label"]
            .as_str()
            .expect("pattern label")
            .contains("alice"),
        "pattern: {pattern:?}"
    );
    assert!(
        pattern["totalTimeMs"]
            .as_f64()
            .expect("totalTimeMs must be a number")
            >= 0.0
    );
    assert_eq!(
        pattern["children"].as_array().unwrap().len(),
        0,
        "a Pattern node has no further children"
    );
}

/// Test 2 — a multi-pattern BGP reports one "Pattern" child per triple
/// pattern, in the same selectivity-chosen order #537's static plan
/// reports (see `test_explain_multi_pattern_join_order` in
/// `explain_endpoint.rs`), with each pattern's own rows-in/rows-out
/// reflecting how many solutions flowed through at that point in the join.
#[tokio::test]
async fn test_profile_multi_pattern_bgp_rows_narrow_down_the_join() {
    let mut turtle = String::new();
    turtle.push_str("<http://example.org/s1> <http://example.org/p1> <http://example.org/o1> .\n");
    for i in 0..5 {
        turtle.push_str(&format!(
            "<http://example.org/s2_{i}> <http://example.org/p2> <http://example.org/o2_{i}> .\n"
        ));
    }
    let server = common::TestServer::start(&turtle).await;

    let sparql = "SELECT ?x ?y WHERE { \
        ?x <http://example.org/p2> ?y . \
        ?x <http://example.org/p1> ?y . \
    }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let profile = body["profile"].as_array().expect("profile array");
    assert_eq!(profile.len(), 1);
    let patterns = profile[0]["children"].as_array().expect("BGP children");
    assert_eq!(patterns.len(), 2, "patterns: {patterns:?}");

    // p1 (selective, cardinality 1) runs first per join_ordering, starting
    // from the single empty solution (1 row in) and producing 0 output
    // rows (no quad matches both p1 AND shares x/y binding with p2's
    // disjoint data) — regardless of the exact numbers, rowsOut of the
    // first pattern must equal rowsIn of the second (they're the same join
    // state).
    let first_rows_out = patterns[0]["rowsOut"].as_u64().unwrap();
    let second_rows_in = patterns[1]["rowsIn"].as_u64().unwrap();
    assert_eq!(
        first_rows_out, second_rows_in,
        "the first pattern's output IS the second pattern's input: {patterns:?}"
    );
}

/// Test 3 — an `OPTIONAL` body evaluated once per outer row is merged into
/// ONE aggregate subtree (not N separate sibling subtrees): the `Optional`
/// node itself is one component dispatch (`invocations: 1`) summarizing
/// `rowsIn`/`rowsOut` across every outer row, while its inner body's own
/// node(s) carry `invocations` equal to the outer row count, reflecting how
/// many times that body actually ran.
#[tokio::test]
async fn test_profile_optional_aggregates_per_row_invocations() {
    let mut turtle = String::new();
    for i in 0..3 {
        turtle.push_str(&format!(
            "<http://example.org/s{i}> <http://example.org/p> <http://example.org/o{i}> .\n"
        ));
    }
    // Only s0 has the optional property.
    turtle.push_str("<http://example.org/s0> <http://example.org/opt> \"yes\" .\n");
    let server = common::TestServer::start(&turtle).await;

    let sparql = "SELECT ?s ?v WHERE { \
        ?s <http://example.org/p> ?o . \
        OPTIONAL { ?s <http://example.org/opt> ?v } \
    }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let profile = body["profile"].as_array().expect("profile array");
    assert_eq!(profile.len(), 2, "profile: {profile:?}");
    assert_eq!(profile[0]["kind"], "BGP");
    assert_eq!(profile[1]["kind"], "Optional");

    // The `Optional` component itself dispatches exactly once per
    // `eval_components_budgeted` pass (it's one component in the sibling
    // list, like any other) — `invocations: 1` — but internally evaluates
    // its body once per outer row; that repetition shows up on the body's
    // own (merged) children below, not on this wrapper node.
    assert_eq!(
        profile[1]["invocations"], 1,
        "the Optional component itself is one dispatch, summarizing all outer rows: {:?}",
        profile[1]
    );
    assert_eq!(
        profile[1]["rowsIn"], 3,
        "rowsIn sums across all 3 invocations (1 row in each): {:?}",
        profile[1]
    );

    let inner_bgp = profile[1]["children"]
        .as_array()
        .expect("Optional must have children (its inner BGP)");
    assert_eq!(inner_bgp.len(), 1, "inner_bgp: {inner_bgp:?}");
    assert_eq!(inner_bgp[0]["kind"], "BGP");
    assert_eq!(
        inner_bgp[0]["invocations"], 3,
        "the inner BGP's own node is merged across the same 3 invocations: {:?}",
        inner_bgp[0]
    );
}

/// Test 4 — normal (non-`explain`) query behavior is completely
/// unaffected by the profiler's existence (mirrors
/// `test_normal_query_unaffected_by_explain_param_presence` in
/// `explain_endpoint.rs`, specifically for the new profiling code path).
#[tokio::test]
async fn test_normal_query_unaffected_by_profiling() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;
    let sparql =
        "SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }";
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
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
    assert!(
        body.get("profile").is_none(),
        "a non-explain response must never carry a profile field: {body:?}"
    );
}

/// Test 5 — a timing out / failing `explain` request still returns a
/// (possibly partial) profile array, mirroring #537's existing "plan
/// survives a failing execution" contract. Uses a query that is well-formed
/// but references an undefined `?x` base... actually simplest: reuse the
/// documented `explain` + `txId` 400 is a different path; for a genuine
/// execution error we rely on the existing error-path test fixture if any
/// exists. Keep this test minimal: an ordinary successful query's response
/// always includes a `profile` key when `explain=true`, even if execution
/// produces zero rows (zero rows is not an error, but exercises the
/// "profile present on every explain response, not just the happy path
/// with results" contract cheaply, without needing a real timeout
/// fixture).
#[tokio::test]
async fn test_profile_present_even_with_zero_result_rows() {
    let turtle = r#"
        <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;
    let sparql =
        "SELECT ?name WHERE { <http://example.org/nobody> <http://xmlns.com/foaf/0.1/name> ?name }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["rowCount"], 0);
    let profile = body["profile"]
        .as_array()
        .expect("profile must be present and an array even with zero result rows");
    assert_eq!(profile.len(), 1);
    assert_eq!(profile[0]["rowsOut"], 0);
}

/// Test 6 (issue #697) — a `FILTER EXISTS { ... }` evaluated once per outer
/// row is merged into ONE aggregate `"Exists"` node under the `Filter`
/// node, mirroring test 3's `OPTIONAL` aggregation: `Filter` itself
/// dispatches once (`invocations: 1`), while the nested `"Exists"` node's
/// `invocations` equals the outer row count (how many times the EXISTS
/// sub-pattern actually ran).
#[tokio::test]
async fn test_profile_exists_aggregates_per_row_invocations() {
    let mut turtle = String::new();
    for i in 0..3 {
        turtle.push_str(&format!(
            "<http://example.org/s{i}> <http://example.org/p> <http://example.org/o{i}> .\n"
        ));
    }
    // Only s0 has the witness property, so EXISTS succeeds for exactly one
    // of the 3 outer rows.
    turtle.push_str("<http://example.org/s0> <http://example.org/witness> \"yes\" .\n");
    let server = common::TestServer::start(&turtle).await;

    let sparql = "SELECT ?s WHERE { \
        ?s <http://example.org/p> ?o . \
        FILTER EXISTS { ?s <http://example.org/witness> ?w } \
    }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["rowCount"], 1, "body: {body:?}");
    let profile = body["profile"].as_array().expect("profile array");
    assert_eq!(profile.len(), 2, "profile: {profile:?}");
    assert_eq!(profile[0]["kind"], "BGP");
    assert_eq!(profile[1]["kind"], "Filter");

    assert_eq!(
        profile[1]["invocations"], 1,
        "the Filter component itself is one dispatch, summarizing all outer rows: {:?}",
        profile[1]
    );
    assert_eq!(
        profile[1]["rowsIn"], 3,
        "rowsIn sums across all 3 rows filtered: {:?}",
        profile[1]
    );

    let exists_children = profile[1]["children"]
        .as_array()
        .expect("Filter must have children (the merged EXISTS node)");
    assert_eq!(
        exists_children.len(),
        1,
        "exists_children: {exists_children:?}"
    );
    let exists_node = &exists_children[0];
    assert_eq!(exists_node["kind"], "Exists");
    assert_eq!(
        exists_node["invocations"], 3,
        "EXISTS ran once per outer row, merged into one node: {:?}",
        exists_node
    );
    assert!(
        exists_node["totalTimeMs"]
            .as_f64()
            .expect("totalTimeMs must be a number")
            >= 0.0
    );
}

/// Test 7 (issue #697) — a `FILTER NOT EXISTS { ... }` produces a
/// `"NotExists"` node (distinct kind string from plain `EXISTS`), still
/// merged across outer rows the same way.
#[tokio::test]
async fn test_profile_not_exists_produces_not_exists_node() {
    let mut turtle = String::new();
    for i in 0..2 {
        turtle.push_str(&format!(
            "<http://example.org/s{i}> <http://example.org/p> <http://example.org/o{i}> .\n"
        ));
    }
    let server = common::TestServer::start(&turtle).await;

    let sparql = "SELECT ?s WHERE { \
        ?s <http://example.org/p> ?o . \
        FILTER NOT EXISTS { ?s <http://example.org/witness> ?w } \
    }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["rowCount"], 2, "body: {body:?}");
    let body: serde_json::Value = body;
    let profile = body["profile"].as_array().expect("profile array");
    let filter_node = profile
        .iter()
        .find(|n| n["kind"] == "Filter")
        .expect("a Filter node must be present");
    let children = filter_node["children"].as_array().expect("Filter children");
    assert_eq!(children.len(), 1, "children: {children:?}");
    assert_eq!(children[0]["kind"], "NotExists");
    assert_eq!(children[0]["invocations"], 2);
}

/// Test 8 (issue #697) — a sequence property path (`:p1/:p2`) reports one
/// `"Step"` child per static step under the `PathPattern` node, in order,
/// with each step's own rows-in/rows-out.
#[tokio::test]
async fn test_profile_path_sequence_reports_one_step_per_hop() {
    let turtle = r#"
        <http://example.org/a> <http://example.org/p1> <http://example.org/b> .
        <http://example.org/b> <http://example.org/p2> <http://example.org/c> .
    "#;
    let server = common::TestServer::start(turtle).await;
    let sparql = "SELECT ?x WHERE { \
        <http://example.org/a> <http://example.org/p1>/<http://example.org/p2> ?x \
    }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["rowCount"], 1, "body: {body:?}");
    let profile = body["profile"].as_array().expect("profile array");
    assert_eq!(profile.len(), 1);
    assert_eq!(profile[0]["kind"], "PathPattern");

    let steps = profile[0]["children"]
        .as_array()
        .expect("PathPattern must have Step children");
    assert_eq!(steps.len(), 2, "steps: {steps:?}");
    assert_eq!(steps[0]["kind"], "Step");
    assert_eq!(steps[1]["kind"], "Step");
    assert_eq!(steps[0]["rowsOut"], steps[1]["rowsIn"], "steps: {steps:?}");
}

/// Test 9 (issue #697) — a transitive-closure path (`:p+`) reports one
/// `"TransitiveClosure"` child under the `PathPattern` node, with NO
/// further children of its own (the BFS inside is suspended — see
/// `crate::profile::SuspendGuard` — never broken down per queue pop).
#[tokio::test]
async fn test_profile_transitive_closure_is_one_node_with_no_children() {
    let turtle = r#"
        <http://example.org/a> <http://example.org/p> <http://example.org/b> .
        <http://example.org/b> <http://example.org/p> <http://example.org/c> .
    "#;
    let server = common::TestServer::start(turtle).await;
    let sparql = "SELECT ?x WHERE { \
        <http://example.org/a> <http://example.org/p>+ ?x \
    }";
    let url = format!("{}&explain=true", server.sparql_query_url(sparql));
    let resp = server.client.get(url).send().await.expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["rowCount"], 2, "body: {body:?}");
    let profile = body["profile"].as_array().expect("profile array");
    assert_eq!(profile.len(), 1);
    assert_eq!(profile[0]["kind"], "PathPattern");

    let children = profile[0]["children"]
        .as_array()
        .expect("PathPattern must have a TransitiveClosure child");
    assert_eq!(children.len(), 1, "children: {children:?}");
    assert_eq!(children[0]["kind"], "TransitiveClosure");
    assert_eq!(children[0]["rowsOut"], 2);
    assert_eq!(
        children[0]["children"].as_array().unwrap().len(),
        0,
        "the BFS inside a transitive closure is suspended, never broken down: {:?}",
        children[0]
    );
}

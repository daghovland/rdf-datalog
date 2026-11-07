/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression coverage for issue [#591](https://github.com/daghovland/rdf-datalog/issues/591):
//! `CLEAR`/`DROP` (default graph, a named graph, `ALL`, `NAMED`) in
//! `sparql_update.rs::apply_prepared_update_with_options` used to call
//! `store.remove_graph(id)` directly, never notifying the reasoner that the
//! graph was cleared. When a rule's derived facts happen to land in the
//! *same* graph as their supporting base facts, a bare `remove_graph`
//! physically wipes both together, which looks consistent and masks the
//! bug (see the same-graph tests below, which pass even on the buggy
//! code). The deterministic case is a derived fact landing in a
//! *different* graph than its base fact — the common RML-mapped-data
//! shape (extensional data in a named graph, derived/intensional facts in
//! the default graph) — where a bare `remove_graph(<g>)` can never touch
//! the default-graph derived fact at all; only notifying the reasoner can
//! retract it.
//!
//! This is a much better match for the user's real-world report (a
//! pipeline that reloads a dataset by clearing it and re-running an RML
//! mapping) than a one-off `DELETE WHERE` would be, since `CLEAR`/`DROP`
//! is the natural "reload this graph's data" operation.
//!
//! Fixed by `sparql_update.rs::clear_graphs_through_reasoner`, which
//! mirrors `graph_store.rs::gsp_delete_inner` (the Graph Store Protocol
//! `DELETE` route, which already handled this correctly): collect the
//! target graph(s)' quads and route them through `apply_reasoner_delta`
//! before falling back to a bare `store.remove_graph` when no reasoner is
//! configured.

mod common;

const EX: &str = "http://ex/";

/// `?x ex:reachable ?y :- ?x ex:link ?y .`
/// `?x ex:reachable ?y :- ?x ex:link ?z . ?z ex:reachable ?y .`
const TRANSITIVE_LINK_RULES: &str = r#"
PREFIX ex: <http://ex/>
ex:reachable[?x, ?y] :- ex:link[?x, ?y] .
ex:reachable[?x, ?y] :- ex:link[?x, ?z], ex:reachable[?z, ?y] .
"#;

/// Same rule shape, but graph-scoped via a trailing `?g` graph variable on
/// every atom: derivations land in the *same* named graph as the base
/// facts that justify them, rather than the default graph. Needed to
/// exercise `DROP GRAPH <g>` meaningfully — `TRANSITIVE_LINK_RULES`
/// (default-graph-only) never fires on data that only exists inside a
/// named graph, which would otherwise make a `DROP GRAPH` test pass
/// vacuously (both sides answer "no derivations" for an unrelated reason).
const GRAPH_SCOPED_TRANSITIVE_LINK_RULES: &str = r#"
PREFIX ex: <http://ex/>
ex:reachable[?x, ?y] ?g :- ex:link[?x, ?y] ?g .
ex:reachable[?x, ?y] ?g :- ex:link[?x, ?z] ?g, ex:reachable[?z, ?y] ?g .
"#;

/// `ex:a ex:linked ex:b :- ex:a ex:link ex:b ?g .` (any graph `?g`), with
/// the derived fact landing in the **default graph** regardless of which
/// named graph the base fact lives in — the RML-mapped-data shape:
/// base/extensional facts loaded into a named graph, derived/intensional
/// facts in the default graph. This is the deterministic case: unlike
/// `GRAPH_SCOPED_TRANSITIVE_LINK_RULES` (where `remove_graph` physically
/// wipes base *and* derived facts together, since they share a graph),
/// here `remove_graph(<g>)` only ever touches the base fact — the
/// derived default-graph fact can only be retracted by the reasoner
/// actually being told about the deletion.
const CROSS_GRAPH_LINK_RULE: &str = r#"
PREFIX ex: <http://ex/>
ex:linked[?x, ?y] :- ex:link[?x, ?y] ?g .
"#;

async fn load_rules(server: &common::TestServer, dataset: &str, rules: &str) {
    let resp = server
        .client
        .post(server.dataset_rules_url(dataset))
        .header("Content-Type", "text/x-datalog")
        .body(rules.to_string())
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 200, "POST /rules failed: {}", resp.status());
}

async fn ask(server: &common::TestServer, dataset: &str, sparql: &str) -> bool {
    let url = format!(
        "{}?query={}",
        server.dataset_sparql_url(dataset),
        urlencoding::encode(sparql)
    );
    let resp = server
        .client
        .get(url)
        .header("Accept", "application/sparql-results+json")
        .send()
        .await
        .expect("query request failed");
    assert!(
        resp.status().is_success(),
        "query failed: {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.expect("invalid JSON");
    body["boolean"].as_bool().unwrap_or(false)
}

async fn update(server: &common::TestServer, dataset: &str, sparql: &str) {
    let resp = server
        .client
        .post(server.dataset_update_url(dataset))
        .header("content-type", "application/sparql-update")
        .body(sparql.to_string())
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        200,
        "update must return 200: {}",
        resp.status()
    );
}

/// `CLEAR DEFAULT` followed by *re-inserting the same base facts* (the
/// realistic "pipeline reloads this graph" shape), then deleting one edge,
/// must produce the same derived closure a from-scratch server would
/// produce — the reasoner must be told about the clear, not just see the
/// store go empty out from under it. Re-inserting the *same* data (rather
/// than disjoint new data) is the discriminating case: stale "already
/// derived"/support bookkeeping left over from before the clear is most
/// likely to suppress re-derivation or miscount support when the exact
/// same facts come back, whereas genuinely new, disjoint data would
/// re-derive normally via semi-naive regardless of any stale state.
#[tokio::test]
async fn clear_default_then_reinsert_same_data_matches_fresh_rebuild() {
    // a -> b -> c
    let base = format!(
        "<{EX}a> <{EX}link> <{EX}b> . \
         <{EX}b> <{EX}link> <{EX}c> ."
    );
    let server = common::TestServer::start_writable(&base).await;
    load_rules(&server, "ds", TRANSITIVE_LINK_RULES).await;

    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}reachable> <{EX}c> }}")
        )
        .await,
        "a must transitively reach c before CLEAR"
    );

    // Simulate a pipeline reload: clear the default graph, then re-insert
    // the exact same data.
    update(&server, "ds", "CLEAR DEFAULT").await;
    let reinsert = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ ex:a ex:link ex:b . ex:b ex:link ex:c . }}"
    );
    update(&server, "ds", &reinsert).await;

    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}reachable> <{EX}c> }}")
        )
        .await,
        "a must transitively reach c again after CLEAR + reinsert of the same data \
         (issue #591: stale reasoner bookkeeping must not suppress re-derivation)"
    );

    // Now delete one edge (the actual reported symptom: deletion not
    // propagating to inferred facts) and compare the full closure against
    // a from-scratch oracle seeded with the surviving data.
    let delete_bc = format!(
        "PREFIX ex: <{EX}>
         DELETE DATA {{ ex:b ex:link ex:c . }}"
    );
    update(&server, "ds", &delete_bc).await;

    let surviving_base = format!("<{EX}a> <{EX}link> <{EX}b> .");
    let oracle = common::TestServer::start_writable(&surviving_base).await;
    load_rules(&oracle, "ds", TRANSITIVE_LINK_RULES).await;

    for (s, o) in [("a", "b"), ("b", "c"), ("a", "c")] {
        let q = format!("ASK {{ <{EX}{s}> <{EX}reachable> <{EX}{o}> }}");
        let actual = ask(&server, "ds", &q).await;
        let expected = ask(&oracle, "ds", &q).await;
        assert_eq!(
            actual, expected,
            "ex:reachable({s},{o}) mismatch after CLEAR DEFAULT + reinsert + DELETE DATA: \
             server-under-test={actual}, oracle={expected} (issue #591)"
        );
    }
}

/// Same scenario via `DROP GRAPH <g>` on a named graph (not just the
/// default graph / `CLEAR DEFAULT`), since the bug is in the same
/// `PreparedOp::ClearGraph | PreparedOp::DropGraph` arm. Uses
/// `GRAPH_SCOPED_TRANSITIVE_LINK_RULES` so derivations actually land
/// inside `<g>` — with the default-graph-only rule shape this test would
/// pass vacuously (both server and oracle would simply never derive
/// anything in `<g>`, for a reason unrelated to the bug), so explicit
/// sanity assertions below confirm the rule really does fire in `<g>`
/// before trusting the comparison.
#[tokio::test]
async fn drop_named_graph_then_reinsert_matches_fresh_rebuild() {
    let server = common::TestServer::start_writable("").await;
    load_rules(&server, "ds", GRAPH_SCOPED_TRANSITIVE_LINK_RULES).await;

    let graph = format!("{EX}g");
    let insert_old = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{graph}> {{ ex:a ex:link ex:b . ex:b ex:link ex:c . }} }}"
    );
    update(&server, "ds", &insert_old).await;

    // Sanity: the graph-scoped rule must actually have derived something
    // inside <g> before the drop, otherwise the comparison below is
    // vacuous.
    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ GRAPH <{graph}> {{ <{EX}a> <{EX}reachable> <{EX}c> }} }}")
        )
        .await,
        "graph-scoped rule must derive a-reachable-c inside <g> before DROP \
         GRAPH -- if this fails, the rule engine doesn't support graph-scoped \
         rules the way this test assumes, and the comparison below would be \
         vacuous"
    );

    update(&server, "ds", &format!("DROP GRAPH <{graph}>")).await;

    let insert_new = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{graph}> {{ ex:x ex:link ex:y . ex:y ex:link ex:z . }} }}"
    );
    update(&server, "ds", &insert_new).await;

    let oracle = common::TestServer::start_writable("").await;
    load_rules(&oracle, "ds", GRAPH_SCOPED_TRANSITIVE_LINK_RULES).await;
    update(&oracle, "ds", &insert_new).await;

    // Sanity: the oracle must derive the new chain in <g>, so the
    // per-pair comparison below is actually exercising something.
    assert!(
        ask(
            &oracle,
            "ds",
            &format!("ASK {{ GRAPH <{graph}> {{ <{EX}x> <{EX}reachable> <{EX}z> }} }}")
        )
        .await,
        "oracle must derive x-reachable-z inside <g> after inserting the new chain"
    );

    for (s, o) in [("x", "y"), ("y", "z"), ("x", "z"), ("a", "b"), ("a", "c")] {
        let q = format!("ASK {{ GRAPH <{graph}> {{ <{EX}{s}> <{EX}reachable> <{EX}{o}> }} }}");
        let actual = ask(&server, "ds", &q).await;
        let expected = ask(&oracle, "ds", &q).await;
        assert_eq!(
            actual, expected,
            "ex:reachable({s},{o}) mismatch after DROP GRAPH + reinsert: \
             server-under-test={actual}, oracle={expected} (issue #591)"
        );
    }
}

/// Deterministic cross-graph case: base fact lives in a named graph
/// `<g>`, derived fact lives in the default graph (the RML-mapped-data
/// shape: extensional data in a named graph, intensional/derived facts
/// in the default graph). `remove_graph(<g>)` only ever touches `<g>`'s
/// own quads, so unlike the same-graph tests above, the derived
/// default-graph fact can *only* be retracted if the reasoner is
/// actually told about the clear — this isn't masked by `remove_graph`
/// physically wiping both base and derived facts together, and doesn't
/// depend on the incremental-vs-full-rebuild fallback threshold either.
async fn assert_cross_graph_clear_retracts_derived_default_graph_fact(clear_stmt: &str) {
    let server = common::TestServer::start_writable("").await;
    load_rules(&server, "ds", CROSS_GRAPH_LINK_RULE).await;

    let graph = format!("{EX}g");
    let insert = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{graph}> {{ ex:a ex:link ex:b . }} }}"
    );
    update(&server, "ds", &insert).await;

    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}linked> <{EX}b> }}")
        )
        .await,
        "derived ex:linked fact must be present in the default graph \
         before {clear_stmt}"
    );

    update(&server, "ds", clear_stmt).await;

    assert!(
        !ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}linked> <{EX}b> }}")
        )
        .await,
        "derived ex:linked fact must be retracted from the default graph \
         after `{clear_stmt}` removed its sole supporting base fact from \
         <g> -- the reasoner must be notified of the clear, not just see \
         <g> disappear out from under it (issue #591)"
    );
}

#[tokio::test]
async fn drop_graph_retracts_cross_graph_derived_fact() {
    assert_cross_graph_clear_retracts_derived_default_graph_fact(&format!("DROP GRAPH <{EX}g>"))
        .await;
}

#[tokio::test]
async fn clear_graph_retracts_cross_graph_derived_fact() {
    assert_cross_graph_clear_retracts_derived_default_graph_fact(&format!("CLEAR GRAPH <{EX}g>"))
        .await;
}

#[tokio::test]
async fn clear_named_retracts_cross_graph_derived_fact() {
    assert_cross_graph_clear_retracts_derived_default_graph_fact("CLEAR NAMED").await;
}

/// A single request that clears a graph and re-inserts base data in the
/// *same* update — the literal shape of a pipeline reload RML mapping
/// run — must still end with the reasoner's derived closure matching
/// what a from-scratch rebuild of the final data would produce. Net
/// set-semantics within one request (deletes and inserts of the exact
/// same quad cancel out) must not be confused with the graph-level
/// `CLEAR`/`DROP` operation never reaching `pending_deletes` at all.
#[tokio::test]
async fn clear_default_and_reinsert_in_one_request_matches_fresh_rebuild() {
    let base = format!(
        "<{EX}a> <{EX}link> <{EX}b> . \
         <{EX}b> <{EX}link> <{EX}c> ."
    );
    let server = common::TestServer::start_writable(&base).await;
    load_rules(&server, "ds", TRANSITIVE_LINK_RULES).await;

    let combined = format!(
        "PREFIX ex: <{EX}>
         CLEAR DEFAULT ;
         INSERT DATA {{ ex:a ex:link ex:b . }}"
    );
    update(&server, "ds", &combined).await;

    let oracle = common::TestServer::start_writable(&format!("<{EX}a> <{EX}link> <{EX}b> .")).await;
    load_rules(&oracle, "ds", TRANSITIVE_LINK_RULES).await;

    for (s, o) in [("a", "b"), ("a", "c"), ("b", "c")] {
        let q = format!("ASK {{ <{EX}{s}> <{EX}reachable> <{EX}{o}> }}");
        let actual = ask(&server, "ds", &q).await;
        let expected = ask(&oracle, "ds", &q).await;
        assert_eq!(
            actual, expected,
            "ex:reachable({s},{o}) mismatch after a single CLEAR DEFAULT ; \
             INSERT DATA request: server-under-test={actual}, \
             oracle={expected} (issue #591)"
        );
    }
}

/// Control test: the Graph Store Protocol `DELETE /{name}/data?graph=<g>`
/// route (`graph_store::gsp_delete_inner`) already routes a cleared
/// graph's quads through `apply_reasoner_delta`, which is the exact
/// pattern `clear_graphs_through_reasoner` was fixed to copy. Confirms
/// that reference pattern genuinely handles the cross-graph case (a
/// derived fact in the default graph, base fact in a named graph) rather
/// than this test's own assumptions being wrong.
#[tokio::test]
async fn gsp_delete_named_graph_retracts_cross_graph_derived_fact() {
    let server = common::TestServer::start_writable("").await;
    load_rules(&server, "ds", CROSS_GRAPH_LINK_RULE).await;

    let graph = format!("{EX}g");
    let insert = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{graph}> {{ ex:a ex:link ex:b . }} }}"
    );
    update(&server, "ds", &insert).await;

    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}linked> <{EX}b> }}")
        )
        .await,
        "derived ex:linked fact must be present in the default graph before DELETE"
    );

    let resp = server
        .client
        .delete(server.dataset_data_graph_url("ds", &graph))
        .send()
        .await
        .expect("GSP DELETE request failed");
    assert!(
        resp.status().is_success(),
        "GSP DELETE must succeed: {}",
        resp.status()
    );

    assert!(
        !ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}linked> <{EX}b> }}")
        )
        .await,
        "derived ex:linked fact must be retracted after GSP DELETE removed \
         its sole supporting base fact from <g>"
    );
}

/// A `CLEAR GRAPH` request with a reasoner configured must still bump
/// `store.generation`, even when it's the *only* op in the request (net
/// inserts/deletes both empty) -- `apply_deletions` removes quads via the
/// `QuadTable`-level `remove_quad` directly, which doesn't bump it itself,
/// unlike `Datastore::remove_graph`'s reasoner-less path. Without the bump
/// in `clear_graphs_through_reasoner`, the If-Match ETag precondition
/// check would silently treat a `CLEAR`-modified store as unchanged.
///
/// Also exercises the incremental (non-fallback) retraction path: the
/// cross-graph tests above all delete a closure's *only* support, which
/// trips `datalog::incremental::FALLBACK_THRESHOLD` and routes through
/// `full_rematerialise` instead of the incremental backward/forward
/// phases -- so they don't actually prove the incremental path handles a
/// cross-graph body atom (`ex:link[?x, ?y] ?g`) correctly, only that
/// *some* path does. Padding with ten unrelated derived facts keeps the
/// single deletion's share of the closure well under the threshold.
#[tokio::test]
async fn clear_graph_with_reasoner_bumps_store_generation() {
    let server = common::TestServer::start_writable("").await;
    load_rules(&server, "ds", CROSS_GRAPH_LINK_RULE).await;

    let graph = format!("{EX}g");
    let other_graph = format!("{EX}h");
    let insert = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{graph}> {{ ex:a ex:link ex:b . }} }}"
    );
    update(&server, "ds", &insert).await;

    // Padding: ten unrelated derived facts in a different graph, so the
    // one deletion below is well under FALLBACK_THRESHOLD and actually
    // exercises the incremental backward/forward phases.
    let padding_triples: String = (0..10)
        .map(|i| format!("ex:p{i} ex:link ex:q{i} . "))
        .collect();
    let insert_padding = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{other_graph}> {{ {padding_triples} }} }}"
    );
    update(&server, "ds", &insert_padding).await;

    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}p0> <{EX}linked> <{EX}q0> }}")
        )
        .await,
        "padding fact must be derived before the CLEAR"
    );

    update(&server, "ds", &format!("CLEAR GRAPH <{graph}>")).await;

    // Retraction on the incremental path: the cleared fact's sole
    // derivation must be gone, but the unrelated padding facts must
    // survive (no over-retraction).
    assert!(
        !ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}linked> <{EX}b> }}")
        )
        .await,
        "ex:linked fact must be retracted on the incremental path after \
         CLEAR GRAPH (issue #591)"
    );
    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}p0> <{EX}linked> <{EX}q0> }}")
        )
        .await,
        "unrelated padding fact must survive -- CLEAR GRAPH <g> must not \
         over-retract derivations from a different graph"
    );

    // Isolate the generation-bump check from the retraction above: insert
    // an unmatched fact into a third graph (no rule fires on it, so
    // clearing it below produces an empty reasoner delta -- net
    // inserts/deletes both empty), capture the ETag, then clear that
    // graph and confirm a stale If-Match is rejected. If `generation`
    // weren't explicitly bumped in `clear_graphs_through_reasoner`,
    // nothing else in this request would bump it, since
    // `apply_deletions`/`apply_insertions` mutate quads via the
    // `QuadTable` level directly, bypassing `Datastore`'s own bump.
    let unmatched_graph = format!("{EX}k");
    let insert_unmatched = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{unmatched_graph}> {{ ex:x ex:unrelated ex:y . }} }}"
    );
    update(&server, "ds", &insert_unmatched).await;

    let ask_sparql = "ASK { ?s ?p ?o }";
    let resp = server
        .client
        .get(server.sparql_query_url(ask_sparql))
        .send()
        .await
        .expect("query request failed");
    let old_etag = resp
        .headers()
        .get("etag")
        .expect("response must carry an ETag")
        .to_str()
        .unwrap()
        .to_string();

    update(&server, "ds", &format!("CLEAR GRAPH <{unmatched_graph}>")).await;

    // A trivial no-op update with If-Match on the pre-CLEAR ETag must now
    // be rejected with 412 Precondition Failed -- if generation wasn't
    // bumped, the server would wrongly still consider the pre-CLEAR ETag
    // current and accept this with 200.
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .header("if-match", &old_etag)
        .body("INSERT DATA { <http://ex/noop> <http://ex/noop> <http://ex/noop> . }")
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        412,
        "stale If-Match must be rejected after CLEAR GRAPH bumped the \
         generation (issue #591): got {}",
        resp.status()
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression coverage for issue [#591](https://github.com/daghovland/rdf-datalog/issues/591):
//! `CLEAR`/`DROP` (default graph, a named graph, `ALL`, `NAMED`) in
//! `sparql_update.rs::apply_prepared_update_with_options` call
//! `store.remove_graph(id)` directly and never feed the removed quads into
//! `pending_deletes` — so the single `apply_reasoner_delta` call at the end
//! of that function never learns the graph was cleared. `remove_graph`
//! physically removes both base *and* derived quads from the store (they
//! are indistinguishable at the storage level), so this looks consistent
//! immediately afterward — the desync only surfaces on a *later* write,
//! once the reasoner's own internal bookkeeping (support counts,
//! derived-from index) has gone stale relative to a store it was never
//! told had changed.
//!
//! This is a much better match for the user's real-world report (a
//! pipeline that reloads a dataset by clearing it and re-running an RML
//! mapping) than a one-off `DELETE WHERE` would be, since `CLEAR`/`DROP`
//! is the natural "reload this graph's data" operation.
//!
//! Contrast with `graph_store.rs::gsp_delete_inner` (the Graph Store
//! Protocol `DELETE` route), which performs the semantically equivalent
//! operation correctly: it collects the graph's quads and routes them
//! through `apply_reasoner_delta` before letting the `None` branch fall
//! back to a bare `store.remove_graph`.

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

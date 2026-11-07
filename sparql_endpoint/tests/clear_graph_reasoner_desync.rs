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

/// `CLEAR DEFAULT` followed by inserting a *different* set of base facts
/// must produce the same derived closure a from-scratch server would
/// produce for that same new data — the reasoner must be told about the
/// clear, not just see the store go empty out from under it.
#[tokio::test]
async fn clear_default_then_reinsert_matches_fresh_rebuild() {
    // a -> b -> c (old data)
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

    // Simulate a pipeline reload: clear the default graph, then insert a
    // disjoint new chain: x -> y -> z.
    update(&server, "ds", "CLEAR DEFAULT").await;
    let insert_new = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ ex:x ex:link ex:y . ex:y ex:link ex:z . }}"
    );
    update(&server, "ds", &insert_new).await;

    // Oracle: a fresh server seeded directly with only the new data.
    let new_base = format!(
        "<{EX}x> <{EX}link> <{EX}y> . \
         <{EX}y> <{EX}link> <{EX}z> ."
    );
    let oracle = common::TestServer::start_writable(&new_base).await;
    load_rules(&oracle, "ds", TRANSITIVE_LINK_RULES).await;

    for (s, o) in [
        ("x", "y"),
        ("y", "z"),
        ("x", "z"),
        // Old facts must not resurrect or linger in either server.
        ("a", "b"),
        ("a", "c"),
        ("b", "c"),
    ] {
        let q = format!("ASK {{ <{EX}{s}> <{EX}reachable> <{EX}{o}> }}");
        let actual = ask(&server, "ds", &q).await;
        let expected = ask(&oracle, "ds", &q).await;
        assert_eq!(
            actual, expected,
            "ex:reachable({s},{o}) mismatch after CLEAR DEFAULT + reinsert: \
             server-under-test={actual}, oracle={expected} (issue #591)"
        );
    }
}

/// Same scenario via `DROP GRAPH <g>` on a named graph (not just the
/// default graph / `CLEAR DEFAULT`), since the bug is in the same
/// `PreparedOp::ClearGraph | PreparedOp::DropGraph` arm.
#[tokio::test]
async fn drop_named_graph_then_reinsert_matches_fresh_rebuild() {
    let server = common::TestServer::start_writable("").await;
    load_rules(&server, "ds", TRANSITIVE_LINK_RULES).await;

    let graph = format!("{EX}g");
    let insert_old = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{graph}> {{ ex:a ex:link ex:b . ex:b ex:link ex:c . }} }}"
    );
    update(&server, "ds", &insert_old).await;

    update(&server, "ds", &format!("DROP GRAPH <{graph}>")).await;

    let insert_new = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ GRAPH <{graph}> {{ ex:x ex:link ex:y . ex:y ex:link ex:z . }} }}"
    );
    update(&server, "ds", &insert_new).await;

    let oracle = common::TestServer::start_writable("").await;
    load_rules(&oracle, "ds", TRANSITIVE_LINK_RULES).await;
    update(&oracle, "ds", &insert_new).await;

    for (s, o) in [("x", "y"), ("y", "z"), ("x", "z"), ("a", "b"), ("a", "c")] {
        let q = format!(
            "ASK {{ GRAPH <{graph}> {{ <{EX}{s}> <{EX}reachable> <{EX}{o}> }} }}"
        );
        let actual = ask(&server, "ds", &q).await;
        let expected = ask(&oracle, "ds", &q).await;
        assert_eq!(
            actual, expected,
            "ex:reachable({s},{o}) mismatch after DROP GRAPH + reinsert: \
             server-under-test={actual}, oracle={expected} (issue #591)"
        );
    }
}

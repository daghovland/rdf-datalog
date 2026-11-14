/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression coverage for issue [#781](https://github.com/daghovland/rdf-datalog/issues/781):
//! the Graph Store Protocol `DELETE` route (`graph_store::gsp_delete_inner`)
//! has the same gap [#774](https://github.com/daghovland/rdf-datalog/issues/774)
//! fixed in `sparql_update.rs`'s `CLEAR`/`DROP` handling — when a reasoner is
//! configured, `gsp_delete_inner` calls `apply_reasoner_delta(&mut reasoner,
//! &mut store, &existing, &[])` and returns, with no `store.generation += 1`
//! afterward. `apply_deletions`/`apply_insertions` mutate quads via the
//! `QuadTable` level directly (`base.named_graphs.remove_quad`/`add_quad`),
//! bypassing `Datastore`'s own generation-incrementing wrapper methods
//! entirely, so a GSP `DELETE` that is the only change to a dataset in a
//! request silently leaves `store.generation` unchanged — defeating the
//! If-Match ETag precondition check and a transaction's optimistic-
//! concurrency generation check.
//!
//! Mirrors [#774]'s own `clear_graph_reasoner_desync.rs::clear_graph_with_reasoner_bumps_store_generation`
//! test, but drives the mutation via `DELETE /{name}/data?graph=<g>`
//! (`gsp_delete_inner`) instead of `CLEAR GRAPH`.

mod common;

const EX: &str = "http://ex/";

/// `ex:a ex:linked ex:b :- ex:a ex:link ex:b ?g .` (any graph `?g`), with the
/// derived fact landing in the default graph regardless of which named
/// graph the base fact lives in.
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

/// A GSP `DELETE /{name}/data?graph=<g>` request with a reasoner configured
/// must still bump `store.generation`, even when it's the only change in
/// the request (net inserts/deletes both empty) -- `apply_deletions`
/// removes quads via the `QuadTable`-level `remove_quad` directly, which
/// doesn't bump it itself, unlike `Datastore::remove_graph`'s reasoner-less
/// path. Without the bump in `gsp_delete_inner`, the If-Match ETag
/// precondition check would silently treat a GSP-DELETE-modified store as
/// unchanged.
///
/// Also exercises the incremental (non-fallback) retraction path: padding
/// with ten unrelated derived facts keeps the single deletion's share of
/// the closure well under `datalog::incremental::FALLBACK_THRESHOLD`, so
/// this isn't just exercising `full_rematerialise`.
#[tokio::test]
async fn gsp_delete_with_reasoner_bumps_store_generation() {
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
        "padding fact must be derived before the GSP DELETE"
    );

    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}linked> <{EX}b> }}")
        )
        .await,
        "derived ex:linked fact must be present in the default graph before GSP DELETE"
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

    // Retraction on the incremental path: the deleted graph's sole
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
         GSP DELETE (issue #781)"
    );
    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}p0> <{EX}linked> <{EX}q0> }}")
        )
        .await,
        "unrelated padding fact must survive -- GSP DELETE of <g> must not \
         over-retract derivations from a different graph"
    );

    // Isolate the generation-bump check from the retraction above: insert
    // an unmatched fact into a third graph (no rule fires on it, so
    // GSP-deleting it below produces an empty reasoner delta -- net
    // inserts/deletes both empty), capture the ETag, then GSP-delete that
    // graph and confirm a stale If-Match is rejected. If `generation`
    // weren't explicitly bumped in `gsp_delete_inner`, nothing else in this
    // request would bump it, since `apply_deletions`/`apply_insertions`
    // mutate quads via the `QuadTable` level directly, bypassing
    // `Datastore`'s own bump.
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

    let resp = server
        .client
        .delete(server.dataset_data_graph_url("ds", &unmatched_graph))
        .send()
        .await
        .expect("GSP DELETE request failed");
    assert!(
        resp.status().is_success(),
        "GSP DELETE must succeed: {}",
        resp.status()
    );

    // A trivial no-op update with If-Match on the pre-DELETE ETag must now
    // be rejected with 412 Precondition Failed -- if generation wasn't
    // bumped, the server would wrongly still consider the pre-DELETE ETag
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
        "stale If-Match must be rejected after GSP DELETE bumped the \
         generation (issue #781): got {}",
        resp.status()
    );
}

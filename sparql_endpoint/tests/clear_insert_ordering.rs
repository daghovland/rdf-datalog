/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression coverage for issue [#782](https://github.com/daghovland/rdf-datalog/issues/782):
//! `sparql_update.rs::apply_prepared_update_with_options` buffers
//! `INSERT DATA`/`DELETE DATA`/`PatternUpdate` ops across the whole request
//! into `pending_inserts`/`pending_deletes`, applying the net delta only
//! once at the end of the op loop, while `CLEAR`/`DROP` apply eagerly
//! *during* the loop (via `clear_graphs_through_reasoner`, added in
//! [#774](https://github.com/daghovland/rdf-datalog/pull/774)). A request
//! like `INSERT DATA { ex:a ex:p ex:b } ; CLEAR DEFAULT` therefore used to
//! leave `ex:a ex:p ex:b` present at the end -- backwards from SPARQL 1.1
//! Update's sequential-operation semantics (§3.1.3), since the eager
//! `CLEAR` ran while the insert was still only buffered (so it saw an
//! empty default graph), and the buffered insert was applied only after
//! the loop finished, with nothing left afterward to remove it.
//!
//! [#774](https://github.com/daghovland/rdf-datalog/pull/774) only tested
//! the reverse order (`CLEAR DEFAULT ; INSERT DATA`, in
//! `clear_graph_reasoner_desync.rs`'s
//! `clear_default_and_reinsert_in_one_request_matches_fresh_rebuild`),
//! which happens to come out correct via the end-of-loop net-delta
//! set-arithmetic. This file covers the opposite ordering (previously
//! broken) plus two additional multi-op sequences that exercise true
//! sequential semantics rather than just the two single-CLEAR orderings.

mod common;

const EX: &str = "http://ex/";

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

async fn ex_a_p_b_present(server: &common::TestServer) -> bool {
    ask(server, "ds", &format!("ASK {{ <{EX}a> <{EX}p> <{EX}b> }}")).await
}

/// `INSERT DATA { ex:a ex:p ex:b } ; CLEAR DEFAULT` in one request must end
/// with the triple ABSENT: sequential semantics says the insert lands
/// first, then the `CLEAR` that textually follows wipes it out. This is
/// the exact scenario #782 reports as (likely) broken.
#[tokio::test]
async fn insert_then_clear_default_leaves_triple_absent() {
    let server = common::TestServer::start_writable("").await;

    let combined = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ ex:a ex:p ex:b . }} ;
         CLEAR DEFAULT"
    );
    update(&server, "ds", &combined).await;

    assert!(
        !ex_a_p_b_present(&server).await,
        "ex:a ex:p ex:b must be absent after INSERT DATA ; CLEAR DEFAULT -- \
         the CLEAR textually follows the INSERT and sequential SPARQL 1.1 \
         Update semantics (§3.1.3) requires it to wipe the just-inserted \
         triple (issue #782)"
    );
}

/// Control: the ordering #774 already covers (`CLEAR DEFAULT ; INSERT
/// DATA {{ same triple }}`) must still end with the triple PRESENT. Any fix
/// for #782 must not regress this -- #774's provenance summary
/// (`provenance/summaries/pr-774.ttl`) explicitly rejected moving
/// `CLEAR`/`DROP` into the `pending_deletes` buffer for exactly this
/// reason.
#[tokio::test]
async fn clear_default_then_insert_leaves_triple_present() {
    let server = common::TestServer::start_writable("").await;

    let combined = format!(
        "PREFIX ex: <{EX}>
         CLEAR DEFAULT ;
         INSERT DATA {{ ex:a ex:p ex:b . }}"
    );
    update(&server, "ds", &combined).await;

    assert!(
        ex_a_p_b_present(&server).await,
        "ex:a ex:p ex:b must be present after CLEAR DEFAULT ; INSERT DATA \
         {{ same triple }} (issue #774 ordering, must not regress)"
    );
}

/// `INSERT DATA { x } ; CLEAR DEFAULT ; INSERT DATA { x }` in one request
/// must end with the triple PRESENT: the first insert is wiped by the
/// CLEAR, then the second insert (textually after the CLEAR) lands on an
/// empty store.
#[tokio::test]
async fn insert_clear_insert_leaves_triple_present() {
    let server = common::TestServer::start_writable("").await;

    let combined = format!(
        "PREFIX ex: <{EX}>
         INSERT DATA {{ ex:a ex:p ex:b . }} ;
         CLEAR DEFAULT ;
         INSERT DATA {{ ex:a ex:p ex:b . }}"
    );
    update(&server, "ds", &combined).await;

    assert!(
        ex_a_p_b_present(&server).await,
        "ex:a ex:p ex:b must be present after INSERT DATA ; CLEAR DEFAULT ; \
         INSERT DATA {{ same triple }} -- the second insert lands after the \
         CLEAR has already run (issue #782)"
    );
}

/// `CLEAR DEFAULT ; INSERT DATA { x } ; CLEAR DEFAULT` in one request must
/// end with the triple ABSENT: the insert lands on the already-cleared
/// store, then the second CLEAR wipes it out again.
#[tokio::test]
async fn clear_insert_clear_leaves_triple_absent() {
    let server = common::TestServer::start_writable("").await;

    let combined = format!(
        "PREFIX ex: <{EX}>
         CLEAR DEFAULT ;
         INSERT DATA {{ ex:a ex:p ex:b . }} ;
         CLEAR DEFAULT"
    );
    update(&server, "ds", &combined).await;

    assert!(
        !ex_a_p_b_present(&server).await,
        "ex:a ex:p ex:b must be absent after CLEAR DEFAULT ; INSERT DATA \
         {{ same triple }} ; CLEAR DEFAULT -- the final CLEAR textually \
         follows the insert and must wipe it (issue #782)"
    );
}

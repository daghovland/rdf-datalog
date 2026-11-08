/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression coverage for issue [#591](https://github.com/daghovland/rdf-datalog/issues/591):
//! a user-reported bug (manual testing against equinor/Dexpi2Imf's real
//! RML mappings / Datalog rules / SPARQL queries) that deletion does not
//! properly propagate into deleting inferred facts. PR #590 already ruled
//! out the `datalog` crate's core `apply_deletions` algorithm for two
//! scenarios; this file tests the `sparql_endpoint`/`sparql_parser` layer
//! specifically, since Dexpi2Imf's real queries use
//! `(imf:connectedTo|^imf:connectedTo)*`-style property paths in `DELETE
//! WHERE`, which had no coverage at all before this file.
//!
//! Each test compares the server-under-test against a from-scratch
//! full-rebuild oracle (a second server seeded with only the surviving
//! base facts and the same rules), mirroring #590's oracle approach,
//! rather than hand-enumerating expected facts.

mod common;

const EX: &str = "http://ex/";

/// `?x ex:reachable ?y :- ?x ex:link ?y .`
/// `?x ex:reachable ?y :- ?x ex:link ?z . ?z ex:reachable ?y .`
///
/// A minimal transitive-closure rule shape, deliberately analogous to
/// `noaka_boundary.datalog`'s `isBoundaryOf`/`isInPackage` transitive
/// closure (the rule shape #590's regression tests already cover at the
/// `datalog` crate level) — here exercised through the HTTP layer with a
/// runtime-loaded ruleset (#390) instead of a compiled-in one.
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

/// `DELETE WHERE` whose pattern uses a `+` property path (not a plain
/// triple pattern) to select which base facts to delete, with an active
/// Datalog reasoner holding a transitive-closure rule. The derived
/// `ex:reachable` facts that depended on the deleted base `ex:link` edge
/// must be retracted, compared against a from-scratch rebuild oracle.
#[tokio::test]
async fn delete_where_with_plus_path_retracts_dependent_inferred_facts() {
    // a -> b -> c -> d
    let base = format!(
        "<{EX}a> <{EX}link> <{EX}b> . \
         <{EX}b> <{EX}link> <{EX}c> . \
         <{EX}c> <{EX}link> <{EX}d> ."
    );
    let server = common::TestServer::start_writable(&base).await;
    load_rules(&server, "ds", TRANSITIVE_LINK_RULES).await;

    // Sanity: a reaches d transitively before any deletion.
    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}reachable> <{EX}d> }}")
        )
        .await,
        "a must transitively reach d before deletion"
    );

    // Delete the b->c edge, selected via a `+` property path rather than a
    // plain triple pattern in WHERE (the untested shape from #591).
    let delete_update = format!(
        "PREFIX ex: <{EX}>
         DELETE {{ ?s ex:link ?o }}
         WHERE  {{ ex:b ex:link+ ?o . ?s ex:link ?o . FILTER(?s = ex:b) }}"
    );
    update(&server, "ds", &delete_update).await;

    // The b->c base edge must be gone.
    assert!(
        !ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}b> <{EX}link> <{EX}c> }}")
        )
        .await,
        "b->c base edge must be deleted"
    );

    // Oracle: a fresh server seeded with only the surviving base facts
    // (a->b, c->d) and the same rules, rebuilt from scratch.
    let surviving_base = format!(
        "<{EX}a> <{EX}link> <{EX}b> . \
         <{EX}c> <{EX}link> <{EX}d> ."
    );
    let oracle = common::TestServer::start_writable(&surviving_base).await;
    load_rules(&oracle, "ds", TRANSITIVE_LINK_RULES).await;

    for (s, o) in [
        ("a", "b"),
        ("b", "c"),
        ("c", "d"),
        ("a", "c"),
        ("a", "d"),
        ("b", "d"),
    ] {
        let q = format!("ASK {{ <{EX}{s}> <{EX}reachable> <{EX}{o}> }}");
        let actual = ask(&server, "ds", &q).await;
        let expected = ask(&oracle, "ds", &q).await;
        assert_eq!(
            actual, expected,
            "ex:reachable({s},{o}) mismatch after DELETE WHERE via `+` path: \
             server-under-test={actual}, oracle={expected}"
        );
    }
}

/// `DELETE WHERE` whose pattern uses a bidirectional `*` path with
/// alternation (`(ex:link|^ex:link)*`) — the exact shape Dexpi2Imf's real
/// queries use (`(imf:connectedTo|^imf:connectedTo)*`) — to select a base
/// fact for deletion. Confirms the inverse/alternation/star combination
/// specifically (not just `+` on a forward predicate) doesn't desync the
/// reasoner from the store.
#[tokio::test]
async fn delete_where_with_bidirectional_star_path_retracts_dependent_inferred_facts() {
    // a -> b -> c -> d
    let base = format!(
        "<{EX}a> <{EX}link> <{EX}b> . \
         <{EX}b> <{EX}link> <{EX}c> . \
         <{EX}c> <{EX}link> <{EX}d> ."
    );
    let server = common::TestServer::start_writable(&base).await;
    load_rules(&server, "ds", TRANSITIVE_LINK_RULES).await;

    assert!(
        ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}a> <{EX}reachable> <{EX}d> }}")
        )
        .await,
        "a must transitively reach d before deletion"
    );

    // Find everything connected to `b` (forward or backward, any distance)
    // via the bidirectional star path, then delete the one edge actually
    // incident on b whose other endpoint is c.
    let delete_update = format!(
        "PREFIX ex: <{EX}>
         DELETE {{ ex:b ex:link ex:c }}
         WHERE  {{ ex:b (ex:link|^ex:link)* ?connected . FILTER(?connected = ex:c) }}"
    );
    update(&server, "ds", &delete_update).await;

    assert!(
        !ask(
            &server,
            "ds",
            &format!("ASK {{ <{EX}b> <{EX}link> <{EX}c> }}")
        )
        .await,
        "b->c base edge must be deleted"
    );

    let surviving_base = format!(
        "<{EX}a> <{EX}link> <{EX}b> . \
         <{EX}c> <{EX}link> <{EX}d> ."
    );
    let oracle = common::TestServer::start_writable(&surviving_base).await;
    load_rules(&oracle, "ds", TRANSITIVE_LINK_RULES).await;

    for (s, o) in [
        ("a", "b"),
        ("b", "c"),
        ("c", "d"),
        ("a", "c"),
        ("a", "d"),
        ("b", "d"),
    ] {
        let q = format!("ASK {{ <{EX}{s}> <{EX}reachable> <{EX}{o}> }}");
        let actual = ask(&server, "ds", &q).await;
        let expected = ask(&oracle, "ds", &q).await;
        assert_eq!(
            actual, expected,
            "ex:reachable({s},{o}) mismatch after DELETE WHERE via bidirectional \
             star path: server-under-test={actual}, oracle={expected}"
        );
    }
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration test: a genuine Datalog `Contradiction` triggered by
//! client-supplied data via `INSERT DATA` must come back as a clean 4xx
//! response, not a crashed connection / dead server process.
//!
//! Before the fix, `DatalogProgram::materialise_one_iteration` `panic!`-ed on
//! a satisfied `RuleHead::Contradiction` body, which — reached via
//! `IncrementalReasoner::apply_insertions` from the SPARQL Update handler —
//! took down the whole `sparql_endpoint` process on one bad client request.
//!
//! Related: [#301](https://github.com/daghovland/rdf-datalog/issues/301)

mod common;

use dag_rdf::Datastore;
use dag_rdf::{
    DEFAULT_GRAPH_ELEMENT_ID, GraphElement, IriReference, QuadPattern, RdfResource, Term,
};
use datalog::{Rule, RuleAtom, RuleHead};
use std::sync::Arc;
use tokio::sync::RwLock;
use turtle::parse_turtle;

const EX_P: &str = "http://ex/p";
const EX_P2: &str = "http://ex/p2";
const EX_ALICE: &str = "http://ex/Alice";
const EX_BOB: &str = "http://ex/Bob";
const EX_CAROL: &str = "http://ex/Carol";

/// Build a "disjoint properties" style contradiction rule:
/// `Contradiction :- { ?x p ?y, ?x p2 ?y }`
///
/// Resources are interned into `ds` so IDs are consistent with any data
/// already loaded into the same store.
fn make_disjoint_properties_contradiction_rule(ds: &mut Datastore) -> Rule {
    let p = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_P.to_string(),
    ))));
    let p2 = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_P2.to_string(),
    ))));
    let g = DEFAULT_GRAPH_ELEMENT_ID;
    Rule {
        head: RuleHead::Contradiction,
        body: vec![
            RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            }),
            RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p2),
                object: Term::Variable("y".to_string()),
            }),
        ],
    }
}

/// INSERT DATA that introduces a genuine contradiction (a base fact that,
/// combined with existing data, satisfies a `RuleHead::Contradiction` rule's
/// body) must be rejected with a 4xx response — not a panic that kills the
/// connection/process — and must leave the store's visible state unchanged.
/// A subsequent, non-contradictory request must still succeed afterwards,
/// proving the server (and its reasoner) survived and remained usable.
#[tokio::test]
async fn test_contradiction_insert_returns_4xx_and_server_survives() {
    // Initial data: Alice already has p2 to Bob (consistent on its own —
    // no `p` edge yet, so the contradiction rule's body isn't satisfied).
    let turtle_data = format!("<{EX_ALICE}> <{EX_P2}> <{EX_BOB}> .");

    let mut ds = Datastore::new(1024);
    parse_turtle(&mut ds, turtle_data.as_bytes()).expect("fixture turtle must parse");

    let rule = make_disjoint_properties_contradiction_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));

    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    // Sanity check: the store starts out queryable and consistent.
    let ask_before = format!("ASK {{ <{EX_ALICE}> <{EX_P2}> <{EX_BOB}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_before))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);

    // INSERT DATA: Alice p Bob. Combined with the existing Alice p2 Bob,
    // this satisfies the contradiction rule's body.
    let bad_update = format!("INSERT DATA {{ <{EX_ALICE}> <{EX_P}> <{EX_BOB}> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(bad_update)
        .send()
        .await
        .expect("POST update must return an HTTP response, not a dropped connection");

    assert!(
        resp.status().is_client_error(),
        "a genuine contradiction from client-supplied data must be reported as 4xx, got {}",
        resp.status()
    );

    // The rejected insert must not be visible: the store was rolled back.
    let ask_rejected = format!("ASK {{ <{EX_ALICE}> <{EX_P}> <{EX_BOB}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_rejected))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "the rejected insert (Alice p Bob) must not be visible after rollback"
    );

    // The pre-existing fact must have survived the rejected transaction.
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_before))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "the pre-existing fact (Alice p2 Bob) must survive the rejected transaction"
    );

    // The server (and its reasoner) must still be usable: a subsequent,
    // non-contradictory INSERT DATA must succeed normally.
    let good_update = format!("INSERT DATA {{ <{EX_CAROL}> <{EX_P2}> <{EX_BOB}> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(good_update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        204,
        "server must remain usable after rejecting a contradiction"
    );

    let ask_carol = format!("ASK {{ <{EX_CAROL}> <{EX_P2}> <{EX_BOB}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_carol))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "a valid insert after the rejected contradiction must take effect"
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for owl:Nothing constraint checking in SPARQL Update.
//!
//! When a server is configured with an incremental reasoner, any INSERT that
//! causes the reasoner to derive `?x a owl:Nothing` must be rejected with
//! HTTP 409 Conflict and rolled back — no inserted triples must become visible.
//!
//! Related: [#127](https://github.com/daghovland/rdf-datalog/issues/127)

mod common;

use dag_rdf::Datastore;
use dag_rdf::{
    DEFAULT_GRAPH_ELEMENT_ID, GraphElement, IriReference, QuadPattern, RdfResource, Term,
};
use datalog::{Rule, RuleAtom, RuleHead};
use std::sync::Arc;
use tokio::sync::RwLock;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";
const EX_FORBIDDEN: &str = "http://example.org/forbidden";
const EX_VALUE: &str = "http://example.org/value";
const EX_OTHER_PROP: &str = "http://example.org/otherProp";
const EX_BAD: &str = "http://example.org/bad";
const EX_GOOD: &str = "http://example.org/good";
const EX_OK: &str = "http://example.org/ok";
const EX_P: &str = "http://example.org/p";
const EX_V: &str = "http://example.org/v";

/// Build the constraint rule:
/// `?x a owl:Nothing :- ?x ex:forbidden ex:value`
///
/// Resources are interned into `ds` so IDs are consistent with any data
/// already loaded into the same store.
fn make_constraint_rule(ds: &mut Datastore) -> Rule {
    let rdf_type = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        RDF_TYPE.to_string(),
    ))));
    let owl_nothing = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        OWL_NOTHING.to_string(),
    ))));
    let ex_forbidden = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_FORBIDDEN.to_string(),
    ))));
    let ex_value = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_VALUE.to_string(),
    ))));
    let g = DEFAULT_GRAPH_ELEMENT_ID;
    Rule {
        head: RuleHead::NormalHead(QuadPattern {
            graph: Term::Resource(g),
            subject: Term::Variable("x".to_string()),
            predicate: Term::Resource(rdf_type),
            object: Term::Resource(owl_nothing),
        }),
        body: vec![RuleAtom::PositivePattern(QuadPattern {
            graph: Term::Resource(g),
            subject: Term::Variable("x".to_string()),
            predicate: Term::Resource(ex_forbidden),
            object: Term::Resource(ex_value),
        })],
    }
}

// ── Test 1: constraint rule prevents the violating INSERT ─────────────────────

/// Inserting a triple that matches the constraint rule must be rejected
/// with HTTP 409 Conflict and the store must remain unchanged.
///
/// Setup:
/// - Rule: `?x a owl:Nothing :- ?x ex:forbidden ex:value`
/// - Action: INSERT DATA { ex:bad ex:forbidden ex:value }
/// - Expected: 409, store empty (triple not inserted)
///
/// Related: [#127](https://github.com/daghovland/rdf-datalog/issues/127)
#[tokio::test]
async fn test_constraint_rule_prevents_commit() {
    let mut ds = Datastore::new(1024);
    let rule = make_constraint_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));

    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    let update = format!("INSERT DATA {{ <{EX_BAD}> <{EX_FORBIDDEN}> <{EX_VALUE}> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        409,
        "INSERT that triggers owl:Nothing must return 409 Conflict"
    );

    // Verify the store is unchanged.
    let ask = format!("ASK {{ <{EX_BAD}> <{EX_FORBIDDEN}> <{EX_VALUE}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "violating triple must NOT be present after 409 rollback"
    );
}

// ── Test 2: non-violating INSERT succeeds ─────────────────────────────────────

/// Inserting a triple that does NOT match the constraint rule must succeed
/// with HTTP 204 and the triple must be present afterwards.
///
/// Setup:
/// - Rule: `?x a owl:Nothing :- ?x ex:forbidden ex:value`
/// - Action: INSERT DATA { ex:good ex:otherProp ex:value }
/// - Expected: 204, triple is present
///
/// Related: [#127](https://github.com/daghovland/rdf-datalog/issues/127)
#[tokio::test]
async fn test_no_violation_allows_commit() {
    let mut ds = Datastore::new(1024);
    let rule = make_constraint_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));

    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    let update = format!("INSERT DATA {{ <{EX_GOOD}> <{EX_OTHER_PROP}> <{EX_VALUE}> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        204,
        "non-violating INSERT must return 204 No Content"
    );

    // Verify the triple is present.
    let ask = format!("ASK {{ <{EX_GOOD}> <{EX_OTHER_PROP}> <{EX_VALUE}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "non-violating triple must be present after 204 commit"
    );
}

// ── Test 3: multi-statement request is rolled back entirely ───────────────────

/// A two-statement SPARQL Update request where the second statement violates
/// the constraint must result in HTTP 409 and BOTH statements rolled back.
///
/// Setup:
/// - Rule: `?x a owl:Nothing :- ?x ex:forbidden ex:value`
/// - Multi-statement:
///   `INSERT DATA { ex:ok ex:p ex:v } ; INSERT DATA { ex:bad ex:forbidden ex:value }`
/// - Expected: 409, both triples absent
///
/// Related: [#127](https://github.com/daghovland/rdf-datalog/issues/127)
#[tokio::test]
async fn test_violation_rolls_back_entire_request() {
    let mut ds = Datastore::new(1024);
    let rule = make_constraint_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));

    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    let update = format!(
        "INSERT DATA {{ <{EX_OK}> <{EX_P}> <{EX_V}> . }} ; \
         INSERT DATA {{ <{EX_BAD}> <{EX_FORBIDDEN}> <{EX_VALUE}> . }}"
    );
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        409,
        "multi-statement INSERT that triggers owl:Nothing must return 409"
    );

    // Verify BOTH triples are absent (full rollback).
    let ask_ok = format!("ASK {{ <{EX_OK}> <{EX_P}> <{EX_V}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_ok))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK ok failed");
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "ex:ok triple must NOT be present after full rollback"
    );

    let ask_bad = format!("ASK {{ <{EX_BAD}> <{EX_FORBIDDEN}> <{EX_VALUE}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_bad))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK bad failed");
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "ex:bad triple must NOT be present after full rollback"
    );
}

// ── Test 4: no constraint check without a reasoner ───────────────────────────

/// Without a reasoner configured, inserting a triple that asserts
/// `?x a owl:Nothing` directly must succeed (204).
///
/// Constraint checking only fires when a reasoner is active; direct
/// insertions of owl:Nothing are not treated as violations.
///
/// Related: [#127](https://github.com/daghovland/rdf-datalog/issues/127)
#[tokio::test]
async fn test_without_reasoner_no_constraint_check() {
    // Start a writable server with NO rules (no reasoner).
    let server = common::TestServer::start_writable("").await;

    let update = format!("INSERT DATA {{ <{EX_BAD}> <{RDF_TYPE}> <{OWL_NOTHING}> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        204,
        "direct owl:Nothing INSERT without a reasoner must return 204 No Content"
    );

    // Verify the triple is present.
    let ask = format!("ASK {{ <{EX_BAD}> <{RDF_TYPE}> <{OWL_NOTHING}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "direct owl:Nothing triple must be present when no reasoner is configured"
    );
}

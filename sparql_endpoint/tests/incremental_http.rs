/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for incremental Datalog reasoning via the HTTP endpoint.
//!
//! Verifies that after INSERT DATA / DELETE DATA, derived triples are updated
//! automatically by the `IncrementalReasoner` wired into the SPARQL Update
//! handler.
//!
//! Related: [#110](https://github.com/daghovland/rdf-datalog/issues/110)

mod common;

use dag_rdf::Datastore;
use dag_rdf::{
    DEFAULT_GRAPH_ELEMENT_ID, GraphElement, IriReference, QuadPattern, RdfResource, Term,
};
use datalog::{Rule, RuleAtom, RuleHead};
use std::sync::Arc;
use tokio::sync::RwLock;
use turtle::parse_turtle;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const EX_MANAGER: &str = "http://ex/Manager";
const EX_EMPLOYEE: &str = "http://ex/Employee";
const EX_ALICE: &str = "http://ex/Alice";
const EX_BOB: &str = "http://ex/Bob";

/// Build the rule: `?x rdf:type ex:Employee :- ?x rdf:type ex:Manager`
///
/// Resources are interned into `ds` so IDs are consistent with any data
/// already loaded into the same store.
fn make_manager_implies_employee_rule(ds: &mut Datastore) -> Rule {
    let rdf_type = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        RDF_TYPE.to_string(),
    ))));
    let manager = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_MANAGER.to_string(),
    ))));
    let employee = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_EMPLOYEE.to_string(),
    ))));
    let g = DEFAULT_GRAPH_ELEMENT_ID;
    Rule {
        head: RuleHead::NormalHead(QuadPattern {
            graph: Term::Resource(g),
            subject: Term::Variable("x".to_string()),
            predicate: Term::Resource(rdf_type),
            object: Term::Resource(employee),
        }),
        body: vec![RuleAtom::PositivePattern(QuadPattern {
            graph: Term::Resource(g),
            subject: Term::Variable("x".to_string()),
            predicate: Term::Resource(rdf_type),
            object: Term::Resource(manager),
        })],
    }
}

// ── Test 1: INSERT DATA triggers inference ────────────────────────────────────

/// After INSERT DATA adds a new extensional triple that fires a Datalog rule,
/// the inferred triple must be queryable immediately.
///
/// Setup:
/// - Initial store: `ex:Alice a ex:Manager` (already inferred as Employee at start)
/// - Rule: `?x a ex:Employee :- ?x a ex:Manager`
/// - Action: INSERT DATA `ex:Bob a ex:Manager`
/// - Expected: SPARQL SELECT finds `ex:Bob a ex:Employee` (inferred)
///
/// Related: [#110](https://github.com/daghovland/rdf-datalog/issues/110)
#[tokio::test]
async fn test_insert_triggers_inference() {
    let turtle_data = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");

    let mut ds = Datastore::new(1024);
    parse_turtle(&mut ds, turtle_data.as_bytes()).expect("fixture turtle must parse");

    let rule = make_manager_implies_employee_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));

    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    // Verify initial state: Alice is already an Employee (derived at startup).
    let select_alice = format!("SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{EX_EMPLOYEE}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&select_alice))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    let bindings = body["results"]["bindings"]
        .as_array()
        .expect("bindings array");
    assert!(
        bindings.iter().any(|b| b["s"]["value"] == EX_ALICE),
        "Alice should already be an Employee before any UPDATE (initial materialisation)"
    );

    // INSERT DATA: Bob is a Manager.
    let update = format!("INSERT DATA {{ <{EX_BOB}> <{RDF_TYPE}> <{EX_MANAGER}> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 204, "INSERT DATA must return 204 No Content");

    // Bob must now be inferred as an Employee.
    let select_bob = format!("ASK {{ <{EX_BOB}> <{RDF_TYPE}> <{EX_EMPLOYEE}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&select_bob))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "Bob must be inferred as ex:Employee after INSERT DATA ex:Bob a ex:Manager"
    );
}

// ── Test 2: DELETE DATA retracts derived triple ───────────────────────────────

/// After DELETE DATA removes the only extensional triple that supports a derived
/// triple, the derived triple must no longer be present.
///
/// Setup:
/// - Initial store: `ex:Alice a ex:Manager` (derived `ex:Alice a ex:Employee`)
/// - Rule: `?x a ex:Employee :- ?x a ex:Manager`
/// - Action: DELETE DATA `ex:Alice a ex:Manager`
/// - Expected: `ex:Alice a ex:Employee` is no longer present
///
/// Related: [#110](https://github.com/daghovland/rdf-datalog/issues/110)
#[tokio::test]
async fn test_delete_removes_inference() {
    let turtle_data = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");

    let mut ds = Datastore::new(1024);
    parse_turtle(&mut ds, turtle_data.as_bytes()).expect("fixture turtle must parse");

    let rule = make_manager_implies_employee_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));

    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    // Verify initial state: Alice is an Employee.
    let ask_alice_employee = format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_alice_employee))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "Alice must initially be inferred as ex:Employee"
    );

    // DELETE DATA: remove Alice's Manager triple.
    let update = format!("DELETE DATA {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 204, "DELETE DATA must return 204 No Content");

    // Alice must no longer be an Employee.
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_alice_employee))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "Alice's ex:Employee triple must be retracted after DELETE DATA ex:Alice a ex:Manager"
    );
}

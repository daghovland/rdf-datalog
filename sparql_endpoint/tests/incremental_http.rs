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

// ── Test 3: Multi-statement update is reasoned atomically ─────────────────────

/// A two-statement INSERT DATA in a single SPARQL Update request produces the
/// same derived facts as two separate inserts.
///
/// Setup:
/// - Rule: `?x a ex:Employee :- ?x a ex:Manager`
/// - Initial store: empty
/// - Multi-statement update:
///   `INSERT DATA { ex:Alice a ex:Manager . } ; INSERT DATA { ex:Bob a ex:Manager . }`
/// - Expected: both Alice and Bob are inferred as ex:Employee
///
/// Related: [#114](https://github.com/daghovland/rdf-datalog/issues/114)
#[tokio::test]
async fn test_multi_insert_atomic_reasoning() {
    let mut ds = Datastore::new(1024);
    let rule = make_manager_implies_employee_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));

    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    // Multi-statement INSERT: Alice and Bob both become Managers in one request.
    let update = format!(
        "INSERT DATA {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> . }} ; \
         INSERT DATA {{ <{EX_BOB}> <{RDF_TYPE}> <{EX_MANAGER}> . }}"
    );
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 204, "multi-statement INSERT must return 204");

    // Both Alice and Bob must be inferred as Employees.
    for (name, iri) in [("Alice", EX_ALICE), ("Bob", EX_BOB)] {
        let ask = format!("ASK {{ <{iri}> <{RDF_TYPE}> <{EX_EMPLOYEE}> . }}");
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
            "{name} must be inferred as ex:Employee after multi-statement INSERT"
        );
    }
}

// ── Test 4: DELETE + INSERT in one request — no stale inferences ──────────────

/// A DELETE DATA followed by an INSERT DATA in the same SPARQL Update request
/// must produce correct derived facts based on the final state only.
///
/// Setup:
/// - Rules:
///   - `?x a ex:Employee :- ?x a ex:Manager`
///   - `?x a ex:Employee :- ?x a ex:Director`
/// - Initial store: `ex:Alice a ex:Manager` (derived: `ex:Alice a ex:Employee`)
/// - Multi-statement update:
///   `DELETE DATA { ex:Alice a ex:Manager . } ; INSERT DATA { ex:Alice a ex:Director . }`
/// - Expected after update:
///   - Alice a ex:Director (base)
///   - Alice a ex:Employee (inferred via Director rule)
///   - Alice NOT a ex:Manager
///
/// Demonstrates atomic batching: the intermediate state where Alice has no type
/// must not produce stale inferences; the reasoner fires once at the end.
///
/// Related: [#114](https://github.com/daghovland/rdf-datalog/issues/114)
#[tokio::test]
async fn test_delete_then_insert_different_type_atomic() {
    const EX_DIRECTOR: &str = "http://ex/Director";

    let turtle_data = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");

    let mut ds = Datastore::new(1024);
    parse_turtle(&mut ds, turtle_data.as_bytes()).expect("fixture turtle must parse");

    // Build two rules: Manager → Employee and Director → Employee.
    let rdf_type = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        RDF_TYPE.to_string(),
    ))));
    let manager = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_MANAGER.to_string(),
    ))));
    let employee = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_EMPLOYEE.to_string(),
    ))));
    let director = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_DIRECTOR.to_string(),
    ))));
    let g = DEFAULT_GRAPH_ELEMENT_ID;

    let rule_manager = Rule {
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
    };
    let rule_director = Rule {
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
            object: Term::Resource(director),
        })],
    };

    let store = Arc::new(RwLock::new(ds));
    let server = common::TestServer::start_with_store_and_rules(
        store,
        vec![rule_manager, rule_director],
        false,
    )
    .await;

    // Verify initial state: Alice is a Manager and (derived) an Employee.
    let ask_manager = format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_manager))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "initial: Alice must be a Manager"
    );

    let ask_employee = format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_employee))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "initial: Alice must be an Employee"
    );

    // Atomic multi-statement update: remove Manager, add Director.
    let update = format!(
        "DELETE DATA {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> . }} ; \
         INSERT DATA {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_DIRECTOR}> . }}"
    );
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(resp.status(), 204, "multi-statement update must return 204");

    // Alice must no longer be a Manager.
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_manager))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "Alice must NOT be a Manager after DELETE DATA"
    );

    // Alice must now be a Director.
    let ask_director = format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_DIRECTOR}> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_director))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "Alice must be a Director after INSERT DATA"
    );

    // Alice must still be an Employee (now inferred via Director rule).
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_employee))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "Alice must still be an Employee (inferred via Director→Employee rule)"
    );
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Layer 2 — semantic integration tests for query-builder-generated SPARQL.
//!
//! Each test represents one "builder scenario" and posts the exact SPARQL the
//! builder would emit to a real `TestServer`.  These tests verify semantic
//! correctness (right rows, right columns) without a browser.
//!
//! Queries use full `<IRI>` syntax — they bypass the frontend's prefix-prepender
//! and must be valid standalone SPARQL.
//!
//! # Phase linking
//! All tests are tagged `"QB Phase N"` in their `#[ignore]` reason so they can
//! be batch-activated with:
//!
//! ```bash
//! grep -rn "QB Phase 1" sparql_endpoint/tests/query_builder_sparql.rs
//! ```

mod common;

use serde_json::Value;

// ── Fixture ───────────────────────────────────────────────────────────────────

/// Shared dataset for all query-builder semantic tests.
///
/// Contains two Person instances (alice, bob), one Company (acme),
/// object-property links (knows, worksFor), data properties (label, age,
/// revenue), and OWL class declarations for class-discovery queries.
const QB_FIXTURE: &str = r#"
@prefix ex:   <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:  <http://www.w3.org/2002/07/owl#> .

ex:alice a ex:Person ;
    rdfs:label "Alice" ;
    ex:age "30" ;
    ex:knows ex:bob ;
    ex:worksFor ex:acme .

ex:bob a ex:Person ;
    rdfs:label "Bob" ;
    ex:age "25" .

ex:acme a ex:Company ;
    rdfs:label "Acme Corp" ;
    ex:revenue "1000000" .

ex:Person  a owl:Class .
ex:Company a owl:Class .
"#;

// ── Helper ────────────────────────────────────────────────────────────────────

async fn sparql_bindings(server: &common::TestServer, sparql: &str) -> Vec<Value> {
    let resp = server
        .client
        .post(server.sparql_url())
        .header("Content-Type", "application/sparql-query")
        .body(sparql.to_string())
        .send()
        .await
        .expect("POST /sparql failed");
    assert_eq!(
        resp.status(),
        200,
        "expected 200 for query:\n{sparql}\nstatus: {}",
        resp.status()
    );
    let body: Value = resp.json().await.expect("response must be JSON");
    body["results"]["bindings"]
        .as_array()
        .expect("results.bindings must be an array")
        .clone()
}

// ── QB Phase 1: single class + data properties ────────────────────────────────
// These tests verify the output of generate_sparql for single-node queries.
// Unignore when implementing QB Phase 1.

#[tokio::test]

async fn builder_single_class_returns_all_instances() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: focus = Person, no properties selected.
    let sparql = "\
SELECT ?s WHERE {
  ?s a <http://example.org/Person> .
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 2, "alice and bob are both Person instances");
}

#[tokio::test]
async fn builder_single_class_one_data_prop_both_instances_returned() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: focus = Person, rdfs:label checked.
    let sparql = "\
SELECT ?s ?s_label WHERE {
  ?s a <http://example.org/Person> .
  OPTIONAL { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label }
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(
        bindings.len(),
        2,
        "both instances returned (OPTIONAL preserves them)"
    );
    assert!(
        bindings.iter().all(|b| b["s_label"]["type"] == "literal"),
        "every row has a label — both alice and bob have rdfs:label"
    );
}

#[tokio::test]
async fn builder_data_props_are_optional_instances_without_prop_still_appear() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Company has no ex:age — querying with an OPTIONAL age prop must still
    // return acme (with ?c_age unbound), not zero rows.
    let sparql = "\
SELECT ?c ?c_age WHERE {
  ?c a <http://example.org/Company> .
  OPTIONAL { ?c <http://example.org/age> ?c_age }
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(
        bindings.len(),
        1,
        "acme returned even though it has no ex:age"
    );
    assert!(
        bindings[0]["c_age"].is_null() || bindings[0]["c_age"] == Value::Null,
        "?c_age should be unbound for acme: {:?}",
        bindings[0]["c_age"]
    );
}

#[tokio::test]
async fn builder_two_data_props_both_projected() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: focus = Person, rdfs:label + ex:age both checked.
    let sparql = "\
SELECT ?s ?s_label ?s_age WHERE {
  ?s a <http://example.org/Person> .
  OPTIONAL { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label }
  OPTIONAL { ?s <http://example.org/age> ?s_age }
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 2);
    // Every row has both label and age (alice and bob both have them).
    assert!(bindings.iter().all(|b| b["s_label"]["type"] == "literal"));
    assert!(bindings.iter().all(|b| b["s_age"]["type"] == "literal"));
}

// ── QB Phase 2: multi-hop object-property links ───────────────────────────────
// These tests verify two- and three-node query graphs.
// Unignore when implementing QB Phase 2.

#[tokio::test]
async fn builder_two_node_chain_inner_join_excludes_instances_without_link() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: Person -[knows]→ Person.
    // Only alice has ex:knows; bob does not.  Required join must exclude bob.
    let sparql = "\
SELECT ?s ?n1 WHERE {
  ?s a <http://example.org/Person> .
  ?s <http://example.org/knows> ?n1 .
  ?n1 a <http://example.org/Person> .
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(
        bindings.len(),
        1,
        "only alice→bob; bob has no outgoing knows"
    );
    assert_eq!(bindings[0]["s"]["value"], "http://example.org/alice");
    assert_eq!(bindings[0]["n1"]["value"], "http://example.org/bob");
}

#[tokio::test]
async fn builder_two_node_chain_with_data_props_on_both() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: Person(?s, label) -[knows]→ Person(?n1, label).
    let sparql = "\
SELECT ?s ?s_label ?n1 ?n1_label WHERE {
  ?s a <http://example.org/Person> .
  OPTIONAL { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label }
  ?s <http://example.org/knows> ?n1 .
  ?n1 a <http://example.org/Person> .
  OPTIONAL { ?n1 <http://www.w3.org/2000/01/rdf-schema#label> ?n1_label }
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0]["s_label"]["value"], "Alice");
    assert_eq!(bindings[0]["n1_label"]["value"], "Bob");
}

#[tokio::test]
async fn builder_cross_class_link_person_to_company() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: Person(?s) -[worksFor]→ Company(?n1).
    let sparql = "\
SELECT ?s ?n1 WHERE {
  ?s a <http://example.org/Person> .
  ?s <http://example.org/worksFor> ?n1 .
  ?n1 a <http://example.org/Company> .
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 1, "only alice worksFor acme");
    assert_eq!(bindings[0]["s"]["value"], "http://example.org/alice");
    assert_eq!(bindings[0]["n1"]["value"], "http://example.org/acme");
}

#[tokio::test]
async fn builder_fan_out_two_object_links_from_root() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: Person(?s) -[knows]→ Person(?n1)
    //                            -[worksFor]→ Company(?n2)
    // Both links required → only alice (who has both) matches.
    let sparql = "\
SELECT ?s ?n1 ?n2 WHERE {
  ?s a <http://example.org/Person> .
  ?s <http://example.org/knows> ?n1 .
  ?n1 a <http://example.org/Person> .
  ?s <http://example.org/worksFor> ?n2 .
  ?n2 a <http://example.org/Company> .
}";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0]["s"]["value"], "http://example.org/alice");
    assert_eq!(bindings[0]["n1"]["value"], "http://example.org/bob");
    assert_eq!(bindings[0]["n2"]["value"], "http://example.org/acme");
}

// ── QB Phase 3: data-property filters ────────────────────────────────────────
// These tests verify FILTER conditions added by Phase 3.
// Unignore when implementing QB Phase 3.

#[tokio::test]
async fn builder_regex_filter_on_label_narrows_results() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: Person(?s) with label filtered to "Alice".
    // When a filter is applied the property becomes required (not OPTIONAL)
    // and a FILTER is added at the top level — this is what generate_sparql emits.
    let sparql = r#"SELECT ?s ?s_label WHERE {
  ?s a <http://example.org/Person> .
  ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label .
  FILTER(regex(?s_label, "Alice", "i"))
}"#;
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 1, "only alice's label matches 'Alice'");
    assert_eq!(bindings[0]["s_label"]["value"], "Alice");
}

#[tokio::test]
async fn builder_equality_filter_on_prop() {
    let server = common::TestServer::start(QB_FIXTURE).await;
    // Builder state: Person(?s) with label filtered to exact value "Bob".
    // Equality filter: only one row expected.
    let sparql = r#"SELECT ?s ?s_label WHERE {
  ?s a <http://example.org/Person> .
  ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label .
  FILTER(?s_label = "Bob")
}"#;
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 1, "only bob has label 'Bob'");
    assert_eq!(bindings[0]["s"]["value"], "http://example.org/bob");
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `POST /{dataset}/rules` — loading/replacing a dataset's
//! live Datalog ruleset at runtime.
//!
//! Related: [#390](https://github.com/daghovland/rdf-datalog/issues/390),
//! [#469](https://github.com/daghovland/rdf-datalog/issues/469).

mod common;

use dag_rdf::Datastore;
use datalog::Rule;
use std::sync::Arc;
use tokio::sync::RwLock;
use turtle::parse_turtle;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const EX_MANAGER: &str = "http://ex/Manager";
const EX_EMPLOYEE: &str = "http://ex/Employee";
const EX_CONTRACTOR: &str = "http://ex/Contractor";
const EX_ALICE: &str = "http://ex/Alice";

/// `?x rdf:type ex:Employee :- ?x rdf:type ex:Manager .`
const MANAGER_IMPLIES_EMPLOYEE_RULES: &str = r#"
PREFIX ex: <http://ex/>
PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
ex:Employee[?x] :- ex:Manager[?x] .
"#;

/// `?x rdf:type ex:Contractor :- ?x rdf:type ex:Manager .` — a *different*
/// consequence than [`MANAGER_IMPLIES_EMPLOYEE_RULES`], used to prove replace
/// (not merge) semantics.
const MANAGER_IMPLIES_CONTRACTOR_RULES: &str = r#"
PREFIX ex: <http://ex/>
PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
ex:Contractor[?x] :- ex:Manager[?x] .
"#;

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
    body["boolean"].as_bool().unwrap_or_else(|| {
        !body["results"]["bindings"]
            .as_array()
            .expect("bindings array")
            .is_empty()
    })
}

async fn ask_default(server: &common::TestServer, sparql: &str) -> bool {
    let url = server.sparql_query_url(sparql);
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
    body["boolean"].as_bool().unwrap_or_else(|| {
        !body["results"]["bindings"]
            .as_array()
            .expect("bindings array")
            .is_empty()
    })
}

/// A dataset that never had a reasoner (created via `POST /$/datasets`, with
/// no `--rules`-equivalent startup config) can be given one at runtime via
/// `POST /{name}/rules`, and the derived facts become queryable.
#[tokio::test]
async fn test_post_rules_new_dataset_no_prior_reasoner() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/newds&dbType=mem")
        .send()
        .await
        .expect("create request failed");

    // Base fact: ex:Alice rdf:type ex:Manager .
    let turtle = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");
    let resp = server
        .client
        .put(server.dataset_data_default_url("newds"))
        .header("Content-Type", "text/turtle")
        .body(turtle)
        .send()
        .await
        .expect("put failed");
    assert!(resp.status().is_success(), "PUT failed: {}", resp.status());

    let resp = server
        .client
        .post(server.dataset_rules_url("newds"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 200, "POST /rules failed: {}", resp.status());

    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "newly-loaded ruleset should have derived Employee from Manager"
    );
}

/// When the default dataset already has a reasoner (from `Config::initial_rules`
/// at startup), `POST /{dataset}/rules` *replaces* it rather than merging:
/// facts only derivable under the old ruleset disappear, facts derivable
/// under the new one appear. Also proves the root `/sparql` route (which
/// reads `AppState.reasoner` directly, not through the dataset registry)
/// observes the swap.
#[tokio::test]
async fn test_post_rules_default_dataset_with_prior_reasoner() {
    let mut ds = Datastore::new(1024);
    let turtle = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");
    parse_turtle(&mut ds, std::io::BufReader::new(turtle.as_bytes())).expect("fixture parses");
    let manager = ds.add_resource(dag_rdf::GraphElement::NodeOrEdge(
        dag_rdf::RdfResource::Iri(dag_rdf::IriReference(EX_MANAGER.to_string())),
    ));
    let employee = ds.add_resource(dag_rdf::GraphElement::NodeOrEdge(
        dag_rdf::RdfResource::Iri(dag_rdf::IriReference(EX_EMPLOYEE.to_string())),
    ));
    let rdf_type = ds.add_resource(dag_rdf::GraphElement::NodeOrEdge(
        dag_rdf::RdfResource::Iri(dag_rdf::IriReference(RDF_TYPE.to_string())),
    ));
    let g = dag_rdf::DEFAULT_GRAPH_ELEMENT_ID;
    let old_rule = Rule {
        head: datalog::RuleHead::NormalHead(dag_rdf::QuadPattern {
            graph: dag_rdf::Term::Resource(g),
            subject: dag_rdf::Term::Variable("x".to_string()),
            predicate: dag_rdf::Term::Resource(rdf_type),
            object: dag_rdf::Term::Resource(employee),
        }),
        body: vec![datalog::RuleAtom::PositivePattern(dag_rdf::QuadPattern {
            graph: dag_rdf::Term::Resource(g),
            subject: dag_rdf::Term::Variable("x".to_string()),
            predicate: dag_rdf::Term::Resource(rdf_type),
            object: dag_rdf::Term::Resource(manager),
        })],
    };
    let store = Arc::new(RwLock::new(ds));
    let server = common::TestServer::start_with_store_and_rules(store, vec![old_rule], false).await;

    // Sanity: the startup-loaded ruleset already derived Employee.
    assert!(
        ask_default(
            &server,
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "startup ruleset should have derived Employee from Manager"
    );

    // Replace with a ruleset that derives Contractor instead.
    let resp = server
        .client
        .post(server.dataset_rules_url("ds"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_CONTRACTOR_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(
        resp.status(),
        200,
        "POST /ds/rules failed: {}",
        resp.status()
    );

    assert!(
        !ask_default(
            &server,
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "old ruleset's derived fact must be gone after replace"
    );
    assert!(
        ask_default(
            &server,
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_CONTRACTOR}> }}")
        )
        .await,
        "new ruleset's derived fact must be present after replace, via the root /sparql route"
    );
}

/// Loading a ruleset into one dataset must not affect a sibling dataset's
/// facts or give it a reasoner it didn't ask for.
#[tokio::test]
async fn test_post_rules_dataset_isolation() {
    let server = common::TestServer::start_writable("").await;
    for name in ["dsA", "dsB"] {
        server
            .client
            .post(server.admin_datasets_url())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("dbName=/{name}&dbType=mem"))
            .send()
            .await
            .expect("create request failed");
        let turtle = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");
        let resp = server
            .client
            .put(server.dataset_data_default_url(name))
            .header("Content-Type", "text/turtle")
            .body(turtle)
            .send()
            .await
            .expect("put failed");
        assert!(resp.status().is_success());
    }

    let resp = server
        .client
        .post(server.dataset_rules_url("dsA"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 200);

    assert!(
        ask(
            &server,
            "dsA",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "dsA should have the derived fact"
    );
    assert!(
        !ask(
            &server,
            "dsB",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "dsB must be unaffected by dsA's ruleset"
    );
}

/// An empty-body `POST /{dataset}/rules` clears the dataset's ruleset:
/// derived facts disappear, base facts remain.
#[tokio::test]
async fn test_post_rules_empty_body_clears_ruleset() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/newds&dbType=mem")
        .send()
        .await
        .expect("create request failed");
    let turtle = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");
    server
        .client
        .put(server.dataset_data_default_url("newds"))
        .header("Content-Type", "text/turtle")
        .body(turtle)
        .send()
        .await
        .expect("put failed");
    server
        .client
        .post(server.dataset_rules_url("newds"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "precondition: ruleset should be active"
    );

    let resp = server
        .client
        .post(server.dataset_rules_url("newds"))
        .header("Content-Type", "text/x-datalog")
        .body("")
        .send()
        .await
        .expect("clear rules post failed");
    assert_eq!(resp.status(), 200);

    assert!(
        !ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "derived fact must be gone after clearing the ruleset"
    );
    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> }}")
        )
        .await,
        "base fact must survive clearing the ruleset"
    );
}

/// A syntactically-invalid Datalog body is rejected with 400, and the
/// dataset's existing ruleset (and its derived facts) are left untouched.
#[tokio::test]
async fn test_post_rules_parse_error_leaves_dataset_untouched() {
    let server = common::TestServer::start_writable("").await;
    server
        .client
        .post(server.admin_datasets_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("dbName=/newds&dbType=mem")
        .send()
        .await
        .expect("create request failed");
    let turtle = format!("<{EX_ALICE}> <{RDF_TYPE}> <{EX_MANAGER}> .");
    server
        .client
        .put(server.dataset_data_default_url("newds"))
        .header("Content-Type", "text/turtle")
        .body(turtle)
        .send()
        .await
        .expect("put failed");
    server
        .client
        .post(server.dataset_rules_url("newds"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");

    let resp = server
        .client
        .post(server.dataset_rules_url("newds"))
        .header("Content-Type", "text/x-datalog")
        .body("this is not valid datalog {{{ :- garbage")
        .send()
        .await
        .expect("bad rules post failed");
    assert_eq!(resp.status(), 400, "malformed body must be rejected");

    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "old ruleset's derived fact must survive a failed replace attempt"
    );
}

/// `POST /{missing}/rules` on a dataset that doesn't exist returns 404.
#[tokio::test]
async fn test_post_rules_nonexistent_dataset() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .post(server.dataset_rules_url("nope"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 404);
}

/// A read-only server rejects `POST /{dataset}/rules` with 403, same as
/// every other dataset-mutating route.
#[tokio::test]
async fn test_post_rules_read_only_server() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .post(server.dataset_rules_url("ds"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 403);
}

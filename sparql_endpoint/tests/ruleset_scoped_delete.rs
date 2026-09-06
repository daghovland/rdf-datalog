/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `POST`/`DELETE /{dataset}/rules/{ruleset-id}` —
//! named, independently-loadable/retractable rulesets within a dataset,
//! extending #390/#568's full-replace `POST /{dataset}/rules`.
//!
//! Related: [#473](https://github.com/daghovland/rdf-datalog/issues/473).
//! Design doc: `docs/plans/RULESET_SCOPED_DELETE_473_PLAN.md`.

mod common;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const EX_MANAGER: &str = "http://ex/Manager";
const EX_EMPLOYEE: &str = "http://ex/Employee";
const EX_CONTRACTOR: &str = "http://ex/Contractor";
const EX_ALICE: &str = "http://ex/Alice";

/// `?x rdf:type ex:Employee :- ?x rdf:type ex:Manager .`
const MANAGER_IMPLIES_EMPLOYEE_RULES: &str = r#"
PREFIX ex: <http://ex/>
ex:Employee[?x] :- ex:Manager[?x] .
"#;

/// `?x rdf:type ex:Contractor :- ?x rdf:type ex:Manager .`
const MANAGER_IMPLIES_CONTRACTOR_RULES: &str = r#"
PREFIX ex: <http://ex/>
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

/// Creates dataset `name`, PUTs `ex:Alice a ex:Manager .` into its default
/// graph. Panics on any request failure.
async fn setup_dataset_with_base_facts(server: &common::TestServer, name: &str) {
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
    assert!(resp.status().is_success(), "PUT failed: {}", resp.status());
}

/// `POST /{dataset}/rules/{id}` on a dataset with no prior reasoner
/// lazily creates one, exactly like the no-id path.
#[tokio::test]
async fn test_post_ruleset_id_new_dataset_creates_reasoner() {
    let server = common::TestServer::start_writable("").await;
    setup_dataset_with_base_facts(&server, "newds").await;

    let resp = server
        .client
        .post(server.dataset_rules_id_url("newds", "a"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 200, "POST /rules/a failed: {}", resp.status());

    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "ruleset 'a' should have derived Employee from Manager"
    );
}

/// `DELETE /{dataset}/rules/{id}` retracts only that named ruleset's
/// derivations, leaving a sibling named ruleset's derivations intact.
#[tokio::test]
async fn test_delete_ruleset_id_removes_only_that_rulesets_derivations() {
    let server = common::TestServer::start_writable("").await;
    setup_dataset_with_base_facts(&server, "newds").await;

    for (id, body) in [
        ("a", MANAGER_IMPLIES_EMPLOYEE_RULES),
        ("b", MANAGER_IMPLIES_CONTRACTOR_RULES),
    ] {
        let resp = server
            .client
            .post(server.dataset_rules_id_url("newds", id))
            .header("Content-Type", "text/x-datalog")
            .body(body)
            .send()
            .await
            .expect("rules post failed");
        assert_eq!(resp.status(), 200, "POST /rules/{id} failed");
    }
    assert!(ask(&server, "newds", &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")).await);
    assert!(ask(&server, "newds", &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_CONTRACTOR}> }}")).await);

    let resp = server
        .client
        .delete(server.dataset_rules_id_url("newds", "a"))
        .send()
        .await
        .expect("delete failed");
    assert_eq!(resp.status(), 200, "DELETE /rules/a failed: {}", resp.status());

    assert!(
        !ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "ruleset 'a's derived fact must be gone after deleting it"
    );
    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_CONTRACTOR}> }}")
        )
        .await,
        "sibling ruleset 'b' must be unaffected"
    );
}

/// A rule loaded under two different ruleset ids survives deletion of one
/// of them — the dedup/rule-identity requirement from the issue.
#[tokio::test]
async fn test_delete_ruleset_id_shared_rule_survives_if_other_ruleset_has_it() {
    let server = common::TestServer::start_writable("").await;
    setup_dataset_with_base_facts(&server, "newds").await;

    for id in ["a", "b"] {
        let resp = server
            .client
            .post(server.dataset_rules_id_url("newds", id))
            .header("Content-Type", "text/x-datalog")
            .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
            .send()
            .await
            .expect("rules post failed");
        assert_eq!(resp.status(), 200, "POST /rules/{id} failed");
    }

    let resp = server
        .client
        .delete(server.dataset_rules_id_url("newds", "a"))
        .send()
        .await
        .expect("delete failed");
    assert_eq!(resp.status(), 200);

    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "the shared rule's derivation must survive: ruleset 'b' still has it"
    );
}

/// `DELETE /{dataset}/rules/{id}` for an id that was never loaded is 404.
#[tokio::test]
async fn test_delete_nonexistent_ruleset_id_404() {
    let server = common::TestServer::start_writable("").await;
    setup_dataset_with_base_facts(&server, "newds").await;

    let resp = server
        .client
        .delete(server.dataset_rules_id_url("newds", "never-loaded"))
        .send()
        .await
        .expect("delete failed");
    assert_eq!(resp.status(), 404);
}

/// `DELETE /{missing}/rules/{id}` on a dataset that doesn't exist is 404.
#[tokio::test]
async fn test_delete_ruleset_id_nonexistent_dataset_404() {
    let server = common::TestServer::start_writable("").await;
    let resp = server
        .client
        .delete(server.dataset_rules_id_url("nope", "a"))
        .send()
        .await
        .expect("delete failed");
    assert_eq!(resp.status(), 404);
}

/// A read-only server rejects `DELETE /{dataset}/rules/{id}` with 403.
#[tokio::test]
async fn test_delete_ruleset_id_read_only_403() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .delete(server.dataset_rules_id_url("ds", "a"))
        .send()
        .await
        .expect("delete failed");
    assert_eq!(resp.status(), 403);
}

/// A second `POST /{dataset}/rules/{id}` with the *same* id replaces just
/// that id's rules, leaving a sibling id's ruleset untouched.
#[tokio::test]
async fn test_post_ruleset_id_replaces_only_that_id_leaving_others() {
    let server = common::TestServer::start_writable("").await;
    setup_dataset_with_base_facts(&server, "newds").await;

    for (id, body) in [
        ("a", MANAGER_IMPLIES_EMPLOYEE_RULES),
        ("b", MANAGER_IMPLIES_CONTRACTOR_RULES),
    ] {
        let resp = server
            .client
            .post(server.dataset_rules_id_url("newds", id))
            .header("Content-Type", "text/x-datalog")
            .body(body)
            .send()
            .await
            .expect("rules post failed");
        assert_eq!(resp.status(), 200);
    }

    // Replace 'a' with an empty ruleset (unload just 'a').
    let resp = server
        .client
        .post(server.dataset_rules_id_url("newds", "a"))
        .header("Content-Type", "text/x-datalog")
        .body("")
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 200);

    assert!(
        !ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_EMPLOYEE}> }}")
        )
        .await,
        "'a' was replaced with an empty ruleset, its derivation must be gone"
    );
    assert!(
        ask(
            &server,
            "newds",
            &format!("ASK {{ <{EX_ALICE}> <{RDF_TYPE}> <{EX_CONTRACTOR}> }}")
        )
        .await,
        "'b' must be untouched by replacing 'a'"
    );
}

/// The plain no-id `POST /{dataset}/rules` (full-replace, from #390/#568)
/// discards all named rulesets too — a subsequent `DELETE .../rules/{id}`
/// for a previously-loaded id is 404, since the full replace supersedes it.
#[tokio::test]
async fn test_plain_post_rules_clears_named_rulesets() {
    let server = common::TestServer::start_writable("").await;
    setup_dataset_with_base_facts(&server, "newds").await;

    let resp = server
        .client
        .post(server.dataset_rules_id_url("newds", "a"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_EMPLOYEE_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 200);

    let resp = server
        .client
        .post(server.dataset_rules_url("newds"))
        .header("Content-Type", "text/x-datalog")
        .body(MANAGER_IMPLIES_CONTRACTOR_RULES)
        .send()
        .await
        .expect("rules post failed");
    assert_eq!(resp.status(), 200);

    let resp = server
        .client
        .delete(server.dataset_rules_id_url("newds", "a"))
        .send()
        .await
        .expect("delete failed");
    assert_eq!(
        resp.status(),
        404,
        "'a' no longer exists as a distinct id after a plain full-replace POST"
    );
}

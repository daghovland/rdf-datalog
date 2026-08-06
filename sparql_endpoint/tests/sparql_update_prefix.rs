/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for SPARQL Update `PREFIX` prologue resolution (issue #392):
//! a `PREFIX` declared in the update request's own prologue must resolve
//! inside `INSERT DATA`/`DELETE DATA` and WHERE-form updates, matching what
//! `SELECT` already does.
//! https://github.com/daghovland/rdf-datalog/issues/392

mod common;

/// The exact repro from issue #392: a `PREFIX` declared in the update
/// request's own prologue must resolve inside `INSERT DATA`.
#[tokio::test]
async fn update_insert_data_resolves_prologue_prefix() {
    let server = common::TestServer::start_writable("").await;

    let update =
        r#"PREFIX ex: <http://example.com/ns/> INSERT DATA { <urn:testpkg> a ex:Thing . }"#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        200,
        "INSERT DATA with a prologue-declared prefix must succeed: {}",
        resp.text().await.unwrap_or_default()
    );

    let sparql = r#"ASK { <urn:testpkg> a <http://example.com/ns/Thing> }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], true,
        "INSERT DATA must have resolved ex:Thing to the declared IRI"
    );
}

/// Same gap for `DELETE DATA`: a prologue-declared prefix must resolve there
/// too, removing the intended triple.
#[tokio::test]
async fn update_delete_data_resolves_prologue_prefix() {
    let server = common::TestServer::start_writable(
        r#"<urn:testpkg> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.com/ns/Thing> ."#,
    )
    .await;

    let update =
        r#"PREFIX ex: <http://example.com/ns/> DELETE DATA { <urn:testpkg> a ex:Thing . }"#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        200,
        "DELETE DATA with a prologue-declared prefix must succeed: {}",
        resp.text().await.unwrap_or_default()
    );

    let sparql = r#"ASK { <urn:testpkg> a <http://example.com/ns/Thing> }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], false,
        "DELETE DATA must have resolved ex:Thing and removed the triple"
    );
}

/// SPARQL 1.1 Update prologue prefixes apply to the whole request, not just
/// the operation immediately following them: a prefix declared before the
/// first `;`-separated operation must still resolve inside a later operation
/// that doesn't redeclare it.
#[tokio::test]
async fn update_multi_op_prologue_prefix_carries_forward() {
    let server = common::TestServer::start_writable("").await;

    let update = r#"
        PREFIX ex: <http://example.com/ns/>
        INSERT DATA { <urn:a> a ex:Thing . } ;
        PREFIX ex2: <http://example.com/ns2/>
        INSERT DATA { <urn:b> a ex:Thing . <urn:c> a ex2:Other . }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        200,
        "multi-op update with carried-forward prefix must succeed: {}",
        resp.text().await.unwrap_or_default()
    );

    for (sparql, label) in [
        (
            r#"ASK { <urn:a> a <http://example.com/ns/Thing> }"#,
            "urn:a ex:Thing (first op)",
        ),
        (
            r#"ASK { <urn:b> a <http://example.com/ns/Thing> }"#,
            "urn:b ex:Thing (second op, ex: carried forward from first prologue)",
        ),
        (
            r#"ASK { <urn:c> a <http://example.com/ns2/Other> }"#,
            "urn:c ex2:Other (second op, ex2: declared there)",
        ),
    ] {
        let resp = server
            .client
            .get(server.sparql_query_url(sparql))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["boolean"], true, "expected triple missing: {label}");
    }
}

/// The same prologue-prefix gap affects WHERE-form updates
/// (`INSERT { template } WHERE { pattern }`), which build their own
/// synthetic SELECT query with a fresh, empty prefix map.
#[tokio::test]
async fn update_insert_where_resolves_prologue_prefix() {
    let server = common::TestServer::start_writable(
        r#"<http://example.com/ns/a> <http://example.com/ns/name> "Alice" ."#,
    )
    .await;

    let update = r#"
        PREFIX ex: <http://example.com/ns/>
        INSERT { ?s ex:label ?name }
        WHERE  { ?s ex:name ?name }
    "#;
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        200,
        "INSERT WHERE with a prologue-declared prefix must succeed: {}",
        resp.text().await.unwrap_or_default()
    );

    let sparql = r#"ASK { <http://example.com/ns/a> <http://example.com/ns/label> "Alice" }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(sparql))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], true,
        "INSERT WHERE must have resolved ex:label/ex:name via the prologue prefix"
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for `GRAPH <iri> { ... }` blocks inside SPARQL UPDATE
//! `INSERT DATA`/`DELETE DATA` (issue #404): the SPARQL 1.1 `QuadData`
//! grammar production used by `INSERT DATA`/`DELETE DATA` explicitly allows
//! `GRAPH` blocks (it's TriG-shaped, not plain-Turtle-shaped), but the
//! endpoint previously rejected them with a parse error
//! (`GRAPH is not a valid subject or graph name`) because
//! `parse_turtle_content` called `turtle::parse_turtle` (plain Turtle only)
//! instead of `turtle::parse_trig`.
//! https://github.com/daghovland/rdf-datalog/issues/404

mod common;

/// The exact repro from issue #404: `INSERT DATA { GRAPH <iri> { ... } }`
/// must succeed, and the triple must land in the named graph — not the
/// default graph.
#[tokio::test]
async fn insert_data_graph_block_lands_in_named_graph() {
    let server = common::TestServer::start_writable("").await;

    let update = r#"
        INSERT DATA {
          GRAPH <https://ssi.example.com/record/1> {
            <https://ssi.example.com/record/1> <https://rdf.equinor.com/ontology/record/replaces> <https://ssi.example.com/record/2> .
          }
        }
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
        "INSERT DATA with a GRAPH block must succeed: {}",
        resp.text().await.unwrap_or_default()
    );

    // The triple must be visible inside the named graph.
    let in_named_graph = r#"ASK {
        GRAPH <https://ssi.example.com/record/1> {
          <https://ssi.example.com/record/1> <https://rdf.equinor.com/ontology/record/replaces> <https://ssi.example.com/record/2>
        }
    }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(in_named_graph))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], true,
        "triple must have landed in the named graph <https://ssi.example.com/record/1>"
    );

    // ...and must NOT be visible in the default graph.
    let in_default_graph = r#"ASK {
        <https://ssi.example.com/record/1> <https://rdf.equinor.com/ontology/record/replaces> <https://ssi.example.com/record/2>
    }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(in_default_graph))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], false,
        "triple must NOT have been flattened into the default graph"
    );
}

/// `DELETE DATA { GRAPH <iri> { ... } }` must independently work too — it
/// shares `parse_turtle_content` with `INSERT DATA`, but is verified on its
/// own rather than assumed symmetric.
#[tokio::test]
async fn delete_data_graph_block_removes_from_named_graph() {
    // Seed the named graph directly via the Graph Store Protocol so this
    // test doesn't depend on INSERT DATA's own GRAPH-block support.
    let server = common::TestServer::start_writable("").await;

    let seed = r#"<https://ssi.example.com/record/1> <https://rdf.equinor.com/ontology/record/replaces> <https://ssi.example.com/record/2> ."#;
    let resp = server
        .client
        .put(server.gsp_named_graph_url("https://ssi.example.com/record/1"))
        .header("content-type", "text/turtle")
        .body(seed)
        .send()
        .await
        .expect("PUT seed graph failed");
    assert_eq!(resp.status(), 201, "seed PUT must succeed");

    let update = r#"
        DELETE DATA {
          GRAPH <https://ssi.example.com/record/1> {
            <https://ssi.example.com/record/1> <https://rdf.equinor.com/ontology/record/replaces> <https://ssi.example.com/record/2> .
          }
        }
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
        "DELETE DATA with a GRAPH block must succeed: {}",
        resp.text().await.unwrap_or_default()
    );

    let ask = r#"ASK {
        GRAPH <https://ssi.example.com/record/1> {
          <https://ssi.example.com/record/1> <https://rdf.equinor.com/ontology/record/replaces> <https://ssi.example.com/record/2>
        }
    }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(ask))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], false,
        "DELETE DATA must have removed the triple from the named graph"
    );
}

/// Regression: plain (no `GRAPH` block) `INSERT DATA` must still land in the
/// default graph, unaffected by the switch to `parse_trig`.
#[tokio::test]
async fn insert_data_without_graph_block_still_targets_default_graph() {
    let server = common::TestServer::start_writable("").await;

    let update = r#"INSERT DATA { <urn:a> <urn:p> <urn:b> . }"#;
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
        "plain INSERT DATA must still succeed: {}",
        resp.text().await.unwrap_or_default()
    );

    let ask = r#"ASK { <urn:a> <urn:p> <urn:b> }"#;
    let resp = server
        .client
        .get(server.sparql_query_url(ask))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["boolean"], true,
        "plain INSERT DATA must still land in the default graph"
    );
}

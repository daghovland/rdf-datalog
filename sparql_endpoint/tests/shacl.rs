/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SHACL HTTP endpoint integration tests.
//!
//! Verifies `POST /{dataset}/shacl` (Fuseki-compatible SHACL validation endpoint).
//!
//! Spec: <https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html>
//! SHACL: <https://www.w3.org/TR/shacl/>

mod common;

const DS: &str = "ds";

/// Data for the conforming case: Alice has an integer age.
const DATA_CONFORMS: &str = r#"
@prefix ex: <http://example.org/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
ex:Alice ex:age "23"^^xsd:integer .
"#;

/// Data for the non-conforming case: Bob has a plain-string age (wrong datatype).
const DATA_VIOLATES: &str = r#"
@prefix ex: <http://example.org/> .
ex:Bob ex:age "twenty-two" .
"#;

/// Shapes requiring `ex:age` to be `xsd:integer` on `ex:Alice` / `ex:Bob`.
fn shapes_datatype(target: &str) -> String {
    format!(
        r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix ex: <http://example.org/> .
ex:AgeShape
    a sh:NodeShape ;
    sh:targetNode {target} ;
    sh:property [
        sh:path ex:age ;
        sh:datatype xsd:integer ;
    ] .
"#
    )
}

/// `POST /{ds}/shacl` with conforming data returns a report with `sh:conforms true`.
///
/// Spec: <https://www.w3.org/TR/shacl/#validation-report>
#[tokio::test]
async fn shacl_post_conforms() {
    let server = common::TestServer::start(DATA_CONFORMS).await;
    let shapes = shapes_datatype("ex:Alice");
    let resp = server
        .client
        .post(format!("{}/{DS}/shacl", server.base_url))
        .header("content-type", "text/turtle")
        .body(shapes)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200, "expected 200 OK");
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("text/turtle"),
        "expected text/turtle response; got {ct}"
    );
    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("true"),
        "sh:conforms true expected in body; got:\n{body}"
    );
    assert!(
        !body.contains("false"),
        "sh:conforms false must not appear when data conforms; got:\n{body}"
    );
}

/// `POST /{ds}/shacl` with violating data returns a report with `sh:conforms false`
/// and at least one `sh:result`.
///
/// Spec: <https://www.w3.org/TR/shacl/#validation-report>
#[tokio::test]
async fn shacl_post_violation() {
    let server = common::TestServer::start(DATA_VIOLATES).await;
    let shapes = shapes_datatype("ex:Bob");
    let resp = server
        .client
        .post(format!("{}/{DS}/shacl", server.base_url))
        .header("content-type", "text/turtle")
        .body(shapes)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200, "expected 200 OK");
    let ct = resp.headers()["content-type"].to_str().unwrap_or("");
    assert!(
        ct.contains("text/turtle"),
        "expected text/turtle response; got {ct}"
    );
    let body = resp.text().await.expect("body must be text");
    assert!(
        body.contains("false"),
        "sh:conforms false expected; got:\n{body}"
    );
    assert!(
        body.contains("sh:result") || body.contains("ValidationResult"),
        "sh:result block expected; got:\n{body}"
    );
}

/// `POST /{ds}/shacl` with a non-existent dataset returns 404.
#[tokio::test]
async fn shacl_post_unknown_dataset() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .post(format!("{}/noSuchDataset/shacl", server.base_url))
        .header("content-type", "text/turtle")
        .body(shapes_datatype("ex:Alice"))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 404, "unknown dataset must return 404");
}

/// `POST /{ds}/shacl` with a malformed Turtle body returns 400.
#[tokio::test]
async fn shacl_post_bad_shapes() {
    let server = common::TestServer::start("").await;
    let resp = server
        .client
        .post(format!("{}/{DS}/shacl", server.base_url))
        .header("content-type", "text/turtle")
        .body("this is not turtle $$$$")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 400, "malformed shapes must return 400");
}

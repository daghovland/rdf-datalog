/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for the generated OpenAPI spec and interactive Swagger UI page.
//!
//! See [`docs/plans/OPENAPI_FRONTEND_386_PLAN.md`](../../docs/plans/OPENAPI_FRONTEND_386_PLAN.md)
//! and [#386](https://github.com/daghovland/rdf-datalog/issues/386).

mod common;

const TURTLE: &str = r#"
    <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> "Alice" .
"#;

/// `GET /api-docs/openapi.json` returns 200 with a JSON body that parses as a
/// valid OpenAPI 3 document and lists the core SPARQL/GSP/admin routes.
#[tokio::test]
async fn openapi_json_is_valid_and_lists_core_routes() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/api-docs/openapi.json", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap().to_owned();
    assert!(
        ct.contains("application/json"),
        "expected JSON content-type, got: {ct}"
    );

    let body: serde_json::Value = resp.json().await.expect("body must be valid JSON");

    assert!(
        body.get("openapi").is_some(),
        "missing `openapi` version field: {body}"
    );
    assert!(body.get("info").is_some(), "missing `info` field: {body}");

    let paths = body.get("paths").expect("missing `paths` field");
    for expected in [
        "/sparql",
        "/rdf-graph-store",
        "/rdf-graphs/{path}",
        "/{name}/sparql",
        "/{name}/update",
        "/{name}/data",
        "/$/ping",
        "/$/datasets",
        "/auth/config",
    ] {
        assert!(
            paths.get(expected).is_some(),
            "expected path `{expected}` to be documented, got paths: {paths}"
        );
    }

    // `/sparql` must document both GET and POST per SPARQL 1.1 Protocol.
    let sparql_path = &paths["/sparql"];
    assert!(sparql_path.get("get").is_some(), "missing GET /sparql");
    assert!(sparql_path.get("post").is_some(), "missing POST /sparql");
}

/// `GET /swagger-ui/` returns 200 HTML that references the openapi.json spec.
#[tokio::test]
async fn swagger_ui_serves_html_referencing_spec() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/swagger-ui/", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let ct = resp.headers()["content-type"].to_str().unwrap().to_owned();
    assert!(ct.contains("text/html"), "expected HTML, got: {ct}");

    let body = resp.text().await.expect("body must be text");
    assert!(
        body.to_lowercase().contains("swagger"),
        "swagger UI page should mention 'swagger', got: {body}"
    );
}

/// `GET /swagger-ui` (no trailing slash) redirects to `/swagger-ui/`.
#[tokio::test]
async fn swagger_ui_redirects_without_trailing_slash() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/swagger-ui", server.base_url))
        .send()
        .await
        .expect("request failed");

    // reqwest follows redirects by default, so the final status is 200 and
    // the final URL should have the trailing slash.
    assert_eq!(resp.status(), 200);
    assert!(
        resp.url().as_str().ends_with("/swagger-ui/"),
        "expected redirect to /swagger-ui/, ended at: {}",
        resp.url()
    );
}

/// `GET /api-docs/openapi.json` also documents the routes deferred by the
/// first pass (#386): SHACL, RML, OTTR, the runtime-ruleset endpoint, and
/// the proprietary transaction API. See
/// [`docs/plans/OPENAPI_FRONTEND_386_PLAN.md`](../../docs/plans/OPENAPI_FRONTEND_386_PLAN.md)
/// and [#517](https://github.com/daghovland/rdf-datalog/issues/517).
#[tokio::test]
async fn openapi_json_lists_deferred_386_routes() {
    let server = common::TestServer::start(TURTLE).await;

    let resp = server
        .client
        .get(format!("{}/api-docs/openapi.json", server.base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be valid JSON");
    let paths = body.get("paths").expect("missing `paths` field");

    for expected in [
        "/{name}/shacl",
        "/{name}/rml",
        "/rml/map",
        "/{name}/ottr",
        "/{name}/rules",
        "/{name}/rules/{ruleset_id}",
        "/transaction/begin",
        "/transaction/{txId}/commit",
        "/transaction/{txId}/rollback",
    ] {
        assert!(
            paths.get(expected).is_some(),
            "expected path `{expected}` to be documented, got paths: {paths}"
        );
    }

    assert!(
        paths["/{name}/shacl"].get("post").is_some(),
        "missing POST /{{name}}/shacl"
    );
    assert!(
        paths["/{name}/rml"].get("post").is_some(),
        "missing POST /{{name}}/rml"
    );
    assert!(
        paths["/rml/map"].get("post").is_some(),
        "missing POST /rml/map"
    );
    assert!(
        paths["/{name}/ottr"].get("post").is_some(),
        "missing POST /{{name}}/ottr"
    );
    assert!(
        paths["/{name}/rules"].get("post").is_some(),
        "missing POST /{{name}}/rules"
    );
    assert!(
        paths["/{name}/rules/{ruleset_id}"].get("post").is_some(),
        "missing POST /{{name}}/rules/{{ruleset_id}}"
    );
    assert!(
        paths["/{name}/rules/{ruleset_id}"].get("delete").is_some(),
        "missing DELETE /{{name}}/rules/{{ruleset_id}}"
    );
    assert!(
        paths["/transaction/begin"].get("post").is_some(),
        "missing POST /transaction/begin"
    );
    assert!(
        paths["/transaction/{txId}/commit"].get("post").is_some(),
        "missing POST /transaction/{{txId}}/commit"
    );
    assert!(
        paths["/transaction/{txId}/rollback"].get("post").is_some(),
        "missing POST /transaction/{{txId}}/rollback"
    );
}

/// Under `ApiKey { require_for_reads: true, .. }`, the docs routes require
/// the same Bearer token as every other GET (read) route — no special-cased
/// public exemption, consistent with the existing `/` frontend's treatment.
#[tokio::test]
async fn openapi_json_requires_auth_when_reads_are_protected() {
    let server =
        common::TestServer::start_writable_with_key_protect_reads(TURTLE, "secret-key").await;

    let unauthenticated = server
        .client
        .get(format!("{}/api-docs/openapi.json", server.base_url))
        .send()
        .await
        .expect("request failed");
    assert_eq!(unauthenticated.status(), 401);

    let authenticated = server
        .client
        .get(format!("{}/api-docs/openapi.json", server.base_url))
        .bearer_auth("secret-key")
        .send()
        .await
        .expect("request failed");
    assert_eq!(authenticated.status(), 200);
}

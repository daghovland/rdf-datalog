/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Generated OpenAPI 3 documentation for the REST/HTTP API surface, served as
//! interactive Swagger UI. Aimed at developers integrating over HTTP, distinct
//! from the `/` query-builder frontend which targets interactive end users.
//!
//! The spec is built programmatically via [`utoipa::openapi::OpenApiBuilder`]
//! rather than by annotating existing handlers with `#[utoipa::path(...)]`
//! macros: the handlers in this crate take ad-hoc extractors (raw string/byte
//! bodies, content-negotiated multi-format responses) built up across many
//! files, so macro-annotating all of them would touch every handler signature
//! for a purely additive documentation feature. See
//! [`docs/plans/OPENAPI_FRONTEND_386_PLAN.md`](../../../docs/plans/OPENAPI_FRONTEND_386_PLAN.md)
//! and [#386](https://github.com/daghovland/rdf-datalog/issues/386).
//!
//! Route coverage in this first pass: the SPARQL 1.1 Protocol, Graph Store
//! HTTP Protocol, and admin (`/$/...`) routes, which have stable,
//! mostly-textual request/response shapes. SHACL, RML, OTTR, the runtime
//! ruleset endpoint, and the proprietary transaction API are deferred — see
//! [#517](https://github.com/daghovland/rdf-datalog/issues/517).
//!
//! Auth treatment: both `GET /api-docs/openapi.json` and `GET /swagger-ui/`
//! are plain `GET` routes with no special-casing in `auth::classify()`, so
//! they get `Permission::Read` exactly like the existing `/` frontend —
//! public by default, gated the same way as every other read route when
//! `require_for_reads`/OIDC roles are configured.

use utoipa::openapi::{
    ContentBuilder, Info, ObjectBuilder, OpenApi, OpenApiBuilder, Paths, PathsBuilder, RefOr,
    Required, Schema,
    path::{
        HttpMethod, Operation, OperationBuilder, Parameter, ParameterBuilder, ParameterIn, PathItem,
    },
    request_body::RequestBodyBuilder,
    response::ResponseBuilder,
    schema::{SchemaType, Type},
};

/// A `text/plain` request body (used for SPARQL Update, SPARQL query bodies
/// posted as `application/sparql-query`, and raw RDF graph uploads).
fn text_request_body(
    description: &str,
    content_type: &str,
) -> utoipa::openapi::request_body::RequestBody {
    RequestBodyBuilder::new()
        .description(Some(description.to_owned()))
        .content(
            content_type,
            ContentBuilder::new().schema(Some(string_schema())).build(),
        )
        .required(Some(Required::True))
        .build()
}

fn string_schema() -> RefOr<Schema> {
    RefOr::T(Schema::Object(
        ObjectBuilder::new()
            .schema_type(SchemaType::Type(Type::String))
            .build(),
    ))
}

fn query_param(name: &str, description: &str, required: bool) -> Parameter {
    ParameterBuilder::new()
        .name(name.to_owned())
        .parameter_in(ParameterIn::Query)
        .description(Some(description.to_owned()))
        .required(if required {
            Required::True
        } else {
            Required::False
        })
        .schema(Some(string_schema()))
        .build()
}

fn path_param(name: &str, description: &str) -> Parameter {
    ParameterBuilder::new()
        .name(name.to_owned())
        .parameter_in(ParameterIn::Path)
        .description(Some(description.to_owned()))
        .required(Required::True)
        .schema(Some(string_schema()))
        .build()
}

fn ok_json_response(description: &str) -> utoipa::openapi::response::Response {
    ResponseBuilder::new()
        .description(description.to_owned())
        .content(
            "application/sparql-results+json",
            ContentBuilder::new().schema(Some(string_schema())).build(),
        )
        .build()
}

fn ok_text_response(description: &str) -> utoipa::openapi::response::Response {
    ResponseBuilder::new()
        .description(description.to_owned())
        .content(
            "text/plain",
            ContentBuilder::new().schema(Some(string_schema())).build(),
        )
        .build()
}

fn ok_rdf_response(description: &str) -> utoipa::openapi::response::Response {
    ResponseBuilder::new()
        .description(description.to_owned())
        .content(
            "text/turtle",
            ContentBuilder::new().schema(Some(string_schema())).build(),
        )
        .build()
}

fn no_content_response(description: &str) -> utoipa::openapi::response::Response {
    ResponseBuilder::new()
        .description(description.to_owned())
        .build()
}

fn op(summary: &str, description: &str, tag: &str) -> OperationBuilder {
    OperationBuilder::new()
        .summary(Some(summary.to_owned()))
        .description(Some(description.to_owned()))
        .tag(tag.to_owned())
}

fn item1(method: HttpMethod, operation: Operation) -> PathItem {
    PathItem::new(method, operation)
}

fn merge(mut item: PathItem, method: HttpMethod, operation: Operation) -> PathItem {
    let extra = PathItem::new(method, operation);
    item.merge_operations(extra);
    item
}

/// Build the SPARQL 1.1 Protocol query operations (shared by `/sparql` and
/// `/{name}/sparql` / `/{name}/query`).
fn sparql_query_path_item(dataset_scoped: bool) -> PathItem {
    let mut params = vec![query_param(
        "query",
        "The SPARQL query string (SPARQL 1.1 Protocol §2.1.1/§2.1.2).",
        false,
    )];
    if dataset_scoped {
        params.insert(0, path_param("name", "Dataset name."));
    }

    let get = op(
        "Execute a SPARQL query (GET)",
        "SPARQL 1.1 Protocol query operation via URL-encoded query parameter.",
        "SPARQL Query",
    )
    .parameters(Some(params.clone()))
    .response(
        "200",
        ok_json_response("Query results (format per Accept header)."),
    )
    .response("400", no_content_response("Malformed query."))
    .response("500", no_content_response("Query execution error."))
    .response("503", no_content_response("Query timed out."))
    .build();

    let post = op(
        "Execute a SPARQL query (POST)",
        "SPARQL 1.1 Protocol query operation. Accepts `application/x-www-form-urlencoded` \
         (`query=...`) or a raw `application/sparql-query` body.",
        "SPARQL Query",
    )
    .parameters(Some(params))
    .request_body(Some(text_request_body(
        "Raw SPARQL query text.",
        "application/sparql-query",
    )))
    .response(
        "200",
        ok_json_response("Query results (format per Accept header)."),
    )
    .response("400", no_content_response("Malformed query."))
    .response("500", no_content_response("Query execution error."))
    .response("503", no_content_response("Query timed out."))
    .build();

    merge(item1(HttpMethod::Get, get), HttpMethod::Post, post)
}

fn sparql_update_path_item() -> PathItem {
    let post = op(
        "Execute a SPARQL Update",
        "SPARQL 1.1 Protocol update operation (INSERT/DELETE DATA, INSERT/DELETE WHERE, LOAD, CLEAR, ...).",
        "SPARQL Update",
    )
    .parameter(path_param("name", "Dataset name."))
    .request_body(Some(text_request_body(
        "Raw SPARQL Update text.",
        "application/sparql-update",
    )))
    .response("204", no_content_response("Update applied."))
    .response("400", no_content_response("Malformed update."))
    .response("403", no_content_response("Server is read-only or write access denied."))
    .build();
    item1(HttpMethod::Post, post)
}

/// Graph Store HTTP Protocol operations, shared by `/rdf-graph-store` (uses
/// `?graph=`/`?default` query params) and `/rdf-graphs/{path}` (direct graph
/// identification by path segment).
fn gsp_path_item(direct: bool) -> PathItem {
    let mut params = vec![
        query_param("graph", "Named graph IRI to operate on.", false),
        query_param("default", "Operate on the default graph.", false),
    ];
    if direct {
        params = vec![path_param("path", "Graph IRI path segment.")];
    }

    let get = op(
        "Fetch a graph",
        "Graph Store HTTP Protocol GET: returns the serialized RDF graph.",
        "Graph Store Protocol",
    )
    .parameters(Some(params.clone()))
    .response(
        "200",
        ok_rdf_response("Graph contents (format per Accept header)."),
    )
    .response("404", no_content_response("Graph does not exist."))
    .build();

    let put = op(
        "Replace a graph",
        "Graph Store HTTP Protocol PUT: replaces the graph's contents with the request body.",
        "Graph Store Protocol",
    )
    .parameters(Some(params.clone()))
    .request_body(Some(text_request_body(
        "RDF graph (Turtle/TriG/JSON-LD/N-Quads).",
        "text/turtle",
    )))
    .response("201", no_content_response("Graph created."))
    .response("204", no_content_response("Graph replaced."))
    .build();

    let post = op(
        "Append to a graph",
        "Graph Store HTTP Protocol POST: merges the request body into the graph's existing contents.",
        "Graph Store Protocol",
    )
    .parameters(Some(params.clone()))
    .request_body(Some(text_request_body("RDF graph (Turtle/TriG/JSON-LD/N-Quads).", "text/turtle")))
    .response("200", no_content_response("Graph updated."))
    .response("201", no_content_response("Graph created."))
    .build();

    let delete = op(
        "Delete a graph",
        "Graph Store HTTP Protocol DELETE: removes the graph entirely.",
        "Graph Store Protocol",
    )
    .parameters(Some(params))
    .response("200", no_content_response("Graph deleted."))
    .response("404", no_content_response("Graph does not exist."))
    .build();

    let item = item1(HttpMethod::Get, get);
    let item = merge(item, HttpMethod::Put, put);
    let item = merge(item, HttpMethod::Post, post);
    merge(item, HttpMethod::Delete, delete)
}

fn dataset_data_path_item() -> PathItem {
    let mut item = gsp_path_item(false);
    item.get = item.get.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, path_param("name", "Dataset name."));
        o
    });
    item.put = item.put.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, path_param("name", "Dataset name."));
        o
    });
    item.post = item.post.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, path_param("name", "Dataset name."));
        o
    });
    item.delete = item.delete.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, path_param("name", "Dataset name."));
        o
    });
    item
}

fn admin_datasets_path_item() -> PathItem {
    let get = op(
        "List datasets",
        "Fuseki-compatible admin API: list all datasets known to this server.",
        "Admin",
    )
    .response(
        "200",
        ok_json_response("JSON array of dataset descriptions."),
    )
    .build();

    let post = op(
        "Create a dataset",
        "Fuseki-compatible admin API: create a new named dataset.",
        "Admin",
    )
    .request_body(Some(text_request_body(
        "Dataset creation parameters (form-encoded `dbName`/`dbType`, or Fuseki assembler Turtle).",
        "application/x-www-form-urlencoded",
    )))
    .response("200", no_content_response("Dataset created."))
    .response("400", no_content_response("Malformed request."))
    .build();

    merge(item1(HttpMethod::Get, get), HttpMethod::Post, post)
}

fn admin_dataset_by_name_path_item() -> PathItem {
    let get = op(
        "Get dataset info",
        "Fuseki-compatible admin API: describe a single dataset.",
        "Admin",
    )
    .parameter(path_param("name", "Dataset name."))
    .response("200", ok_json_response("Dataset description."))
    .response("404", no_content_response("Dataset does not exist."))
    .build();

    let delete = op(
        "Delete a dataset",
        "Fuseki-compatible admin API: delete a named dataset.",
        "Admin",
    )
    .parameter(path_param("name", "Dataset name."))
    .response("200", no_content_response("Dataset deleted."))
    .response("404", no_content_response("Dataset does not exist."))
    .build();

    merge(item1(HttpMethod::Get, get), HttpMethod::Delete, delete)
}

fn simple_get(summary: &str, description: &str, tag: &str) -> PathItem {
    let get = op(summary, description, tag)
        .response("200", ok_json_response("Success."))
        .build();
    item1(HttpMethod::Get, get)
}

fn simple_post(summary: &str, description: &str, tag: &str) -> PathItem {
    let post = op(summary, description, tag)
        .response("200", ok_text_response("Success."))
        .build();
    item1(HttpMethod::Post, post)
}

/// Build the full OpenAPI document for this crate's public HTTP surface.
pub fn build_openapi() -> OpenApi {
    let paths: Paths = PathsBuilder::new()
        .path("/sparql", sparql_query_path_item(false))
        .path("/{name}/sparql", sparql_query_path_item(true))
        .path("/{name}/query", sparql_query_path_item(true))
        .path("/{name}/update", sparql_update_path_item())
        .path("/rdf-graph-store", gsp_path_item(false))
        .path("/rdf-graphs/{path}", gsp_path_item(true))
        .path("/{name}/data", dataset_data_path_item())
        .path(
            "/$/ping",
            simple_get("Liveness/readiness ping", "Fuseki-compatible `/$/ping`.", "Admin"),
        )
        .path(
            "/$/server",
            simple_get(
                "Server status",
                "Fuseki-compatible admin API: server-wide status information.",
                "Admin",
            ),
        )
        .path("/$/datasets", admin_datasets_path_item())
        .path("/$/datasets/{name}", admin_dataset_by_name_path_item())
        .path(
            "/$/compact",
            simple_post(
                "Compact the changelog",
                "Fuseki-compatible admin API: compact the durable changelog (no-op in-memory).",
                "Admin",
            ),
        )
        .path(
            "/auth/config",
            simple_get(
                "Authentication configuration",
                "Returns the active authentication mode and non-secret configuration. Always public.",
                "Auth",
            ),
        )
        .path(
            "/void",
            simple_get("VoID dataset description", "RDF VoID (Vocabulary of Interlinked Datasets) description of this server's datasets.", "Discovery"),
        )
        .build();

    OpenApiBuilder::new()
        .info(Info::new(
            "Dagalog SPARQL Endpoint",
            env!("CARGO_PKG_VERSION"),
        ))
        .paths(paths)
        .build()
}

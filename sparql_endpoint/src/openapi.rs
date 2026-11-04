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
//! Route coverage: the SPARQL 1.1 Protocol, Graph Store HTTP Protocol, and
//! admin (`/$/...`) routes landed first, since they have stable,
//! mostly-textual request/response shapes — see
//! [#386](https://github.com/daghovland/rdf-datalog/issues/386). SHACL
//! validation, RML/OTTR mapping, the runtime-ruleset endpoint, and the
//! proprietary transaction API were deferred from that first pass because
//! their request bodies are multipart, mapping-language-specific, or
//! otherwise awkward to express cleanly in a first-pass OpenAPI schema; they
//! are covered below, following the same hand-built pattern — see
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

/// A plain `application/json` response (used by handlers that return axum
/// `Json<...>`, as opposed to the SPARQL-results-shaped JSON of
/// [`ok_json_response`]).
fn ok_plain_json_response(description: &str) -> utoipa::openapi::response::Response {
    ResponseBuilder::new()
        .description(description.to_owned())
        .content(
            "application/json",
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

/// A `multipart/form-data` request body (used for RML and OTTR mapping
/// uploads, each of which accepts multiple named parts).
fn multipart_request_body(description: &str) -> utoipa::openapi::request_body::RequestBody {
    RequestBodyBuilder::new()
        .description(Some(description.to_owned()))
        .content(
            "multipart/form-data",
            ContentBuilder::new().schema(Some(string_schema())).build(),
        )
        .required(Some(Required::True))
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
    let mut params = vec![
        query_param(
            "query",
            "The SPARQL query string (SPARQL 1.1 Protocol §2.1.1/§2.1.2).",
            false,
        ),
        query_param(
            "explain",
            "If `true`, return a JSON EXPLAIN report (query plan, join/component \
             order, per-pattern index/cardinality estimate, total timing) instead \
             of query results. Not a SPARQL 1.1 Protocol parameter; bypasses \
             result content negotiation. Not yet supported together with `txId`.",
            false,
        ),
    ];
    // `txId` (proprietary transaction API, #125) is only honored by the
    // root `/sparql` handler (`query::sparql_get`/`sparql_post_with_state`);
    // the dataset-scoped `/{name}/sparql`/`/{name}/query` handlers in
    // `dataset_routes.rs` don't look at it.
    if !dataset_scoped {
        params.push(query_param(
            "txId",
            "Execute against the open transaction's pending delta (proprietary \
             transaction API; see `POST /transaction/begin`). Not yet supported \
             together with `explain`.",
            false,
        ));
    }
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

    // The root `/sparql` POST handler (`query::sparql_post_with_state`) also
    // accepts a raw `application/sparql-update` body (or `update=...` form
    // field) and dispatches it as a SPARQL Update — this is the only way to
    // write within a transaction (`?txId=...`), since the dataset-scoped
    // `/{name}/update` route doesn't honor `txId` (see `sparql_query_path_item`
    // above). The dataset-scoped `/{name}/sparql`/`/{name}/query` routes don't
    // have this dual behavior, so only document it for the root path.
    let post_body = if dataset_scoped {
        text_request_body("Raw SPARQL query text.", "application/sparql-query")
    } else {
        RequestBodyBuilder::new()
            .description(Some(
                "Raw SPARQL query text, or (to write within a transaction via \
                 `?txId=...`) a raw SPARQL Update body."
                    .to_owned(),
            ))
            .content(
                "application/sparql-query",
                ContentBuilder::new().schema(Some(string_schema())).build(),
            )
            .content(
                "application/sparql-update",
                ContentBuilder::new().schema(Some(string_schema())).build(),
            )
            .required(Some(Required::True))
            .build()
    };

    let post = op(
        "Execute a SPARQL query (POST)",
        "SPARQL 1.1 Protocol query operation. Accepts `application/x-www-form-urlencoded` \
         (`query=...`) or a raw `application/sparql-query` body.",
        "SPARQL Query",
    )
    .parameters(Some(params))
    .request_body(Some(post_body))
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

    let head = op(
        "Check whether a graph exists",
        "Graph Store HTTP Protocol HEAD: same as GET but returns no body, \
         for existence checks.",
        "Graph Store Protocol",
    )
    .parameters(Some(params.clone()))
    .response("200", no_content_response("Graph exists."))
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
    let item = merge(item, HttpMethod::Head, head);
    let item = merge(item, HttpMethod::Put, put);
    let item = merge(item, HttpMethod::Post, post);
    merge(item, HttpMethod::Delete, delete)
}

fn dataset_data_path_item() -> PathItem {
    let mut item = gsp_path_item(false);
    item.get = item.get.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, RefOr::T(path_param("name", "Dataset name.")));
        o
    });
    item.head = item.head.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, RefOr::T(path_param("name", "Dataset name.")));
        o
    });
    item.put = item.put.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, RefOr::T(path_param("name", "Dataset name.")));
        o
    });
    item.post = item.post.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, RefOr::T(path_param("name", "Dataset name.")));
        o
    });
    item.delete = item.delete.map(|mut o| {
        o.parameters
            .get_or_insert_with(Vec::new)
            .insert(0, RefOr::T(path_param("name", "Dataset name.")));
        o
    });
    item
}

/// `GET/HEAD /{name}/get` — read-only alias for `/{name}/data` (no
/// PUT/POST/DELETE). See [`crate::dataset_routes::dataset_data_get`] /
/// [`crate::dataset_routes::dataset_data_head`].
fn dataset_get_path_item() -> PathItem {
    let params = vec![path_param("name", "Dataset name.")];

    let get = op(
        "Fetch the dataset's default graph (read-only alias)",
        "Read-only alias for `GET /{name}/data` with no `?graph=`/`?default` \
         query parameters — always returns the dataset's default graph.",
        "Graph Store Protocol",
    )
    .parameters(Some(params.clone()))
    .response(
        "200",
        ok_rdf_response("Graph contents (format per Accept header)."),
    )
    .response("404", no_content_response("Dataset does not exist."))
    .build();

    let head = op(
        "Check whether the dataset's default graph exists (read-only alias)",
        "Read-only alias for `HEAD /{name}/data`: same as GET but returns no body.",
        "Graph Store Protocol",
    )
    .parameters(Some(params))
    .response("200", no_content_response("Dataset exists."))
    .response("404", no_content_response("Dataset does not exist."))
    .build();

    merge(item1(HttpMethod::Get, get), HttpMethod::Head, head)
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

/// `GET/POST /$/ready` — readiness check (#414), deliberately reusing
/// `admin::admin_ping` for both methods (same as `/$/ping`). See the doc
/// comment on [`crate::admin::admin_ping`] for why GET and POST, and
/// readiness vs. liveness, coincide here.
fn admin_ready_path_item() -> PathItem {
    let summary = "Readiness check";
    let description =
        "Readiness check (#414); currently equivalent to `/$/ping` (GET and POST both handled).";
    let get = op(summary, description, "Admin")
        .response("200", ok_json_response("Success."))
        .build();
    let post = op(summary, description, "Admin")
        .response("200", ok_text_response("Success."))
        .build();
    merge(item1(HttpMethod::Get, get), HttpMethod::Post, post)
}

/// `POST /{name}/shacl` — SHACL validation. See
/// [`crate::shacl_endpoint::dataset_shacl_post`].
fn shacl_path_item() -> PathItem {
    let post = op(
        "Validate a dataset against SHACL shapes",
        "Fuseki-compatible SHACL validation endpoint. Validates the dataset's \
         default-graph-union against the shapes graph in the request body and \
         returns a SHACL validation report graph.",
        "SHACL",
    )
    .parameter(path_param("name", "Dataset name."))
    .parameter(query_param(
        "graph",
        "Graph selector. Only `union` (the default) is currently supported.",
        false,
    ))
    .request_body(Some(text_request_body(
        "SHACL shapes graph (Turtle).",
        "text/turtle",
    )))
    .response(
        "200",
        ok_rdf_response("SHACL validation report graph (Turtle)."),
    )
    .response("400", no_content_response("Malformed shapes graph."))
    .response("404", no_content_response("Dataset does not exist."))
    .build();
    item1(HttpMethod::Post, post)
}

/// `POST /{name}/rml` — apply an RML mapping into a named dataset. See
/// [`crate::rml_endpoint::dataset_rml_post`].
fn rml_dataset_path_item() -> PathItem {
    let post = op(
        "Apply an RML mapping to a dataset",
        "Applies an RML mapping (a `mapping` part plus zero or more named \
         source-file parts, uploaded as `multipart/form-data`) and inserts the \
         generated RDF into the named dataset.",
        "RML",
    )
    .parameter(path_param("name", "Dataset name."))
    .request_body(Some(multipart_request_body(
        "Multipart body: a `mapping` part (RML mapping Turtle) plus named source-file parts.",
    )))
    .response(
        "200",
        ok_text_response("Mapping applied; number of triples inserted."),
    )
    .response(
        "400",
        no_content_response("Malformed mapping or multipart body."),
    )
    .response("403", no_content_response("Server is read-only."))
    .response("404", no_content_response("Dataset does not exist."))
    .response(
        "413",
        no_content_response("Upload exceeds the configured size limit."),
    )
    .build();
    item1(HttpMethod::Post, post)
}

/// `POST /rml/map` — apply an RML mapping statelessly, returning the
/// generated RDF directly. See [`crate::rml_endpoint::rml_map_post`].
fn rml_map_path_item() -> PathItem {
    let post = op(
        "Apply an RML mapping and return the resulting RDF",
        "Applies an RML mapping (same multipart shape as `POST /{name}/rml`) \
         without touching any dataset, and returns the generated RDF graph \
         (format per Accept header).",
        "RML",
    )
    .request_body(Some(multipart_request_body(
        "Multipart body: a `mapping` part (RML mapping Turtle) plus named source-file parts.",
    )))
    .response(
        "200",
        ok_rdf_response("Generated RDF graph (format per Accept header)."),
    )
    .response(
        "400",
        no_content_response("Malformed mapping or multipart body."),
    )
    .response(
        "413",
        no_content_response("Upload exceeds the configured size limit."),
    )
    .build();
    item1(HttpMethod::Post, post)
}

/// `POST /{name}/ottr` — expand stOTTR/wOTTR documents into a named
/// dataset. See [`crate::ottr_endpoint::dataset_ottr_post`].
fn ottr_path_item() -> PathItem {
    let post = op(
        "Expand OTTR templates into a dataset",
        "Expands stOTTR (default) or wOTTR (Turtle/TriG `Content-Type` per \
         part) template/instance documents, uploaded as `multipart/form-data`, \
         and inserts the resulting RDF into the named dataset.",
        "OTTR",
    )
    .parameter(path_param("name", "Dataset name."))
    .request_body(Some(multipart_request_body(
        "Multipart body: one or more stOTTR or wOTTR document parts.",
    )))
    .response(
        "200",
        ok_text_response("Expansion applied; number of triples inserted."),
    )
    .response(
        "400",
        no_content_response("Malformed OTTR document or multipart body."),
    )
    .response("403", no_content_response("Server is read-only."))
    .response("404", no_content_response("Dataset does not exist."))
    .build();
    item1(HttpMethod::Post, post)
}

/// `POST /{name}/rules` — replace a dataset's entire live ruleset. See
/// [`crate::rules_endpoint::dataset_rules_post`].
fn rules_path_item() -> PathItem {
    let post = op(
        "Load/replace a dataset's runtime ruleset",
        "Parses the request body as Datalog rules and replaces the dataset's \
         entire live ruleset: rules not present in the new body stop firing, \
         and every rule in the new body ends up derived. An empty body clears \
         the ruleset entirely.",
        "Runtime Ruleset",
    )
    .parameter(path_param("name", "Dataset name."))
    .request_body(Some(text_request_body(
        "Datalog rules text.",
        "text/x-datalog",
    )))
    .response("200", ok_plain_json_response("`{\"rules_loaded\": <n>}`."))
    .response("400", no_content_response("Malformed rules body."))
    .response("403", no_content_response("Server is read-only."))
    .response("404", no_content_response("Dataset does not exist."))
    .response(
        "409",
        no_content_response("New ruleset is contradictory over the dataset's existing data."),
    )
    .build();
    item1(HttpMethod::Post, post)
}

/// `POST`/`DELETE /{name}/rules/{ruleset_id}` — load/replace or retract one
/// named ruleset within a dataset's live combined ruleset (#473). See
/// [`crate::rules_endpoint::dataset_rules_id_post`] and
/// [`crate::rules_endpoint::dataset_rules_id_delete`].
fn rules_by_id_path_item() -> PathItem {
    let post = op(
        "Load/replace one named ruleset",
        "Parses the request body as Datalog rules and replaces exactly the \
         named ruleset's rules within the dataset's live combined ruleset, \
         leaving every other named ruleset untouched.",
        "Runtime Ruleset",
    )
    .parameter(path_param("name", "Dataset name."))
    .parameter(path_param(
        "ruleset_id",
        "Ruleset id, scoped to this dataset.",
    ))
    .request_body(Some(text_request_body(
        "Datalog rules text.",
        "text/x-datalog",
    )))
    .response(
        "200",
        ok_plain_json_response("`{\"rules_loaded\": <n>, \"ruleset_id\": <id>}`."),
    )
    .response("400", no_content_response("Malformed rules body."))
    .response("403", no_content_response("Server is read-only."))
    .response("404", no_content_response("Dataset does not exist."))
    .response(
        "409",
        no_content_response("New ruleset is contradictory over the dataset's existing data."),
    )
    .build();

    let delete = op(
        "Retract one named ruleset",
        "Retracts exactly the named ruleset's rules from the dataset's live \
         combined ruleset. A rule also belonging to a different still-live \
         ruleset id survives.",
        "Runtime Ruleset",
    )
    .parameter(path_param("name", "Dataset name."))
    .parameter(path_param(
        "ruleset_id",
        "Ruleset id, scoped to this dataset.",
    ))
    .response(
        "200",
        ok_plain_json_response("`{\"rules_removed\": <n>, \"ruleset_id\": <id>}`."),
    )
    .response("403", no_content_response("Server is read-only."))
    .response(
        "404",
        no_content_response("Dataset does not exist, or ruleset_id was never loaded."),
    )
    .response(
        "409",
        no_content_response("Remaining ruleset is contradictory over the dataset's existing data."),
    )
    .build();

    merge(item1(HttpMethod::Post, post), HttpMethod::Delete, delete)
}

/// `GET /describe?uri=<encoded IRI>` — dereferenceable resource IRIs
/// (#493). See [`crate::describe::describe_get`].
fn describe_path_item() -> PathItem {
    let get = op(
        "Describe a resource",
        "Returns every outbound (`<iri> ?p ?o`) and inbound (`?s ?p <iri>`) \
         triple known about `uri`, merged into a fresh graph (format per \
         Accept header). Not SPARQL's outbound-only `DESCRIBE <iri>`.",
        "Discovery",
    )
    .parameter(query_param("uri", "The resource IRI to describe.", true))
    .response(
        "200",
        ok_rdf_response("Graph contents (format per Accept header)."),
    )
    .response("400", no_content_response("Missing or malformed `uri`."))
    .response(
        "404",
        no_content_response("Resource is not known to the store."),
    )
    .build();
    item1(HttpMethod::Get, get)
}

/// `GET /id/{*path}` — generic slash-URI namespace that 303-redirects into
/// `/describe` (#493). See [`crate::describe::id_redirect_get`].
fn id_redirect_path_item() -> PathItem {
    let get = op(
        "Resolve a slash-URI resource IRI",
        "303-redirects to `GET /describe?uri=<base_iri>/id/<path>`, per the \
         slash-URI dereferenceable-resource convention.",
        "Discovery",
    )
    .parameter(path_param("path", "Resource path segment(s) under /id/."))
    .response("303", no_content_response("Redirect to /describe?uri=..."))
    .build();
    item1(HttpMethod::Get, get)
}

/// `GET /ns/agentprov/session` — agent-provenance session hash-URI
/// namespace document (#567). See
/// [`crate::agentprov_session::agentprov_session_document_get`].
fn agentprov_session_path_item() -> PathItem {
    let get = op(
        "Agent-provenance session namespace document",
        "Returns every outbound triple whose subject IRI falls under the \
         `https://dagalog.no/ns/agentprov/session#` namespace (format per \
         Accept header). Returns 200 with an empty graph when no sessions \
         are loaded, unlike `/describe`'s 404 for an unknown resource.",
        "Discovery",
    )
    .response(
        "200",
        ok_rdf_response("Namespace document (format per Accept header)."),
    )
    .build();
    item1(HttpMethod::Get, get)
}

/// `POST /upload` — legacy Turtle upload. See
/// [`crate::upload::upload_turtle`].
fn upload_path_item() -> PathItem {
    let post = op(
        "Upload a Turtle document",
        "Legacy convenience endpoint: parses the request body as Turtle and \
         merges it into the default graph, or into a named graph when \
         `?graph=<iri>` is given. Superseded by the Graph Store HTTP \
         Protocol (`/rdf-graph-store`).",
        "Legacy",
    )
    .parameter(query_param(
        "graph",
        "Named graph IRI to merge into, instead of the default graph.",
        false,
    ))
    .request_body(Some(text_request_body(
        "RDF graph (Turtle).",
        "text/turtle",
    )))
    .response("200", no_content_response("Upload applied."))
    .response(
        "400",
        no_content_response("Malformed Turtle, or non-absolute graph IRI."),
    )
    .response("403", no_content_response("Server is read-only."))
    .build();
    item1(HttpMethod::Post, post)
}

/// `GET /vqs/productive-values?class=<IRI>&property=<IRI>` — query-builder
/// VQS support. See [`crate::vqs_routes::productive_values`].
fn vqs_productive_values_path_item() -> PathItem {
    let get = op(
        "Productive data values for a class/property pair",
        "Returns the productive data values for `property` on instances of \
         `class`, using the Wld reference configuration, for use by the \
         query-builder frontend's autocomplete.",
        "Query Builder",
    )
    .parameter(query_param("class", "Class IRI.", true))
    .parameter(query_param("property", "Property IRI.", true))
    .response(
        "200",
        ok_plain_json_response("`{\"covered\": bool, \"values\": [...]}`."),
    )
    .build();
    item1(HttpMethod::Get, get)
}

/// `POST /transaction/begin` — begin a transaction. See
/// [`crate::transaction_routes::transaction_begin`].
fn transaction_begin_path_item() -> PathItem {
    let post = op(
        "Begin a transaction",
        "Proprietary transaction API: snapshots the current store generation \
         and returns a transaction id. Reads/writes against `/sparql` with a \
         `txId` query parameter are buffered against this transaction until \
         `/commit` or `/rollback`, or a 60-second idle timeout.",
        "Transaction",
    )
    .response("200", ok_json_response("`{\"txId\": \"<uuid>\"}`."))
    .build();
    item1(HttpMethod::Post, post)
}

/// `POST /transaction/{txId}/commit` — commit a transaction. See
/// [`crate::transaction_routes::transaction_commit`].
fn transaction_commit_path_item() -> PathItem {
    let post = op(
        "Commit a transaction",
        "Proprietary transaction API: atomically applies the transaction's \
         buffered inserts/deletes. Returns 409 if the store's generation has \
         changed since the transaction began (optimistic concurrency).",
        "Transaction",
    )
    .parameter(path_param(
        "txId",
        "Transaction id, from `POST /transaction/begin`.",
    ))
    .response("204", no_content_response("Transaction committed."))
    .response("403", no_content_response("Server is read-only."))
    .response(
        "404",
        no_content_response("Transaction not found (or already expired)."),
    )
    .response(
        "409",
        no_content_response("Store was modified since the transaction began."),
    )
    .build();
    item1(HttpMethod::Post, post)
}

/// `POST /transaction/{txId}/rollback` — roll back a transaction. See
/// [`crate::transaction_routes::transaction_rollback`].
fn transaction_rollback_path_item() -> PathItem {
    let post = op(
        "Roll back a transaction",
        "Proprietary transaction API: discards the transaction's buffered \
         changes. The store is never modified.",
        "Transaction",
    )
    .parameter(path_param(
        "txId",
        "Transaction id, from `POST /transaction/begin`.",
    ))
    .response("204", no_content_response("Transaction rolled back."))
    .response(
        "404",
        no_content_response("Transaction not found (or already expired)."),
    )
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
        .path("/{name}/get", dataset_get_path_item())
        .path(
            "/$/ping",
            simple_get("Liveness/readiness ping", "Fuseki-compatible `/$/ping`.", "Admin"),
        )
        .path("/$/ready", admin_ready_path_item())
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
        .path(
            "/.well-known/void",
            simple_get(
                "VoID dataset description",
                "RDF VoID (Vocabulary of Interlinked Datasets) description of this \
                 server's datasets, at the `.well-known` URI (same handler as `/void`).",
                "Discovery",
            ),
        )
        .path("/describe", describe_path_item())
        .path("/id/{path}", id_redirect_path_item())
        .path("/ns/agentprov/session", agentprov_session_path_item())
        .path("/upload", upload_path_item())
        .path("/vqs/productive-values", vqs_productive_values_path_item())
        .path("/{name}/shacl", shacl_path_item())
        .path("/{name}/rml", rml_dataset_path_item())
        .path("/rml/map", rml_map_path_item())
        .path("/{name}/ottr", ottr_path_item())
        .path("/{name}/rules", rules_path_item())
        .path("/{name}/rules/{ruleset_id}", rules_by_id_path_item())
        .path("/transaction/begin", transaction_begin_path_item())
        .path("/transaction/{txId}/commit", transaction_commit_path_item())
        .path(
            "/transaction/{txId}/rollback",
            transaction_rollback_path_item(),
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

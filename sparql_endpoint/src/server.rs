/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode, header, request::Parts},
    middleware,
    routing::{get, post},
};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::timeout::TimeoutLayer;

/// HTTP methods that never mutate server state.
///
/// Cross-origin requests using these methods are allowed from any origin by
/// default (`Access-Control-Allow-Origin: *`): `allow_credentials` is never
/// set on this server, so no cookies/session credentials can leak, and
/// permitting cross-origin reads is a legitimate use case (a web UI hosted on
/// a different origin querying the endpoint).
fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Build the `AllowOrigin` policy used for the server's single `CorsLayer`.
///
/// Splits behavior by the *intended* request method rather than the method of
/// the request actually carrying the `Origin` header, because a CORS
/// preflight is always an `OPTIONS` request — the method it's asking
/// permission for is carried in `Access-Control-Request-Method`, not
/// `parts.method`.
///
/// - Safe methods (`GET`/`HEAD`, or an `OPTIONS` preflight for one of them,
///   or a bare `OPTIONS` request with no `Access-Control-Request-Method` at
///   all) are approved for any origin.
/// - Any other (state-changing) method is approved only when the request's
///   `Origin` exactly matches one entry of `allowed_origins`. With the
///   default empty list this means state-changing cross-origin requests get
///   no CORS approval, so browsers refuse to send them — closing the gap
///   described in
///   <https://github.com/daghovland/rdf-datalog/issues/362> where an
///   unauthenticated (`AuthConfig::None`, the documented default) instance
///   combined with a blanket `allow_origin(Any)` let any web page a victim's
///   browser visits issue blind cross-origin writes.
fn build_allow_origin(allowed_origins: Vec<String>) -> AllowOrigin {
    let allowed_origins = Arc::new(allowed_origins);
    AllowOrigin::predicate(move |origin: &HeaderValue, parts: &Parts| {
        let requested_method: Option<Method> = if parts.method == Method::OPTIONS {
            parts
                .headers
                .get(header::ACCESS_CONTROL_REQUEST_METHOD)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| Method::from_bytes(s.as_bytes()).ok())
        } else {
            Some(parts.method.clone())
        };

        match requested_method {
            None => true,
            Some(method) if is_safe_method(&method) => true,
            Some(_) => origin
                .to_str()
                .is_ok_and(|origin| allowed_origins.iter().any(|allowed| allowed == origin)),
        }
    })
}

/// Build the axum router with all routes and CORS middleware.
pub fn build_router(state: AppState) -> Router {
    // RML mapping routes accept arbitrary source files as multipart parts,
    // which routinely exceed axum's server-wide 2 MB DefaultBodyLimit.
    // Override it for just these two routes; every other route keeps 2 MB.
    let rml_body_limit = DefaultBodyLimit::max(state.config.max_rml_upload_bytes);

    // RDF write routes accept whole RDF graphs / SHACL shapes graphs as a raw
    // request body, which routinely exceed axum's server-wide 2 MB
    // DefaultBodyLimit for realistic datasets. Override it for these routes;
    // every other route keeps 2 MB. See #274.
    let rdf_body_limit = DefaultBodyLimit::max(state.config.max_rdf_upload_bytes);

    // Request-level timeout (#367): bounds how long any single request may
    // occupy a connection. `TimeoutLayer` is response-preserving (it wraps
    // an Infallible service and stays Infallible), returning a plain 408
    // response rather than propagating an error — so no `HandleErrorLayer`
    // is needed. Note this only bounds *connection occupancy*: when it
    // fires, the client gets 408 and the connection is freed, but the
    // handler future keeps running to completion in the background.
    let timeout = TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(state.config.request_timeout_secs),
    );

    let cors = CorsLayer::new()
        .allow_origin(build_allow_origin(
            state.config.cors_allowed_origins.clone(),
        ))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::HEAD,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::ACCEPT,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]);

    Router::new()
        // ── VoID dataset description (§4, P2) ───────────────────────────────
        .route("/.well-known/void", get(crate::void::void_handler))
        .route("/void", get(crate::void::void_handler))
        // ── Auth config (always public — no middleware) ───────────────────────
        .route("/auth/config", get(crate::auth::auth_config_handler))
        // ── Frontend + legacy upload ─────────────────────────────────────────
        .route("/", get(crate::frontend::serve_frontend))
        // ── Backlog/provenance dashboard (schema-specific, see #381) ────────
        .route(
            "/backlog",
            get(crate::backlog_frontend::serve_backlog_frontend),
        )
        .route(
            "/upload",
            post(crate::upload::upload_turtle).layer(rdf_body_limit),
        )
        // ── VQS productive-extension index (query-builder support) ──────────
        .route(
            "/vqs/productive-values",
            get(crate::vqs_routes::productive_values),
        )
        // ── Transaction API (proprietary BEGIN / COMMIT / ROLLBACK) ─────────
        .route(
            "/transaction/begin",
            post(crate::transaction_routes::transaction_begin),
        )
        .route(
            "/transaction/{txId}/commit",
            post(crate::transaction_routes::transaction_commit),
        )
        .route(
            "/transaction/{txId}/rollback",
            post(crate::transaction_routes::transaction_rollback),
        )
        // ── SPARQL Protocol — root endpoint ──────────────────────────────────
        .route("/sparql", get(crate::query::sparql_get))
        .route("/sparql", post(crate::query::sparql_post))
        // ── Graph Store Protocol — root endpoint ─────────────────────────────
        .route(
            "/rdf-graph-store",
            get(crate::graph_store::gsp_get)
                .head(crate::graph_store::gsp_head)
                .put(crate::graph_store::gsp_put)
                .post(crate::graph_store::gsp_post)
                .delete(crate::graph_store::gsp_delete)
                .layer(rdf_body_limit),
        )
        // ── Direct graph identification (§4.1) ───────────────────────────────
        .route(
            "/rdf-graphs/{*path}",
            get(crate::graph_store::direct_gsp_get)
                .head(crate::graph_store::direct_gsp_head)
                .put(crate::graph_store::direct_gsp_put)
                .post(crate::graph_store::direct_gsp_post)
                .delete(crate::graph_store::direct_gsp_delete)
                .layer(rdf_body_limit),
        )
        // ── Admin API (`/$/...`) ─────────────────────────────────────────────
        .route(
            "/$/ping",
            get(crate::admin::admin_ping).post(crate::admin::admin_ping),
        )
        // `/$/ready` — readiness check (issue #414), deliberately reusing
        // `admin_ping` rather than a distinct handler: see the doc comment
        // on `admin::admin_ping` for why the two are equivalent in this
        // architecture (no route is live before startup finishes, so
        // liveness and readiness coincide here).
        .route(
            "/$/ready",
            get(crate::admin::admin_ping).post(crate::admin::admin_ping),
        )
        .route("/$/server", get(crate::admin::admin_server))
        .route(
            "/$/datasets",
            get(crate::admin::admin_list_datasets).post(crate::admin::admin_create_dataset),
        )
        .route(
            "/$/datasets/{name}",
            get(crate::admin::admin_get_dataset).delete(crate::admin::admin_delete_dataset),
        )
        .route("/$/compact", post(crate::admin::admin_compact))
        // ── Per-dataset query (`/{name}/sparql`, `/{name}/query`) ────────────
        .route(
            "/{name}/sparql",
            get(crate::dataset_routes::dataset_sparql_get)
                .post(crate::dataset_routes::dataset_sparql_post),
        )
        .route(
            "/{name}/query",
            get(crate::dataset_routes::dataset_sparql_get)
                .post(crate::dataset_routes::dataset_sparql_post),
        )
        // ── Per-dataset SPARQL Update (`/{name}/update`) ─────────────────────
        .route(
            "/{name}/update",
            post(crate::dataset_routes::dataset_update_post),
        )
        // ── Per-dataset SHACL validation (`/{name}/shacl`) ───────────────────
        .route(
            "/{name}/shacl",
            post(crate::shacl_endpoint::dataset_shacl_post).layer(rdf_body_limit),
        )
        // ── Per-dataset RML mapping (`/{name}/rml`) ──────────────────────────
        .route(
            "/{name}/rml",
            post(crate::rml_endpoint::dataset_rml_post).layer(rml_body_limit),
        )
        // ── Stateless RML mapping (`/rml/map`) — apply a mapping, return RDF ──
        .route(
            "/rml/map",
            post(crate::rml_endpoint::rml_map_post).layer(rml_body_limit),
        )
        // ── Per-dataset OTTR expansion (`/{name}/ottr`) ──────────────────────
        .route(
            "/{name}/ottr",
            post(crate::ottr_endpoint::dataset_ottr_post),
        )
        // ── Per-dataset GSP (`/{name}/data`, `/{name}/get`) ──────────────────
        .route(
            "/{name}/data",
            get(crate::dataset_routes::dataset_data_get)
                .head(crate::dataset_routes::dataset_data_head)
                .put(crate::dataset_routes::dataset_data_put)
                .post(crate::dataset_routes::dataset_data_post)
                .delete(crate::dataset_routes::dataset_data_delete)
                .layer(rdf_body_limit),
        )
        .route(
            "/{name}/get",
            get(crate::dataset_routes::dataset_data_get)
                .head(crate::dataset_routes::dataset_data_head),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state)
        .layer(timeout)
        .layer(cors)
}

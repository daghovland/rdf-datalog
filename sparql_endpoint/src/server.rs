/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::AppState;
use axum::{
    Router, middleware,
    routing::{get, post},
};
use tower_http::cors::{Any, CorsLayer};

/// Build the axum router with all routes and CORS middleware.
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
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
        // ── Frontend + legacy upload ─────────────────────────────────────────
        .route("/", get(crate::frontend::serve_frontend))
        .route("/upload", post(crate::upload::upload_turtle))
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
                .delete(crate::graph_store::gsp_delete),
        )
        // ── Direct graph identification (§4.1) ───────────────────────────────
        .route(
            "/rdf-graphs/{*path}",
            get(crate::graph_store::direct_gsp_get)
                .head(crate::graph_store::direct_gsp_head)
                .put(crate::graph_store::direct_gsp_put)
                .post(crate::graph_store::direct_gsp_post)
                .delete(crate::graph_store::direct_gsp_delete),
        )
        // ── Admin API (`/$/...`) ─────────────────────────────────────────────
        .route(
            "/$/ping",
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
        // ── Per-dataset GSP (`/{name}/data`, `/{name}/get`) ──────────────────
        .route(
            "/{name}/data",
            get(crate::dataset_routes::dataset_data_get)
                .head(crate::dataset_routes::dataset_data_head)
                .put(crate::dataset_routes::dataset_data_put)
                .post(crate::dataset_routes::dataset_data_post)
                .delete(crate::dataset_routes::dataset_data_delete),
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
        .layer(cors)
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Fuseki-compatible admin API under `/$/...`.
//!
//! Groups C (ping/server), D (list/info), E (create/delete).
//!
//! Spec: <https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html>

use crate::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use dag_rdf::Datastore;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── C: ping + server info ─────────────────────────────────────────────────────

/// `GET /$/ping` and `POST /$/ping` — liveness check.
pub async fn admin_ping() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// `GET /$/server` — server metadata (version, dataset list).
pub async fn admin_server(State(state): State<AppState>) -> impl IntoResponse {
    let registry = state.registry.read().await;
    let dataset_names: Vec<String> = registry.names().iter().map(|n| format!("/{n}")).collect();
    let body = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "datasets": dataset_names
    });
    (StatusCode::OK, Json(body))
}

// ── D: list + info ────────────────────────────────────────────────────────────

/// `GET /$/datasets` — list all datasets.
pub async fn admin_list_datasets(State(state): State<AppState>) -> impl IntoResponse {
    let registry = state.registry.read().await;
    (StatusCode::OK, Json(registry.all_datasets_json()))
}

/// `GET /$/datasets/{name}` — info for one dataset.
pub async fn admin_get_dataset(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let registry = state.registry.read().await;
    match registry.dataset_info_json(&name) {
        Some(info) => (StatusCode::OK, Json(info)).into_response(),
        None => (StatusCode::NOT_FOUND, "Dataset not found").into_response(),
    }
}

// ── E: create + delete ────────────────────────────────────────────────────────

/// `POST /$/datasets` — create a new in-memory dataset.
///
/// Form body: `dbName=/{name}&dbType=mem`
pub async fn admin_create_dataset(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !ct.contains("application/x-www-form-urlencoded") {
        return (
            StatusCode::BAD_REQUEST,
            "Content-Type must be application/x-www-form-urlencoded",
        )
            .into_response();
    }

    let body_str = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8").into_response(),
    };

    let mut db_name: Option<String> = None;
    let mut db_type: Option<String> = None;
    for part in body_str.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            let v = urlencoding::decode(v).unwrap_or(std::borrow::Cow::Borrowed(v));
            match k {
                "dbName" => db_name = Some(v.into_owned()),
                "dbType" => db_type = Some(v.into_owned()),
                _ => {}
            }
        }
    }

    let name = match db_name {
        Some(n) => n,
        None => return (StatusCode::BAD_REQUEST, "Missing dbName").into_response(),
    };
    match db_type.as_deref() {
        Some("mem") => {}
        Some(t) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Unsupported dbType '{t}'; only 'mem' is supported"),
            )
                .into_response();
        }
        None => return (StatusCode::BAD_REQUEST, "Missing dbType").into_response(),
    }

    let name = name.trim_start_matches('/').to_string();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "Dataset name cannot be empty").into_response();
    }

    let mut registry = state.registry.write().await;
    if registry.exists(&name) {
        return (StatusCode::CONFLICT, "Dataset already exists").into_response();
    }

    let new_store = Arc::new(RwLock::new(Datastore::new(1024)));
    registry.insert(&name, new_store);
    StatusCode::OK.into_response()
}

/// `DELETE /$/datasets/{name}` — remove a dataset.
pub async fn admin_delete_dataset(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let mut registry = state.registry.write().await;
    if registry.remove(&name) {
        StatusCode::OK.into_response()
    } else {
        (StatusCode::NOT_FOUND, "Dataset not found").into_response()
    }
}

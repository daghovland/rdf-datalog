/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Per-dataset route handlers: `/{name}/sparql`, `/{name}/data`, `/{name}/update`.
//!
//! Each handler extracts the dataset from the registry, builds a dataset-scoped
//! AppState, then delegates to the shared inner functions.
//!
//! Groups A, B, F (Fuseki compatibility).
//! Spec: <https://jena.apache.org/documentation/fuseki2/fuseki-server-protocol.html>

use crate::{AppState, constraints, graph_store, query, sparql_update};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use dag_rdf::Datastore;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn get_dataset_store(state: &AppState, name: &str) -> Option<Arc<RwLock<Datastore>>> {
    state.registry.read().await.get(name)
}

fn dataset_state(state: &AppState, ds_store: Arc<RwLock<Datastore>>) -> AppState {
    AppState {
        store: ds_store,
        registry: state.registry.clone(),
        config: state.config.clone(),
        jwks_cache: state.jwks_cache.clone(),
        changelog: state.changelog.clone(),
        // Each dataset has its own store, hence its own VQS index cache.
        vqs_cache: Arc::new(RwLock::new(None)),
        // Per-dataset incremental reasoning is not yet supported (D6 scope).
        // See: https://github.com/daghovland/rdf-datalog/issues/110
        reasoner: None,
        network_policy: state.network_policy.clone(),
        // Transactions are server-wide; per-dataset transactions are not yet supported.
        // See: https://github.com/daghovland/rdf-datalog/issues/125
        transactions: state.transactions.clone(),
    }
}

// ── A: per-dataset query (`/{name}/sparql`, `/{name}/query`) ──────────────────

pub async fn dataset_sparql_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
    params: Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };
    query::sparql_get_with_state(dataset_state(&state, ds), params, headers).await
}

pub async fn dataset_sparql_post(
    State(state): State<AppState>,
    Path(name): Path<String>,
    params: Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };
    query::sparql_post_with_state(dataset_state(&state, ds), params, headers, body).await
}

// ── B: per-dataset GSP (`/{name}/data`, `/{name}/get`) ───────────────────────

pub async fn dataset_data_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };
    graph_store::gsp_get_inner(dataset_state(&state, ds), params, headers).await
}

pub async fn dataset_data_head(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };
    graph_store::gsp_head_inner(dataset_state(&state, ds), params, headers).await
}

pub async fn dataset_data_put(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };
    graph_store::gsp_put_inner(dataset_state(&state, ds), params, headers, body).await
}

pub async fn dataset_data_post(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };
    // Fuseki /{name}/data creates named graphs on POST even when they don't
    // exist yet — real Fuseki clients rely on this without a prior PUT.
    graph_store::gsp_post_inner(dataset_state(&state, ds), params, headers, body, true).await
}

pub async fn dataset_data_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };
    graph_store::gsp_delete_inner(dataset_state(&state, ds), params).await
}

// ── F: per-dataset SPARQL Update (`/{name}/update`) ──────────────────────────

pub async fn dataset_update_post(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let Some(ds) = get_dataset_store(&state, &name).await else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };

    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let update_str: String = if ct.contains("application/sparql-update") {
        match String::from_utf8(body.to_vec()) {
            Ok(s) => s,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "Invalid UTF-8 in update body").into_response();
            }
        }
    } else if ct.contains("application/x-www-form-urlencoded") {
        let body_str = match String::from_utf8(body.to_vec()) {
            Ok(s) => s,
            Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8 in body").into_response(),
        };
        let val = body_str.split('&').find_map(|part| {
            let (k, v) = part.split_once('=')?;
            (k == "update").then(|| v.replace('+', " "))
        });
        match val {
            Some(s) => urlencoding_decode(&s),
            None => {
                return (StatusCode::BAD_REQUEST, "Missing 'update' parameter").into_response();
            }
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            "Unsupported Content-Type for SPARQL Update",
        )
            .into_response();
    };

    let ops = match sparql_update::parse_update(&update_str) {
        Ok(ops) => ops,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Update parse error: {e}")).into_response();
        }
    };

    // Acquire store write lock first, then commit changelog inside the critical
    // section so commit-order == apply-order under concurrent writers.
    let mut store = ds.write().await;

    // Optimistic concurrency control: if the client supplied an If-Match header,
    // its value must match the current store generation ETag.
    // See: https://github.com/daghovland/rdf-datalog/issues/124
    if let Some(if_match) = headers.get("if-match").and_then(|v| v.to_str().ok()) {
        let requested = if_match.trim().trim_matches('"');
        let current = format!("{}", store.generation);
        if requested != current {
            return (
                StatusCode::PRECONDITION_FAILED,
                "Precondition Failed: ETag mismatch",
            )
                .into_response();
        }
    }

    // Parse Turtle content once; build WAL entries and prepared apply in one pass.
    let (prepared, log_entries) = match sparql_update::prepare_update(&store, ops) {
        Ok(pair) => pair,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Update prepare error: {e}"),
            )
                .into_response();
        }
    };

    if let Some(ref changelog) = state.changelog {
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.append_batch(&log_entries) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }

    // Per-dataset incremental reasoning is not yet supported; see issue #110.
    // Constraint checking (owl:Nothing) is also skipped here since dataset
    // stores have no reasoner.  See query.rs for the main-store implementation.
    match sparql_update::apply_prepared_update(
        &mut store,
        prepared,
        None,
        state.network_policy.clone(),
    ) {
        Ok((net_inserts, net_deletes)) => {
            // Check for owl:Nothing violations.  The dataset AppState always has
            // reasoner=None (see dataset_state()), but we call check_owl_nothing
            // anyway in case per-dataset reasoning is added in the future.
            // Related: https://github.com/daghovland/rdf-datalog/issues/127
            if state.reasoner.is_some() {
                let violations = constraints::check_owl_nothing(&store, 10, 10);
                if !violations.is_empty() {
                    for &q in &net_inserts {
                        store.remove_quad(q);
                    }
                    for &q in &net_deletes {
                        store.add_quad(q);
                    }
                    let body = constraints::format_409_body(&violations);
                    return (StatusCode::CONFLICT, body).into_response();
                }
            }
            StatusCode::OK.into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Update execution error: {e}"),
        )
            .into_response(),
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            let hex = format!("{h1}{h2}");
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                out.push(byte as char);
            }
        } else {
            out.push(c);
        }
    }
    out
}

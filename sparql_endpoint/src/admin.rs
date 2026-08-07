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
use dag_rdf::{Datastore, GraphElement, RdfLiteral, RdfResource};
use ingress::{IriReference, RDF_TYPE};
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::RwLock;

/// `fuseki:name` — literal-valued name of a Fuseki assembler `Service`.
const FUSEKI_NAME_PREDICATE: &str = "http://jena.apache.org/fuseki#name";

/// `fuseki:endpoint` — endpoint configuration blocks on a `Service`.
const FUSEKI_ENDPOINT_PREDICATE: &str = "http://jena.apache.org/fuseki#endpoint";

/// `fuseki:dataset` — the dataset node referenced by a `Service`.
const FUSEKI_DATASET_PREDICATE: &str = "http://jena.apache.org/fuseki#dataset";

/// `ja:MemoryDataset` — the only dataset type Dagalog actually supports via
/// this API; anything else declared in the assembler is created as an
/// in-memory dataset anyway, but now with a warning instead of silence.
const JA_MEMORY_DATASET: &str = "http://jena.hpl.hp.com/2005/11/Assembler#MemoryDataset";

// ── C: ping + server info ─────────────────────────────────────────────────────

/// `GET /$/ping` and `POST /$/ping` — liveness check.
///
/// Also wired up as `GET /$/ready` (issue [#414](https://github.com/daghovland/rdf-datalog/issues/414)):
/// `serve_on_listener` (`sparql_endpoint/src/lib.rs`) binds the TCP listener
/// *before* running all synchronous startup work (changelog replay,
/// `IncrementalReasoner::new` initial materialisation, dataset registry
/// construction), and only starts `axum::serve` — i.e. only starts routing
/// *any* request, including this one — once all of that has finished. There
/// is no further async/background initialization after `axum::serve` starts,
/// so a 200 from this handler is already a fully correct readiness signal by
/// construction, not just a liveness one. A bare TCP-port check (as used by
/// some Testcontainers wait strategies) can observe the listening socket
/// before `axum::serve` begins routing, which is the gap `/$/ready` closes:
/// callers that need to distinguish "port open" from "actually serving" can
/// poll this route instead. The two routes intentionally share one handler
/// rather than diverging, since there is no separate "started but not ready"
/// state in this architecture to give `/$/ready` a distinct answer for.
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
    let body_str = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8").into_response(),
    };

    let name = if ct.contains("application/x-www-form-urlencoded") {
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
        name
    } else if ct.contains("text/turtle") {
        // Fuseki-compatible dataset creation accepts a Turtle assembler payload.
        match extract_fuseki_name_from_assembler(&body_str) {
            Ok(n) => n,
            Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            "Content-Type must be application/x-www-form-urlencoded or text/turtle",
        )
            .into_response();
    };

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

/// Parses a Fuseki assembler `text/turtle` payload and extracts the
/// `fuseki:name` literal, real Turtle parsing rather than a substring search
/// (see [#415](https://github.com/daghovland/rdf-datalog/issues/415)).
///
/// Also logs (but does not reject on) two forms of assembler configuration
/// Dagalog doesn't actually honor:
/// - a declared `fuseki:dataset` whose `rdf:type` isn't `ja:MemoryDataset`
///   (Dagalog only creates in-memory datasets via this API regardless);
/// - any `fuseki:endpoint` blocks (Dagalog always exposes its fixed set of
///   dataset-scoped routes regardless of what's declared here).
///
/// Returns `Err` with a user-facing message on Turtle parse failure or when
/// no `fuseki:name` string-literal triple is found.
fn extract_fuseki_name_from_assembler(body: &str) -> Result<String, String> {
    // Fuseki assembler documents conventionally use fragment-relative IRIs
    // for the service/dataset nodes (e.g. `<#service>`), which need a base
    // IRI to resolve per RFC 3986 — there is no requester-supplied base for
    // this admin endpoint, so a fixed synthetic one is used purely so those
    // relative references resolve consistently within this one parse.
    const ASSEMBLER_BASE_IRI: &str = "urn:x-dagalog:fuseki-assembler";
    let mut datastore = Datastore::new(64);
    if let Err(e) = turtle::parse_turtle_with_base(
        &mut datastore,
        Cursor::new(body.as_bytes()),
        ASSEMBLER_BASE_IRI,
    ) {
        return Err(format!("Invalid Turtle in assembler payload: {e}"));
    }

    let name_pred = datastore
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            FUSEKI_NAME_PREDICATE.to_string(),
        )));
    let name = datastore
        .get_triples_with_predicate(name_pred)
        .find_map(
            |triple| match datastore.resources.get_graph_element(triple.obj) {
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some(s.clone()),
                _ => None,
            },
        );
    let name = match name {
        Some(n) => n,
        None => {
            return Err("Could not extract fuseki:name from assembler Turtle".to_string());
        }
    };

    // New (issue #415): warn instead of silently dropping unsupported
    // assembler configuration.
    let type_pred = datastore
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_string())));
    let dataset_pred = datastore
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            FUSEKI_DATASET_PREDICATE.to_string(),
        )));
    for service_triple in datastore.get_triples_with_predicate(dataset_pred) {
        for type_triple in
            datastore.get_triples_with_subject_predicate(service_triple.obj, type_pred)
        {
            if let GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(type_iri))) =
                datastore.resources.get_graph_element(type_triple.obj)
                && type_iri != JA_MEMORY_DATASET
            {
                log::warn!(
                    "Fuseki assembler declares dataset type '{type_iri}'; Dagalog only \
                     supports in-memory datasets via POST /$/datasets and is creating one \
                     anyway (see issue #415)"
                );
            }
        }
    }

    let endpoint_pred = datastore
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            FUSEKI_ENDPOINT_PREDICATE.to_string(),
        )));
    if datastore
        .get_triples_with_predicate(endpoint_pred)
        .next()
        .is_some()
    {
        log::warn!(
            "Fuseki assembler declares fuseki:endpoint configuration; Dagalog always exposes \
             its fixed set of dataset-scoped routes and does not honor endpoint config from \
             this payload (see issue #415)"
        );
    }

    Ok(name)
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

// ── F: compaction ─────────────────────────────────────────────────────────────

/// `POST /$/compact` — atomically rewrite the changelog as a minimal snapshot.
///
/// Replaces the full mutation history with a single batch of `InsertQuad` entries
/// for each currently-live quad.  Returns JSON `{"entries_before": N, "entries_after": M}`.
///
/// Returns 405 when the server is running in in-memory mode (no changelog).
pub async fn admin_compact(State(state): State<AppState>) -> axum::response::Response {
    let Some(ref changelog_lock) = state.changelog else {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            "Server is not in persistent mode — no changelog to compact",
        )
            .into_response();
    };

    let store = state.store.read().await;
    let mut changelog = changelog_lock.lock().await;

    match changelog.compact(&store) {
        Ok((before, after)) => {
            let body = serde_json::json!({
                "entries_before": before,
                "entries_after": after,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

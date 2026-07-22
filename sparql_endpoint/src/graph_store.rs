/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Handlers for the SPARQL 1.1 Graph Store HTTP Protocol endpoint.
//!
//! Routes: `GET | PUT | POST | DELETE | HEAD  /rdf-graph-store`
//!
//! Graph identification uses the indirect model (§4.2):
//! - `?default`          → the default graph
//! - `?graph=<abs-iri>`  → a named graph
//! - (no param)          → the Graph Store itself (only meaningful for POST create)
//!
//! Specification: <https://www.w3.org/TR/sparql11-http-rdf-update/>

use crate::{
    AppState,
    persistence::{LogEntry, to_repr},
    serialize::{
        serialize_graph, serialize_nquads, serialize_nquads_graph, serialize_trig,
        serialize_trig_graph,
    },
};
use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use dag_rdf::{
    GraphElement, GraphElementId, IriReference, RdfResource, ingress::DEFAULT_GRAPH_ELEMENT_ID,
};
use ingress::NetworkPolicy;
use std::{collections::HashMap, io::Cursor};

// ── Graph identification helpers ─────────────────────────────────────────────

/// Validate that `iri` is an absolute IRI (has a syntactically valid scheme).
///
/// Per §4.2: "The query string IRI MUST be an absolute IRI and the server MUST
/// respond with a 400 Bad Request if it is not."
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#indirect-graph-identification>
fn is_absolute_iri(iri: &str) -> bool {
    // Scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"
    iri.find(':').is_some_and(|colon| {
        colon > 0
            && iri[..colon].chars().enumerate().all(|(i, c)| {
                if i == 0 {
                    c.is_ascii_alphabetic()
                } else {
                    c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'
                }
            })
    })
}

/// Resolved graph target for read operations (GET / HEAD).
enum ReadTarget {
    Default,
    Named(GraphElementId),
}

/// Resolve `?default` / `?graph=<iri>` for a read operation.
///
/// Returns `Err(response)` on bad request or 404.
#[allow(clippy::result_large_err)]
fn resolve_read_target(
    params: &HashMap<String, String>,
    store: &dag_rdf::Datastore,
) -> Result<ReadTarget, axum::response::Response> {
    if params.contains_key("default") {
        return Ok(ReadTarget::Default);
    }
    if let Some(iri) = params.get("graph") {
        if !is_absolute_iri(iri) {
            return Err((StatusCode::BAD_REQUEST, "graph IRI must be absolute").into_response());
        }
        return match store.lookup_named_graph_id(iri) {
            Some(id) if store.named_graph_exists(id) => Ok(ReadTarget::Named(id)),
            _ => Err((StatusCode::NOT_FOUND, "Named graph not found").into_response()),
        };
    }
    Err((
        StatusCode::BAD_REQUEST,
        "GET /rdf-graph-store requires ?default or ?graph=<iri>",
    )
        .into_response())
}

/// Resolved graph target for write operations (PUT / POST merge / DELETE).
enum WriteTarget {
    Default,
    /// Named graph; `is_new` is true when the IRI was not yet interned.
    Named {
        id: GraphElementId,
        is_new: bool,
    },
}

/// Resolve `?default` / `?graph=<iri>` for a write operation.
///
/// Unlike `resolve_read_target`, this interns the IRI if needed.
#[allow(clippy::result_large_err)]
fn resolve_write_target(
    params: &HashMap<String, String>,
    store: &mut dag_rdf::Datastore,
) -> Result<WriteTarget, axum::response::Response> {
    if params.contains_key("default") {
        return Ok(WriteTarget::Default);
    }
    if let Some(iri) = params.get("graph") {
        if !is_absolute_iri(iri) {
            return Err((StatusCode::BAD_REQUEST, "graph IRI must be absolute").into_response());
        }
        let is_new = store.lookup_named_graph_id(iri).is_none();
        let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.clone())));
        let id = store.resources.add_resource(elem);
        return Ok(WriteTarget::Named { id, is_new });
    }
    Err((
        StatusCode::BAD_REQUEST,
        "PUT/DELETE /rdf-graph-store requires ?default or ?graph=<iri>",
    )
        .into_response())
}

// ── RDF content negotiation ───────────────────────────────────────────────────

pub(crate) enum RdfFormat {
    Turtle,
    NTriples,
    NQuads,
    TriG,
    JsonLd,
}

pub(crate) fn negotiate_rdf_format(accept: Option<&str>) -> Option<RdfFormat> {
    let accept = match accept {
        None | Some("") => return Some(RdfFormat::Turtle),
        Some(a) => a,
    };
    for part in accept.split(',') {
        let mime = part.split(';').next().unwrap_or("").trim();
        match mime {
            "text/turtle" | "text/*" | "*/*" => return Some(RdfFormat::Turtle),
            "application/n-triples" => return Some(RdfFormat::NTriples),
            "application/n-quads" => return Some(RdfFormat::NQuads),
            "application/trig" => return Some(RdfFormat::TriG),
            "application/ld+json" => return Some(RdfFormat::JsonLd),
            _ => {}
        }
    }
    None
}

/// Recognised RDF upload content-types.
pub(crate) fn rdf_upload_format(ct: &str) -> Option<UploadFormat> {
    let mime = ct.split(';').next().unwrap_or("").trim();
    match mime {
        "text/turtle" | "application/x-turtle" | "application/rdf+xml" => {
            Some(UploadFormat::Turtle)
        }
        "application/n-triples" => Some(UploadFormat::NTriples),
        "application/n-quads" => Some(UploadFormat::NQuads),
        "application/trig" => Some(UploadFormat::TriG),
        "application/ld+json" => Some(UploadFormat::JsonLd),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UploadFormat {
    Turtle,
    NTriples,
    NQuads,
    TriG,
    JsonLd,
}

// ── Shared graph serialisation ────────────────────────────────────────────────

pub(crate) fn graph_response_parts(
    store: &dag_rdf::Datastore,
    graph_id: GraphElementId,
    accept: Option<&str>,
) -> axum::response::Response {
    match negotiate_rdf_format(accept) {
        Some(RdfFormat::Turtle) => (
            StatusCode::OK,
            [("content-type", "text/turtle; charset=utf-8; version=1.2")],
            serialize_graph(store, graph_id),
        )
            .into_response(),
        Some(RdfFormat::NTriples) => (
            StatusCode::OK,
            [("content-type", "application/n-triples; version=1.2")],
            serialize_graph(store, graph_id),
        )
            .into_response(),
        Some(RdfFormat::NQuads) => (
            StatusCode::OK,
            [("content-type", "application/n-quads; version=1.2")],
            serialize_nquads_graph(store, graph_id),
        )
            .into_response(),
        Some(RdfFormat::TriG) => (
            StatusCode::OK,
            [("content-type", "application/trig; version=1.2")],
            serialize_trig_graph(store, graph_id),
        )
            .into_response(),
        Some(RdfFormat::JsonLd) => {
            // Build a temporary Datastore containing only the requested graph's
            // triples in the default-graph slot, then serialize as JSON-LD.
            let mut tmp = dag_rdf::Datastore::new(256);
            let quads: Vec<_> = store.named_graphs.get_graph(graph_id).collect();
            for q in quads {
                let s = tmp.add_resource(store.resources.get_graph_element(q.subject).clone());
                let p = tmp.add_resource(store.resources.get_graph_element(q.predicate).clone());
                let o = tmp.add_resource(store.resources.get_graph_element(q.obj).clone());
                tmp.add_quad(dag_rdf::ingress::Quad {
                    triple_id: dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID,
                    subject: s,
                    predicate: p,
                    obj: o,
                });
            }
            let body = jsonld_parser::serialize_jsonld(&tmp);
            (
                StatusCode::OK,
                [("content-type", "application/ld+json; charset=utf-8")],
                body,
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_ACCEPTABLE,
            "No supported RDF format in Accept",
        )
            .into_response(),
    }
}

// ── Copy parsed triples into a named graph ────────────────────────────────────

/// Copy all triples from the default graph of `src` into `graph_id` of `dst`.
///
/// Resources are re-interned into `dst`'s `GraphElementManager` so IDs stay
/// consistent across the two stores.
fn copy_default_graph_to(
    src: &dag_rdf::Datastore,
    dst: &mut dag_rdf::Datastore,
    graph_id: GraphElementId,
) {
    let quads: Vec<_> = src
        .named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .collect();
    for q in quads {
        let s = dst.add_resource(src.resources.get_graph_element(q.subject).clone());
        let p = dst.add_resource(src.resources.get_graph_element(q.predicate).clone());
        let o = dst.add_resource(src.resources.get_graph_element(q.obj).clone());
        dst.add_quad(dag_rdf::ingress::Quad {
            triple_id: graph_id,
            subject: s,
            predicate: p,
            obj: o,
        });
    }
}

/// Copy all quads from `src` into `dst`, preserving named graph IRIs.
fn copy_dataset_to(src: &dag_rdf::Datastore, dst: &mut dag_rdf::Datastore) {
    for q in src.named_graphs.quad_list.iter().copied() {
        let graph = dst.add_resource(src.resources.get_graph_element(q.triple_id).clone());
        let s = dst.add_resource(src.resources.get_graph_element(q.subject).clone());
        let p = dst.add_resource(src.resources.get_graph_element(q.predicate).clone());
        let o = dst.add_resource(src.resources.get_graph_element(q.obj).clone());
        dst.add_quad(dag_rdf::ingress::Quad {
            triple_id: graph,
            subject: s,
            predicate: p,
            obj: o,
        });
    }
}

/// Translate quads from the default graph of `src` into `dst` IDs, using
/// `target_graph_id` as the graph slot.  Resources are interned into `dst`
/// if not already present.  Quads are NOT added to `dst`.
fn translate_default_graph_quads(
    src: &dag_rdf::Datastore,
    dst: &mut dag_rdf::Datastore,
    target_graph_id: GraphElementId,
) -> Vec<dag_rdf::ingress::Quad> {
    src.named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .map(|q| {
            let s = dst.add_resource(src.resources.get_graph_element(q.subject).clone());
            let p = dst.add_resource(src.resources.get_graph_element(q.predicate).clone());
            let o = dst.add_resource(src.resources.get_graph_element(q.obj).clone());
            dag_rdf::ingress::Quad {
                triple_id: target_graph_id,
                subject: s,
                predicate: p,
                obj: o,
            }
        })
        .collect()
}

/// Collect all quads in `graph_id` from `store` (already in main-store IDs).
fn collect_graph_quads(
    store: &dag_rdf::Datastore,
    graph_id: GraphElementId,
) -> Vec<dag_rdf::ingress::Quad> {
    store.named_graphs.get_graph(graph_id).collect()
}

fn graph_iri_for(tmp: &dag_rdf::Datastore, graph_id: GraphElementId) -> Option<String> {
    if graph_id == DEFAULT_GRAPH_ELEMENT_ID {
        return None;
    }
    match tmp.resources.get_graph_element(graph_id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri))) => Some(iri.clone()),
        _ => None,
    }
}

/// Parse `body` using the format indicated by `fmt` into a temporary `Datastore`.
///
/// `network` controls how JSON-LD external `@context` URLs are handled when `fmt` is JSON-LD.
#[allow(clippy::result_large_err)]
pub(crate) fn parse_rdf_body(
    body: &[u8],
    fmt: UploadFormat,
    network: NetworkPolicy,
) -> Result<dag_rdf::Datastore, axum::response::Response> {
    let mut tmp = dag_rdf::Datastore::new(256);
    let result: Result<(), String> = match fmt {
        UploadFormat::Turtle | UploadFormat::NTriples => {
            turtle::parse_turtle(&mut tmp, Cursor::new(body)).map_err(|e| e.to_string())
        }
        UploadFormat::NQuads => {
            turtle::parse_nquads(&mut tmp, Cursor::new(body)).map_err(|e| e.to_string())
        }
        UploadFormat::TriG => {
            turtle::parse_trig(&mut tmp, Cursor::new(body)).map_err(|e| e.to_string())
        }
        UploadFormat::JsonLd => match network {
            NetworkPolicy::Allow => {
                let loader =
                    std::sync::Arc::new(jsonld_parser::StaticDocumentLoader::with_schema_org());
                jsonld_parser::parse_jsonld_with_loader(&mut tmp, Cursor::new(body), loader)
                    .map_err(|e| e.to_string())
            }
            _ => jsonld_parser::parse_jsonld(&mut tmp, Cursor::new(body), network)
                .map_err(|e| e.to_string()),
        },
    };
    result
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("RDF parse error: {e}")).into_response())?;
    Ok(tmp)
}

// ── Handlers ─────────────────────────────────────────────────────────────────
//
// Each public handler is a thin wrapper that extracts axum State/Query then
// delegates to a `_inner` function.  The `_inner` functions take a plain
// `AppState` (and raw params), so per-dataset route handlers can call them
// after substituting the dataset-specific store.

pub async fn gsp_get(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    gsp_get_inner(state, params, headers).await
}

pub async fn gsp_get_inner(
    state: AppState,
    params: HashMap<String, String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let store = state.store.read().await;
    let accept = headers.get("accept").and_then(|v| v.to_str().ok());

    // No graph param → whole-dataset response for multi-graph formats (Fuseki extension).
    if !params.contains_key("default") && !params.contains_key("graph") {
        return match negotiate_rdf_format(accept) {
            Some(RdfFormat::NQuads) => (
                StatusCode::OK,
                [("content-type", "application/n-quads; version=1.2")],
                serialize_nquads(&store),
            )
                .into_response(),
            Some(RdfFormat::TriG) => (
                StatusCode::OK,
                [("content-type", "application/trig; version=1.2")],
                serialize_trig(&store),
            )
                .into_response(),
            // Fuseki extension (Bravo/records compat): whole-dataset JSON-LD.
            // `serialize_jsonld` already covers the entire Datastore (default
            // graph plus every named graph, wrapped per JSON-LD's dataset
            // representation) — see #219.
            Some(RdfFormat::JsonLd) => (
                StatusCode::OK,
                [("content-type", "application/ld+json; charset=utf-8")],
                jsonld_parser::serialize_jsonld(&store),
            )
                .into_response(),
            _ => (
                StatusCode::BAD_REQUEST,
                "GET /rdf-graph-store requires ?default or ?graph=<iri>",
            )
                .into_response(),
        };
    }

    let graph_id = match resolve_read_target(&params, &store) {
        Ok(ReadTarget::Default) => DEFAULT_GRAPH_ELEMENT_ID,
        Ok(ReadTarget::Named(id)) => id,
        Err(r) => return r,
    };
    graph_response_parts(&store, graph_id, accept)
}

pub async fn gsp_head(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    gsp_head_inner(state, params, headers).await
}

pub async fn gsp_head_inner(
    state: AppState,
    params: HashMap<String, String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let store = state.store.read().await;
    let accept = headers.get("accept").and_then(|v| v.to_str().ok());

    // Mirror gsp_get_inner: no-param whole-dataset for HEAD too.
    if !params.contains_key("default") && !params.contains_key("graph") {
        let ct = match negotiate_rdf_format(accept) {
            Some(RdfFormat::NQuads) => "application/n-quads",
            Some(RdfFormat::TriG) => "application/trig",
            // Mirrors the JsonLd arm in gsp_get_inner — see #219.
            Some(RdfFormat::JsonLd) => "application/ld+json; charset=utf-8",
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    "GET /rdf-graph-store requires ?default or ?graph=<iri>",
                )
                    .into_response();
            }
        };
        return (StatusCode::OK, [("content-type", ct)], "").into_response();
    }

    let graph_id = match resolve_read_target(&params, &store) {
        Ok(ReadTarget::Default) => DEFAULT_GRAPH_ELEMENT_ID,
        Ok(ReadTarget::Named(id)) => id,
        Err(r) => return r,
    };
    let get_resp = graph_response_parts(&store, graph_id, accept);
    let (parts, _body) = get_resp.into_parts();
    Response::from_parts(parts, Body::empty())
}

pub async fn gsp_put(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    gsp_put_inner(state, params, headers, body).await
}

pub async fn gsp_put_inner(
    state: AppState,
    params: HashMap<String, String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let fmt = match rdf_upload_format(ct) {
        Some(f) => f,
        None => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported Content-Type for RDF upload",
            )
                .into_response();
        }
    };

    let tmp = match parse_rdf_body(&body, fmt, state.network_policy.clone()) {
        Ok(t) => t,
        Err(r) => return r,
    };

    // Acquire the store write lock FIRST, then commit to changelog inside the
    // critical section.  This ensures commit-order == apply-order and prevents
    // concurrent writes from diverging the live state from the durable log.
    let mut store = state.store.write().await;
    let (graph_id, is_new) = match resolve_write_target(&params, &mut store) {
        Ok(WriteTarget::Default) => (DEFAULT_GRAPH_ELEMENT_ID, false),
        Ok(WriteTarget::Named { id, is_new }) => (id, is_new),
        Err(r) => return r,
    };

    let graph_iri: Option<String> = params.get("graph").cloned();
    if let Some(ref changelog) = state.changelog {
        let mut entries = Vec::new();
        entries.push(LogEntry::ClearGraph {
            graph: graph_iri.clone(),
        });
        for q in tmp.named_graphs.get_graph(DEFAULT_GRAPH_ELEMENT_ID) {
            entries.push(LogEntry::InsertQuad {
                graph: graph_iri.clone(),
                s: to_repr(tmp.resources.get_graph_element(q.subject)),
                p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                o: to_repr(tmp.resources.get_graph_element(q.obj)),
            });
        }
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.append_batch(&entries) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }

    if let Some(ref reasoner_arc) = state.reasoner {
        let old_quads = collect_graph_quads(&store, graph_id);
        let new_quads = translate_default_graph_quads(&tmp, &mut store, graph_id);
        let mut reasoner = reasoner_arc.lock().await;
        let existing_old: Vec<_> = old_quads
            .into_iter()
            .filter(|q| store.named_graphs.contains(q))
            .collect();
        reasoner.apply_deletions(&mut store, &existing_old);
        reasoner.apply_insertions(&mut store, &new_quads);
    } else {
        store.remove_graph(graph_id);
        copy_default_graph_to(&tmp, &mut store, graph_id);
    }

    if is_new {
        StatusCode::CREATED.into_response()
    } else {
        StatusCode::NO_CONTENT.into_response()
    }
}

pub async fn gsp_delete(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> axum::response::Response {
    gsp_delete_inner(state, params).await
}

pub async fn gsp_delete_inner(
    state: AppState,
    params: HashMap<String, String>,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    // Validate params early (before taking any locks).
    let graph_iri: Option<String> = if params.contains_key("default") {
        None
    } else if let Some(iri) = params.get("graph") {
        if !is_absolute_iri(iri) {
            return (StatusCode::BAD_REQUEST, "graph IRI must be absolute").into_response();
        }
        Some(iri.clone())
    } else {
        return (
            StatusCode::BAD_REQUEST,
            "DELETE /rdf-graph-store requires ?default or ?graph=<iri>",
        )
            .into_response();
    };

    // Acquire the store write lock first, then commit to changelog inside the
    // critical section so commit-order == apply-order.
    let mut store = state.store.write().await;

    let graph_id = match &graph_iri {
        None => DEFAULT_GRAPH_ELEMENT_ID,
        Some(iri) => match store.lookup_named_graph_id(iri) {
            Some(id) if store.named_graph_exists(id) => id,
            _ => return (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
        },
    };

    if let Some(ref changelog) = state.changelog {
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.log_clear_graph(graph_iri.as_deref()) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }

    if let Some(ref reasoner_arc) = state.reasoner {
        let old_quads = collect_graph_quads(&store, graph_id);
        let existing: Vec<_> = old_quads
            .into_iter()
            .filter(|q| store.named_graphs.contains(q))
            .collect();
        let mut reasoner = reasoner_arc.lock().await;
        reasoner.apply_deletions(&mut store, &existing);
    } else {
        store.remove_graph(graph_id);
    }
    StatusCode::NO_CONTENT.into_response()
}

pub async fn gsp_post(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    // The standard GSP endpoint follows spec §5.5: POST to ?graph=<iri> that
    // does not exist returns 404 (SHOULD, not creates).
    gsp_post_inner(state, params, headers, body, false).await
}

pub async fn gsp_post_inner(
    state: AppState,
    params: HashMap<String, String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
    // Fuseki compat: create the named graph if it doesn't already exist.
    // The W3C GSP spec §5.5 says SHOULD 404 for nonexistent graphs; Fuseki's
    // per-dataset /data endpoint creates them instead so existing clients work.
    create_if_missing: bool,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let fmt = match rdf_upload_format(ct) {
        Some(f) => f,
        None => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported Content-Type for RDF upload",
            )
                .into_response();
        }
    };

    if body.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }

    let tmp = match parse_rdf_body(&body, fmt, state.network_policy.clone()) {
        Ok(t) => t,
        Err(r) => return r,
    };

    // ── Case 1: ?default — merge into default graph ───────────────────────────
    if params.contains_key("default") {
        // Acquire write lock first, then commit changelog under it.
        let mut store = state.store.write().await;
        if let Some(ref changelog) = state.changelog {
            let entries: Vec<_> = tmp
                .named_graphs
                .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
                .map(|q| LogEntry::InsertQuad {
                    graph: None,
                    s: to_repr(tmp.resources.get_graph_element(q.subject)),
                    p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                    o: to_repr(tmp.resources.get_graph_element(q.obj)),
                })
                .collect();
            let mut cl = changelog.lock().await;
            if let Err(e) = cl.append_batch(&entries) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("persistence error: {e}"),
                )
                    .into_response();
            }
        }
        if let Some(ref reasoner_arc) = state.reasoner {
            let new_quads =
                translate_default_graph_quads(&tmp, &mut store, DEFAULT_GRAPH_ELEMENT_ID);
            let mut reasoner = reasoner_arc.lock().await;
            reasoner.apply_insertions(&mut store, &new_quads);
        } else {
            copy_default_graph_to(&tmp, &mut store, DEFAULT_GRAPH_ELEMENT_ID);
        }
        return StatusCode::NO_CONTENT.into_response();
    }

    // ── Case 2: ?graph=<iri> — merge into a named graph ─────────────────────
    //
    // Spec §5.5 SHOULD: return 404 when the named graph does not exist.
    // Fuseki extension: create-if-missing (create_if_missing=true).
    if let Some(iri) = params.get("graph") {
        if !is_absolute_iri(iri) {
            return (StatusCode::BAD_REQUEST, "graph IRI must be absolute").into_response();
        }
        // Acquire write lock first, then check existence and commit changelog.
        let mut store = state.store.write().await;
        let (graph_id, created) = match store.lookup_named_graph_id(iri) {
            Some(id) if store.named_graph_exists(id) => (id, false),
            _ if create_if_missing => {
                let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.clone())));
                (store.resources.add_resource(elem), true)
            }
            _ => return (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
        };
        if let Some(ref changelog) = state.changelog {
            let entries: Vec<_> = tmp
                .named_graphs
                .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
                .map(|q| LogEntry::InsertQuad {
                    graph: Some(iri.clone()),
                    s: to_repr(tmp.resources.get_graph_element(q.subject)),
                    p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                    o: to_repr(tmp.resources.get_graph_element(q.obj)),
                })
                .collect();
            let mut cl = changelog.lock().await;
            if let Err(e) = cl.append_batch(&entries) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("persistence error: {e}"),
                )
                    .into_response();
            }
        }
        if let Some(ref reasoner_arc) = state.reasoner {
            let new_quads = translate_default_graph_quads(&tmp, &mut store, graph_id);
            let mut reasoner = reasoner_arc.lock().await;
            reasoner.apply_insertions(&mut store, &new_quads);
        } else {
            copy_default_graph_to(&tmp, &mut store, graph_id);
        }
        return if created {
            StatusCode::CREATED.into_response()
        } else {
            StatusCode::NO_CONTENT.into_response()
        };
    }

    // ── Case 3: no param — create a new graph with a server-assigned IRI ─────
    if fmt == UploadFormat::TriG || fmt == UploadFormat::NQuads || fmt == UploadFormat::JsonLd {
        let mut store = state.store.write().await;
        if let Some(ref changelog) = state.changelog {
            let entries: Vec<_> = tmp
                .named_graphs
                .quad_list
                .iter()
                .copied()
                .map(|q| LogEntry::InsertQuad {
                    graph: graph_iri_for(&tmp, q.triple_id),
                    s: to_repr(tmp.resources.get_graph_element(q.subject)),
                    p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                    o: to_repr(tmp.resources.get_graph_element(q.obj)),
                })
                .collect();
            let mut cl = changelog.lock().await;
            if let Err(e) = cl.append_batch(&entries) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("persistence error: {e}"),
                )
                    .into_response();
            }
        }
        if let Some(ref reasoner_arc) = state.reasoner {
            // Translate all quads across all graphs, then feed to reasoner.
            let new_quads: Vec<_> = tmp
                .named_graphs
                .quad_list
                .iter()
                .copied()
                .map(|q| {
                    let g =
                        store.add_resource(tmp.resources.get_graph_element(q.triple_id).clone());
                    let s = store.add_resource(tmp.resources.get_graph_element(q.subject).clone());
                    let p =
                        store.add_resource(tmp.resources.get_graph_element(q.predicate).clone());
                    let o = store.add_resource(tmp.resources.get_graph_element(q.obj).clone());
                    dag_rdf::ingress::Quad {
                        triple_id: g,
                        subject: s,
                        predicate: p,
                        obj: o,
                    }
                })
                .collect();
            let mut reasoner = reasoner_arc.lock().await;
            reasoner.apply_insertions(&mut store, &new_quads);
        } else {
            copy_dataset_to(&tmp, &mut store);
        }
        return StatusCode::NO_CONTENT.into_response();
    }

    let new_iri = {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{}/rdf-graph-store/graph/{}", state.config.base_iri, ns)
    };
    // Acquire write lock, then commit changelog, then mutate memory.
    let mut store = state.store.write().await;
    if let Some(ref changelog) = state.changelog {
        let entries: Vec<_> = tmp
            .named_graphs
            .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
            .map(|q| LogEntry::InsertQuad {
                graph: Some(new_iri.clone()),
                s: to_repr(tmp.resources.get_graph_element(q.subject)),
                p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                o: to_repr(tmp.resources.get_graph_element(q.obj)),
            })
            .collect();
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.append_batch(&entries) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }
    let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(new_iri.clone())));
    let graph_id = store.resources.add_resource(elem);
    if let Some(ref reasoner_arc) = state.reasoner {
        let new_quads = translate_default_graph_quads(&tmp, &mut store, graph_id);
        let mut reasoner = reasoner_arc.lock().await;
        reasoner.apply_insertions(&mut store, &new_quads);
    } else {
        copy_default_graph_to(&tmp, &mut store, graph_id);
    }
    let location = format!(
        "{}/rdf-graph-store?graph={}",
        state.config.base_iri,
        percent_encode(&new_iri)
    );
    (StatusCode::CREATED, [("location", location.as_str())], "").into_response()
}

// ── Direct graph identification (§4.1, optional) ─────────────────────────────
//
// In the direct model the request URI IS the named graph IRI.
// Route: /rdf-graphs/{*path}
//
// Spec §4.1: <https://www.w3.org/TR/sparql11-http-rdf-update/#direct-graph-identification>

/// Build the graph IRI for a direct-identification request.
fn direct_graph_iri(base_iri: &str, path: &str) -> String {
    format!("{}/rdf-graphs/{}", base_iri.trim_end_matches('/'), path)
}

/// `GET /rdf-graphs/*path` — retrieve a named graph by its request URI.
pub async fn direct_gsp_get(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let graph_iri = direct_graph_iri(&state.config.base_iri, &path);
    let store = state.store.read().await;
    match store.lookup_named_graph_id(&graph_iri) {
        Some(id) if store.named_graph_exists(id) => {
            let accept = headers.get("accept").and_then(|v| v.to_str().ok());
            graph_response_parts(&store, id, accept)
        }
        _ => (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
    }
}

/// `PUT /rdf-graphs/*path` — replace (or create) a named graph at its request URI.
///
/// Returns 201 Created for new graphs, 204 No Content for replacements.
pub async fn direct_gsp_put(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let fmt = match rdf_upload_format(ct) {
        Some(f) => f,
        None => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported Content-Type for RDF upload",
            )
                .into_response();
        }
    };
    let tmp = match parse_rdf_body(&body, fmt, state.network_policy.clone()) {
        Ok(t) => t,
        Err(r) => return r,
    };
    let graph_iri = direct_graph_iri(&state.config.base_iri, &path);
    let mut store = state.store.write().await;
    // Check new-ness before interning (interning would make the lookup return Some).
    let is_new = store.lookup_named_graph_id(&graph_iri).is_none();
    let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(graph_iri.clone())));
    let graph_id = store.resources.add_resource(elem);

    if let Some(ref changelog) = state.changelog {
        let mut entries = vec![LogEntry::ClearGraph {
            graph: Some(graph_iri.clone()),
        }];
        for q in tmp.named_graphs.get_graph(DEFAULT_GRAPH_ELEMENT_ID) {
            entries.push(LogEntry::InsertQuad {
                graph: Some(graph_iri.clone()),
                s: to_repr(tmp.resources.get_graph_element(q.subject)),
                p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                o: to_repr(tmp.resources.get_graph_element(q.obj)),
            });
        }
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.append_batch(&entries) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }

    if let Some(ref reasoner_arc) = state.reasoner {
        let old_quads = collect_graph_quads(&store, graph_id);
        let new_quads = translate_default_graph_quads(&tmp, &mut store, graph_id);
        let existing_old: Vec<_> = old_quads
            .into_iter()
            .filter(|q| store.named_graphs.contains(q))
            .collect();
        let mut reasoner = reasoner_arc.lock().await;
        reasoner.apply_deletions(&mut store, &existing_old);
        reasoner.apply_insertions(&mut store, &new_quads);
    } else {
        store.remove_graph(graph_id);
        copy_default_graph_to(&tmp, &mut store, graph_id);
    }
    if is_new {
        StatusCode::CREATED.into_response()
    } else {
        StatusCode::NO_CONTENT.into_response()
    }
}

/// `DELETE /rdf-graphs/*path` — remove the named graph whose IRI is the request URI.
///
/// Returns 204 No Content on success, 404 if the graph does not exist.
pub async fn direct_gsp_delete(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }
    let graph_iri = direct_graph_iri(&state.config.base_iri, &path);
    let mut store = state.store.write().await;
    let graph_id = match store.lookup_named_graph_id(&graph_iri) {
        Some(id) if store.named_graph_exists(id) => id,
        _ => return (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
    };

    if let Some(ref changelog) = state.changelog {
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.log_clear_graph(Some(&graph_iri)) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }

    if let Some(ref reasoner_arc) = state.reasoner {
        let old_quads = collect_graph_quads(&store, graph_id);
        let existing: Vec<_> = old_quads
            .into_iter()
            .filter(|q| store.named_graphs.contains(q))
            .collect();
        let mut reasoner = reasoner_arc.lock().await;
        reasoner.apply_deletions(&mut store, &existing);
    } else {
        store.remove_graph(graph_id);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `HEAD /rdf-graphs/*path` — return headers only for the graph at the request URI.
pub async fn direct_gsp_head(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let graph_iri = direct_graph_iri(&state.config.base_iri, &path);
    let store = state.store.read().await;
    let id = match store.lookup_named_graph_id(&graph_iri) {
        Some(id) if store.named_graph_exists(id) => id,
        _ => return (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
    };
    let accept = headers.get("accept").and_then(|v| v.to_str().ok());
    let get_resp = graph_response_parts(&store, id, accept);
    let (parts, _body) = get_resp.into_parts();
    Response::from_parts(parts, Body::empty())
}

/// `POST /rdf-graphs/*path` — merge triples into the named graph at the request URI.
///
/// Returns 200/204 on success, 404 if the graph does not exist, 415 for an
/// unsupported Content-Type, 400 for a parse error.
pub async fn direct_gsp_post(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let fmt = match rdf_upload_format(ct) {
        Some(f) => f,
        None => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported Content-Type for RDF upload",
            )
                .into_response();
        }
    };
    let tmp = match parse_rdf_body(&body, fmt, state.network_policy.clone()) {
        Ok(t) => t,
        Err(r) => return r,
    };
    let graph_iri = direct_graph_iri(&state.config.base_iri, &path);
    let mut store = state.store.write().await;
    let graph_id = match store.lookup_named_graph_id(&graph_iri) {
        Some(id) if store.named_graph_exists(id) => id,
        _ => return (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
    };

    if let Some(ref changelog) = state.changelog {
        let entries: Vec<_> = tmp
            .named_graphs
            .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
            .map(|q| LogEntry::InsertQuad {
                graph: Some(graph_iri.clone()),
                s: to_repr(tmp.resources.get_graph_element(q.subject)),
                p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                o: to_repr(tmp.resources.get_graph_element(q.obj)),
            })
            .collect();
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.append_batch(&entries) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }

    if let Some(ref reasoner_arc) = state.reasoner {
        let new_quads = translate_default_graph_quads(&tmp, &mut store, graph_id);
        let mut reasoner = reasoner_arc.lock().await;
        reasoner.apply_insertions(&mut store, &new_quads);
    } else {
        copy_default_graph_to(&tmp, &mut store, graph_id);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Percent-encode all bytes that are not unreserved URI characters.
fn percent_encode(s: &str) -> String {
    s.bytes()
        .fold(String::with_capacity(s.len() * 3), |mut acc, b| {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    acc.push(b as char)
                }
                _ => acc.push_str(&format!("%{:02X}", b)),
            }
            acc
        })
}

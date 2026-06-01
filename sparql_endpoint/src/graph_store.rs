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

use crate::{AppState, serialize::serialize_graph};
use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use dag_rdf::{
    GraphElement, GraphElementId, IriReference, RdfResource, ingress::DEFAULT_GRAPH_ELEMENT_ID,
};
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

enum RdfFormat {
    Turtle,
    NTriples,
}

/// Negotiate an RDF serialisation format from an `Accept` header value.
///
/// Returns `None` if the client's `Accept` excludes all supported RDF formats
/// (caller should respond `406 Not Acceptable`).
///
/// Supported: `text/turtle`, `application/n-triples`, `*/*`, `text/*`.
fn negotiate_rdf_format(accept: Option<&str>) -> Option<RdfFormat> {
    let accept = match accept {
        None | Some("") => return Some(RdfFormat::Turtle),
        Some(a) => a,
    };
    for part in accept.split(',') {
        let mime = part.split(';').next().unwrap_or("").trim();
        match mime {
            "text/turtle" | "text/*" | "*/*" => return Some(RdfFormat::Turtle),
            "application/n-triples" => return Some(RdfFormat::NTriples),
            _ => {}
        }
    }
    None
}

fn is_turtle_content_type(ct: &str) -> bool {
    let mime = ct.split(';').next().unwrap_or("").trim();
    matches!(
        mime,
        "text/turtle" | "application/x-turtle" | "application/n-triples" | "application/rdf+xml"
    )
}

// ── Shared graph serialisation ────────────────────────────────────────────────

/// Build the GET response body for a graph, or a 406 if the Accept is unsatisfied.
fn graph_response_parts(
    store: &dag_rdf::Datastore,
    graph_id: GraphElementId,
    accept: Option<&str>,
) -> axum::response::Response {
    match negotiate_rdf_format(accept) {
        Some(RdfFormat::Turtle) => {
            let body = serialize_graph(store, graph_id);
            (
                StatusCode::OK,
                [("content-type", "text/turtle; charset=utf-8")],
                body,
            )
                .into_response()
        }
        Some(RdfFormat::NTriples) => {
            let body = serialize_graph(store, graph_id);
            (
                StatusCode::OK,
                [("content-type", "application/n-triples")],
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
        dst.named_graphs.add_quad(dag_rdf::ingress::Quad {
            triple_id: graph_id,
            subject: s,
            predicate: p,
            obj: o,
        });
    }
}

/// Parse `body` as Turtle into a temporary `Datastore`.
///
/// Returns `Err(400-response)` on parse failure.
#[allow(clippy::result_large_err)]
fn parse_turtle_body(body: &[u8]) -> Result<dag_rdf::Datastore, axum::response::Response> {
    let mut tmp = dag_rdf::Datastore::new(256);
    turtle::parse_turtle(&mut tmp, Cursor::new(body)).map_err(|e| {
        (StatusCode::BAD_REQUEST, format!("Turtle parse error: {e}")).into_response()
    })?;
    Ok(tmp)
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /rdf-graph-store?default` or `?graph=<iri>`
///
/// Spec §5.2: <https://www.w3.org/TR/sparql11-http-rdf-update/#http-get>
pub async fn gsp_get(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let store = state.store.read().await;
    let graph_id = match resolve_read_target(&params, &store) {
        Ok(ReadTarget::Default) => DEFAULT_GRAPH_ELEMENT_ID,
        Ok(ReadTarget::Named(id)) => id,
        Err(r) => return r,
    };
    let accept = headers.get("accept").and_then(|v| v.to_str().ok());
    graph_response_parts(&store, graph_id, accept)
}

/// `HEAD /rdf-graph-store?default` or `?graph=<iri>`
///
/// Spec §5.6: "identical to GET except that the server MUST NOT return a
/// message-body."
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-head>
pub async fn gsp_head(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let store = state.store.read().await;
    let graph_id = match resolve_read_target(&params, &store) {
        Ok(ReadTarget::Default) => DEFAULT_GRAPH_ELEMENT_ID,
        Ok(ReadTarget::Named(id)) => id,
        Err(r) => return r,
    };
    let accept = headers.get("accept").and_then(|v| v.to_str().ok());
    // Build the same response as GET, then strip the body.
    let get_resp = graph_response_parts(&store, graph_id, accept);
    let (parts, _body) = get_resp.into_parts();
    Response::from_parts(parts, Body::empty())
}

/// `PUT /rdf-graph-store?default` or `?graph=<iri>`
///
/// Spec §5.3: drop existing content, insert the payload.
/// New graph → 201 Created; existing graph → 204 No Content.
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-put>
pub async fn gsp_put(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
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
    if !is_turtle_content_type(ct) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be text/turtle or application/n-triples",
        )
            .into_response();
    }

    let tmp = match parse_turtle_body(&body) {
        Ok(t) => t,
        Err(r) => return r,
    };

    let mut store = state.store.write().await;
    let (graph_id, is_new) = match resolve_write_target(&params, &mut store) {
        Ok(WriteTarget::Default) => (DEFAULT_GRAPH_ELEMENT_ID, false),
        Ok(WriteTarget::Named { id, is_new }) => (id, is_new),
        Err(r) => return r,
    };

    // DROP SILENT + INSERT DATA
    store.remove_graph(graph_id);
    copy_default_graph_to(&tmp, &mut store, graph_id);

    if is_new {
        StatusCode::CREATED.into_response()
    } else {
        StatusCode::NO_CONTENT.into_response()
    }
}

/// `DELETE /rdf-graph-store?default` or `?graph=<iri>`
///
/// Spec §5.4: drop the graph; 404 if named graph does not exist.
/// <https://www.w3.org/TR/sparql11-http-rdf-update/#http-delete>
pub async fn gsp_delete(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let mut store = state.store.write().await;

    if params.contains_key("default") {
        store.remove_graph(DEFAULT_GRAPH_ELEMENT_ID);
        return StatusCode::NO_CONTENT.into_response();
    }

    if let Some(iri) = params.get("graph") {
        if !is_absolute_iri(iri) {
            return (StatusCode::BAD_REQUEST, "graph IRI must be absolute").into_response();
        }
        return match store.lookup_named_graph_id(iri) {
            Some(id) if store.named_graph_exists(id) => {
                store.remove_graph(id);
                StatusCode::NO_CONTENT.into_response()
            }
            _ => (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
        };
    }

    (
        StatusCode::BAD_REQUEST,
        "DELETE /rdf-graph-store requires ?default or ?graph=<iri>",
    )
        .into_response()
}

/// `POST /rdf-graph-store`        → create a new graph (server assigns IRI)
/// `POST /rdf-graph-store?default` → merge into the default graph
/// `POST /rdf-graph-store?graph=<iri>` → merge into named graph (404 if absent)
///
/// Spec §5.5: <https://www.w3.org/TR/sparql11-http-rdf-update/#http-post>
pub async fn gsp_post(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
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
    if !is_turtle_content_type(ct) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be text/turtle or application/n-triples",
        )
            .into_response();
    }

    // Empty body → 204 No Content (spec §5.5)
    if body.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }

    let tmp = match parse_turtle_body(&body) {
        Ok(t) => t,
        Err(r) => return r,
    };

    let mut store = state.store.write().await;

    // POST to ?default → merge into default graph
    if params.contains_key("default") {
        copy_default_graph_to(&tmp, &mut store, DEFAULT_GRAPH_ELEMENT_ID);
        return StatusCode::NO_CONTENT.into_response();
    }

    // POST to ?graph=<iri> → merge into named graph (404 if absent)
    if let Some(iri) = params.get("graph") {
        if !is_absolute_iri(iri) {
            return (StatusCode::BAD_REQUEST, "graph IRI must be absolute").into_response();
        }
        return match store.lookup_named_graph_id(iri) {
            Some(id) if store.named_graph_exists(id) => {
                copy_default_graph_to(&tmp, &mut store, id);
                StatusCode::NO_CONTENT.into_response()
            }
            _ => (StatusCode::NOT_FOUND, "Named graph not found").into_response(),
        };
    }

    // POST to the Graph Store itself → create a new graph with a server-assigned IRI
    let new_iri = {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{}/rdf-graph-store/graph/{}", state.config.base_iri, ns)
    };
    let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(new_iri.clone())));
    let graph_id = store.resources.add_resource(elem);
    copy_default_graph_to(&tmp, &mut store, graph_id);

    // Return 201 with Location pointing to the indirect-identification URL
    let location = format!(
        "{}/rdf-graph-store?graph={}",
        state.config.base_iri,
        percent_encode(&new_iri)
    );
    (StatusCode::CREATED, [("location", location.as_str())], "").into_response()
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

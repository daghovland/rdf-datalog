/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Handler for POST /upload — accepts Turtle RDF and merges into the default
//! graph, or into a named graph when `?graph=<iri>` is given.
//!
//! This is a convenience endpoint for interactive use via the browser UI.
//! It will be superseded by the SPARQL Graph Store HTTP Protocol (see SERVER.md).

use crate::{
    AppState,
    persistence::{LogEntry, to_repr},
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use dag_rdf::{Datastore, GraphElement, GraphElementId, IriReference, RdfResource};
use dag_rdf::{ingress::DEFAULT_GRAPH_ELEMENT_ID, ingress::Quad};
use std::{collections::HashMap, io::Cursor};

/// Validate that `iri` is an absolute IRI (has a syntactically valid scheme).
///
/// Mirrors `graph_store::is_absolute_iri` — kept local since that helper is
/// private to `graph_store` and this endpoint predates the GSP module.
fn is_absolute_iri(iri: &str) -> bool {
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

pub async fn upload_turtle(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type.contains("text/turtle") && !content_type.contains("application/x-turtle") {
        return (
            StatusCode::BAD_REQUEST,
            "Content-Type must be text/turtle or application/x-turtle",
        )
            .into_response();
    }

    // Optional named-graph target (issue #44). An absent or empty value
    // falls back to the default graph, preserving prior behavior.
    let graph_iri: Option<String> = match params.get("graph") {
        Some(iri) if !iri.is_empty() => {
            if !is_absolute_iri(iri) {
                return (StatusCode::BAD_REQUEST, "graph IRI must be absolute").into_response();
            }
            Some(iri.clone())
        }
        _ => None,
    };

    // Parse into a temporary store so we can enumerate the inserted quads for
    // the persistence changelog before applying them to the real store.
    // Use body length / 50 as a rough triple-count estimate (~50 bytes/triple in Turtle).
    let size_hint = ((body.len() / 50) as u32).max(256);
    let mut tmp = Datastore::new(size_hint);
    if let Err(e) = turtle::parse_turtle(&mut tmp, Cursor::new(body.to_vec())) {
        return (StatusCode::BAD_REQUEST, format!("Turtle parse error: {e}")).into_response();
    }

    let mut store = state.store.write().await;

    // Resolve (and, if necessary, intern) the target graph id. Named graphs
    // are created on first upload — this is a convenience endpoint, so we
    // don't require the graph to pre-exist (unlike the GSP PUT/POST 404
    // behavior for indirect graph identification).
    let target_graph_id: GraphElementId = match &graph_iri {
        None => DEFAULT_GRAPH_ELEMENT_ID,
        Some(iri) => {
            let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.clone())));
            store.add_resource(elem)
        }
    };

    if let Some(ref changelog) = state.changelog {
        let entries: Vec<_> = tmp
            .named_graphs
            .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
            .map(|q| LogEntry::InsertQuad {
                graph: graph_iri.clone(),
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

    // Copy parsed triples from tmp into the real store's target graph.
    let quads: Vec<_> = tmp
        .named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .collect();
    for q in quads {
        let s = store.add_resource(tmp.resources.get_graph_element(q.subject).clone());
        let p = store.add_resource(tmp.resources.get_graph_element(q.predicate).clone());
        let o = store.add_resource(tmp.resources.get_graph_element(q.obj).clone());
        store.named_graphs.add_quad(Quad {
            triple_id: target_graph_id,
            subject: s,
            predicate: p,
            obj: o,
        });
    }

    (StatusCode::OK, "Data uploaded successfully").into_response()
}

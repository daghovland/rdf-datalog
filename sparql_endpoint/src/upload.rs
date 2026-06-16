/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Handler for POST /upload — accepts Turtle RDF and merges into the default graph.
//!
//! This is a convenience endpoint for interactive use via the browser UI.
//! It will be superseded by the SPARQL Graph Store HTTP Protocol (see SERVER.md).

use crate::{
    AppState,
    persistence::{LogEntry, to_repr},
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use dag_rdf::{Datastore, ingress::DEFAULT_GRAPH_ELEMENT_ID};
use std::io::Cursor;

pub async fn upload_turtle(
    State(state): State<AppState>,
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

    // Parse into a temporary store so we can enumerate the inserted quads for
    // the persistence changelog before applying them to the real store.
    // Use body length / 50 as a rough triple-count estimate (~50 bytes/triple in Turtle).
    let size_hint = ((body.len() / 50) as u32).max(256);
    let mut tmp = Datastore::new(size_hint);
    if let Err(e) = turtle::parse_turtle(&mut tmp, Cursor::new(body.to_vec())) {
        return (StatusCode::BAD_REQUEST, format!("Turtle parse error: {e}")).into_response();
    }

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

    // Copy parsed triples from tmp into the real store's default graph.
    let quads: Vec<_> = tmp
        .named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .collect();
    for q in quads {
        let s = store.add_resource(tmp.resources.get_graph_element(q.subject).clone());
        let p = store.add_resource(tmp.resources.get_graph_element(q.predicate).clone());
        let o = store.add_resource(tmp.resources.get_graph_element(q.obj).clone());
        store.named_graphs.add_quad(dag_rdf::ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s,
            predicate: p,
            obj: o,
        });
    }

    (StatusCode::OK, "Data uploaded successfully").into_response()
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `POST /{name}/ottr` — expand stOTTR templates/instances (uploaded as
//! `multipart/form-data`) into a named dataset.
//!
//! Request:
//! ```text
//! POST /{dataset}/ottr
//! Content-Type: multipart/form-data; boundary=...
//!
//! --boundary
//! Content-Disposition: form-data; name="document"
//!
//! [stOTTR template + instance text]
//! --boundary--
//! ```
//!
//! Each part is a self-contained (or partial) stOTTR document; all parsed
//! documents are merged (templates pooled, instances concatenated) via
//! `ottr::expand_documents` before expansion, so templates and instances may
//! be split across separate parts. Part names carry no meaning.
//!
//! See `docs/plans/OTTR_HTTP_ENDPOINT_PLAN.md` for the full design.
//!
//! Spec: <https://spec.ottr.xyz/>

use crate::AppState;
use crate::persistence::{LogEntry, to_repr};
use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dag_rdf::{Datastore, GraphElementId, ingress::DEFAULT_GRAPH_ELEMENT_ID};
use ottr::ast::StottrDocument;

/// The named-graph IRI a quad belongs to, or `None` for the default graph.
///
/// Identical to `rml_endpoint::graph_iri_for` — kept as a separate copy since
/// the two endpoints are independent adapters and neither should depend on
/// the other's internals.
fn graph_iri_for(store: &Datastore, graph_id: GraphElementId) -> Option<String> {
    if graph_id == DEFAULT_GRAPH_ELEMENT_ID {
        return None;
    }
    store
        .resources
        .get_named_resource(graph_id)
        .map(|iri| iri.0.clone())
}

/// Read every part of the multipart body as a stOTTR document.
///
/// Returns `400 Bad Request` if there are zero parts, or if any part fails
/// to parse as stOTTR.
#[allow(clippy::result_large_err)]
async fn parse_multipart_documents(
    multipart: &mut Multipart,
) -> Result<Vec<StottrDocument>, Response> {
    let mut docs = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("multipart error: {e}")).into_response())?
    {
        let text = field.text().await.map_err(|e| {
            (StatusCode::BAD_REQUEST, format!("multipart error: {e}")).into_response()
        })?;
        let doc = ottr::parse_stottr(&text).map_err(|e| {
            (StatusCode::BAD_REQUEST, format!("stOTTR parse error: {e}")).into_response()
        })?;
        docs.push(doc);
    }

    if docs.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "no stOTTR document parts in request",
        )
            .into_response());
    }

    Ok(docs)
}

pub async fn dataset_ottr_post(
    State(state): State<AppState>,
    Path(name): Path<String>,
    mut multipart: Multipart,
) -> Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let Some(ds_lock) = state.registry.read().await.get(&name) else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };

    let docs = match parse_multipart_documents(&mut multipart).await {
        Ok(d) => d,
        Err(resp) => return resp,
    };

    let mut tmp_store = Datastore::new(1024);
    if let Err(e) = ottr::expand_documents(&docs, &mut tmp_store) {
        return (
            StatusCode::BAD_REQUEST,
            format!("OTTR expansion error: {e}"),
        )
            .into_response();
    }

    let quads: Vec<_> = tmp_store.named_graphs.get_all_quads().collect();

    if let Some(ref changelog) = state.changelog {
        let entries: Vec<_> = quads
            .iter()
            .map(|q| LogEntry::InsertQuad {
                graph: graph_iri_for(&tmp_store, q.triple_id),
                s: to_repr(tmp_store.resources.get_graph_element(q.subject)),
                p: to_repr(tmp_store.resources.get_graph_element(q.predicate)),
                o: to_repr(tmp_store.resources.get_graph_element(q.obj)),
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

    let mut store = ds_lock.write().await;
    for q in &quads {
        let graph = store.add_resource(tmp_store.resources.get_graph_element(q.triple_id).clone());
        let s = store.add_resource(tmp_store.resources.get_graph_element(q.subject).clone());
        let p = store.add_resource(tmp_store.resources.get_graph_element(q.predicate).clone());
        let o = store.add_resource(tmp_store.resources.get_graph_element(q.obj).clone());
        store.add_quad(dag_rdf::ingress::Quad {
            triple_id: graph,
            subject: s,
            predicate: p,
            obj: o,
        });
    }

    (
        StatusCode::OK,
        format!("OTTR expansion applied: {} triples inserted", quads.len()),
    )
        .into_response()
}

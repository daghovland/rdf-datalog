/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `POST /{name}/rml` — apply an RML mapping (uploaded as `multipart/form-data`)
//! to a named dataset.
//!
//! Request:
//! ```text
//! POST /{dataset}/rml
//! Content-Type: multipart/form-data; boundary=...
//!
//! --boundary
//! Content-Disposition: form-data; name="mapping"
//!
//! [RML mapping Turtle]
//! --boundary
//! Content-Disposition: form-data; name="people.csv"; filename="people.csv"
//!
//! [source file bytes]
//! --boundary--
//! ```
//!
//! See `docs/plans/RML_REST_ENDPOINT_PLAN.md` for the full design.
//!
//! Spec: <https://www.w3.org/TR/rml/>

use crate::AppState;
use crate::persistence::{LogEntry, to_repr};
use axum::{
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use dag_rdf::{Datastore, GraphElementId, ingress::DEFAULT_GRAPH_ELEMENT_ID};
use std::path::PathBuf;

/// The mapping and its source files, materialized to a temporary directory so
/// `rml::apply_rml_mapping`'s filesystem-based API can read them.
///
/// The `TempDir` is dropped (deleting the directory) when this value goes out
/// of scope, so no on-disk artifacts survive the request.
struct MaterializedMapping {
    tmp_dir: tempfile::TempDir,
    mapping_path: PathBuf,
}

/// Read every part of a `mapping`-shaped multipart body into a temp directory.
///
/// One part named `mapping` (required) becomes `<tmp>/mapping.ttl`; every
/// other part must carry a `filename`, which becomes `<tmp>/<filename>`.
#[allow(clippy::result_large_err)]
async fn materialize_multipart(multipart: &mut Multipart) -> Result<MaterializedMapping, Response> {
    let tmp_dir = tempfile::TempDir::new().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create temp dir: {e}"),
        )
            .into_response()
    })?;
    let mapping_path = tmp_dir.path().join("mapping.ttl");
    let mut has_mapping = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("multipart error: {e}")).into_response())?
    {
        let is_mapping = field.name() == Some("mapping");
        let file_name = field.file_name().map(str::to_owned);

        if !is_mapping {
            let Some(name) = &file_name else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "every non-mapping part must have a filename",
                )
                    .into_response());
            };
            if name.is_empty() || name.contains('/') || name.contains('\\') || name == ".." {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("invalid file name: {name}"),
                )
                    .into_response());
            }
        }

        let bytes = field.bytes().await.map_err(|e| {
            (StatusCode::BAD_REQUEST, format!("multipart error: {e}")).into_response()
        })?;

        let dest = if is_mapping {
            has_mapping = true;
            mapping_path.clone()
        } else {
            tmp_dir.path().join(file_name.expect("checked above"))
        };
        std::fs::write(&dest, &bytes).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to write temp file: {e}"),
            )
                .into_response()
        })?;
    }

    if !has_mapping {
        return Err((StatusCode::BAD_REQUEST, "missing required `mapping` part").into_response());
    }

    Ok(MaterializedMapping {
        mapping_path,
        tmp_dir,
    })
}

/// Apply `materialized`'s mapping into a fresh `Datastore`, or `400` on `RmlError`.
#[allow(clippy::result_large_err)]
fn run_mapping(materialized: &MaterializedMapping) -> Result<Datastore, Response> {
    let mut tmp_store = Datastore::new(1024);
    rml::apply_rml_mapping(
        &materialized.mapping_path,
        materialized.tmp_dir.path(),
        &mut tmp_store,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, format!("RML mapping error: {e}")).into_response())?;
    Ok(tmp_store)
}

/// The named-graph IRI a quad belongs to, or `None` for the default graph.
fn graph_iri_for(store: &Datastore, graph_id: GraphElementId) -> Option<String> {
    if graph_id == DEFAULT_GRAPH_ELEMENT_ID {
        return None;
    }
    store
        .resources
        .get_named_resource(graph_id)
        .map(|iri| iri.0.clone())
}

pub async fn dataset_rml_post(
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

    let materialized = match materialize_multipart(&mut multipart).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let tmp_store = match run_mapping(&materialized) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

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
        format!("RML mapping applied: {} triples inserted", quads.len()),
    )
        .into_response()
}

/// `POST /rml/map` — apply an RML mapping and return the generated RDF
/// directly, without touching any dataset. See
/// `docs/plans/RML_REST_ENDPOINT_PLAN.md` ("Stateless mapping endpoint").
pub async fn rml_map_post(headers: HeaderMap, mut multipart: Multipart) -> Response {
    let materialized = match materialize_multipart(&mut multipart).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let tmp_store = match run_mapping(&materialized) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let accept = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok());
    match crate::graph_store::negotiate_rdf_format(accept) {
        Some(crate::graph_store::RdfFormat::NQuads) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/n-quads")],
            crate::serialize::serialize_nquads(&tmp_store),
        )
            .into_response(),
        Some(crate::graph_store::RdfFormat::TriG) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/trig")],
            crate::serialize::serialize_trig(&tmp_store),
        )
            .into_response(),
        _ => crate::graph_store::graph_response_parts(&tmp_store, DEFAULT_GRAPH_ELEMENT_ID, accept),
    }
}

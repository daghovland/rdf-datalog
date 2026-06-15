/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `POST /{name}/shacl` — Fuseki-compatible SHACL validation endpoint.
//!
//! Request:
//! ```text
//! POST /{dataset}/shacl
//! Content-Type: text/turtle
//! [SHACL shapes graph]
//! ```
//!
//! Response: `200 text/turtle` with a SHACL validation report graph.
//!
//! Spec: <https://www.w3.org/TR/shacl/#validation-report>

use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use dag_rdf::Datastore;

pub async fn dataset_shacl_post(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let Some(ds_lock) = state.registry.read().await.get(&name) else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };

    let mut shapes_store = Datastore::new(4096);
    if let Err(e) = turtle::parse_turtle(&mut shapes_store, std::io::BufReader::new(body.as_ref()))
    {
        return (
            StatusCode::BAD_REQUEST,
            format!("Invalid Turtle shapes graph: {e}"),
        )
            .into_response();
    }

    let data = ds_lock.read().await;
    let report = match shacl::validate(&data, &shapes_store) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("SHACL validation error: {e}"),
            )
                .into_response();
        }
    };
    drop(data);

    let turtle_body = shacl::report_to_turtle(&report);
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/turtle; charset=utf-8",
        )],
        turtle_body,
    )
        .into_response()
}

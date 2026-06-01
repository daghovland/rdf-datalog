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

use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
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

    let cursor = Cursor::new(body.to_vec());
    let mut store = state.store.write().await;
    match turtle::parse_turtle(&mut store, cursor) {
        Ok(()) => (StatusCode::OK, "Data uploaded successfully").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("Turtle parse error: {e}")).into_response(),
    }
}

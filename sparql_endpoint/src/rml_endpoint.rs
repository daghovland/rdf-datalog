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
use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
};

pub async fn dataset_rml_post(
    State(_state): State<AppState>,
    Path(_name): Path<String>,
    _multipart: Multipart,
) -> axum::response::Response {
    (StatusCode::NOT_IMPLEMENTED, "RML endpoint not yet implemented").into_response()
}

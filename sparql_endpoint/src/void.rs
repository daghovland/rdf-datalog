/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! VoID (Vocabulary of Interlinked Datasets) description endpoint.
//!
//! Routes: `GET /.well-known/void` and `GET /void`
//! Spec: <https://www.w3.org/TR/void/>

use crate::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Handler for `GET /.well-known/void` and `GET /void`.
///
/// Returns a Turtle document describing the dataset per the VoID vocabulary.
pub async fn void_handler(State(state): State<AppState>) -> Response {
    let store = state.store.read().await;
    let triple_count = store.named_graphs.quad_list.len();
    let body = void_turtle(&state.config.base_iri, triple_count);
    (
        StatusCode::OK,
        [("content-type", "text/turtle; charset=utf-8")],
        body,
    )
        .into_response()
}

/// Generate a VoID description as a Turtle document.
pub fn void_turtle(base_iri: &str, triple_count: usize) -> String {
    format!(
        "@prefix void: <http://rdfs.org/ns/void#> .\n\
         @prefix dcterms: <http://purl.org/dc/terms/> .\n\
         \n\
         <{base_iri}/.well-known/void> a void:Dataset ;\n\
             void:sparqlEndpoint <{base_iri}/sparql> ;\n\
             void:triples {triple_count} .\n",
    )
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Handlers for GET /sparql and POST /sparql.
//!
//! Implements the SPARQL 1.1 Protocol:
//! - `GET  /sparql?query=<encoded>` — query endpoint
//! - `POST /sparql  application/x-www-form-urlencoded  query=<encoded>` — form query
//! - `POST /sparql  application/sparql-query` — raw body query

use crate::{
    AppState,
    negotiate::{SelectFormat, negotiate_select_format},
    serialize::sparql_json::{ask_to_sparql_json, to_sparql_json},
    service_desc::service_description_turtle,
};
use axum::{
    extract::{Query as AxumQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use sparql_parser::{ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;

/// `GET /sparql?query=<url-encoded SPARQL>`
pub async fn sparql_get(
    State(state): State<AppState>,
    params: AxumQuery<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    sparql_get_with_state(state, params, headers).await
}

/// Inner implementation of sparql_get — accepts a direct AppState so
/// per-dataset route handlers can reuse it.
pub async fn sparql_get_with_state(
    state: AppState,
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if !params.contains_key("query") {
        let accept = headers.get("accept").and_then(|v| v.to_str().ok());
        let wants_rdf = accept
            .map(|a| a.contains("text/turtle") || a.contains("application/rdf"))
            .unwrap_or(false);
        if wants_rdf || accept.is_none() {
            let turtle = service_description_turtle(&state.config.base_iri);
            return (
                StatusCode::OK,
                [("content-type", "text/turtle; charset=utf-8")],
                turtle,
            )
                .into_response();
        }
        return (StatusCode::BAD_REQUEST, "Missing query parameter").into_response();
    }

    let query_str = &params["query"];
    run_select_query(query_str, &headers, &state).await
}

/// `POST /sparql`
pub async fn sparql_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    sparql_post_with_state(state, headers, body).await
}

/// Inner implementation of sparql_post.
pub async fn sparql_post_with_state(
    state: AppState,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let query_str: String = if content_type.contains("application/sparql-query") {
        match String::from_utf8(body.to_vec()) {
            Ok(s) => s,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "Invalid UTF-8 in query body").into_response();
            }
        }
    } else if content_type.contains("application/x-www-form-urlencoded") {
        let body_str = match String::from_utf8(body.to_vec()) {
            Ok(s) => s,
            Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8 in body").into_response(),
        };
        let query_val = body_str.split('&').find_map(|part| {
            let (k, v) = part.split_once('=')?;
            (k == "query").then(|| v.replace('+', " "))
        });
        match query_val {
            Some(q) => urlencoding_decode(&q),
            None => {
                return (StatusCode::BAD_REQUEST, "Missing 'query' in form body").into_response();
            }
        }
    } else {
        return (StatusCode::BAD_REQUEST, "Unsupported Content-Type").into_response();
    };

    run_select_query(&query_str, &headers, &state).await
}

async fn run_select_query(query_str: &str, headers: &HeaderMap, state: &AppState) -> Response {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let query = match parse_query(query_str, &mut ctx) {
        Ok((_, q)) => q,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Parse error: {:?}", e)).into_response();
        }
    };

    let store = state.store.read().await;
    let result = match execute(&query, &store) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Execution error: {}", e),
            )
                .into_response();
        }
    };

    match result {
        QueryResult::Ask(boolean) => {
            let body = ask_to_sparql_json(boolean);
            (
                StatusCode::OK,
                [(
                    "content-type",
                    "application/sparql-results+json; charset=utf-8",
                )],
                body,
            )
                .into_response()
        }
        QueryResult::Select(select_result) => {
            let accept = headers.get("accept").and_then(|v| v.to_str().ok());
            let body = to_sparql_json(&select_result);
            let _ = negotiate_select_format(accept);
            (
                StatusCode::OK,
                [(
                    "content-type",
                    "application/sparql-results+json; charset=utf-8",
                )],
                body,
            )
                .into_response()
        }
    }
}

/// Minimal percent-decoding for URL query parameters.
fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            let hex = format!("{}{}", h1, h2);
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                out.push(byte as char);
            }
        } else {
            out.push(c);
        }
    }
    out
}

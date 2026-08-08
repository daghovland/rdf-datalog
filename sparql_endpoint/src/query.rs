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
    constraints::{check_owl_nothing, format_409_body},
    negotiate::{SelectFormat, negotiate_select_format},
    reasoner_delta::apply_reasoner_delta,
    serialize::{
        serialize_construct_ntriples,
        sparql_json::{ask_to_sparql_json, to_sparql_json},
        sparql_xml::{ask_to_sparql_xml, to_sparql_xml},
        to_sparql_csv,
    },
    service_desc::service_description_turtle,
    sparql_update::{
        PreparedOp, apply_prepared_update_with_options, parse_update, prepare_update,
        translate_to_main_ids,
    },
};
use axum::{
    extract::{Query as AxumQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use dag_rdf::Datastore;
use sparql_parser::{ParserContext, QueryResult, execute_with_base, parse_query};
use std::collections::HashMap;
use std::time::Duration;

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

    // If a txId is provided, execute the query against a snapshot of the store
    // with the transaction's pending delta applied.
    if let Some(tx_id) = params.get("txId") {
        return run_transactional_query(query_str, tx_id, &headers, &state).await;
    }

    run_select_query(query_str, &headers, &state).await
}

/// `POST /sparql`
pub async fn sparql_post(
    State(state): State<AppState>,
    params: AxumQuery<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    sparql_post_with_state(state, params, headers, body).await
}

/// Inner implementation of sparql_post.
pub async fn sparql_post_with_state(
    state: AppState,
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // SPARQL Update (direct body) — may be transactional if txId is present.
    if content_type.contains("application/sparql-update") {
        let update_str = match String::from_utf8(body.to_vec()) {
            Ok(s) => s,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "Invalid UTF-8 in update body").into_response();
            }
        };
        if let Some(tx_id) = params.get("txId") {
            return run_transactional_update(&update_str, tx_id, &state).await;
        }
        return run_update(&update_str, &state, &headers).await;
    }

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
        // Check for `update=` parameter first (SPARQL Update via form)
        if let Some(update_val) = body_str.split('&').find_map(|part| {
            let (k, v) = part.split_once('=')?;
            (k == "update").then(|| urlencoding_decode(&v.replace('+', " ")))
        }) {
            if let Some(tx_id) = params.get("txId") {
                return run_transactional_update(&update_val, tx_id, &state).await;
            }
            return run_update(&update_val, &state, &headers).await;
        }
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

    // Transactional read: execute query against snapshot + pending delta.
    if let Some(tx_id) = params.get("txId") {
        return run_transactional_query(&query_str, tx_id, &headers, &state).await;
    }

    run_select_query(&query_str, &headers, &state).await
}

async fn run_update(update_str: &str, state: &AppState, headers: &HeaderMap) -> Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }
    let ops = match parse_update(update_str) {
        Ok(ops) => ops,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Update parse error: {e}")).into_response();
        }
    };
    let mut store = state.store.write().await;
    // Optimistic concurrency control: if the client supplied an If-Match header,
    // its value must match the current store generation ETag.
    // See: https://github.com/daghovland/rdf-datalog/issues/124
    if let Some(if_match) = headers.get("if-match").and_then(|v| v.to_str().ok()) {
        let requested = if_match.trim().trim_matches('"');
        let current = format!("{}", store.generation);
        if requested != current {
            return (
                StatusCode::PRECONDITION_FAILED,
                "Precondition Failed: ETag mismatch",
            )
                .into_response();
        }
    }
    let (prepared, log_entries) = match prepare_update(&store, ops) {
        Ok(pair) => pair,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Update prepare error: {e}"),
            )
                .into_response();
        }
    };
    if let Some(ref changelog) = state.changelog {
        let mut cl = changelog.lock().await;
        if let Err(e) = cl.append_batch(&log_entries) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("persistence error: {e}"),
            )
                .into_response();
        }
    }
    let result = if let Some(ref reasoner_arc) = state.reasoner {
        let mut reasoner = reasoner_arc.lock().await;
        apply_prepared_update_with_options(
            &mut store,
            prepared,
            Some(&mut *reasoner),
            state.network_policy.clone(),
            state.allow_loopback_for_ssrf_tests,
        )
    } else {
        apply_prepared_update_with_options(
            &mut store,
            prepared,
            None,
            state.network_policy.clone(),
            state.allow_loopback_for_ssrf_tests,
        )
    };
    let (net_inserts, net_deletes) = match result {
        Ok(delta) => delta,
        // A genuine Contradiction (RuleHead::Contradiction rule fired) is a
        // client-caused constraint violation, not a server error: the store
        // has already been rolled back by `apply_prepared_update`. Report it
        // the same way as the existing owl:Nothing check below (409),
        // instead of a generic 500. See
        // https://github.com/daghovland/rdf-datalog/issues/301
        Err(crate::sparql_update::UpdateError::Contradiction(msg)) => {
            let body = format!("Transaction rejected: {msg}\n");
            return (StatusCode::CONFLICT, body).into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Update error: {e}"),
            )
                .into_response();
        }
    };

    // Constraint check: if a reasoner is active, reject and roll back any
    // transaction that produces owl:Nothing instances in the default graph.
    // Without a reasoner there are no derived facts, so direct insertions of
    // owl:Nothing are not treated as constraint violations.
    // Related: https://github.com/daghovland/rdf-datalog/issues/127
    if state.reasoner.is_some() {
        let violations = check_owl_nothing(&store, 10, 10);
        if !violations.is_empty() {
            // Roll back: remove what was inserted, restore what was deleted.
            for &q in &net_inserts {
                store.remove_quad(q);
            }
            for &q in &net_deletes {
                store.add_quad(q);
            }
            // Update the reasoner with the inverse delta so derived facts
            // reflect the reverted state. This reverts back to a state that
            // was already known-consistent (the pre-update snapshot), so a
            // Contradiction here would indicate a bug rather than bad client
            // data; surface it as a 500 rather than trying to be clever.
            // See https://github.com/daghovland/rdf-datalog/issues/301
            if let Some(ref reasoner_arc) = state.reasoner {
                let mut reasoner = reasoner_arc.lock().await;
                if let Err(e) =
                    apply_reasoner_delta(&mut reasoner, &mut store, &net_inserts, &net_deletes)
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!(
                            "failed to roll back after owl:Nothing violation \
                             (store may be inconsistent): {e}"
                        ),
                    )
                        .into_response();
                }
            }
            let body = format_409_body(&violations);
            return (StatusCode::CONFLICT, body).into_response();
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

// ── Transactional reads ───────────────────────────────────────────────────────

/// Execute a SPARQL query against the snapshot of the store at transaction
/// begin time, overlaid with the transaction's buffered inserts/deletes.
///
/// Returns 404 if the transaction is not found.  No ETag is set — the snapshot
/// is not a committed state.
async fn run_transactional_query(
    query_str: &str,
    tx_id: &str,
    headers: &HeaderMap,
    state: &AppState,
) -> Response {
    // Build a delta-overlay view: clone live store, apply pending delta.
    let view = {
        let store = state.store.read().await;
        let registry = state.transactions.lock().await;
        let tx = match registry.get(tx_id) {
            Some(tx) => tx,
            None => {
                return (StatusCode::NOT_FOUND, "Transaction not found").into_response();
            }
        };
        let mut view = store.clone();
        for &q in &tx.pending_inserts {
            view.named_graphs.add_quad(q);
        }
        for &q in &tx.pending_deletes {
            view.named_graphs.remove_quad(q);
        }
        view
    };

    // Execute query against the view (same logic as run_select_query).
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let query = match parse_query(query_str, &mut ctx) {
        Ok((_, q)) => q,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Parse error: {:?}", e)).into_response();
        }
    };
    // Thread the effective base through so `IRI()`/`URI()` can resolve a
    // runtime string argument at evaluation time too, not just IRIs written
    // directly in query syntax (`ParserContext::base` handles those at parse
    // time — #217). See #346.
    let result = match execute_with_base(
        &query,
        &view,
        state.network_policy.clone(),
        ctx.base.as_deref(),
        configured_query_timeout(state.config.max_query_timeout_secs),
    ) {
        Ok(r) => r,
        Err(e) => return query_execution_error_response(&e),
    };
    format_query_result(result, headers, &view)
}

// ── Transactional writes ──────────────────────────────────────────────────────

/// Parse and buffer a SPARQL Update inside an open transaction.
///
/// The update is not applied to the live store; instead the prepared quads are
/// appended to the transaction's `pending_inserts` / `pending_deletes` lists.
///
/// Supported: `INSERT DATA`, `DELETE DATA`.
/// Unsupported inside a transaction: `CLEAR`, `DROP`, `CREATE`, `LOAD`,
/// `INSERT WHERE`, `DELETE WHERE` — these return HTTP 400.
///
/// Returns 200 (not 204) because the changes are buffered, not committed.
/// Returns 404 if the transaction is not found.
async fn run_transactional_update(update_str: &str, tx_id: &str, state: &AppState) -> Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let ops = match parse_update(update_str) {
        Ok(ops) => ops,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Update parse error: {e}")).into_response();
        }
    };

    // Lock the live store for write so we can intern new resources into it.
    // Resource interning does not increment the generation counter.
    let mut store = state.store.write().await;

    let (prepared, _log_entries) = match prepare_update(&store, ops) {
        Ok(pair) => pair,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Update prepare error: {e}"),
            )
                .into_response();
        }
    };

    // Validate and translate each PreparedOp into raw quads.
    let mut new_inserts: Vec<dag_rdf::ingress::Quad> = Vec::new();
    let mut new_deletes: Vec<dag_rdf::ingress::Quad> = Vec::new();

    for op in prepared {
        match op {
            PreparedOp::InsertData(tmp) => {
                // Intern resources into the live store and collect quad IDs
                // valid in the live store.
                let quads = translate_to_main_ids(&mut store, &tmp);
                new_inserts.extend(quads);
            }
            PreparedOp::DeleteData(tmp) => {
                // Intern resources and collect candidate quads.  We defer
                // filtering against pending_inserts until we hold the
                // transaction lock so we can also check previous-request
                // inserts buffered in tx.pending_inserts.
                let quads = translate_to_main_ids(&mut store, &tmp);
                // Pre-filter to quads that exist in the live store or in the
                // inserts buffered by this specific request.
                let candidates: Vec<_> = quads
                    .into_iter()
                    .filter(|q| store.named_graphs.contains(q) || new_inserts.contains(q))
                    .collect();
                new_deletes.extend(candidates);
            }
            PreparedOp::PatternUpdate { .. }
            | PreparedOp::ClearDefault
            | PreparedOp::ClearNamed
            | PreparedOp::ClearAll
            | PreparedOp::ClearGraph(_)
            | PreparedOp::DropDefault
            | PreparedOp::DropNamed
            | PreparedOp::DropAll
            | PreparedOp::DropGraph(_)
            | PreparedOp::CreateGraph(_)
            | PreparedOp::LoadGraph { .. } => {
                return (
                    StatusCode::BAD_REQUEST,
                    "Only INSERT DATA and DELETE DATA are supported inside a transaction",
                )
                    .into_response();
            }
        }
    }

    // Append to the transaction's pending delta.
    let mut registry = state.transactions.lock().await;
    let tx = match registry.get_mut(tx_id) {
        Some(tx) => tx,
        None => {
            return (StatusCode::NOT_FOUND, "Transaction not found").into_response();
        }
    };
    tx.pending_inserts.extend(new_inserts);
    // Extend deletes: also keep quads that were inserted by earlier requests
    // to this same transaction (they are in tx.pending_inserts but not in the
    // live store yet).
    let all_pending_inserts = tx.pending_inserts.clone();
    tx.pending_deletes.extend(
        new_deletes
            .into_iter()
            .filter(|q| store.named_graphs.contains(q) || all_pending_inserts.contains(q)),
    );
    tx.last_activity = std::time::Instant::now();

    // Return 200 — the changes are buffered, not committed.
    StatusCode::OK.into_response()
}

/// Format a `QueryResult` into an HTTP response (no ETag).
///
/// Used by both the normal query path (which adds an ETag separately) and the
/// transactional-read path (which does not add an ETag).
fn format_query_result(result: QueryResult, headers: &HeaderMap, store: &Datastore) -> Response {
    match result {
        QueryResult::Ask(boolean) => {
            let accept = headers.get("accept").and_then(|v| v.to_str().ok());
            let fmt = match negotiate_select_format(accept) {
                Some(f) => f,
                None => {
                    return (
                        StatusCode::NOT_ACCEPTABLE,
                        "No supported format in Accept header for ASK results",
                    )
                        .into_response();
                }
            };
            let (body, ct) = match fmt {
                SelectFormat::SparqlXml => (
                    ask_to_sparql_xml(boolean),
                    "application/sparql-results+xml; charset=utf-8",
                ),
                _ => (
                    ask_to_sparql_json(boolean),
                    "application/sparql-results+json; charset=utf-8",
                ),
            };
            (StatusCode::OK, [("content-type", ct)], body).into_response()
        }
        QueryResult::Select(select_result) => {
            let accept = headers.get("accept").and_then(|v| v.to_str().ok());
            let fmt = match negotiate_select_format(accept) {
                Some(f) => f,
                None => {
                    return (
                        StatusCode::NOT_ACCEPTABLE,
                        "No supported format in Accept header for SELECT results",
                    )
                        .into_response();
                }
            };
            let (body, ct) = match fmt {
                SelectFormat::SparqlXml => (
                    to_sparql_xml(&select_result, store),
                    "application/sparql-results+xml; charset=utf-8",
                ),
                SelectFormat::Csv => (to_sparql_csv(&select_result), "text/csv; charset=utf-8"),
                SelectFormat::SparqlJson => (
                    to_sparql_json(&select_result, store),
                    "application/sparql-results+json; charset=utf-8",
                ),
            };
            (StatusCode::OK, [("content-type", ct)], body).into_response()
        }
        QueryResult::Construct(triples) => {
            let body = serialize_construct_ntriples(&triples, store);
            (
                StatusCode::OK,
                [(
                    "content-type",
                    "application/n-triples; charset=utf-8; version=1.2",
                )],
                body,
            )
                .into_response()
        }
        QueryResult::Describe(triples) => {
            let body = serialize_construct_ntriples(&triples, store);
            (
                StatusCode::OK,
                [(
                    "content-type",
                    "application/n-triples; charset=utf-8; version=1.2",
                )],
                body,
            )
                .into_response()
        }
    }
}

async fn run_select_query(query_str: &str, headers: &HeaderMap, state: &AppState) -> Response {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let query = match parse_query(query_str, &mut ctx) {
        Ok((_, q)) => q,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Parse error: {:?}", e)).into_response();
        }
    };

    let store = state.store.read().await;
    let etag = format!("\"{}\"", store.generation);
    // See #346: thread the effective base through for evaluation-time
    // `IRI()`/`URI()` resolution.
    let result = match execute_with_base(
        &query,
        &store,
        state.network_policy.clone(),
        ctx.base.as_deref(),
        configured_query_timeout(state.config.max_query_timeout_secs),
    ) {
        Ok(r) => r,
        Err(e) => return query_execution_error_response(&e),
    };

    match result {
        QueryResult::Ask(boolean) => {
            let accept = headers.get("accept").and_then(|v| v.to_str().ok());
            let fmt = match negotiate_select_format(accept) {
                Some(f) => f,
                None => {
                    return (
                        StatusCode::NOT_ACCEPTABLE,
                        "No supported format in Accept header for ASK results",
                    )
                        .into_response();
                }
            };
            let (body, ct) = match fmt {
                SelectFormat::SparqlXml => (
                    ask_to_sparql_xml(boolean),
                    "application/sparql-results+xml; charset=utf-8",
                ),
                _ => (
                    ask_to_sparql_json(boolean),
                    "application/sparql-results+json; charset=utf-8",
                ),
            };
            with_etag(
                (StatusCode::OK, [("content-type", ct)], body).into_response(),
                &etag,
            )
        }
        QueryResult::Select(select_result) => {
            let accept = headers.get("accept").and_then(|v| v.to_str().ok());
            let fmt = match negotiate_select_format(accept) {
                Some(f) => f,
                None => {
                    return (
                        StatusCode::NOT_ACCEPTABLE,
                        "No supported format in Accept header for SELECT results",
                    )
                        .into_response();
                }
            };
            let (body, ct) = match fmt {
                SelectFormat::SparqlXml => (
                    to_sparql_xml(&select_result, &store),
                    "application/sparql-results+xml; charset=utf-8",
                ),
                SelectFormat::Csv => (to_sparql_csv(&select_result), "text/csv; charset=utf-8"),
                SelectFormat::SparqlJson => (
                    to_sparql_json(&select_result, &store),
                    "application/sparql-results+json; charset=utf-8",
                ),
            };
            with_etag(
                (StatusCode::OK, [("content-type", ct)], body).into_response(),
                &etag,
            )
        }
        QueryResult::Construct(triples) => {
            let body = serialize_construct_ntriples(&triples, &store);
            with_etag(
                (
                    StatusCode::OK,
                    [(
                        "content-type",
                        "application/n-triples; charset=utf-8; version=1.2",
                    )],
                    body,
                )
                    .into_response(),
                &etag,
            )
        }
        QueryResult::Describe(triples) => {
            // Stub: serialise the same way as CONSTRUCT until a proper DESCRIBE
            // result format is determined (issue #49).
            let body = serialize_construct_ntriples(&triples, &store);
            with_etag(
                (
                    StatusCode::OK,
                    [(
                        "content-type",
                        "application/n-triples; charset=utf-8; version=1.2",
                    )],
                    body,
                )
                    .into_response(),
                &etag,
            )
        }
    }
}

/// Convert `Config::max_query_timeout_secs` into the `Option<Duration>`
/// [`execute_with_base`] expects. `0` is treated as "disabled" (the common
/// convention for a "maximum ... seconds" config field with no separate
/// on/off flag), matching how a caller would reasonably signal "no timeout"
/// with this field's type. The default (30) is a real, enforced bound. See
/// [#372](https://github.com/daghovland/rdf-datalog/issues/372).
fn configured_query_timeout(max_query_timeout_secs: u64) -> Option<Duration> {
    if max_query_timeout_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(max_query_timeout_secs))
    }
}

/// Map a query execution error to an HTTP response, giving the cooperative
/// query-timeout case (#372) its own `503 Service Unavailable` — distinct
/// from the generic `500` used for every other execution error — so a
/// client (or an operator watching status codes) can tell "this query is too
/// expensive to run within the configured limit" apart from "the query
/// itself is broken". `execute`/`execute_with_base` use a plain
/// `Result<_, String>` (no dedicated error enum to match on), so this is a
/// string-content check against the exact message `Deadline::check` returns;
/// see `sparql_parser::deadline`.
fn query_execution_error_response(message: &str) -> Response {
    if message.contains("exceeded the configured timeout") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Query execution error: {message}"),
        )
            .into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Execution error: {message}"),
        )
            .into_response()
    }
}

fn with_etag(mut response: Response, etag: &str) -> Response {
    if let Ok(val) = HeaderValue::from_str(etag) {
        response.headers_mut().insert("etag", val);
    }
    response
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

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `POST /{dataset}/rules` — load or replace a dataset's live Datalog ruleset at runtime.
//!
//! Parses the request body with `datalog_parser::parse` (the same parser `--rules`/
//! `Config::initial_rules` uses at startup) and **replaces** the target dataset's entire
//! reasoner: any previously-loaded ruleset and everything it derived is discarded, the
//! new ruleset is stratified and fully re-materialised from the dataset's current
//! extensional (base) facts. Works for a dataset that never had a reasoner before
//! (lazily creating one) as well as one that already has a reasoner (whether from
//! `Config::initial_rules` at startup or an earlier call to this same endpoint).
//!
//! This is a full-ruleset-replace, not a per-ruleset-scoped add/delete — see
//! [`docs/plans/RUNTIME_RULESET_ENDPOINT_390_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/RUNTIME_RULESET_ENDPOINT_390_PLAN.md)
//! for the scope rationale. An empty (zero-rule) body clears the dataset's ruleset
//! entirely, equivalent to "unload".
//!
//! Related: [#390](https://github.com/daghovland/rdf-datalog/issues/390),
//! [#469](https://github.com/daghovland/rdf-datalog/issues/469).

use crate::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use dag_rdf::{Quad, QuadTable};
use datalog::IncrementalReasoner;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn dataset_rules_post(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !(ct.is_empty() || ct.contains("text/x-datalog") || ct.contains("text/plain")) {
        return (
            StatusCode::BAD_REQUEST,
            "Content-Type must be text/x-datalog or text/plain",
        )
            .into_response();
    }

    let Some(entry) = state.registry.read().await.get_entry(&name) else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };

    let body_str = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8 in rules body").into_response(),
    };

    // Hold the store write lock across parse + rebuild: `datalog_parser::parse`
    // interns new IRIs into the store as it parses, and a failed parse must
    // leave the dataset completely untouched (no partial interning visible,
    // no ruleset change) — see plan doc test
    // `test_post_rules_parse_error_leaves_dataset_untouched`.
    let mut store = entry.store.write().await;
    let rules = match datalog_parser::parse(&body_str, &mut store) {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Datalog parse error: {e}")).into_response();
        }
    };
    let rules_loaded = rules.len();

    // Rebuild the store to extensional-only facts before (re-)materialising:
    // any facts derived under a *previous* ruleset must not leak in as if
    // they were base facts under the new one. Mirrors the same pattern
    // `IncrementalReasoner::rebuild_from_base` uses internally.
    let base_facts: Vec<Quad> = store.named_graphs.extensional_quads().collect();
    let hint = base_facts.len() as u32;
    store.named_graphs = QuadTable::new(hint);
    for q in base_facts {
        store.named_graphs.add_quad(q);
    }

    if rules_loaded == 0 {
        // Empty ruleset: unload — no reasoner at all, matching the
        // no-`--rules` startup state.
        *entry.reasoner.write().await = None;
        return (StatusCode::OK, Json(serde_json::json!({"rules_loaded": 0}))).into_response();
    }

    let new_reasoner = match IncrementalReasoner::new(rules, &mut store) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::CONFLICT,
                format!("New ruleset is contradictory over the dataset's existing data: {e}"),
            )
                .into_response();
        }
    };

    *entry.reasoner.write().await = Some(Arc::new(Mutex::new(new_reasoner)));

    (
        StatusCode::OK,
        Json(serde_json::json!({"rules_loaded": rules_loaded})),
    )
        .into_response()
}

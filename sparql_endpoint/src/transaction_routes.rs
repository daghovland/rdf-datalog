/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! HTTP handlers for the proprietary transaction API.
//!
//! ```text
//! POST /transaction/begin                 → 200 { "txId": "<uuid>" }
//! POST /transaction/<txId>/commit         → 204  (409 on generation mismatch)
//! POST /transaction/<txId>/rollback       → 204
//! ```
//!
//! Transactional reads and writes go through the existing `/sparql` handlers
//! when a `txId` query parameter is present; see [`crate::query`] for those.
//!
//! Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)

use crate::reasoner_delta::apply_reasoner_delta;
use crate::{AppState, constraints};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

// ── POST /transaction/begin ───────────────────────────────────────────────────

/// Begin a new transaction.
///
/// Snapshots the current store generation and returns a UUID transaction ID.
/// The transaction stays open until `/commit` or `/rollback` is called, or
/// until it is evicted by the 60-second idle timeout.
pub async fn transaction_begin(State(state): State<AppState>) -> Response {
    let generation = state.store.read().await.generation;

    let mut registry = state.transactions.lock().await;
    // Evict stale transactions lazily on every begin.
    registry.purge_stale(60);
    let tx_id = registry.begin(generation);
    drop(registry);

    let body = serde_json::json!({ "txId": tx_id });
    (StatusCode::OK, Json(body)).into_response()
}

// ── POST /transaction/<txId>/commit ──────────────────────────────────────────

/// Commit an open transaction.
///
/// Applies the buffered inserts and deletes atomically.  Returns 409 if the
/// store's generation has changed since the transaction began (optimistic
/// concurrency control).
pub async fn transaction_commit(
    State(state): State<AppState>,
    Path(tx_id): Path<String>,
) -> Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    // Extract the transaction from the registry (404 if not found).
    let tx = {
        let mut registry = state.transactions.lock().await;
        match registry.remove(&tx_id) {
            Some(tx) => tx,
            None => {
                return (StatusCode::NOT_FOUND, "Transaction not found").into_response();
            }
        }
    };

    let mut store = state.store.write().await;

    // Optimistic concurrency: reject if the store was modified after begin.
    if store.generation != tx.snapshot_generation {
        return (
            StatusCode::CONFLICT,
            "Conflict: store was modified since transaction began (generation mismatch)",
        )
            .into_response();
    }

    // Compute net deletes: quads to remove that actually exist in the store.
    let net_deletes: Vec<_> = tx
        .pending_deletes
        .iter()
        .copied()
        .filter(|q| store.named_graphs.contains(q))
        .collect();
    for &q in &net_deletes {
        store.remove_quad(q);
    }

    // Compute net inserts: quads to add that weren't cancelled by a delete.
    let delete_set: std::collections::HashSet<dag_rdf::ingress::Quad> =
        net_deletes.iter().copied().collect();
    let net_inserts: Vec<_> = tx
        .pending_inserts
        .iter()
        .copied()
        .filter(|q| !delete_set.contains(q))
        .collect();
    for &q in &net_inserts {
        store.add_quad(q);
    }

    // Always increment the generation on a successful commit — even an empty
    // commit "claims" the current generation so that any other transaction that
    // began at the same snapshot is invalidated.
    if net_inserts.is_empty() && net_deletes.is_empty() {
        store.generation += 1;
    }

    // Update the incremental reasoner (if one is configured).
    //
    // A genuine Contradiction (RuleHead::Contradiction rule fired) is a
    // client-caused constraint violation: `apply_reasoner_delta` has already
    // rolled the commit back and rebuilt a consistent closure. Report it as
    // 409, matching the owl:Nothing convention below, instead of crashing or
    // committing an inconsistent transaction.
    // See https://github.com/daghovland/rdf-datalog/issues/301
    let reasoner_slot = state.reasoner.read().await;
    if let Some(ref reasoner_arc) = *reasoner_slot {
        let mut reasoner = reasoner_arc.lock().await;
        if let Err(e) = apply_reasoner_delta(&mut reasoner, &mut store, &net_deletes, &net_inserts)
        {
            return e.into_response();
        }
    }

    // Constraint check: roll back and return 409 if owl:Nothing is instantiated.
    if state.reasoner.read().await.is_some() {
        let violations = constraints::check_owl_nothing(&store, 10, 10);
        if !violations.is_empty() {
            // Undo: reverse inserts and deletes.
            for &q in &net_inserts {
                store.remove_quad(q);
            }
            for &q in &net_deletes {
                store.add_quad(q);
            }
            // This reverts to a state that was already known-consistent (the
            // pre-commit snapshot), so a Contradiction here would indicate a
            // bug rather than bad client data; surface it as 500.
            let reasoner_slot = state.reasoner.read().await;
            if let Some(ref reasoner_arc) = *reasoner_slot {
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
            let body = constraints::format_409_body(&violations);
            return (StatusCode::CONFLICT, body).into_response();
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

// ── POST /transaction/<txId>/rollback ────────────────────────────────────────

/// Roll back an open transaction.
///
/// Discards all buffered changes.  The store is never modified.
pub async fn transaction_rollback(
    State(state): State<AppState>,
    Path(tx_id): Path<String>,
) -> Response {
    let mut registry = state.transactions.lock().await;
    match registry.remove(&tx_id) {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => (StatusCode::NOT_FOUND, "Transaction not found").into_response(),
    }
}

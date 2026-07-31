/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Shared helper for applying a net insert/delete delta through an
//! [`IncrementalReasoner`], with rollback on a genuine `Contradiction`.
//!
//! `datalog::IncrementalReasoner::{apply_insertions, apply_deletions}` can
//! return `Err(ReasoningError::Contradiction)` for data supplied by an
//! untrusted client (e.g. `INSERT DATA`, a transaction commit, or a Graph
//! Store Protocol `PUT`/`POST`). Previously this panicked and took down the
//! whole server process; see
//! [#301](https://github.com/daghovland/rdf-datalog/issues/301).
//!
//! [`apply_reasoner_delta`] centralises the recovery: on error it undoes the
//! net delta (idempotent no-ops if the reasoner already applied part of it)
//! and rebuilds a consistent derived closure from the surviving base facts via
//! [`IncrementalReasoner::rebuild_from_base`], so callers can map the error to
//! a clean 4xx response instead of crashing or leaving the store corrupted.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dag_rdf::Datastore;
use dag_rdf::ingress::Quad;
use datalog::IncrementalReasoner;
use std::fmt;

/// Outcome of a failed [`apply_reasoner_delta`] call.
///
/// Distinguishes a client-triggered contradiction that was cleanly rolled
/// back (map to 409 Conflict) from a rollback that itself failed to restore
/// a consistent state (map to 500 — the store may need operator attention).
#[derive(Debug)]
pub enum DeltaError {
    /// A `RuleHead::Contradiction` rule fired. The delta has already been
    /// rolled back and the derived closure rebuilt from the surviving base
    /// facts by the time this is returned — `store` is back to its
    /// pre-call state.
    Contradiction(String),
    /// Rollback itself failed: rebuilding the closure from the surviving
    /// base facts (after undoing the delta) *also* hit a contradiction. This
    /// should not happen if the store was consistent before this call; the
    /// store's state is not guaranteed to be sound.
    RollbackFailed(String),
}

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeltaError::Contradiction(m) => write!(f, "{m}"),
            DeltaError::RollbackFailed(m) => write!(f, "{m}"),
        }
    }
}

impl IntoResponse for DeltaError {
    /// 409 Conflict for a cleanly-rolled-back contradiction (mirrors the
    /// existing `owl:Nothing` constraint-violation convention); 500 if
    /// rollback itself could not restore a consistent state.
    fn into_response(self) -> Response {
        match self {
            DeltaError::Contradiction(msg) => (
                StatusCode::CONFLICT,
                format!("Transaction rejected: {msg}\n"),
            )
                .into_response(),
            DeltaError::RollbackFailed(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("update failed and could not be cleanly rolled back: {msg}"),
            )
                .into_response(),
        }
    }
}

/// Apply `net_deletes` then `net_inserts` to `store` via `reasoner`.
///
/// On success, `store` reflects the delta as applied by `reasoner` (matching
/// the existing call sites' behaviour). On a genuine contradiction, the delta
/// is rolled back, the derived closure is rebuilt from the surviving base
/// facts, and `Err(DeltaError::Contradiction)` is returned — suitable for a
/// 409 Conflict response, mirroring the existing `owl:Nothing`
/// constraint-violation convention.
///
/// If rebuilding after rollback *itself* fails (the surviving base facts are
/// already contradictory — should not happen if the store was consistent
/// before this call), `Err(DeltaError::RollbackFailed)` is returned; callers
/// should treat this as a 500, since rollback could not restore a known-good
/// state.
pub fn apply_reasoner_delta(
    reasoner: &mut IncrementalReasoner,
    store: &mut Datastore,
    net_deletes: &[Quad],
    net_inserts: &[Quad],
) -> Result<(), DeltaError> {
    let mut contradiction: Option<String> = None;

    if !net_deletes.is_empty()
        && let Err(e) = reasoner.apply_deletions(store, net_deletes)
    {
        contradiction = Some(e.to_string());
    }
    if contradiction.is_none()
        && !net_inserts.is_empty()
        && let Err(e) = reasoner.apply_insertions(store, net_inserts)
    {
        contradiction = Some(e.to_string());
    }

    if let Some(msg) = contradiction {
        // Undo the net delta. add_quad/remove_quad are idempotent no-ops when
        // the quad is already absent/present, so this is safe regardless of
        // how far apply_deletions/apply_insertions got before failing.
        for &q in net_inserts {
            store.remove_quad(q);
        }
        for &q in net_deletes {
            store.add_quad(q);
        }
        if let Err(e2) = reasoner.rebuild_from_base(store) {
            return Err(DeltaError::RollbackFailed(format!(
                "contradiction rollback failed to restore a consistent state: {e2} \
                 (original contradiction: {msg})"
            )));
        }
        return Err(DeltaError::Contradiction(msg));
    }

    Ok(())
}

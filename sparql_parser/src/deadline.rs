/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Cooperative query-timeout cancellation (issue
//! [#372](https://github.com/daghovland/rdf-datalog/issues/372)).
//!
//! `sparql_endpoint::Config::max_query_timeout_secs` used to be dead code —
//! nothing enforced it, so a runaway query (an expensive transitive-closure
//! property path, or a large Cartesian-product BGP) could run indefinitely.
//!
//! A naive `tokio::time::timeout` wrapped around query execution was
//! considered and rejected: [`crate::execute_with_base`] is a fully
//! synchronous, non-yielding call — there is no `.await` point inside it for
//! an async timeout future to preempt, so the wrapping future never gets
//! polled again until the synchronous call has already returned regardless
//! of the timeout. `spawn_blocking` plus an owned per-query `Datastore`
//! clone would work, but at a real, avoidable performance cost for the
//! common case. Cooperative cancellation — the evaluator itself periodically
//! checking an absolute deadline at natural loop-iteration boundaries — was
//! the deliberate choice instead, trading "instant" cancellation (a
//! best-effort bound: work already in flight between two checks still runs
//! to completion) for zero overhead when no timeout is configured and no
//! extra cost for the async runtime.

use crate::error::ExecError;
use std::time::{Duration, Instant};

/// An absolute wall-clock deadline for a single query evaluation.
///
/// Threaded by reference through the evaluator's loop-bearing functions
/// (the BGP/OPTIONAL/UNION/MINUS/GRAPH/subquery join chain, and the
/// property-path/transitive-closure chain) so each can bail out with a
/// [`check`](Deadline::check) once the configured timeout has elapsed.
///
/// `None` — no timeout configured — is the common case (every caller except
/// the SPARQL HTTP endpoint's configured timeout passes this) and must be a
/// true zero-cost no-op: [`check`](Deadline::check) never calls
/// `Instant::now()` when there is no deadline, so there is no
/// syscall-per-loop-iteration regression for the unconfigured default.
#[derive(Clone, Copy, Debug)]
pub struct Deadline(Option<Instant>);

impl Deadline {
    /// No timeout configured. Every [`check`](Deadline::check) call is a
    /// no-op `Ok(())`.
    pub fn none() -> Self {
        Deadline(None)
    }

    /// Construct a deadline `timeout` from now (`None` = no timeout).
    pub fn from_timeout(timeout: Option<Duration>) -> Self {
        Deadline(timeout.map(|d| Instant::now() + d))
    }

    /// Cheap check for use at a loop-iteration boundary.
    ///
    /// `Ok(())` if there is no configured deadline, or the deadline has not
    /// yet passed. [`ExecError::Timeout`] once it has passed.
    pub fn check(&self) -> Result<(), ExecError> {
        match self.0 {
            None => Ok(()),
            Some(deadline) => {
                if Instant::now() >= deadline {
                    Err(ExecError::Timeout)
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_deadline_never_expires() {
        let d = Deadline::none();
        assert!(d.check().is_ok());
        // Still fine an arbitrary amount of "later" — there's no instant to
        // compare against at all.
        std::thread::sleep(Duration::from_millis(5));
        assert!(d.check().is_ok());
    }

    #[test]
    fn not_yet_expired_deadline_is_ok() {
        let d = Deadline::from_timeout(Some(Duration::from_secs(60)));
        assert!(d.check().is_ok());
    }

    #[test]
    fn already_expired_deadline_is_err() {
        // A deadline set to "0 seconds from now" is essentially already
        // elapsed by the time `check` runs.
        let d = Deadline::from_timeout(Some(Duration::from_millis(0)));
        std::thread::sleep(Duration::from_millis(2));
        assert!(d.check().is_err());
    }
}

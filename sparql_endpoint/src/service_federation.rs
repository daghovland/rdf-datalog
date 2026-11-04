/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Async pre-resolution of `SERVICE <endpoint> { ... }` clauses, and the
//! executor-wiring glue that feeds the fetched results into a synchronous
//! `sparql_parser::execute_with_base` call.
//!
//! See issue [#744](https://github.com/daghovland/rdf-datalog/issues/744),
//! part of epic [#741](https://github.com/daghovland/rdf-datalog/issues/741)
//! — `docs/plans/SPARQL_FEDERATION_PLAN.md`.
//!
//! # Why pre-resolve rather than fetch inline
//!
//! `sparql_parser::execute`/`execute_with_base` are deliberately **not**
//! `async fn` — they're called synchronously both from the CLI (no tokio
//! runtime) and from this crate's async handlers, which hold the datastore
//! under an async `RwLock` *read* guard while calling them. So every
//! `SERVICE` endpoint reachable from a query's top-level `WHERE` clause is
//! walked and fetched *before* the synchronous executor runs, concurrently,
//! from this async context; the results are installed into a thread-local
//! (`sparql_parser::execute`'s `ServiceResultsGuard` — see that module for
//! why a thread-local rather than a new parameter threaded through ~8 call
//! sites and every recursive evaluator arm) and the `QueryComponent::Service`
//! evaluation arm simply looks them up by key instead of doing any I/O
//! itself.
//!
//! # SSRF hardening
//!
//! This module does **not** reimplement SSRF protection: it reuses
//! `sparql_update`'s `ssrf_preflight`/`is_blocked_ip`-backed
//! `build_hardened_client`/`read_capped_response` helpers (issues #135,
//! #138, #365 — PRs #137, #138, #376), the same ones `LOAD <url>` uses.
//! The only difference from `LOAD` is the HTTP method and headers: `POST`
//! with `Content-Type: application/sparql-query` and the serialized query
//! body (`sparql_parser::service_body_to_sparql_text`), `Accept:
//! application/sparql-results+json`.
//!
//! # Triple-term / blank-node limitation (known, documented)
//!
//! Each fetched `application/sparql-results+json` body is deserialized via
//! `crate::serialize::sparql_json::from_sparql_json`, which needs `&mut
//! Datastore` only to intern RDF 1.2 triple-term (`<<( s p o )>>`) bindings
//! and fresh blank nodes. The async handlers in `query.rs` only hold a
//! `RwLockReadGuard` on the real store at the point pre-resolution would
//! otherwise need one, so each fetch is deserialized against a **throwaway,
//! per-call `Datastore::new(64)`** rather than the real query datastore.
//! This is correct for the overwhelmingly common case (IRI/literal
//! bindings), but has two known consequences, deliberately left
//! unaddressed here and tracked as a follow-up
//! (see the issue filed alongside this PR, sub-issue of #741):
//!
//! - A `GraphElement::TripleTerm` binding in a federated result carries
//!   `GraphElementId`s that only resolve correctly against *that* scratch
//!   store, not the real query datastore — such a binding would silently
//!   resolve to the wrong (or a dangling) value if ever displayed/joined
//!   against real data by ID.
//! - A blank-node binding (`RdfResource::AnonymousBlankNode(n)`) is
//!   allocated from the scratch store's own blank-node counter, which can
//!   numerically collide with blank nodes already in the real query
//!   datastore, causing spurious joins/output clashes.
//!
//! Both are rare in practice (triple terms and blank nodes in federated
//! SPARQL JSON results are uncommon) and are judged not worth blocking the
//! common-case feature on; restructuring the two `query.rs` call sites to
//! pre-resolve before acquiring *any* store lock (avoiding the scratch-store
//! requirement entirely) was considered and rejected for this issue as a
//! bigger, separate change — see the PR description's "Non-obvious
//! decisions" section.

use crate::sparql_update::{build_hardened_client, read_capped_response, ssrf_preflight};
use dag_rdf::Datastore;
use ingress::NetworkPolicy;
use sparql_parser::ast::QueryComponent;
use sparql_parser::{ServiceKey, SolutionRow, collect_service_calls};
use std::collections::HashMap;

/// Fetch every non-SILENT, constant-endpoint `SERVICE` call reachable from
/// `where_clause_components` concurrently, and return a map ready to hand
/// to `sparql_parser::execute::ServiceResultsGuard::install`.
///
/// Returns one `Result` per distinct `(endpoint, body)` pair found by
/// [`collect_service_calls`] — a single failing endpoint does not prevent
/// the others from resolving. Call this *before* acquiring any lock on the
/// real store (see this module's doc comment): it performs real network
/// I/O and must not be awaited while holding `AppState::store`'s async
/// `RwLock` read guard.
///
/// Only call this when `network` is [`NetworkPolicy::Allow`] or
/// [`NetworkPolicy::AllowList`] — same restriction as `LOAD`'s `fetch_rdf`.
pub async fn pre_resolve_services(
    where_clause_components: &[QueryComponent],
    network: &NetworkPolicy,
    allow_loopback: bool,
) -> Vec<(ServiceKey, Result<Vec<SolutionRow>, String>)> {
    let calls = collect_service_calls(where_clause_components);
    let mut handles = Vec::with_capacity(calls.len());
    for (endpoint, body) in calls {
        let network = network.clone();
        let endpoint_for_task = endpoint.clone();
        let body_for_task = body.clone();
        let handle = tokio::task::spawn_blocking(move || {
            fetch_service_blocking(&endpoint_for_task, &body_for_task, &network, allow_loopback)
        });
        handles.push((ServiceKey { endpoint, body }, handle));
    }

    let mut results = Vec::with_capacity(handles.len());
    for (key, handle) in handles {
        let outcome = match handle.await {
            Ok(r) => r,
            Err(join_err) => Err(format!("SERVICE fetch task panicked: {join_err}")),
        };
        results.push((key, outcome));
    }
    results
}

/// Blocking: `POST` the SERVICE body to `endpoint` as
/// `application/sparql-query`, requesting `application/sparql-results+json`,
/// then deserialize the response into `SolutionRow`s via a throwaway scratch
/// `Datastore` (see this module's doc comment for why).
///
/// Runs inside `tokio::task::spawn_blocking` (see
/// [`pre_resolve_services`]) — never call this directly from async code
/// without that wrapper, since it blocks the calling thread on network I/O.
fn fetch_service_blocking(
    endpoint: &str,
    body: &str,
    network: &NetworkPolicy,
    allow_loopback: bool,
) -> Result<Vec<SolutionRow>, String> {
    let client =
        build_hardened_client(endpoint, network, allow_loopback, ssrf_preflight, "SERVICE")?;

    let resp = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/sparql-query")
        .header(reqwest::header::ACCEPT, "application/sparql-results+json")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("HTTP request failed for <{endpoint}>: {e}"))?;

    let (bytes, _content_type) = read_capped_response(resp, endpoint, "SERVICE")?;
    let json_str = String::from_utf8(bytes)
        .map_err(|e| format!("SERVICE <{endpoint}>: response body is not valid UTF-8: {e}"))?;

    // Scratch store: see this module's doc comment for the triple-term /
    // blank-node limitation this implies.
    let mut scratch = Datastore::new(64);
    let select_result = crate::serialize::sparql_json::from_sparql_json(&json_str, &mut scratch)
        .map_err(|e| format!("SERVICE <{endpoint}>: malformed SPARQL JSON results: {e:?}"))?;
    Ok(select_result.rows)
}

/// Build the `HashMap` [`sparql_parser::ServiceResultsGuard::install`]
/// expects from [`pre_resolve_services`]'s per-call results, surfacing the
/// first failure (if any) as an `Err` — a non-SILENT SERVICE whose fetch
/// fails is a hard error for this issue (#744); `SILENT`/policy nuance is
/// #745.
pub fn collect_or_first_error(
    results: Vec<(ServiceKey, Result<Vec<SolutionRow>, String>)>,
) -> Result<HashMap<ServiceKey, Vec<SolutionRow>>, sparql_parser::ExecError> {
    let mut map = HashMap::with_capacity(results.len());
    for (key, outcome) in results {
        match outcome {
            Ok(rows) => {
                map.insert(key, rows);
            }
            Err(reason) => {
                return Err(sparql_parser::ExecError::ServiceFetchFailed {
                    endpoint: key.endpoint,
                    reason,
                });
            }
        }
    }
    Ok(map)
}

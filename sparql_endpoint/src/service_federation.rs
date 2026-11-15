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
//! See issues [#744](https://github.com/daghovland/rdf-datalog/issues/744)
//! (the initial wiring) and
//! [#745](https://github.com/daghovland/rdf-datalog/issues/745) (`SILENT`/
//! error/timeout policy, below), part of epic
//! [#741](https://github.com/daghovland/rdf-datalog/issues/741) —
//! `docs/plans/SPARQL_FEDERATION_PLAN.md`.
//!
//! # `SILENT` / error / timeout policy (#745)
//!
//! A `SILENT SERVICE` is still fetched — it is *not* skipped outright (that
//! was a gap left by #744: a SILENT call against a healthy endpoint must
//! still return the remote's real results). Only a failed fetch degrades to
//! empty results for it; a non-`SILENT` call's failure still errors the
//! whole query. "Failed" covers connection failure, a non-2xx status, a
//! malformed results body, and a timeout; see `collect_or_first_error`
//! for exactly how each is dispatched, and `FetchFailure` for why timeout
//! is distinguished from every other failure.
//!
//! Each fetch's own HTTP request is bounded by `request_timeout`
//! (ultimately the query's configured `--timeout` budget, computed once in
//! `query.rs` and shared with the synchronous evaluator that runs
//! afterward) so a hung remote endpoint cannot make a federated query
//! exceed it — see `pre_resolve_services`'s doc comment.
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
//! otherwise need one, so every fetch for a query is deserialized against a
//! **throwaway scratch `Datastore`** (shared across every `SERVICE` call in
//! that one query — see [`deserialize_fetched_bodies`]), not the real query
//! datastore. This is correct for the overwhelmingly common case
//! (IRI/literal bindings), but originally (issue
//! [#772](https://github.com/daghovland/rdf-datalog/issues/772), sub-issue
//! of #741) had two known consequences:
//!
//! - A `GraphElement::TripleTerm` binding in a federated result carries
//!   `GraphElementId`s that only resolve correctly against *that* scratch
//!   store, not the real query datastore — such a binding would silently
//!   resolve to the wrong (or a dangling) value if ever displayed/joined
//!   against real data by ID. **Still open** — see
//!   [#809](https://github.com/daghovland/rdf-datalog/issues/809), filed as
//!   a follow-up when #772 fixed only the blank-node half below: unlike a
//!   blank node, a triple term's equality depends on its embedded
//!   `subject`/`predicate`/`obj` IDs matching entries in whichever store
//!   resolves them later, so neither sharing nor offsetting the scratch
//!   store's counter helps.
//! - A blank-node binding (`RdfResource::AnonymousBlankNode(n)`) was
//!   allocated from a *per-call* scratch store's own blank-node counter,
//!   which could numerically collide both with blank nodes already in the
//!   real query datastore and with a *different* `SERVICE` call's blank
//!   nodes in the same query (the latter being a correctness bug on its own
//!   terms: per SPARQL 1.1 §15.3, blank node labels are scoped to one
//!   results document and must never be treated as equal across two
//!   documents). **Fixed by #772**: every `SERVICE` call in one query now
//!   deserializes against one *shared* scratch store, seeded at
//!   [`FEDERATED_BLANK_NODE_BASE`] so its blank-node IDs also don't collide
//!   with the real store's own, for any realistic dataset size — see
//!   [`deserialize_fetched_bodies`]'s doc comment for the full reasoning.
//!
//! Restructuring the two `query.rs` call sites to pre-resolve before
//! acquiring *any* store lock (avoiding the scratch-store requirement
//! entirely, and so fixing the triple-term case too) remains a bigger,
//! separate change than either #772 or #809's scope — see
//! [#809](https://github.com/daghovland/rdf-datalog/issues/809)'s own
//! description for the specific options considered and rejected for it.

use crate::sparql_update::{build_hardened_client, read_capped_response, ssrf_preflight};
use dag_rdf::Datastore;
use ingress::NetworkPolicy;
use sparql_parser::ast::QueryComponent;
use sparql_parser::{ServiceKey, SolutionRow, collect_service_calls};
use std::collections::HashMap;
use std::time::Duration;

/// Why a SERVICE fetch failed, distinguishing "the configured `--timeout`
/// elapsed" (maps to [`sparql_parser::ExecError::Timeout`], same as every
/// other timeout) from every other failure mode (network error, non-2xx,
/// malformed body, SSRF rejection — maps to
/// [`sparql_parser::ExecError::ServiceFetchFailed`]). See issue #745.
#[derive(Debug)]
pub(crate) enum FetchFailure {
    /// The per-request timeout passed to `fetch_service_body_blocking` elapsed.
    Timeout,
    /// Any other failure; the message is already human-readable.
    Other(String),
}

/// Fetch every constant-endpoint `SERVICE` call (both `SILENT` and
/// non-`SILENT`) reachable from `where_clause_components` concurrently, and
/// return one outcome per distinct `(endpoint, body)` pair found by
/// [`collect_service_calls`] — a single failing endpoint does not prevent
/// the others from resolving. The returned `bool` is `true` only if *every*
/// occurrence of that pair in the query was `SILENT` (a pair appearing both
/// SILENT and non-SILENT in the same query is treated as non-SILENT for
/// error-handling purposes, since at least one call site needs the error to
/// propagate) — see `collect_or_first_error`'s caller for how that `bool`
/// is used.
///
/// Call this *before* acquiring any lock on the real store (see this
/// module's doc comment): it performs real network I/O and must not be
/// awaited while holding `AppState::store`'s async `RwLock` read guard.
///
/// `request_timeout`, when `Some`, bounds each fetch's own HTTP request —
/// pass the query's remaining configured `--timeout` budget so a hung
/// remote endpoint cannot make a federated query exceed it (issue #745).
/// `None` means no timeout (the production default, 30s, set by
/// `build_hardened_client` itself, still applies as a hard ceiling).
///
/// Only call this when `network` is [`NetworkPolicy::Allow`] or
/// [`NetworkPolicy::AllowList`] — same restriction as `LOAD`'s `fetch_rdf`.
pub(crate) async fn pre_resolve_services(
    where_clause_components: &[QueryComponent],
    network: &NetworkPolicy,
    allow_loopback: bool,
    request_timeout: Option<Duration>,
) -> Vec<(ServiceKey, bool, Result<Vec<SolutionRow>, FetchFailure>)> {
    let calls = collect_service_calls(where_clause_components);

    // Dedupe by key first: the same `(endpoint, body)` pair can appear more
    // than once (e.g. inside both branches of a UNION), and merging here
    // means it is fetched exactly once. A pair is only SILENT overall if
    // every occurrence was SILENT.
    let mut by_key: HashMap<ServiceKey, bool> = HashMap::new();
    for (endpoint, body, silent) in calls {
        by_key
            .entry(ServiceKey { endpoint, body })
            .and_modify(|all_silent| *all_silent = *all_silent && silent)
            .or_insert(silent);
    }

    let mut handles = Vec::with_capacity(by_key.len());
    for (key, silent) in by_key {
        let network = network.clone();
        let endpoint_for_task = key.endpoint.clone();
        let body_for_task = key.body.clone();
        let handle = tokio::task::spawn_blocking(move || {
            fetch_service_body_blocking(
                &endpoint_for_task,
                &body_for_task,
                &network,
                allow_loopback,
                request_timeout,
            )
        });
        handles.push((key, silent, handle));
    }

    let mut bodies = Vec::with_capacity(handles.len());
    for (key, silent, handle) in handles {
        let outcome = match handle.await {
            Ok(r) => r,
            Err(join_err) => Err(FetchFailure::Other(format!(
                "SERVICE fetch task panicked: {join_err}"
            ))),
        };
        bodies.push((key, silent, outcome));
    }

    // Deserialize every successfully-fetched body now, all against one
    // *shared* scratch `Datastore` (not one scratch store per fetch) — see
    // `deserialize_fetched_bodies`'s doc comment for why sharing the store
    // across every call in this one query matters (issue #772).
    deserialize_fetched_bodies(bodies)
}

/// Blocking: `POST` the SERVICE body to `endpoint` as
/// `application/sparql-query`, requesting `application/sparql-results+json`,
/// and return the raw response body text — deserialization happens
/// separately, in [`deserialize_fetched_bodies`], once every concurrent
/// fetch for this query has completed (see that function's doc comment for
/// why).
///
/// `request_timeout`, when `Some`, is set as this specific request's
/// `reqwest` timeout, overriding `build_hardened_client`'s 30s client
/// default — see `pre_resolve_services`'s doc comment.
///
/// Runs inside `tokio::task::spawn_blocking` (see
/// `pre_resolve_services`) — never call this directly from async code
/// without that wrapper, since it blocks the calling thread on network I/O.
fn fetch_service_body_blocking(
    endpoint: &str,
    body: &str,
    network: &NetworkPolicy,
    allow_loopback: bool,
    request_timeout: Option<Duration>,
) -> Result<String, FetchFailure> {
    let client =
        build_hardened_client(endpoint, network, allow_loopback, ssrf_preflight, "SERVICE")
            .map_err(FetchFailure::Other)?;

    let mut req = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/sparql-query")
        .header(reqwest::header::ACCEPT, "application/sparql-results+json")
        .body(body.to_string());
    if let Some(t) = request_timeout {
        req = req.timeout(t);
    }
    let resp = req.send().map_err(|e| {
        if e.is_timeout() {
            FetchFailure::Timeout
        } else {
            FetchFailure::Other(format!("HTTP request failed for <{endpoint}>: {e}"))
        }
    })?;

    let (bytes, _content_type) =
        read_capped_response(resp, endpoint, "SERVICE").map_err(FetchFailure::Other)?;
    let json_str = String::from_utf8(bytes).map_err(|e| {
        FetchFailure::Other(format!(
            "SERVICE <{endpoint}>: response body is not valid UTF-8: {e}"
        ))
    })?;

    Ok(json_str)
}

/// Blank-node `GraphElementId`s assigned while deserializing federated
/// `SERVICE` results are seeded starting from this value, so they cannot
/// numerically collide with blank nodes already present in the real query
/// datastore at the time the query runs (issue #772). This does not make a
/// collision mathematically impossible — the real store's own blank-node
/// counter also starts at 0 and grows without bound as more data is loaded —
/// but closes the common case (any dataset with fewer than ~1 billion blank
/// nodes) with no extra lock acquisition on the real store, which is what
/// made the full fix (restructuring `sparql_endpoint::query`'s handlers to
/// get real `&mut Datastore` access before/during pre-resolution) too large
/// for this issue — see this module's doc comment.
const FEDERATED_BLANK_NODE_BASE: u32 = 1 << 30;

/// Deserialize every successfully-fetched `SERVICE` response body for one
/// query into [`SolutionRow`]s, all against **one shared** scratch
/// `Datastore` rather than a separate scratch store per fetch.
///
/// Sharing one store across every `SERVICE` call in the query (not just
/// seeding each store's blank-node counter from the same base) matters
/// because RDF 1.2/SPARQL 1.1 §15.3 blank node labels are scoped to one
/// results document: the label `"b0"` in one endpoint's response and the
/// label `"b0"` in a *different* endpoint's response denote unrelated blank
/// nodes and must never be joined as equal. Deserializing each response
/// against its own fresh store (the pre-#772 behaviour, just with the
/// scratch store reused per-call instead of per-query) would have given both
/// their first blank node the same low numeric ID, making them spuriously
/// equal under [`GraphElement`]'s derived `PartialEq` wherever two federated
/// bindings are compared (e.g. `merge_solutions`' natural join) — the exact
/// opposite of what per-document label scoping requires. One shared store
/// (seeded once, at [`FEDERATED_BLANK_NODE_BASE`]) keeps every blank node
/// encountered while resolving this query numerically distinct, both from
/// each other and from the real store's own blank nodes, by construction.
///
/// Called once `pre_resolve_services` has awaited every concurrent fetch, so
/// this step is deliberately sequential — these are all pure, non-blocking
/// `serde_json`-driven parses (the `spawn_blocking` boundary above covers the
/// actual network I/O, not this), not a source of real concurrency to begin
/// with.
///
/// Triple-term bindings are *not* covered by this fix: a
/// `GraphElement::TripleTerm`'s `subject`/`predicate`/`obj` are
/// `GraphElementId`s into whichever store resolves them later, and sharing
/// the scratch store across calls (or offsetting its counter) does nothing
/// for a store other than the scratch one — see issue
/// [#809](https://github.com/daghovland/rdf-datalog/issues/809), filed as a
/// follow-up, and the `#[ignore]`d regression test in
/// `sparql_endpoint/tests/service_federation.rs`.
fn deserialize_fetched_bodies(
    bodies: Vec<(ServiceKey, bool, Result<String, FetchFailure>)>,
) -> Vec<(ServiceKey, bool, Result<Vec<SolutionRow>, FetchFailure>)> {
    let mut scratch = Datastore::new(64);
    scratch.resources.anon_resource_count = FEDERATED_BLANK_NODE_BASE;

    bodies
        .into_iter()
        .map(|(key, silent, outcome)| {
            let result = outcome.and_then(|json_str| {
                crate::serialize::sparql_json::from_sparql_json(&json_str, &mut scratch)
                    .map(|select_result| select_result.rows)
                    .map_err(|e| {
                        FetchFailure::Other(format!(
                            "SERVICE <{}>: malformed SPARQL JSON results: {e:?}",
                            key.endpoint
                        ))
                    })
            });
            (key, silent, result)
        })
        .collect()
}

/// Build the `HashMap` [`sparql_parser::ServiceResultsGuard::install`]
/// expects from `pre_resolve_services`'s per-call results.
///
/// A `SILENT` call (the `bool`) whose fetch failed is simply omitted from
/// the map rather than erroring — `QueryComponent::Service`'s evaluation arm
/// already falls back to empty results for any key with no entry, so this
/// is exactly the "treat as zero rows" behavior issue #745 requires. A
/// non-`SILENT` call's failure still errors: a timeout maps to
/// [`sparql_parser::ExecError::Timeout`] (consistent with every other
/// query-timeout), anything else to
/// [`sparql_parser::ExecError::ServiceFetchFailed`].
pub(crate) fn collect_or_first_error(
    results: Vec<(ServiceKey, bool, Result<Vec<SolutionRow>, FetchFailure>)>,
) -> Result<HashMap<ServiceKey, Vec<SolutionRow>>, sparql_parser::ExecError> {
    let mut map = HashMap::with_capacity(results.len());
    for (key, silent, outcome) in results {
        match outcome {
            Ok(rows) => {
                map.insert(key, rows);
            }
            Err(failure) => {
                if silent {
                    continue;
                }
                return Err(match failure {
                    FetchFailure::Timeout => sparql_parser::ExecError::Timeout,
                    FetchFailure::Other(reason) => sparql_parser::ExecError::ServiceFetchFailed {
                        endpoint: key.endpoint,
                        reason,
                    },
                });
            }
        }
    }
    Ok(map)
}

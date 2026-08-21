/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Dereferenceable resource IRIs (#493).
//!
//! Routes: `GET /describe?uri=<encoded IRI>` (the description endpoint) and
//! `GET /id/{*path}` (a generic slash-URI namespace that 303-redirects into
//! it).
//!
//! See `docs/plans/DEREFERENCEABLE_RESOURCE_IRIS_493_PLAN.md` for the full
//! design and the reasoning behind every decision below, in particular why
//! this does NOT reuse SPARQL's `DESCRIBE <iri>` (`Query::Describe` is
//! outbound-only by deliberate design, see #281) and why there is no auth
//! bypass here unlike the #441 vocabulary routes.

use crate::{AppState, graph_store::graph_response_parts};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use dag_rdf::{Datastore, ingress::DEFAULT_GRAPH_ELEMENT_ID};
use std::collections::HashMap;

/// Loose IRI sanity check: not empty, no whitespace/control characters, and
/// contains a `scheme://` separator. Deliberately not full RFC 3986
/// validation -- this only needs to catch obviously-not-an-IRI input (e.g. a
/// bare phrase) before it's used as a literal resource lookup key, since
/// the lookup itself is a `HashMap` key comparison, not a string built into
/// a query (no injection surface to defend against here).
fn looks_like_iri(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(char::is_whitespace) && s.contains("://")
}

/// Merge outbound (`<iri> ?p ?o`) and inbound (`?s ?p <iri>`) triples for the
/// given IRI into a fresh temporary `Datastore`'s default graph, for
/// serialisation via the existing GSP content-negotiation machinery.
///
/// Returns `None` if `iri` was never interned in `store` (nothing is known
/// about it at all) -- the caller turns that into `404`.
///
/// Outbound-only `DESCRIBE <iri>` (`sparql_parser::execute`'s
/// `Query::Describe`) is deliberately not reused here: per #281's
/// documented scope decision, it only collects triples where the resource
/// is the subject. For a `bl:Issue`, most of what's worth describing points
/// *at* it (e.g. an `agp:AgentSession`'s `agp:reasoningFor`), not from it,
/// so both directions are gathered directly from the subject/object quad
/// indexes instead.
fn describe_datastore(store: &Datastore, iri: &str) -> Option<Datastore> {
    let resource_id = store.lookup_named_graph_id(iri)?;

    let mut tmp = Datastore::new(64);
    for quad in store.named_graphs.get_quads_with_subject(resource_id) {
        let s = tmp.add_resource(store.resources.get_graph_element(quad.subject).clone());
        let p = tmp.add_resource(store.resources.get_graph_element(quad.predicate).clone());
        let o = tmp.add_resource(store.resources.get_graph_element(quad.obj).clone());
        tmp.add_quad(dag_rdf::ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s,
            predicate: p,
            obj: o,
        });
    }
    for quad in store.named_graphs.get_quads_with_object(resource_id) {
        let s = tmp.add_resource(store.resources.get_graph_element(quad.subject).clone());
        let p = tmp.add_resource(store.resources.get_graph_element(quad.predicate).clone());
        let o = tmp.add_resource(store.resources.get_graph_element(quad.obj).clone());
        tmp.add_quad(dag_rdf::ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s,
            predicate: p,
            obj: o,
        });
    }
    Some(tmp)
}

/// `GET /describe?uri=<encoded IRI>` -- the description endpoint every 303
/// redirect (from `/id/*` or, in future, any other dagalog.no-owned
/// resource-IRI namespace) targets.
pub async fn describe_get(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let uri = match params.get("uri") {
        Some(u) if !u.is_empty() => u,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing required query parameter: uri",
            )
                .into_response();
        }
    };

    if !looks_like_iri(uri) {
        return (
            StatusCode::BAD_REQUEST,
            "uri parameter is not a valid absolute IRI",
        )
            .into_response();
    }

    let store = state.store.read().await;
    let described = match describe_datastore(&store, uri) {
        Some(ds) => ds,
        None => {
            return (
                StatusCode::NOT_FOUND,
                format!("No data known for resource: {uri}"),
            )
                .into_response();
        }
    };

    let accept = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok());
    graph_response_parts(&described, DEFAULT_GRAPH_ELEMENT_ID, accept)
}

/// `GET /id/{*path}` -- generic dagalog.no-owned slash-URI resource
/// namespace. Reconstructs the full resource IRI as `base_iri` + the
/// request path and 303s to its description at `/describe?uri=...`.
///
/// A relative `Location` is used deliberately (see plan doc §1): it avoids
/// leaking a misconfigured/internal `base_iri` to external clients, and
/// resolves against whatever host the client actually used to reach this
/// route.
pub async fn id_redirect_get(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let resource_iri = format!(
        "{}/id/{}",
        state.config.base_iri.trim_end_matches('/'),
        path
    );
    let location = format!("/describe?uri={}", percent_encode(&resource_iri));
    (StatusCode::SEE_OTHER, [(header::LOCATION, location)], ()).into_response()
}

/// Minimal percent-encoding for building the `Location` header's `uri=`
/// query value. Encodes everything outside the RFC 3986 "unreserved" set,
/// which is always safe (over-encoding a query-string value is never
/// wrong), rather than pulling in a new crate dependency for this one call
/// site.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

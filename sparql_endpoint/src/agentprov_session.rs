/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `GET /ns/agentprov/session` (#567) -- the hash-URI namespace document
//! `agp:AgentSession`/`agp:TranscriptSummary`/`agp:Decision` individuals
//! (all minted under `https://dagalog.no/ns/agentprov/session#<id>`, see
//! `scripts/new-provenance-summary.sh`) dereference to once a client strips
//! the `#fragment`, per the hash-URI convention #441 already established
//! for the `bl:`/`agp:` vocabulary terms.
//!
//! See `docs/plans/AGENTPROV_SESSION_DEREF_567_PLAN.md` for the full design
//! and why this route differs from `/describe` (#493) in three ways:
//! outbound-only (this is a namespace document describing the resources it
//! defines, not a single resource's full neighbourhood), 200 rather than
//! 404 when no sessions are loaded (a namespace document describing an
//! empty set is still valid), and unauthenticated at the Caddy edge (the
//! underlying data -- `provenance/summaries/*.ttl` -- is already public,
//! unlike arbitrary live dataset content `/describe` can expose).

use crate::{AppState, graph_store::graph_response_parts};
use axum::{
    extract::State,
    http::{HeaderMap, header},
    response::Response,
};
use dag_rdf::{Datastore, ingress::DEFAULT_GRAPH_ELEMENT_ID};
use ingress::{GraphElement, RdfResource};

/// The `agp:AgentSession` hash-URI namespace, fixed regardless of any
/// deployment's `base_iri` -- this matches
/// `scripts/new-provenance-summary.sh`'s hardcoded `session:` prefix
/// (`https://dagalog.no/ns/agentprov/session#`), not a value derived from
/// `Config::base_iri`.
const SESSION_NS: &str = "https://dagalog.no/ns/agentprov/session#";

/// Build a fresh `Datastore` containing every outbound triple (`<iri> ?p
/// ?o`) whose subject IRI falls under [`SESSION_NS`], from every quad in
/// `store`'s default graph. Always returns `Some`, even when no such triple
/// exists (an empty namespace document is still valid) -- unlike
/// `describe::describe_datastore`, which returns `None` for a genuinely
/// unknown single resource.
fn session_namespace_datastore(store: &Datastore) -> Datastore {
    let mut tmp = Datastore::new(64);
    for quad in store.named_graphs.get_all_quads() {
        let subject_element = store.resources.get_graph_element(quad.subject);
        let is_session_subject = matches!(
            subject_element,
            GraphElement::NodeOrEdge(RdfResource::Iri(iri)) if iri.0.starts_with(SESSION_NS)
        );
        if !is_session_subject {
            continue;
        }
        let s = tmp.add_resource(subject_element.clone());
        let p = tmp.add_resource(store.resources.get_graph_element(quad.predicate).clone());
        let o = tmp.add_resource(store.resources.get_graph_element(quad.obj).clone());
        tmp.add_quad(dag_rdf::ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s,
            predicate: p,
            obj: o,
        });
    }
    tmp
}

/// `GET /ns/agentprov/session` -- see module docs.
pub async fn agentprov_session_document_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let store = state.store.read().await;
    let described = session_namespace_datastore(&store);
    let accept = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok());
    graph_response_parts(&described, DEFAULT_GRAPH_ELEMENT_ID, accept)
}

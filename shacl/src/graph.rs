/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Helpers for querying and mutating a `Datastore`.

use dag_rdf::{Datastore, GraphElement, GraphElementId, IriReference, RdfLiteral, RdfResource};
use ingress::{RDF_FIRST, RDF_NIL, RDF_REST};

// ── Read-only lookups ─────────────────────────────────────────────────────────

/// Look up the `GraphElementId` of an IRI in `ds` without modifying it.
/// Returns `None` if the IRI has never been interned.
pub fn lookup_iri(ds: &Datastore, iri: &str) -> Option<GraphElementId> {
    ds.resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            iri.to_string(),
        ))))
        .copied()
}

/// Return all objects for triples `(subject, pred_iri, ?)` in the default graph.
pub fn get_objects(ds: &Datastore, subject: GraphElementId, pred_iri: &str) -> Vec<GraphElementId> {
    let Some(pred_id) = lookup_iri(ds, pred_iri) else {
        return vec![];
    };
    ds.get_triples_with_subject_predicate(subject, pred_id)
        .map(|t| t.obj)
        .collect()
}

/// Return the first object for `(subject, pred_iri, ?)`, or `None`.
pub fn get_object(
    ds: &Datastore,
    subject: GraphElementId,
    pred_iri: &str,
) -> Option<GraphElementId> {
    get_objects(ds, subject, pred_iri).into_iter().next()
}

/// Traverse an RDF list starting at `head` and return the list items.
pub fn rdf_list(ds: &Datastore, head: GraphElementId) -> Vec<GraphElementId> {
    let rdf_nil = lookup_iri(ds, RDF_NIL);
    let Some(rdf_first) = lookup_iri(ds, RDF_FIRST) else {
        return vec![];
    };
    let Some(rdf_rest) = lookup_iri(ds, RDF_REST) else {
        return vec![];
    };

    let mut items = Vec::new();
    let mut current = head;
    loop {
        if rdf_nil == Some(current) {
            break;
        }
        for t in ds.get_triples_with_subject_predicate(current, rdf_first) {
            items.push(t.obj);
        }
        match ds
            .get_triples_with_subject_predicate(current, rdf_rest)
            .next()
        {
            None => break,
            Some(t) => current = t.obj,
        }
    }
    items
}

/// Return the IRI string for element `id` in `ds`, or `None` if it is not an IRI.
pub fn iri_string(ds: &Datastore, id: GraphElementId) -> Option<String> {
    ds.resources.get_named_resource(id).map(|iri| iri.0.clone())
}

/// Return a display string for any graph element (IRI, blank node, or literal).
pub fn element_display(ds: &Datastore, id: GraphElementId) -> String {
    match ds.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => iri.0.clone(),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => format!("_:b{n}"),
        GraphElement::GraphLiteral(lit) => lit.to_string(),
    }
}

/// Extract an unsigned 64-bit integer from a literal element.
pub fn elem_to_u64(ds: &Datastore, id: GraphElementId) -> Option<u64> {
    match ds.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.to_string().parse().ok(),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            literal.parse().ok()
        }
        _ => None,
    }
}

/// Extract a boolean from a literal element.
pub fn elem_to_bool(ds: &Datastore, id: GraphElementId) -> Option<bool> {
    match ds.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => Some(*b),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            literal.parse().ok()
        }
        _ => None,
    }
}

/// Return `true` if element `id` is an IRI resource.
pub fn is_iri(ds: &Datastore, id: GraphElementId) -> bool {
    matches!(
        ds.resources.get_graph_element(id),
        GraphElement::NodeOrEdge(RdfResource::Iri(_))
    )
}

/// Return `true` if element `id` is a blank node.
pub fn is_blank_node(ds: &Datastore, id: GraphElementId) -> bool {
    matches!(
        ds.resources.get_graph_element(id),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(_))
    )
}

// ── Mutation helpers ──────────────────────────────────────────────────────────

/// Intern an IRI into `ds`, returning its `GraphElementId`.
/// Idempotent: calling twice with the same IRI returns the same ID.
pub fn intern_iri(ds: &mut Datastore, iri: &str) -> GraphElementId {
    ds.resources
        .add_node_resource(RdfResource::Iri(IriReference(iri.to_string())))
}

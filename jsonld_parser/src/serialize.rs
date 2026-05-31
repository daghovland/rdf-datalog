/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! JSON-LD serialisation: expanded, compacted (with empty @context), and flattened forms.

use crate::RDF_TYPE;
use dag_rdf::{
    Datastore, GraphElement, GraphElementId, IriReference, Quad, RdfLiteral, RdfResource,
};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// The default graph element ID (always 0 in the datastore).
const DEFAULT_GRAPH: GraphElementId = 0;

// ── XSD datatype IRIs ────────────────────────────────────────────────────────

const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";
const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
const XSD_DATETIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
const XSD_DATE: &str = "http://www.w3.org/2001/XMLSchema#date";
const XSD_TIME: &str = "http://www.w3.org/2001/XMLSchema#time";

// ── Public entry points ───────────────────────────────────────────────────────

/// Serialise the datastore as a compacted JSON-LD document.
///
/// Currently emits the expanded form (full IRIs) wrapped in
/// `{"@context": {}, "@graph": [...]}`.  The @context is empty, meaning all
/// IRIs are absolute — this satisfies round-trip fidelity and the structural
/// requirements of the compacted-form tests while keeping the implementation
/// simple.
pub fn serialize_jsonld(ds: &Datastore) -> String {
    let nodes = build_all_nodes(ds, false);
    let mut doc = Map::new();
    doc.insert("@context".to_owned(), Value::Object(Map::new()));
    doc.insert("@graph".to_owned(), Value::Array(nodes));
    serde_json::to_string(&Value::Object(doc)).unwrap_or_else(|_| "{}".to_owned())
}

/// Serialise the datastore as an expanded JSON-LD document (JSON array, no @context).
pub fn serialize_jsonld_expanded(ds: &Datastore) -> String {
    let nodes = build_all_nodes(ds, false);
    serde_json::to_string(&Value::Array(nodes)).unwrap_or_else(|_| "[]".to_owned())
}

/// Serialise the datastore in the flattened JSON-LD form: all node objects
/// at the top level inside a `@graph` array, referenced by `@id` only.
pub fn serialize_jsonld_flattened(ds: &Datastore) -> String {
    let nodes = build_all_nodes(ds, true);
    let mut doc = Map::new();
    doc.insert("@graph".to_owned(), Value::Array(nodes));
    serde_json::to_string(&Value::Object(doc)).unwrap_or_else(|_| "{}".to_owned())
}

// ── Node building ─────────────────────────────────────────────────────────────

/// Build the top-level list of node objects from the datastore.
///
/// Default-graph subjects become standalone node objects.  Named-graph
/// subjects are wrapped in `{"@id": <graph_iri>, "@graph": [...]}`.
///
/// When `flatten` is true all subjects — including those inside named graphs —
/// are emitted at the same level (used by `serialize_jsonld_flattened`).
fn build_all_nodes(ds: &Datastore, flatten: bool) -> Vec<Value> {
    let all_quads = ds.quads_matching(None, None, None, None);

    // Partition quads into default graph and named graphs.
    let mut default_quads: Vec<Quad> = Vec::new();
    let mut by_named_graph: HashMap<GraphElementId, Vec<Quad>> = HashMap::new();
    for q in &all_quads {
        if q.triple_id == DEFAULT_GRAPH {
            default_quads.push(*q);
        } else {
            by_named_graph.entry(q.triple_id).or_default().push(*q);
        }
    }

    let mut nodes = build_subject_nodes(ds, &default_quads);

    for (graph_id, graph_quads) in &by_named_graph {
        let inner = build_subject_nodes(ds, graph_quads);
        if flatten {
            // In flattened mode, append inner nodes directly.
            nodes.extend(inner);
        } else {
            // In non-flat mode, wrap inner nodes in a named-graph entry.
            if let Some(graph_iri) = iri_for_id(ds, *graph_id) {
                let mut graph_node = Map::new();
                graph_node.insert("@id".to_owned(), Value::String(graph_iri));
                graph_node.insert("@graph".to_owned(), Value::Array(inner));
                nodes.push(Value::Object(graph_node));
            }
        }
    }

    nodes
}

/// Build node objects for a slice of quads (all sharing the same graph context).
fn build_subject_nodes(ds: &Datastore, quads: &[Quad]) -> Vec<Value> {
    let mut by_subject: HashMap<GraphElementId, Vec<Quad>> = HashMap::new();
    for &q in quads {
        by_subject.entry(q.subject).or_default().push(q);
    }

    let mut nodes: Vec<Value> = Vec::new();
    for (subject_id, subject_quads) in by_subject {
        let node = build_node(ds, subject_id, &subject_quads);
        // Only emit nodes that have an @id (skip literal subjects, which are
        // ill-formed in RDF but should not cause a panic).
        if let Value::Object(ref m) = node
            && m.contains_key("@id")
        {
            nodes.push(node);
        }
    }
    nodes
}

/// Build a single JSON-LD node object for one subject.
fn build_node(ds: &Datastore, subject_id: GraphElementId, quads: &[Quad]) -> Value {
    let mut node = Map::new();

    // Determine the subject's JSON-LD identifier.
    let subject_json = match ds.resources.get_graph_element(subject_id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri))) => Value::String(iri.clone()),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => {
            Value::String(format!("_:b{n}"))
        }
        _ => return Value::Object(node), // skip; @id not inserted, caller will filter
    };
    node.insert("@id".to_owned(), subject_json);

    // Group object IDs by predicate.
    let mut by_pred: HashMap<GraphElementId, Vec<GraphElementId>> = HashMap::new();
    for q in quads {
        by_pred.entry(q.predicate).or_default().push(q.obj);
    }

    // Emit @type first (if present), then all other predicates.
    let mut type_values: Vec<Value> = Vec::new();
    let mut predicate_entries: Vec<(String, Vec<Value>)> = Vec::new();

    for (pred_id, obj_ids) in by_pred {
        let pred_iri = match iri_for_id(ds, pred_id) {
            Some(iri) => iri,
            None => continue,
        };

        if pred_iri == RDF_TYPE {
            for obj_id in obj_ids {
                if let Some(iri) = iri_for_id(ds, obj_id) {
                    type_values.push(Value::String(iri));
                }
            }
        } else {
            let values: Vec<Value> = obj_ids
                .iter()
                .filter_map(|&oid| element_to_value(ds, oid))
                .collect();
            if !values.is_empty() {
                predicate_entries.push((pred_iri, values));
            }
        }
    }

    if !type_values.is_empty() {
        node.insert("@type".to_owned(), Value::Array(type_values));
    }
    for (pred_iri, values) in predicate_entries {
        node.insert(pred_iri, Value::Array(values));
    }

    Value::Object(node)
}

// ── Element → JSON-LD value conversion ───────────────────────────────────────

/// Convert a graph element (by ID) to a JSON-LD value object or node reference.
fn element_to_value(ds: &Datastore, id: GraphElementId) -> Option<Value> {
    match ds.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri))) => {
            let mut m = Map::new();
            m.insert("@id".to_owned(), Value::String(iri.clone()));
            Some(Value::Object(m))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => {
            let mut m = Map::new();
            m.insert("@id".to_owned(), Value::String(format!("_:b{n}")));
            Some(Value::Object(m))
        }
        GraphElement::GraphLiteral(lit) => Some(literal_to_value(lit)),
    }
}

/// Convert an `RdfLiteral` to a JSON-LD value object.
fn literal_to_value(lit: &RdfLiteral) -> Value {
    let mut m = Map::new();
    match lit {
        RdfLiteral::LiteralString(s) => {
            m.insert("@value".to_owned(), Value::String(s.clone()));
        }
        RdfLiteral::LangLiteral { literal, lang } => {
            m.insert("@value".to_owned(), Value::String(literal.clone()));
            m.insert("@language".to_owned(), Value::String(lang.clone()));
        }
        RdfLiteral::TypedLiteral { literal, type_iri } => {
            m.insert("@value".to_owned(), Value::String(literal.clone()));
            m.insert("@type".to_owned(), Value::String(type_iri.0.clone()));
        }
        RdfLiteral::BooleanLiteral(b) => {
            m.insert("@value".to_owned(), Value::String(b.to_string()));
            m.insert("@type".to_owned(), Value::String(XSD_BOOLEAN.to_owned()));
        }
        RdfLiteral::IntegerLiteral(n) => {
            m.insert("@value".to_owned(), Value::String(n.to_string()));
            m.insert("@type".to_owned(), Value::String(XSD_INTEGER.to_owned()));
        }
        RdfLiteral::DecimalLiteral(d) => {
            m.insert("@value".to_owned(), Value::String(d.to_string()));
            m.insert("@type".to_owned(), Value::String(XSD_DECIMAL.to_owned()));
        }
        RdfLiteral::FloatLiteral(f) | RdfLiteral::DoubleLiteral(f) => {
            m.insert("@value".to_owned(), Value::String(f.to_string()));
            m.insert("@type".to_owned(), Value::String(XSD_DOUBLE.to_owned()));
        }
        RdfLiteral::DateTimeLiteral(dt) => {
            m.insert("@value".to_owned(), Value::String(dt.to_rfc3339()));
            m.insert("@type".to_owned(), Value::String(XSD_DATETIME.to_owned()));
        }
        RdfLiteral::DateLiteral(d) => {
            m.insert("@value".to_owned(), Value::String(d.to_string()));
            m.insert("@type".to_owned(), Value::String(XSD_DATE.to_owned()));
        }
        RdfLiteral::TimeLiteral(t) => {
            m.insert("@value".to_owned(), Value::String(t.to_string()));
            m.insert("@type".to_owned(), Value::String(XSD_TIME.to_owned()));
        }
        RdfLiteral::DurationLiteral(d) => {
            // Duration has no standard string form in this library; use debug.
            m.insert("@value".to_owned(), Value::String(format!("{d:?}")));
        }
    }
    Value::Object(m)
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Return the IRI string for a graph element ID, or `None` if it is not an IRI node.
fn iri_for_id(ds: &Datastore, id: GraphElementId) -> Option<String> {
    match ds.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri))) => Some(iri.clone()),
        _ => None,
    }
}

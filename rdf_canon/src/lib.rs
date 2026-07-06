/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! RDF Dataset Canonicalization (RDFC-1.0 / URDNA2015) over Dagalog's [`Datastore`].
//!
//! Wraps the [`rdf-canon`](https://crates.io/crates/rdf-canon) crate and provides
//! conversion from Dagalog's internal representation to `oxrdf` types.
//!
//! Related issues: [#69](https://github.com/daghovland/rdf-datalog/issues/69),
//! [#70](https://github.com/daghovland/rdf-datalog/issues/70),
//! [#71](https://github.com/daghovland/rdf-datalog/issues/71).

use dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID;
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, RdfResource};
use ingress::{
    XSD_BOOLEAN, XSD_DATE, XSD_DATE_TIME, XSD_DECIMAL, XSD_DOUBLE, XSD_DURATION, XSD_FLOAT,
    XSD_INTEGER, XSD_TIME,
};
use oxrdf::{BlankNode, Dataset, GraphName, Literal, NamedNode, Quad, Subject, Term};
use rdf_canon_upstream::canonicalize;

// ── Conversion helpers ────────────────────────────────────────────────────────

fn resource_to_subject(resource: &RdfResource) -> Subject {
    match resource {
        RdfResource::Iri(iri) => Subject::NamedNode(NamedNode::new_unchecked(iri.0.clone())),
        RdfResource::AnonymousBlankNode(id) => {
            Subject::BlankNode(BlankNode::new_unchecked(format!("b{id}")))
        }
    }
}

fn resource_to_term(resource: &RdfResource) -> Term {
    match resource {
        RdfResource::Iri(iri) => Term::NamedNode(NamedNode::new_unchecked(iri.0.clone())),
        RdfResource::AnonymousBlankNode(id) => {
            Term::BlankNode(BlankNode::new_unchecked(format!("b{id}")))
        }
    }
}

fn resource_to_predicate(resource: &RdfResource) -> Option<NamedNode> {
    match resource {
        RdfResource::Iri(iri) => Some(NamedNode::new_unchecked(iri.0.clone())),
        // Blank nodes cannot be predicates in RDF 1.1.
        RdfResource::AnonymousBlankNode(_) => None,
    }
}

fn literal_to_oxrdf(literal: &RdfLiteral) -> Literal {
    match literal {
        RdfLiteral::LiteralString(s) => Literal::new_simple_literal(s),
        RdfLiteral::LangLiteral { lang, literal } => {
            Literal::new_language_tagged_literal(literal, lang)
                .unwrap_or_else(|_| Literal::new_simple_literal(literal))
        }
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            Literal::new_typed_literal(literal, NamedNode::new_unchecked(type_iri.0.clone()))
        }
        RdfLiteral::BooleanLiteral(b) => Literal::new_typed_literal(
            if *b { "true" } else { "false" },
            NamedNode::new_unchecked(XSD_BOOLEAN),
        ),
        RdfLiteral::IntegerLiteral(i) => {
            Literal::new_typed_literal(i.to_string(), NamedNode::new_unchecked(XSD_INTEGER))
        }
        RdfLiteral::DecimalLiteral(d) => {
            Literal::new_typed_literal(d.to_string(), NamedNode::new_unchecked(XSD_DECIMAL))
        }
        RdfLiteral::FloatLiteral(f) => {
            Literal::new_typed_literal(f.to_string(), NamedNode::new_unchecked(XSD_FLOAT))
        }
        RdfLiteral::DoubleLiteral(f) => {
            Literal::new_typed_literal(f.to_string(), NamedNode::new_unchecked(XSD_DOUBLE))
        }
        RdfLiteral::DateTimeLiteral(dt) => Literal::new_typed_literal(
            dt.format("%Y-%m-%dT%H:%M:%S%.fZ").to_string(),
            NamedNode::new_unchecked(XSD_DATE_TIME),
        ),
        RdfLiteral::DateLiteral(d) => {
            Literal::new_typed_literal(d.to_string(), NamedNode::new_unchecked(XSD_DATE))
        }
        RdfLiteral::TimeLiteral(t) => {
            Literal::new_typed_literal(t.to_string(), NamedNode::new_unchecked(XSD_TIME))
        }
        RdfLiteral::DurationLiteral(dur) => {
            let total_secs = dur.num_seconds().unsigned_abs();
            let days = total_secs / 86400;
            let hours = (total_secs % 86400) / 3600;
            let minutes = (total_secs % 3600) / 60;
            let secs = total_secs % 60;
            let sign = if dur.num_seconds() < 0 { "-" } else { "" };
            let s = format!("{sign}P{days}DT{hours}H{minutes}M{secs}S");
            Literal::new_typed_literal(s, NamedNode::new_unchecked(XSD_DURATION))
        }
    }
}

fn element_to_subject(element: &GraphElement) -> Option<Subject> {
    match element {
        GraphElement::NodeOrEdge(r) => Some(resource_to_subject(r)),
        // Literals cannot be subjects in RDF 1.1.
        GraphElement::GraphLiteral(_) => None,
    }
}

fn element_to_term(element: &GraphElement) -> Option<Term> {
    match element {
        GraphElement::NodeOrEdge(r) => Some(resource_to_term(r)),
        GraphElement::GraphLiteral(l) => Some(Term::Literal(literal_to_oxrdf(l))),
    }
}

fn element_to_predicate(element: &GraphElement) -> Option<NamedNode> {
    match element {
        GraphElement::NodeOrEdge(r) => resource_to_predicate(r),
        GraphElement::GraphLiteral(_) => None,
    }
}

fn element_to_graph_name(element: &GraphElement) -> Option<GraphName> {
    match element {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => Some(GraphName::NamedNode(
            NamedNode::new_unchecked(iri.0.clone()),
        )),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => Some(
            GraphName::BlankNode(BlankNode::new_unchecked(format!("bg{id}"))),
        ),
        GraphElement::GraphLiteral(_) => None,
    }
}

// ── Dataset construction ──────────────────────────────────────────────────────

/// Convert all quads in `store.named_graphs` into an `oxrdf::Dataset`.
///
/// Quads whose graph ID is [`DEFAULT_GRAPH_ELEMENT_ID`] are placed in
/// `GraphName::DefaultGraph`; all others use `GraphName::NamedNode`.
///
/// Quads with invalid structure (blank-node predicates, literal subjects /
/// graph names) are silently skipped — they cannot appear in valid RDF 1.1.
fn datastore_to_oxrdf_dataset(store: &Datastore) -> Dataset {
    let mut dataset = Dataset::default();
    for quad in store.named_graphs.get_all_quads() {
        let graph_name = if quad.triple_id == DEFAULT_GRAPH_ELEMENT_ID {
            GraphName::DefaultGraph
        } else {
            let g = store.resources.get_graph_element(quad.triple_id);
            match element_to_graph_name(g) {
                Some(gn) => gn,
                None => continue,
            }
        };

        if let Some(oxquad) =
            build_oxrdf_quad(store, quad.subject, quad.predicate, quad.obj, graph_name)
        {
            dataset.insert(&oxquad);
        }
    }
    dataset
}

/// Build a single `oxrdf::Quad` from Dagalog element IDs, or return `None` if
/// the quad is structurally invalid (can't happen with data inserted through the
/// public API, but we guard defensively).
fn build_oxrdf_quad(
    store: &Datastore,
    subject_id: GraphElementId,
    predicate_id: GraphElementId,
    object_id: GraphElementId,
    graph_name: GraphName,
) -> Option<Quad> {
    let subject = element_to_subject(store.resources.get_graph_element(subject_id))?;
    let predicate = element_to_predicate(store.resources.get_graph_element(predicate_id))?;
    let object = element_to_term(store.resources.get_graph_element(object_id))?;
    Some(Quad::new(subject, predicate, object, graph_name))
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Canonicalize the full dataset to an N-Quads string (RDFC-1.0 / URDNA2015).
///
/// Blank nodes are renamed to deterministic labels `_:c14n0`, `_:c14n1`, …
/// and all quads are sorted lexicographically.  Two isomorphic datasets produce
/// identical output.
///
/// Returns `Err(msg)` if the underlying `rdf-canon` algorithm fails (in
/// practice this only happens on degenerate inputs with very deep blank-node
/// cycles).
pub fn canonicalize_dataset(store: &Datastore) -> Result<String, String> {
    let dataset = datastore_to_oxrdf_dataset(store);
    canonicalize(&dataset).map_err(|e| e.to_string())
}

/// Canonicalize a single named graph to an N-Quads string (RDFC-1.0).
///
/// Only the triples belonging to the graph identified by `graph_id` are
/// included.  Pass [`DEFAULT_GRAPH_ELEMENT_ID`] (= 0) for the default graph.
///
/// All extracted triples are placed in the default graph position so that the
/// output is equivalent to N-Triples (no graph component), which is what the
/// records-library checksum algorithm expects.
///
/// Returns `Err(msg)` on algorithm failure (see [`canonicalize_dataset`]).
pub fn canonicalize_graph(store: &Datastore, graph_id: GraphElementId) -> Result<String, String> {
    let mut dataset = Dataset::default();
    for quad in store.named_graphs.get_graph(graph_id) {
        if let Some(oxquad) = build_oxrdf_quad(
            store,
            quad.subject,
            quad.predicate,
            quad.obj,
            GraphName::DefaultGraph,
        ) {
            dataset.insert(&oxquad);
        }
    }
    canonicalize(&dataset).map_err(|e| e.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::{IriReference, RdfResource, Triple};

    // Intern an IRI into the given datastore and return its ID.
    fn iri_in(store: &mut Datastore, s: &str) -> GraphElementId {
        store.add_node_resource(RdfResource::Iri(IriReference(s.to_owned())))
    }

    // Intern a blank node by name and return its ID.
    fn blank_in(store: &mut Datastore, name: &str) -> GraphElementId {
        store
            .resources
            .get_or_create_named_anon_resource(name.to_owned())
    }

    /// Dataset with no blank nodes → deterministic N-Quads (sorted lex).
    #[test]
    fn test_canonicalize_iri_only_dataset() {
        let mut store = Datastore::new(100);
        let s = iri_in(&mut store, "http://example.org/s");
        let p = iri_in(&mut store, "http://example.org/p");
        let o = iri_in(&mut store, "http://example.org/o");
        store.add_triple(Triple {
            subject: s,
            predicate: p,
            obj: o,
        });

        let result = canonicalize_dataset(&store).expect("canonicalize should succeed");
        // Must contain the IRI triple
        assert!(result.contains("<http://example.org/s>"));
        assert!(result.contains("<http://example.org/p>"));
        assert!(result.contains("<http://example.org/o>"));
        // No blank-node labels in the output
        assert!(!result.contains("_:"));
    }

    /// Two triples sorted: the canonical output is always lexicographically sorted.
    #[test]
    fn test_canonicalize_sorted_output() {
        let mut store = Datastore::new(100);
        let p = iri_in(&mut store, "http://example.org/p");
        // Add in reverse alphabetical order to confirm output is sorted.
        let s2 = iri_in(&mut store, "http://example.org/z");
        let o2 = iri_in(&mut store, "http://example.org/o2");
        let s1 = iri_in(&mut store, "http://example.org/a");
        let o1 = iri_in(&mut store, "http://example.org/o1");
        store.add_triple(Triple {
            subject: s2,
            predicate: p,
            obj: o2,
        });
        store.add_triple(Triple {
            subject: s1,
            predicate: p,
            obj: o1,
        });

        let result = canonicalize_dataset(&store).expect("canonicalize should succeed");
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "expected exactly 2 N-Quads lines");
        // Lexicographically, lines starting with <http://example.org/a …> comes first.
        assert!(
            lines[0] < lines[1],
            "output lines should be sorted: {lines:?}"
        );
    }

    /// Dataset with blank nodes → canonical labels _:c14n0, _:c14n1.
    #[test]
    fn test_canonicalize_blank_nodes() {
        let mut store = Datastore::new(100);
        let p = iri_in(&mut store, "http://example.org/p");
        let b1 = blank_in(&mut store, "x");
        let b2 = blank_in(&mut store, "y");
        store.add_triple(Triple {
            subject: b1,
            predicate: p,
            obj: b2,
        });

        let result = canonicalize_dataset(&store).expect("canonicalize should succeed");
        // The output must use _:c14n* labels.
        assert!(
            result.contains("_:c14n"),
            "expected canonical blank-node labels, got: {result}"
        );
        // Original names must not appear.
        assert!(!result.contains("_:x"));
        assert!(!result.contains("_:y"));
    }

    /// Isomorphic datasets (same structure, different blank-node names) must
    /// produce identical canonical strings.
    #[test]
    fn test_isomorphic_datasets_same_canonical_string() {
        let p_iri = "http://example.org/p";
        let o_iri = "http://example.org/o";

        let mut store1 = Datastore::new(100);
        let p1 = iri_in(&mut store1, p_iri);
        let o1 = iri_in(&mut store1, o_iri);
        let b1 = blank_in(&mut store1, "alpha");
        store1.add_triple(Triple {
            subject: b1,
            predicate: p1,
            obj: o1,
        });

        let mut store2 = Datastore::new(100);
        let p2 = iri_in(&mut store2, p_iri);
        let o2 = iri_in(&mut store2, o_iri);
        let b2 = blank_in(&mut store2, "beta");
        store2.add_triple(Triple {
            subject: b2,
            predicate: p2,
            obj: o2,
        });

        let c1 = canonicalize_dataset(&store1).unwrap();
        let c2 = canonicalize_dataset(&store2).unwrap();
        assert_eq!(
            c1, c2,
            "isomorphic datasets must have identical canonical forms"
        );
    }

    /// `canonicalize_graph` for the default graph produces the same output as
    /// `canonicalize_dataset` when there is only a default graph.
    #[test]
    fn test_canonicalize_graph_default() {
        let mut store = Datastore::new(100);
        let s = iri_in(&mut store, "http://example.org/s");
        let p = iri_in(&mut store, "http://example.org/p");
        let o = iri_in(&mut store, "http://example.org/o");
        store.add_triple(Triple {
            subject: s,
            predicate: p,
            obj: o,
        });

        let full = canonicalize_dataset(&store).unwrap();
        let graph = canonicalize_graph(&store, DEFAULT_GRAPH_ELEMENT_ID).unwrap();
        assert_eq!(full, graph);
    }

    /// `canonicalize_graph` isolates a named graph from the rest of the dataset.
    #[test]
    fn test_canonicalize_named_graph() {
        let mut store = Datastore::new(100);
        let g_iri = "http://example.org/graph1";
        let g = iri_in(&mut store, g_iri);
        let s = iri_in(&mut store, "http://example.org/s");
        let p = iri_in(&mut store, "http://example.org/p");
        let o = iri_in(&mut store, "http://example.org/o");
        // Triple in the named graph
        store.add_named_graph_triple(
            g,
            Triple {
                subject: s,
                predicate: p,
                obj: o,
            },
        );
        // Triple in the default graph (should NOT appear in named-graph canon)
        let s2 = iri_in(&mut store, "http://example.org/other");
        let o2 = iri_in(&mut store, "http://example.org/val");
        store.add_triple(Triple {
            subject: s2,
            predicate: p,
            obj: o2,
        });

        let canon = canonicalize_graph(&store, g).unwrap();
        // Must contain the named-graph triple
        assert!(
            canon.contains("<http://example.org/s>"),
            "expected s in canon: {canon}"
        );
        // Must NOT contain the default-graph-only triple
        assert!(
            !canon.contains("<http://example.org/other>"),
            "unexpected other in canon: {canon}"
        );
    }

    /// Empty dataset/graph → empty string.
    #[test]
    fn test_canonicalize_empty_dataset() {
        let store = Datastore::new(10);
        let result = canonicalize_dataset(&store).unwrap();
        assert!(result.is_empty(), "expected empty string, got: {result}");
    }
}

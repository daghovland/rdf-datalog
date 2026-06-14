/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::ingress::{DEFAULT_GRAPH_ELEMENT_ID, GraphElementId, Quad, Triple};
use crate::{GraphElement, GraphElementManager, IriReference, QuadTable, RdfLiteral, RdfResource};

/// Top-level RDF dataset store, mirroring DagSemTools.Rdf.Datastore.
///
/// Contains:
/// - `named_graphs`: the main quad store for all named and default-graph triples.
/// - `reified_triples`: quads for RDF reification (triple IDs as graph component).
/// - `resources`: the interning store mapping GraphElement ↔ GraphElementId.
#[derive(Clone)]
pub struct Datastore {
    pub reified_triples: QuadTable,
    pub named_graphs: QuadTable,
    pub resources: GraphElementManager,
}

impl Datastore {
    pub fn new(init_rdf_size: u32) -> Self {
        let init_triples = std::cmp::max(10, (init_rdf_size / 60) as usize) as u32;
        Datastore {
            reified_triples: QuadTable::new(init_triples),
            named_graphs: QuadTable::new(init_triples),
            resources: GraphElementManager::new(init_rdf_size),
        }
    }

    // ── Resource management ──────────────────────────────────────────────────

    pub fn add_resource(&mut self, resource: GraphElement) -> GraphElementId {
        self.resources.add_resource(resource)
    }

    pub fn add_literal_resource(&mut self, literal: RdfLiteral) -> GraphElementId {
        self.resources.add_literal_resource(literal)
    }

    pub fn add_node_resource(&mut self, node: RdfResource) -> GraphElementId {
        self.resources.add_node_resource(node)
    }

    pub fn new_anonymous_blank_node(&mut self) -> GraphElementId {
        self.resources.create_unnamed_anon_resource()
    }

    // ── Triple / quad insertion ───────────────────────────────────────────────

    /// Add a triple to the default graph.
    pub fn add_triple(&mut self, triple: Triple) {
        let quad = Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: triple.subject,
            predicate: triple.predicate,
            obj: triple.obj,
        };
        self.named_graphs.add_quad(quad);
    }

    pub fn add_quad(&mut self, quad: Quad) {
        self.named_graphs.add_quad(quad);
    }

    pub fn add_named_graph_triple(&mut self, graph: GraphElementId, triple: Triple) {
        self.named_graphs.add_quad(Quad {
            triple_id: graph,
            subject: triple.subject,
            predicate: triple.predicate,
            obj: triple.obj,
        });
    }

    pub fn add_reified_triple(&mut self, triple: Triple, id: GraphElementId) {
        self.reified_triples.add_quad(Quad {
            triple_id: id,
            subject: triple.subject,
            predicate: triple.predicate,
            obj: triple.obj,
        });
    }

    // ── Quad queries (default graph) ─────────────────────────────────────────

    pub fn get_triples_with_subject(
        &self,
        subject: GraphElementId,
    ) -> impl Iterator<Item = Triple> + '_ {
        self.named_graphs
            .get_quads_with_id_subject(DEFAULT_GRAPH_ELEMENT_ID, subject)
            .map(|q| Triple {
                subject: q.subject,
                predicate: q.predicate,
                obj: q.obj,
            })
    }

    pub fn get_triples_with_object(
        &self,
        object: GraphElementId,
    ) -> impl Iterator<Item = Triple> + '_ {
        self.named_graphs
            .get_quads_with_id_object(DEFAULT_GRAPH_ELEMENT_ID, object)
            .map(|q| Triple {
                subject: q.subject,
                predicate: q.predicate,
                obj: q.obj,
            })
    }

    pub fn get_triples_with_predicate(
        &self,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Triple> + '_ {
        self.named_graphs
            .get_quads_with_id_predicate(DEFAULT_GRAPH_ELEMENT_ID, predicate)
            .map(|q| Triple {
                subject: q.subject,
                predicate: q.predicate,
                obj: q.obj,
            })
    }

    pub fn get_triples_with_subject_predicate(
        &self,
        subject: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Triple> + '_ {
        self.named_graphs
            .get_quads_with_id_subject_predicate(DEFAULT_GRAPH_ELEMENT_ID, subject, predicate)
            .map(|q| Triple {
                subject: q.subject,
                predicate: q.predicate,
                obj: q.obj,
            })
    }

    pub fn get_triples_with_object_predicate(
        &self,
        object: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Triple> + '_ {
        self.named_graphs
            .get_quads_with_id_object_predicate(DEFAULT_GRAPH_ELEMENT_ID, object, predicate)
            .map(|q| Triple {
                subject: q.subject,
                predicate: q.predicate,
                obj: q.obj,
            })
    }

    pub fn contains_triple(&self, triple: &Triple) -> bool {
        self.named_graphs.contains(&Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: triple.subject,
            predicate: triple.predicate,
            obj: triple.obj,
        })
    }

    pub fn contains_quad(&self, quad: &Quad) -> bool {
        self.named_graphs.contains(quad)
    }

    // ── Graph management ─────────────────────────────────────────────────────

    /// Look up the `GraphElementId` for a named graph IRI, returning `None` if
    /// the IRI has never been interned (graph was never written to).
    pub fn lookup_named_graph_id(&self, iri: &str) -> Option<GraphElementId> {
        let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_owned())));
        self.resources.resource_map.get(&elem).copied()
    }

    /// Return `true` if the graph identified by `graph_id` has at least one quad.
    ///
    /// The default graph (`DEFAULT_GRAPH_ELEMENT_ID`) is always considered to
    /// exist, even when empty, matching SPARQL DROP DEFAULT semantics.
    pub fn named_graph_exists(&self, graph_id: GraphElementId) -> bool {
        if graph_id == DEFAULT_GRAPH_ELEMENT_ID {
            return true;
        }
        self.named_graphs.graph_exists(graph_id)
    }

    /// Remove all quads belonging to graph `graph_id`.
    ///
    /// Equivalent to SPARQL `DROP SILENT GRAPH <g>` / `DROP SILENT DEFAULT`.
    pub fn remove_graph(&mut self, graph_id: GraphElementId) {
        self.named_graphs.remove_graph(graph_id);
    }

    /// Remove a single quad from named_graphs; no-op if absent.
    pub fn remove_quad(&mut self, quad: crate::ingress::Quad) {
        self.named_graphs.remove_quad(quad);
    }

    // ── Reified triple queries ────────────────────────────────────────────────

    pub fn get_reified_triples_with_id(
        &self,
        id: GraphElementId,
    ) -> impl Iterator<Item = Triple> + '_ {
        self.reified_triples.get_graph(id).map(|q| Triple {
            subject: q.subject,
            predicate: q.predicate,
            obj: q.obj,
        })
    }

    pub fn get_reified_triples_with_subject(
        &self,
        subject: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.reified_triples.get_quads_with_subject(subject)
    }

    pub fn get_reified_triples_with_predicate(
        &self,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.reified_triples.get_quads_with_predicate(predicate)
    }

    pub fn quads_matching(
        &self,
        graph: Option<GraphElementId>,
        subject: Option<GraphElementId>,
        predicate: Option<GraphElementId>,
        object: Option<GraphElementId>,
    ) -> Vec<Quad> {
        match (graph, subject, predicate, object) {
            (Some(g), Some(s), Some(p), Some(o)) => {
                if self.named_graphs.contains(&Quad {
                    triple_id: g,
                    subject: s,
                    predicate: p,
                    obj: o,
                }) {
                    vec![Quad {
                        triple_id: g,
                        subject: s,
                        predicate: p,
                        obj: o,
                    }]
                } else {
                    vec![]
                }
            }
            (Some(g), Some(s), Some(p), None) => self
                .named_graphs
                .get_quads_with_id_subject_predicate(g, s, p)
                .collect(),
            (Some(g), Some(s), None, Some(o)) => self
                .named_graphs
                .get_quads_with_id_subject_object(g, s, o)
                .collect(),
            (Some(g), None, Some(p), Some(o)) => self
                .named_graphs
                .get_quads_with_id_object_predicate(g, o, p)
                .collect(),
            (Some(g), Some(s), None, None) => {
                self.named_graphs.get_quads_with_id_subject(g, s).collect()
            }
            (Some(g), None, Some(p), None) => self
                .named_graphs
                .get_quads_with_id_predicate(g, p)
                .collect(),
            (Some(g), None, None, Some(o)) => {
                self.named_graphs.get_quads_with_id_object(g, o).collect()
            }
            (Some(g), None, None, None) => self.named_graphs.get_graph(g).collect(),
            (None, Some(s), Some(p), Some(o)) => {
                // This is tricky as we don't have a cross-graph subject-predicate-object index easily accessible that returns quads
                // But we can iterate over all quads and filter, or if we assume it's small...
                self.named_graphs
                    .get_all_quads()
                    .filter(|q| q.subject == s && q.predicate == p && q.obj == o)
                    .collect()
            }
            (None, Some(s), Some(p), None) => self
                .named_graphs
                .get_quads_with_subject_predicate(s, p)
                .collect(),
            (None, Some(s), None, Some(o)) => self
                .named_graphs
                .get_quads_with_subject_object(s, o)
                .collect(),
            (None, None, Some(p), Some(o)) => self
                .named_graphs
                .get_quads_with_object_predicate(o, p)
                .collect(),
            (None, Some(s), None, None) => self.named_graphs.get_quads_with_subject(s).collect(),
            (None, None, Some(p), None) => self.named_graphs.get_quads_with_predicate(p).collect(),
            (None, None, None, Some(o)) => self.named_graphs.get_quads_with_object(o).collect(),
            (None, None, None, None) => self.named_graphs.get_all_quads().collect(),
        }
    }
}

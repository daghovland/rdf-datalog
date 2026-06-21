/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Navigation graph N — the finite directed labelled graph of classes, datatypes,
//! and properties that constrains which queries a user is allowed to build.
//!
//! Definition (paper §2.1):
//! - Each vertex is either a **class** (object vertex) or a **datatype** (data vertex).
//! - Each labelled directed edge corresponds to a property.
//! - No edge starts from a datatype vertex.
//! - Every object edge (source and target both classes) has a corresponding inverse
//!   edge in N.
//!
//! The graph can be constructed manually (`add_class`, `add_datatype`,
//! `add_object_property`, `add_data_property`) or derived automatically from an RDF
//! datastore via `NavGraph::from_datastore`, which inspects `rdfs:domain` /
//! `rdfs:range` triples.

use dag_rdf::{Datastore, GraphElement, GraphElementId, IriReference, RdfResource};
use ingress::{OWL_OBJECT_INVERSE_OF, RDFS_DOMAIN, RDFS_RANGE};
use std::collections::{HashMap, HashSet};

pub type NavNodeId = u32;
pub type NavEdgeId = u32;

/// Whether a navigation-graph node is a class (object vertex) or a datatype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavNodeKind {
    Class,
    Datatype,
}

/// A vertex in the navigation graph.
#[derive(Debug, Clone)]
pub struct NavNode {
    pub id: NavNodeId,
    /// The IRI identifying this class or datatype.
    pub iri: String,
    pub kind: NavNodeKind,
}

impl NavNode {
    pub fn is_class(&self) -> bool {
        self.kind == NavNodeKind::Class
    }
    pub fn is_datatype(&self) -> bool {
        self.kind == NavNodeKind::Datatype
    }
}

/// A directed, labelled edge in the navigation graph.
#[derive(Debug, Clone)]
pub struct NavEdge {
    pub id: NavEdgeId,
    /// The property IRI.
    pub iri: String,
    pub src: NavNodeId,
    pub tgt: NavNodeId,
    /// For object edges: the id of the inverse edge in N.
    /// `None` for data edges (target is a datatype).
    pub inverse: Option<NavEdgeId>,
}

impl NavEdge {
    /// True when the target is a class (object property).
    pub fn is_object_edge(&self) -> bool {
        self.inverse.is_some()
    }
    /// True when the target is a datatype (data property).
    pub fn is_data_edge(&self) -> bool {
        self.inverse.is_none()
    }
}

/// The navigation graph N.
///
/// Constructed manually via `add_*` methods or automatically via
/// `NavGraph::from_datastore`.
#[derive(Debug, Default)]
pub struct NavGraph {
    nodes: Vec<NavNode>,
    edges: Vec<NavEdge>,
    node_by_iri: HashMap<String, NavNodeId>,
    /// outgoing edge ids per source node
    out_edges: HashMap<NavNodeId, Vec<NavEdgeId>>,
    /// incoming edge ids per target node
    in_edges: HashMap<NavNodeId, Vec<NavEdgeId>>,
}

impl NavGraph {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Node accessors ────────────────────────────────────────────────────────

    pub fn node(&self, id: NavNodeId) -> &NavNode {
        &self.nodes[id as usize]
    }

    pub fn node_by_iri(&self, iri: &str) -> Option<NavNodeId> {
        self.node_by_iri.get(iri).copied()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn class_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_class()).count()
    }

    pub fn datatype_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_datatype()).count()
    }

    pub fn classes(&self) -> impl Iterator<Item = &NavNode> {
        self.nodes.iter().filter(|n| n.is_class())
    }

    pub fn datatypes(&self) -> impl Iterator<Item = &NavNode> {
        self.nodes.iter().filter(|n| n.is_datatype())
    }

    // ── Edge accessors ────────────────────────────────────────────────────────

    pub fn edge(&self, id: NavEdgeId) -> &NavEdge {
        &self.edges[id as usize]
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn edges(&self) -> impl Iterator<Item = &NavEdge> {
        self.edges.iter()
    }

    pub fn object_edges(&self) -> impl Iterator<Item = &NavEdge> {
        self.edges.iter().filter(|e| e.is_object_edge())
    }

    pub fn data_edges(&self) -> impl Iterator<Item = &NavEdge> {
        self.edges.iter().filter(|e| e.is_data_edge())
    }

    /// All edges leaving `node` (both object and data edges).
    pub fn outgoing_edges(&self, node: NavNodeId) -> &[NavEdgeId] {
        self.out_edges.get(&node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// All edges arriving at `node`.
    pub fn incoming_edges(&self, node: NavNodeId) -> &[NavEdgeId] {
        self.in_edges.get(&node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The inverse edge of an object edge, or `None` for data edges.
    pub fn inverse_edge(&self, edge: NavEdgeId) -> Option<NavEdgeId> {
        self.edges[edge as usize].inverse
    }

    // ── Builders ─────────────────────────────────────────────────────────────

    /// Add a class vertex; returns its id.  A second call with the same IRI
    /// returns the existing id.
    pub fn add_class(&mut self, iri: &str) -> NavNodeId {
        self.add_node(iri, NavNodeKind::Class)
    }

    /// Add a datatype vertex; returns its id.
    pub fn add_datatype(&mut self, iri: &str) -> NavNodeId {
        self.add_node(iri, NavNodeKind::Datatype)
    }

    fn add_node(&mut self, iri: &str, kind: NavNodeKind) -> NavNodeId {
        if let Some(&id) = self.node_by_iri.get(iri) {
            return id;
        }
        let id = self.nodes.len() as NavNodeId;
        self.nodes.push(NavNode {
            id,
            iri: iri.to_owned(),
            kind,
        });
        self.node_by_iri.insert(iri.to_owned(), id);
        id
    }

    /// Add an object property edge `src -[iri]-> tgt` and its mandatory inverse
    /// `tgt -[inverse_iri]-> src`.  Returns `(forward_id, inverse_id)`.
    ///
    /// When a property is its own inverse (e.g. `knows`, `borders` in the paper's
    /// Figure 1), pass the same IRI for both `iri` and `inverse_iri`.  Two distinct
    /// edge ids are still allocated so the graph remains consistently directed.
    pub fn add_object_property(
        &mut self,
        iri: &str,
        src: NavNodeId,
        tgt: NavNodeId,
        inverse_iri: &str,
    ) -> (NavEdgeId, NavEdgeId) {
        let fwd_id = self.edges.len() as NavEdgeId;
        let inv_id = fwd_id + 1;

        self.edges.push(NavEdge {
            id: fwd_id,
            iri: iri.to_owned(),
            src,
            tgt,
            inverse: Some(inv_id),
        });
        self.edges.push(NavEdge {
            id: inv_id,
            iri: inverse_iri.to_owned(),
            src: tgt,
            tgt: src,
            inverse: Some(fwd_id),
        });

        self.out_edges.entry(src).or_default().push(fwd_id);
        self.in_edges.entry(tgt).or_default().push(fwd_id);
        self.out_edges.entry(tgt).or_default().push(inv_id);
        self.in_edges.entry(src).or_default().push(inv_id);

        (fwd_id, inv_id)
    }

    /// Add a data property edge `src -[iri]-> tgt` where `tgt` is a datatype.
    /// Data edges have no inverse.  Returns the edge id.
    pub fn add_data_property(
        &mut self,
        iri: &str,
        src: NavNodeId,
        tgt: NavNodeId,
    ) -> NavEdgeId {
        let id = self.edges.len() as NavEdgeId;
        self.edges.push(NavEdge {
            id,
            iri: iri.to_owned(),
            src,
            tgt,
            inverse: None,
        });
        self.out_edges.entry(src).or_default().push(id);
        self.in_edges.entry(tgt).or_default().push(id);
        id
    }

    // ── Automatic derivation ──────────────────────────────────────────────────

    /// Derive a navigation graph automatically from a `Datastore` by inspecting
    /// `rdfs:domain` and `rdfs:range` triples.
    ///
    /// - Properties whose range is an XSD datatype IRI become data edges.
    /// - Properties whose range is a non-XSD IRI become object edges (with an
    ///   auto-generated inverse whose IRI is `{property_iri}^inverse`).
    ///
    /// If `owl:inverseOf` triples are present, they are used to name the inverse
    /// edge instead of the auto-generated name.
    ///
    /// The result covers every property that has both a domain and a range
    /// declaration in the store.
    pub fn from_datastore(ds: &Datastore) -> Self {
        let mut g = NavGraph::new();

        let Some(domain_pid) = lookup_iri(ds, RDFS_DOMAIN) else {
            return g;
        };
        let Some(range_pid) = lookup_iri(ds, RDFS_RANGE) else {
            return g;
        };

        // property_id → domain class IRI
        let domains: HashMap<GraphElementId, String> = ds
            .get_triples_with_predicate(domain_pid)
            .filter_map(|t| Some((t.subject, iri_of(ds, t.obj)?)))
            .collect();

        // property_id → range IRI
        let ranges: HashMap<GraphElementId, String> = ds
            .get_triples_with_predicate(range_pid)
            .filter_map(|t| Some((t.subject, iri_of(ds, t.obj)?)))
            .collect();

        // property_id → inverse property IRI (from owl:inverseOf)
        let mut inverse_of: HashMap<GraphElementId, String> = HashMap::new();
        if let Some(inv_pid) = lookup_iri(ds, OWL_OBJECT_INVERSE_OF) {
            for t in ds.get_triples_with_predicate(inv_pid) {
                if let Some(inv_iri) = iri_of(ds, t.obj) {
                    inverse_of.insert(t.subject, inv_iri);
                }
            }
        }

        // Property IRIs that are explicitly declared as the inverse of another
        // property via owl:inverseOf.  They will be added automatically when their
        // "forward" partner is processed, so we must not process them independently.
        let declared_as_inverse: HashSet<String> = inverse_of.values().cloned().collect();

        let mut added: HashSet<String> = HashSet::new();

        for (prop_id, domain_iri) in &domains {
            let Some(range_iri) = ranges.get(prop_id) else {
                continue;
            };
            let Some(prop_iri) = iri_of(ds, *prop_id) else {
                continue;
            };
            if added.contains(&prop_iri) || declared_as_inverse.contains(&prop_iri) {
                continue;
            }

            let src = g.add_class(domain_iri);

            if is_xsd_datatype(range_iri) {
                let tgt = g.add_datatype(range_iri);
                g.add_data_property(&prop_iri, src, tgt);
                added.insert(prop_iri);
            } else {
                let tgt = g.add_class(range_iri);
                let inv_iri = inverse_of
                    .get(prop_id)
                    .cloned()
                    .unwrap_or_else(|| format!("{}^inverse", prop_iri));
                g.add_object_property(&prop_iri, src, tgt, &inv_iri);
                added.insert(prop_iri);
                added.insert(inv_iri);
            }
        }

        g
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn lookup_iri(ds: &Datastore, iri: &str) -> Option<GraphElementId> {
    let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_owned())));
    ds.resources.resource_map.get(&elem).copied()
}

fn iri_of(ds: &Datastore, id: GraphElementId) -> Option<String> {
    match ds.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s))) => Some(s.clone()),
        _ => None,
    }
}

fn is_xsd_datatype(iri: &str) -> bool {
    iri.starts_with("http://www.w3.org/2001/XMLSchema#")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helpers to build the paper's Figure 1 navigation graph.
    const PERSON: &str = "http://example.org/Person";
    const COUNTRY: &str = "http://example.org/Country";
    const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
    const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
    const PROP_AGE: &str = "http://example.org/age";
    const PROP_PERSON_NAME: &str = "http://example.org/personName";
    const PROP_POPULATION: &str = "http://example.org/population";
    const PROP_COUNTRY_NAME: &str = "http://example.org/countryName";
    const PROP_KNOWS: &str = "http://example.org/knows";
    const PROP_VISITED: &str = "http://example.org/visited";
    const PROP_VISITED_BY: &str = "http://example.org/visitedBy";
    const PROP_BORDERS: &str = "http://example.org/borders";

    fn paper_nav_graph() -> NavGraph {
        let mut g = NavGraph::new();

        let person = g.add_class(PERSON);
        let country = g.add_class(COUNTRY);
        let integer = g.add_datatype(XSD_INTEGER);
        let string = g.add_datatype(XSD_STRING);

        // data properties
        g.add_data_property(PROP_AGE, person, integer);
        g.add_data_property(PROP_PERSON_NAME, person, string);
        g.add_data_property(PROP_POPULATION, country, integer);
        g.add_data_property(PROP_COUNTRY_NAME, country, string);

        // object properties (each must have an inverse in N)
        g.add_object_property(PROP_KNOWS, person, person, PROP_KNOWS); // self-inverse
        g.add_object_property(PROP_VISITED, person, country, PROP_VISITED_BY);
        g.add_object_property(PROP_BORDERS, country, country, PROP_BORDERS); // self-inverse

        g
    }

    /// Adding a class and retrieving it by IRI works.
    #[test]
    fn nav_graph_add_class() {
        let mut g = NavGraph::new();
        let id = g.add_class(PERSON);
        assert_eq!(g.node_count(), 1);
        assert_eq!(g.node(id).iri, PERSON);
        assert!(g.node(id).is_class());
        assert_eq!(g.node_by_iri(PERSON), Some(id));
    }

    /// Adding a datatype produces a non-class node.
    #[test]
    fn nav_graph_add_datatype() {
        let mut g = NavGraph::new();
        let id = g.add_datatype(XSD_INTEGER);
        assert!(g.node(id).is_datatype());
        assert!(!g.node(id).is_class());
    }

    /// Adding the same IRI twice returns the same node id.
    #[test]
    fn nav_graph_idempotent_add() {
        let mut g = NavGraph::new();
        let id1 = g.add_class(PERSON);
        let id2 = g.add_class(PERSON);
        assert_eq!(id1, id2);
        assert_eq!(g.node_count(), 1);
    }

    /// `add_object_property` creates two directed edges, each the inverse of the other.
    #[test]
    fn nav_graph_object_property_has_inverse() {
        let mut g = NavGraph::new();
        let person = g.add_class(PERSON);
        let country = g.add_class(COUNTRY);

        let (fwd, inv) = g.add_object_property(PROP_VISITED, person, country, PROP_VISITED_BY);

        assert_eq!(g.edge_count(), 2);
        assert_eq!(g.inverse_edge(fwd), Some(inv));
        assert_eq!(g.inverse_edge(inv), Some(fwd));

        let fwd_edge = g.edge(fwd);
        assert_eq!(fwd_edge.src, person);
        assert_eq!(fwd_edge.tgt, country);
        assert_eq!(fwd_edge.iri, PROP_VISITED);
        assert!(fwd_edge.is_object_edge());

        let inv_edge = g.edge(inv);
        assert_eq!(inv_edge.src, country);
        assert_eq!(inv_edge.tgt, person);
        assert_eq!(inv_edge.iri, PROP_VISITED_BY);
    }

    /// A self-inverse object property (knows, borders) works correctly.
    #[test]
    fn nav_graph_self_inverse_object_property() {
        let mut g = NavGraph::new();
        let person = g.add_class(PERSON);
        let (fwd, inv) = g.add_object_property(PROP_KNOWS, person, person, PROP_KNOWS);

        assert_eq!(g.edge(fwd).iri, PROP_KNOWS);
        assert_eq!(g.edge(inv).iri, PROP_KNOWS);
        assert_eq!(g.inverse_edge(fwd), Some(inv));
        assert_eq!(g.inverse_edge(inv), Some(fwd));
    }

    /// `add_data_property` creates one edge with no inverse.
    #[test]
    fn nav_graph_data_property_has_no_inverse() {
        let mut g = NavGraph::new();
        let person = g.add_class(PERSON);
        let integer = g.add_datatype(XSD_INTEGER);

        let age = g.add_data_property(PROP_AGE, person, integer);

        assert_eq!(g.edge_count(), 1);
        assert!(g.inverse_edge(age).is_none());
        assert!(g.edge(age).is_data_edge());
    }

    /// Outgoing edges of Person in the paper's Figure 1 graph are correct.
    ///
    /// Person has: age, personName (data), knows (→Person), visited (→Country),
    /// and visitedBy (from Country→Person lands in Person's incoming, but
    /// as directed edges leaving Person we expect: age, personName, knows_fwd,
    /// visited_fwd, and knows_inv (since knows is Person→Person for both directions)).
    #[test]
    fn nav_graph_outgoing_edges_from_person() {
        let g = paper_nav_graph();
        let person = g.node_by_iri(PERSON).unwrap();
        let out = g.outgoing_edges(person);

        // Person has 4 outgoing edges:
        // age (data), personName (data), knows_fwd (object), visited_fwd (object)
        // knows_inv also leaves person (since knows is Person→Person)
        // So: age, personName, knows_fwd, knows_inv, visited_fwd = 5 outgoing
        assert_eq!(out.len(), 5);

        // All outgoing edge sources must be Person
        for &eid in out {
            assert_eq!(g.edge(eid).src, person);
        }
    }

    /// Country's outgoing edges include the visitedBy inverse and borders.
    #[test]
    fn nav_graph_outgoing_edges_from_country() {
        let g = paper_nav_graph();
        let country = g.node_by_iri(COUNTRY).unwrap();
        let out = g.outgoing_edges(country);

        // Country has: population (data), countryName (data),
        // visitedBy_fwd (object, Country→Person), borders_fwd + borders_inv
        // = population, countryName, visitedBy, borders_fwd, borders_inv = 5 outgoing
        assert_eq!(out.len(), 5);
        for &eid in out {
            assert_eq!(g.edge(eid).src, country);
        }
    }

    /// The full paper Figure 1 graph has the expected node and edge counts.
    ///
    /// 2 classes + 2 datatypes = 4 nodes.
    /// 4 data edges + 3 object properties × 2 (fwd+inv) = 10 edges.
    #[test]
    fn nav_graph_paper_figure1_counts() {
        let g = paper_nav_graph();
        assert_eq!(g.class_count(), 2);
        assert_eq!(g.datatype_count(), 2);
        assert_eq!(g.node_count(), 4);
        assert_eq!(g.edge_count(), 10); // 4 data + 6 object (3 props × 2 directions)
        assert_eq!(g.data_edges().count(), 4);
        assert_eq!(g.object_edges().count(), 6);
    }

    /// `from_datastore` runs without panicking on an empty datastore and returns
    /// an empty navigation graph.
    #[test]
    fn nav_graph_from_empty_datastore() {
        let ds = Datastore::new(100);
        let g = NavGraph::from_datastore(&ds);
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }

    /// `from_datastore` extracts a data property from `rdfs:domain`/`rdfs:range` triples.
    #[test]
    fn nav_graph_from_datastore_data_property() {
        use dag_rdf::Datastore;
        use turtle::parse_turtle;

        let ttl = r#"
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
            @prefix ex:   <http://example.org/> .
            @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

            ex:age rdfs:domain ex:Person ;
                   rdfs:range  xsd:integer .
        "#;

        let mut ds = Datastore::new(1000);
        parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

        let g = NavGraph::from_datastore(&ds);

        let person = g.node_by_iri("http://example.org/Person").expect("Person class");
        let integer = g
            .node_by_iri("http://www.w3.org/2001/XMLSchema#integer")
            .expect("xsd:integer datatype");

        assert!(g.node(person).is_class());
        assert!(g.node(integer).is_datatype());
        assert_eq!(g.data_edges().count(), 1);
        assert_eq!(g.object_edges().count(), 0);
    }

    /// `from_datastore` extracts an object property and creates its inverse.
    #[test]
    fn nav_graph_from_datastore_object_property() {
        use dag_rdf::Datastore;
        use turtle::parse_turtle;

        let ttl = r#"
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
            @prefix owl:  <http://www.w3.org/2002/07/owl#> .
            @prefix ex:   <http://example.org/> .

            ex:visited rdfs:domain ex:Person ;
                       rdfs:range  ex:Country ;
                       owl:inverseOf ex:visitedBy .
            ex:visitedBy rdfs:domain ex:Country ;
                         rdfs:range  ex:Person .
        "#;

        let mut ds = Datastore::new(1000);
        parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

        let g = NavGraph::from_datastore(&ds);

        assert_eq!(g.class_count(), 2);
        assert_eq!(g.object_edges().count(), 2); // visited + visitedBy
        assert_eq!(g.data_edges().count(), 0);

        let visited_id = g
            .edges()
            .find(|e| e.iri == "http://example.org/visited")
            .map(|e| e.id)
            .expect("visited edge");
        let inv = g.inverse_edge(visited_id).expect("inverse of visited");
        assert_eq!(g.edge(inv).iri, "http://example.org/visitedBy");
    }
}

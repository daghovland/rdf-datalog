/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/
pub use ::ingress::{GraphElement, RdfResource, RdfLiteral, IriReference};
use std::collections::HashMap;

pub mod ingress;
pub mod quadtable;
pub mod query;
pub mod datastore;
pub use crate::ingress::*;
pub use crate::quadtable::*;
pub use crate::query::*;
pub use crate::datastore::*;

pub struct GraphElementManager {
    pub resource_map: HashMap<GraphElement, GraphElementId>,
    pub resource_list: Vec<GraphElement>,
    pub resource_count: u32,
    pub anon_resource_count: u32,
    pub anon_resource_map: HashMap<String, GraphElementId>,
}

impl GraphElementManager {
    /// Creates a new manager with the default graph pre-populated at ID 0,
    /// matching DagSemTools behaviour where `defaultGraphElementId = 0`.
    pub fn new(init_rdf_size: u32) -> Self {
        let init_resources = std::cmp::max(10, (init_rdf_size / 10) as usize);
        let default_graph =
            GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(DEFAULT_GRAPH_IRI.to_string())));
        let mut resource_map = HashMap::with_capacity(init_resources);
        let mut resource_list = Vec::with_capacity(init_resources);
        resource_map.insert(default_graph.clone(), DEFAULT_GRAPH_ELEMENT_ID);
        resource_list.push(default_graph);
        GraphElementManager {
            resource_map,
            resource_list,
            resource_count: 1, // 0 is the default graph
            anon_resource_count: 0,
            anon_resource_map: HashMap::new(),
        }
    }

    pub fn get_graph_element(&self, resource_id: GraphElementId) -> &GraphElement {
        if resource_id >= self.resource_count {
            panic!("Resource Id out of range");
        }
        &self.resource_list[resource_id as usize]
    }

    pub fn get_resource(&self, resource_id: GraphElementId) -> Option<&RdfResource> {
        match self.get_graph_element(resource_id) {
            GraphElement::NodeOrEdge(r) => Some(r),
            GraphElement::GraphLiteral(_) => None,
        }
    }

    pub fn get_named_resource(&self, resource_id: GraphElementId) -> Option<&IriReference> {
        match self.get_resource(resource_id) {
            Some(RdfResource::Iri(i)) => Some(i),
            _ => None,
        }
    }

    pub fn reset_blank_nodes_map(&mut self) {
        self.anon_resource_map.clear();
    }

    pub fn get_iri_resource_ids(&self) -> Vec<GraphElementId> {
        self.resource_map
            .iter()
            .filter_map(|(key, &value)| {
                match key {
                    GraphElement::GraphLiteral(_) => None,
                    GraphElement::NodeOrEdge(node) => {
                        match node {
                            RdfResource::Iri(_) => Some(value),
                            _ => None,
                        }
                    }
                }
            })
            .collect()
    }

    pub fn add_resource(&mut self, resource: GraphElement) -> GraphElementId {
        if let Some(&id) = self.resource_map.get(&resource) {
            id
        } else {
            let id = self.resource_count;
            self.resource_list.push(resource.clone());
            self.resource_map.insert(resource, id);
            self.resource_count += 1;
            id
        }
    }

    pub fn add_literal_resource(&mut self, literal_resource: RdfLiteral) -> GraphElementId {
        self.add_resource(GraphElement::GraphLiteral(literal_resource))
    }

    pub fn add_node_resource(&mut self, node_resource: RdfResource) -> GraphElementId {
        self.add_resource(GraphElement::NodeOrEdge(node_resource))
    }

    pub fn create_unnamed_anon_resource(&mut self) -> GraphElementId {
        self.anon_resource_count += 1;
        let new_anon_resource = RdfResource::AnonymousBlankNode(self.anon_resource_count);
        self.add_node_resource(new_anon_resource)
    }

    pub fn get_or_create_named_anon_resource(&mut self, name: String) -> GraphElementId {
        if let Some(&id) = self.anon_resource_map.get(&name) {
            id
        } else {
            let id = self.create_unnamed_anon_resource();
            self.anon_resource_map.insert(name, id);
            id
        }
    }

    pub fn get_resource_triple(&self, triple: Triple) -> TripleResource {
        TripleResource {
            subject: self.resource_list[triple.subject as usize].clone(),
            predicate: self.resource_list[triple.predicate as usize].clone(),
            obj: self.resource_list[triple.obj as usize].clone(),
        }
    }

    pub fn get_resource_quad(&self, quad: Quad) -> QuadResource {
        QuadResource {
            triple_id: self.resource_list[quad.triple_id as usize].clone(),
            subject: self.resource_list[quad.subject as usize].clone(),
            predicate: self.resource_list[quad.predicate as usize].clone(),
            obj: self.resource_list[quad.obj as usize].clone(),
        }
    }
}

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_graph_element_manager_initialization() {
            // ID 0 is reserved for the default graph, so resource_count starts at 1.
            let manager = GraphElementManager::new(100);
            assert_eq!(manager.resource_count, 1);
            assert_eq!(manager.resource_list.len(), 1);
        }

        #[test]
        fn test_add_and_get_resource() {
            let mut manager = GraphElementManager::new(10);
            let iri = IriReference("http://example.org/test".to_string());
            let res = RdfResource::Iri(iri.clone());

            // First user resource is ID 1 (ID 0 = default graph).
            let id = manager.add_node_resource(res);
            assert_eq!(id, 1);

            let retrieved = manager.get_named_resource(id).unwrap();
            assert_eq!(retrieved.0, "http://example.org/test");
        }

        #[test]
        fn test_duplicate_resource_returns_same_id() {
            let mut manager = GraphElementManager::new(10);
            let res1 = RdfResource::Iri(IriReference("same".to_string()));
            let res2 = RdfResource::Iri(IriReference("same".to_string()));

            let id1 = manager.add_node_resource(res1);
            let id2 = manager.add_node_resource(res2);

            assert_eq!(id1, id2);
            assert_eq!(manager.resource_count, 2); // default graph + "same"
        }

        #[test]
        #[should_panic(expected = "Resource Id out of range")]
        fn test_get_out_of_bounds_panics() {
            let manager = GraphElementManager::new(10);
            manager.get_graph_element(99);
        }
    }

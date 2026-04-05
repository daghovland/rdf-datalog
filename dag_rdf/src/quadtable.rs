/*
Copyright (C) 2024 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use std::collections::HashMap;
use crate::ingress::{Quad, Triple, GraphElementId, QuadListIndex, TripleListIndex};

pub struct QuadTable {
    pub quad_list: Vec<Quad>,
    pub quad_count: TripleListIndex,
    pub four_keys_index: HashMap<Quad, QuadListIndex>,
    pub triple_index: HashMap<Triple, QuadListIndex>,
    pub triple_id_index: HashMap<GraphElementId, Vec<QuadListIndex>>,
    pub predicate_index: HashMap<GraphElementId, Vec<QuadListIndex>>,
    pub subject_predicate_index: HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
    pub object_predicate_index: HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
}

impl QuadTable {
    pub fn new(init_rdf_size: u32) -> Self {
        let init_triples = std::cmp::max(10, (init_rdf_size / 60) as usize);
        QuadTable {
            quad_list: Vec::with_capacity(init_triples),
            quad_count: 0,
            four_keys_index: HashMap::with_capacity(init_triples),
            triple_index: HashMap::new(),
            triple_id_index: HashMap::new(),
            predicate_index: HashMap::new(),
            subject_predicate_index: HashMap::new(),
            object_predicate_index: HashMap::new(),
        }
    }

    pub fn get_quad_list_entry(&self, index: QuadListIndex) -> Quad {
        self.quad_list[index]
    }

    pub fn add_triple_id_index(&mut self, id: GraphElementId, triple_index: QuadListIndex) {
        self.triple_id_index
            .entry(id)
            .or_insert_with(Vec::new)
            .push(triple_index);
    }

    pub fn add_predicate_index(&mut self, predicate: GraphElementId, triple_index: QuadListIndex) {
        self.predicate_index
            .entry(predicate)
            .or_insert_with(Vec::new)
            .push(triple_index);
    }

    pub fn add_subject_predicate_index(
        &mut self,
        subject: GraphElementId,
        predicate: GraphElementId,
        triple_index: QuadListIndex,
    ) {
        self.subject_predicate_index
            .entry(subject)
            .or_insert_with(HashMap::new)
            .entry(predicate)
            .or_insert_with(Vec::new)
            .push(triple_index);
    }

    pub fn add_object_predicate_index(
        &mut self,
        object: GraphElementId,
        predicate: GraphElementId,
        triple_index: QuadListIndex,
    ) {
        self.object_predicate_index
            .entry(object)
            .or_insert_with(HashMap::new)
            .entry(predicate)
            .or_insert_with(Vec::new)
            .push(triple_index);
    }

    pub fn add_quad(&mut self, quad: Quad) {
        if !self.four_keys_index.contains_key(&quad) {
            let current_index = self.quad_count;
            self.add_subject_predicate_index(quad.subject, quad.predicate, current_index);
            self.add_object_predicate_index(quad.obj, quad.predicate, current_index);
            self.add_predicate_index(quad.predicate, current_index);
            self.add_triple_id_index(quad.triple_id, current_index);
            
            self.quad_list.push(quad);
            self.four_keys_index.insert(quad, current_index);
            self.quad_count += 1;
        }
    }

    pub fn contains(&self, q: &Quad) -> bool {
        self.four_keys_index.contains_key(q)
    }

    pub fn get_quads_with_subject(&self, subject: GraphElementId) -> impl Iterator<Item = Quad> + '_ {
        self.subject_predicate_index
            .get(&subject)
            .into_iter()
            .flat_map(|m| m.values())
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    pub fn get_quads_with_object(&self, object: GraphElementId) -> impl Iterator<Item = Quad> + '_ {
        self.object_predicate_index
            .get(&object)
            .into_iter()
            .flat_map(|m| m.values())
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    pub fn get_quads_with_predicate(&self, predicate: GraphElementId) -> impl Iterator<Item = Quad> + '_ {
        self.predicate_index
            .get(&predicate)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    pub fn get_graph(&self, id: GraphElementId) -> impl Iterator<Item = Quad> + '_ {
        self.triple_id_index
            .get(&id)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    pub fn get_quads_with_subject_predicate(
        &self,
        subject: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.subject_predicate_index
            .get(&subject)
            .and_then(|m| m.get(&predicate))
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    pub fn get_quads_with_object_predicate(
        &self,
        object: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.object_predicate_index
            .get(&object)
            .and_then(|m| m.get(&predicate))
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    pub fn get_quads_with_subject_object(
        &self,
        subject: GraphElementId,
        object: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject(subject).filter(move |q| q.obj == object)
    }

    pub fn get_quads_with_id_subject(
        &self,
        id: GraphElementId,
        subject: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject(subject).filter(move |q| q.triple_id == id)
    }

    pub fn get_quads_with_id_predicate(
        &self,
        id: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_predicate(predicate).filter(move |q| q.triple_id == id)
    }

    pub fn get_quads_with_id_object(
        &self,
        id: GraphElementId,
        object: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_object(object).filter(move |q| q.triple_id == id)
    }

    pub fn get_quads_with_id_subject_predicate(
        &self,
        id: GraphElementId,
        subject: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject_predicate(subject, predicate)
            .filter(move |q| q.triple_id == id)
    }

    pub fn get_quads_with_id_subject_object(
        &self,
        id: GraphElementId,
        subject: GraphElementId,
        object: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject_object(subject, object)
            .filter(move |q| q.triple_id == id)
    }

    pub fn get_quads_with_id_object_predicate(
        &self,
        id: GraphElementId,
        object: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_object_predicate(object, predicate)
            .filter(move |q| q.triple_id == id)
    }

    /// Iterate over all quads in insertion order.
    pub fn get_all_quads(&self) -> impl Iterator<Item = Quad> + '_ {
        self.quad_list.iter().copied()
    }
}

/*
Copyright (C) 2024 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::ingress::{GraphElementId, Quad, QuadListIndex, TripleListIndex};
use std::collections::{HashMap, HashSet};

/// Multi-index store for quads, with lookups by predicate, subject+predicate,
/// object+predicate, graph/triple ID, and full-quad dedup.
#[derive(Clone)]
pub struct QuadTable {
    /// All quads in insertion order.
    pub quad_list: Vec<Quad>,
    /// Number of quads stored (equals `quad_list.len()`).
    pub quad_count: TripleListIndex,
    /// Full-quad membership set, used for dedup and `contains`.
    pub four_keys_index: HashSet<Quad>,
    /// Index from graph/reification ID to quad-list indexes.
    pub triple_id_index: HashMap<GraphElementId, Vec<QuadListIndex>>,
    /// Index from predicate ID to quad-list indexes.
    pub predicate_index: HashMap<GraphElementId, Vec<QuadListIndex>>,
    /// Index from subject ID to predicate ID to quad-list indexes.
    pub subject_predicate_index:
        HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
    /// Index from object ID to predicate ID to quad-list indexes.
    pub object_predicate_index:
        HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
    /// Intensional (IDB) quads produced by the reasoner. Quads not in this set are extensional (EDB) facts.
    pub intensional_quads: HashSet<Quad>,
}

impl QuadTable {
    /// Creates an empty `QuadTable`, sizing internal capacity from `init_rdf_size`.
    pub fn new(init_rdf_size: u32) -> Self {
        let init_triples = std::cmp::max(10, (init_rdf_size / 60) as usize);
        QuadTable {
            quad_list: Vec::with_capacity(init_triples),
            quad_count: 0,
            four_keys_index: HashSet::with_capacity(init_triples),
            triple_id_index: HashMap::new(),
            predicate_index: HashMap::new(),
            subject_predicate_index: HashMap::new(),
            object_predicate_index: HashMap::new(),
            intensional_quads: HashSet::with_capacity(init_triples),
        }
    }

    /// Returns the quad stored at `index` in `quad_list`.
    pub fn get_quad_list_entry(&self, index: QuadListIndex) -> Quad {
        self.quad_list[index]
    }

    /// Records `triple_index` under graph/reification ID `id` in `triple_id_index`.
    pub fn add_triple_id_index(&mut self, id: GraphElementId, triple_index: QuadListIndex) {
        self.triple_id_index
            .entry(id)
            .or_default()
            .push(triple_index);
    }

    /// Records `triple_index` under `predicate` in `predicate_index`.
    pub fn add_predicate_index(&mut self, predicate: GraphElementId, triple_index: QuadListIndex) {
        self.predicate_index
            .entry(predicate)
            .or_default()
            .push(triple_index);
    }

    /// Records `triple_index` under `subject`/`predicate` in `subject_predicate_index`.
    pub fn add_subject_predicate_index(
        &mut self,
        subject: GraphElementId,
        predicate: GraphElementId,
        triple_index: QuadListIndex,
    ) {
        self.subject_predicate_index
            .entry(subject)
            .or_default()
            .entry(predicate)
            .or_default()
            .push(triple_index);
    }

    /// Records `triple_index` under `object`/`predicate` in `object_predicate_index`.
    pub fn add_object_predicate_index(
        &mut self,
        object: GraphElementId,
        predicate: GraphElementId,
        triple_index: QuadListIndex,
    ) {
        self.object_predicate_index
            .entry(object)
            .or_default()
            .entry(predicate)
            .or_default()
            .push(triple_index);
    }

    /// Inserts `quad` and updates all indexes; no-op if already present.
    pub fn add_quad(&mut self, quad: Quad) {
        if self.four_keys_index.insert(quad) {
            let current_index = self.quad_count;
            self.add_subject_predicate_index(quad.subject, quad.predicate, current_index);
            self.add_object_predicate_index(quad.obj, quad.predicate, current_index);
            self.add_predicate_index(quad.predicate, current_index);
            self.add_triple_id_index(quad.triple_id, current_index);
            self.quad_list.push(quad);
            self.quad_count += 1;
        }
    }

    /// Returns `true` if `q` is present.
    pub fn contains(&self, q: &Quad) -> bool {
        self.four_keys_index.contains(q)
    }

    /// Remove a single quad.  No-op if the quad is not present.
    ///
    /// Rebuilds all indexes; O(n) in the number of quads.
    pub fn remove_quad(&mut self, target: Quad) {
        if !self.four_keys_index.contains(&target) {
            return;
        }
        let kept: Vec<Quad> = self
            .quad_list
            .iter()
            .copied()
            .filter(|q| *q != target)
            .collect();
        // Preserve which of the kept quads were intensional (IDB) before we reset.
        let kept_intensional: HashSet<Quad> = kept
            .iter()
            .copied()
            .filter(|q| self.intensional_quads.contains(q))
            .collect();
        let hint = kept.len() as u32;
        *self = QuadTable::new(hint);
        for q in kept {
            self.add_quad(q);
        }
        self.intensional_quads = kept_intensional;
    }

    /// Truncate the table back to its first `len` quads (in insertion order),
    /// discarding everything appended after that point and rebuilding all
    /// indexes from the surviving prefix.
    ///
    /// Intended for cheap undo-log rollback of a failed insertion/re-derivation
    /// call: since [`Self::add_quad`]/[`Self::add_intensional_quad`] only ever
    /// *append* to `quad_list` (never insert in the middle), "everything added
    /// during this call" is exactly `quad_list[len..]` when `len` was the count
    /// captured before the call started. No-op if `len >= self.quad_list.len()`.
    ///
    /// This is O(len) index-rebuild work (the same cost `remove_quad` already
    /// pays for a single quad) but performs no rule evaluation — unlike a full
    /// re-materialisation, nothing is re-derived. See
    /// [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    pub fn truncate_to(&mut self, len: usize) {
        if len >= self.quad_list.len() {
            return;
        }
        let kept: Vec<Quad> = self.quad_list[..len].to_vec();
        // Preserve which of the kept quads were intensional (IDB) before we reset.
        let kept_intensional: HashSet<Quad> = kept
            .iter()
            .copied()
            .filter(|q| self.intensional_quads.contains(q))
            .collect();
        let hint = kept.len() as u32;
        *self = QuadTable::new(hint);
        for q in kept {
            self.add_quad(q);
        }
        self.intensional_quads = kept_intensional;
    }

    /// Iterate over quads with the given subject.
    pub fn get_quads_with_subject(
        &self,
        subject: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.subject_predicate_index
            .get(&subject)
            .into_iter()
            .flat_map(|m| m.values())
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    /// Iterate over quads with the given object.
    pub fn get_quads_with_object(&self, object: GraphElementId) -> impl Iterator<Item = Quad> + '_ {
        self.object_predicate_index
            .get(&object)
            .into_iter()
            .flat_map(|m| m.values())
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    /// Iterate over quads with the given predicate.
    pub fn get_quads_with_predicate(
        &self,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.predicate_index
            .get(&predicate)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    /// Iterate over quads belonging to graph/reification ID `id`.
    pub fn get_graph(&self, id: GraphElementId) -> impl Iterator<Item = Quad> + '_ {
        self.triple_id_index
            .get(&id)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

    /// Iterate over quads with the given subject and predicate.
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

    /// Iterate over quads with the given object and predicate.
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

    /// Iterate over quads with the given subject and object.
    pub fn get_quads_with_subject_object(
        &self,
        subject: GraphElementId,
        object: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject(subject)
            .filter(move |q| q.obj == object)
    }

    /// Iterate over quads with the given graph ID and subject.
    pub fn get_quads_with_id_subject(
        &self,
        id: GraphElementId,
        subject: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject(subject)
            .filter(move |q| q.triple_id == id)
    }

    /// Iterate over quads with the given graph ID and predicate.
    pub fn get_quads_with_id_predicate(
        &self,
        id: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_predicate(predicate)
            .filter(move |q| q.triple_id == id)
    }

    /// Iterate over quads with the given graph ID and object.
    pub fn get_quads_with_id_object(
        &self,
        id: GraphElementId,
        object: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_object(object)
            .filter(move |q| q.triple_id == id)
    }

    /// Iterate over quads with the given graph ID, subject, and predicate.
    pub fn get_quads_with_id_subject_predicate(
        &self,
        id: GraphElementId,
        subject: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject_predicate(subject, predicate)
            .filter(move |q| q.triple_id == id)
    }

    /// Iterate over quads with the given graph ID, subject, and object.
    pub fn get_quads_with_id_subject_object(
        &self,
        id: GraphElementId,
        subject: GraphElementId,
        object: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject_object(subject, object)
            .filter(move |q| q.triple_id == id)
    }

    /// Iterate over quads with the given graph ID, object, and predicate.
    pub fn get_quads_with_id_object_predicate(
        &self,
        id: GraphElementId,
        object: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_object_predicate(object, predicate)
            .filter(move |q| q.triple_id == id)
    }

    /// Return `true` if any quad in this table has `triple_id == graph_id`.
    pub fn graph_exists(&self, graph_id: GraphElementId) -> bool {
        self.triple_id_index.contains_key(&graph_id)
    }

    /// Remove all quads with `triple_id == graph_id` and rebuild indexes.
    ///
    /// Equivalent to SPARQL `DROP SILENT GRAPH <graph_id>`.
    /// O(n) over all quads; acceptable for infrequent PUT / DELETE operations.
    pub fn remove_graph(&mut self, graph_id: GraphElementId) {
        if !self.triple_id_index.contains_key(&graph_id) {
            return;
        }
        let kept: Vec<Quad> = self
            .quad_list
            .iter()
            .copied()
            .filter(|q| q.triple_id != graph_id)
            .collect();
        // Preserve intensional (IDB) flags for kept quads.
        let kept_intensional: HashSet<Quad> = kept
            .iter()
            .copied()
            .filter(|q| self.intensional_quads.contains(q))
            .collect();
        let hint = kept.len() as u32;
        *self = QuadTable::new(hint);
        for quad in kept {
            self.add_quad(quad);
        }
        self.intensional_quads = kept_intensional;
    }

    /// Iterate over all quads in insertion order.
    pub fn get_all_quads(&self) -> impl Iterator<Item = Quad> + '_ {
        self.quad_list.iter().copied()
    }

    /// Mark this quad as intensional (IDB, reasoner-produced). Must be called after `add_quad`.
    ///
    /// A quad that is already present and extensional (EDB) is left alone:
    /// once a fact is asserted, a rule later re-deriving the same fact must
    /// not downgrade its EDB status — doing so would make it (wrongly)
    /// disappear from [`Self::extensional_quads`], which
    /// `IncrementalReasoner::full_rematerialise`/`full_rematerialise_rules`/
    /// `rebuild_from_base` treat as the authoritative base-fact set to
    /// rebuild the closure from. See
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162), which
    /// found this via a two-rule positive cycle (`A⊑B` + `B⊑A`) where each
    /// rule re-derives the other's base fact.
    pub fn mark_intensional(&mut self, quad: Quad) {
        if !self.is_extensional(&quad) {
            self.intensional_quads.insert(quad);
        }
    }

    /// Add a quad and immediately mark it as intensional (IDB), unless it is
    /// already present as an extensional (EDB) fact — see
    /// [`Self::mark_intensional`]'s doc comment for why that case must be a
    /// no-op rather than downgrading the quad's EDB status. Used by the
    /// reasoner.
    pub fn add_intensional_quad(&mut self, quad: Quad) {
        let already_extensional = self.is_extensional(&quad);
        self.add_quad(quad);
        if !already_extensional {
            self.intensional_quads.insert(quad);
        }
    }

    /// True iff the quad is present and is extensional (EDB, not derived by any rule).
    pub fn is_extensional(&self, q: &Quad) -> bool {
        self.contains(q) && !self.intensional_quads.contains(q)
    }

    /// Iterate over all extensional (EDB) quads.
    pub fn extensional_quads(&self) -> impl Iterator<Item = Quad> + '_ {
        self.quad_list
            .iter()
            .copied()
            .filter(|q| !self.intensional_quads.contains(q))
    }

    /// Iterate over all intensional (IDB) quads.
    pub fn intensional_quads_iter(&self) -> impl Iterator<Item = Quad> + '_ {
        self.quad_list
            .iter()
            .copied()
            .filter(|q| self.intensional_quads.contains(q))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_quad(g: u32, s: u32, p: u32, o: u32) -> Quad {
        Quad {
            triple_id: g,
            subject: s,
            predicate: p,
            obj: o,
        }
    }

    #[test]
    fn test_extensional_quad_not_intensional() {
        let mut table = QuadTable::new(10);
        let q = make_quad(0, 1, 2, 3);
        table.add_quad(q);
        assert!(
            table.is_extensional(&q),
            "quad added with add_quad should be extensional (EDB)"
        );
        assert_eq!(
            table.intensional_quads_iter().count(),
            0,
            "intensional_quads_iter should be empty for extensional quad"
        );
    }

    #[test]
    fn test_intensional_quad_not_extensional() {
        let mut table = QuadTable::new(10);
        let q = make_quad(0, 1, 2, 3);
        table.add_intensional_quad(q);
        assert!(
            !table.is_extensional(&q),
            "quad added with add_intensional_quad should not be extensional"
        );
        let intensional: Vec<Quad> = table.intensional_quads_iter().collect();
        assert_eq!(
            intensional,
            vec![q],
            "intensional_quads_iter should yield the quad"
        );
    }

    #[test]
    fn test_remove_quad_clears_intensional_flag() {
        let mut table = QuadTable::new(10);
        let q = make_quad(0, 1, 2, 3);
        table.add_intensional_quad(q);
        assert!(table.contains(&q));
        table.remove_quad(q);
        assert!(!table.contains(&q), "quad should be gone after remove");
        assert_eq!(
            table.intensional_quads_iter().count(),
            0,
            "intensional set should be empty after remove"
        );
    }

    /// A quad asserted first (`add_quad`, extensional/EDB) that a rule later
    /// also happens to derive (`add_intensional_quad`) must **keep** its EDB
    /// status — the second call must be a no-op with respect to the
    /// intensional flag. Before the fix, `add_intensional_quad` (and
    /// `mark_intensional`) unconditionally inserted into `intensional_quads`,
    /// downgrading an asserted fact to "derived-only" the moment any rule
    /// happened to re-derive it (e.g. `EquivalentClasses`' two directional
    /// rules each re-deriving the other's asserted instance) — silently
    /// dropping it from `extensional_quads()`, which
    /// `IncrementalReasoner::full_rematerialise`/`full_rematerialise_rules`/
    /// `rebuild_from_base` treat as the authoritative base-fact set to
    /// rebuild the closure from. See
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162).
    #[test]
    fn test_asserted_then_derived_quad_stays_extensional() {
        let mut table = QuadTable::new(10);
        let q = make_quad(0, 1, 2, 3);
        table.add_quad(q);
        table.add_intensional_quad(q);
        assert!(
            table.is_extensional(&q),
            "a quad asserted before a rule also derives it must stay extensional (EDB)"
        );
        assert_eq!(
            table.intensional_quads_iter().count(),
            0,
            "the quad must not appear in the intensional set"
        );
        let extensional: Vec<Quad> = table.extensional_quads().collect();
        assert_eq!(
            extensional,
            vec![q],
            "the quad must still appear in extensional_quads()"
        );
    }
}

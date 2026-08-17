/*
Copyright (C) 2024 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::ingress::{GraphElementId, Quad, QuadListIndex, TripleListIndex};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct QuadTable {
    pub quad_list: Vec<Quad>,
    pub quad_count: TripleListIndex,
    /// Full-quad dedup index, doubling as a `Quad -> QuadListIndex` reverse
    /// lookup so `remove_quad` can locate a quad's own position without
    /// scanning `quad_list`.
    pub four_keys_index: HashMap<Quad, QuadListIndex>,
    pub triple_id_index: HashMap<GraphElementId, Vec<QuadListIndex>>,
    pub predicate_index: HashMap<GraphElementId, Vec<QuadListIndex>>,
    pub subject_predicate_index:
        HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
    pub object_predicate_index:
        HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
    /// Intensional (IDB) quads produced by the reasoner. Quads not in this set are extensional (EDB) facts.
    pub intensional_quads: HashSet<Quad>,
}

impl QuadTable {
    pub fn new(init_rdf_size: u32) -> Self {
        let init_triples = std::cmp::max(10, (init_rdf_size / 60) as usize);
        QuadTable {
            quad_list: Vec::with_capacity(init_triples),
            quad_count: 0,
            four_keys_index: HashMap::with_capacity(init_triples),
            triple_id_index: HashMap::new(),
            predicate_index: HashMap::new(),
            subject_predicate_index: HashMap::new(),
            object_predicate_index: HashMap::new(),
            intensional_quads: HashSet::with_capacity(init_triples),
        }
    }

    pub fn get_quad_list_entry(&self, index: QuadListIndex) -> Quad {
        self.quad_list[index]
    }

    pub fn add_triple_id_index(&mut self, id: GraphElementId, triple_index: QuadListIndex) {
        self.triple_id_index
            .entry(id)
            .or_default()
            .push(triple_index);
    }

    pub fn add_predicate_index(&mut self, predicate: GraphElementId, triple_index: QuadListIndex) {
        self.predicate_index
            .entry(predicate)
            .or_default()
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
            .or_default()
            .entry(predicate)
            .or_default()
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
            .or_default()
            .entry(predicate)
            .or_default()
            .push(triple_index);
    }

    pub fn add_quad(&mut self, quad: Quad) {
        if !self.four_keys_index.contains_key(&quad) {
            let current_index = self.quad_count;
            self.four_keys_index.insert(quad, current_index);
            self.add_subject_predicate_index(quad.subject, quad.predicate, current_index);
            self.add_object_predicate_index(quad.obj, quad.predicate, current_index);
            self.add_predicate_index(quad.predicate, current_index);
            self.add_triple_id_index(quad.triple_id, current_index);
            self.quad_list.push(quad);
            self.quad_count += 1;
        }
    }

    pub fn contains(&self, q: &Quad) -> bool {
        self.four_keys_index.contains_key(q)
    }

    /// Remove the entry equal to `old` from `map[key]`, pruning the outer
    /// key if its `Vec` becomes empty. Order among the remaining entries is
    /// not preserved (uses `swap_remove`) since none of the index consumers
    /// rely on it.
    fn remove_flat_index_entry(
        map: &mut HashMap<GraphElementId, Vec<QuadListIndex>>,
        key: GraphElementId,
        old: QuadListIndex,
    ) {
        if let Some(vec) = map.get_mut(&key) {
            if let Some(pos) = vec.iter().position(|&v| v == old) {
                vec.swap_remove(pos);
            }
            if vec.is_empty() {
                map.remove(&key);
            }
        }
    }

    /// Same as [`Self::remove_flat_index_entry`] but for the two-level
    /// subject/object-then-predicate indexes, pruning both the inner and
    /// (if now empty) the outer key.
    fn remove_nested_index_entry(
        map: &mut HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
        outer: GraphElementId,
        inner: GraphElementId,
        old: QuadListIndex,
    ) {
        if let Some(inner_map) = map.get_mut(&outer) {
            Self::remove_flat_index_entry(inner_map, inner, old);
            if inner_map.is_empty() {
                map.remove(&outer);
            }
        }
    }

    /// Rewrite the entry equal to `old` to `new` in `map[key]`. Used when a
    /// quad's `QuadListIndex` changes because `remove_quad`'s swap-removal
    /// relocated it.
    fn relocate_flat_index_entry(
        map: &mut HashMap<GraphElementId, Vec<QuadListIndex>>,
        key: GraphElementId,
        old: QuadListIndex,
        new: QuadListIndex,
    ) {
        if let Some(vec) = map.get_mut(&key)
            && let Some(pos) = vec.iter().position(|&v| v == old)
        {
            vec[pos] = new;
        }
    }

    /// Same as [`Self::relocate_flat_index_entry`] but for the two-level
    /// indexes.
    fn relocate_nested_index_entry(
        map: &mut HashMap<GraphElementId, HashMap<GraphElementId, Vec<QuadListIndex>>>,
        outer: GraphElementId,
        inner: GraphElementId,
        old: QuadListIndex,
        new: QuadListIndex,
    ) {
        if let Some(inner_map) = map.get_mut(&outer) {
            Self::relocate_flat_index_entry(inner_map, inner, old, new);
        }
    }

    /// Remove a single quad.  No-op if the quad is not present.
    ///
    /// Removes `target`'s own entries directly from every index, then fixes
    /// up (rather than rebuilds) the rest: `quad_list` is compacted via a
    /// swap-removal (the previously-last quad is moved into `target`'s old
    /// slot), so the only other index entries that change are the four
    /// belonging to that relocated quad. Cost is proportional to the size of
    /// the (small) index buckets `target` and the relocated quad belong to,
    /// not to the total number of quads in the store — see
    /// [#535](https://github.com/daghovland/rdf-datalog/issues/535).
    ///
    /// Note this means `quad_list`/`get_all_quads` no longer reflect pure
    /// insertion order once any `remove_quad` call has happened; nothing in
    /// this crate or its callers relies on that beyond the point a deletion
    /// occurs.
    pub fn remove_quad(&mut self, target: Quad) {
        let Some(target_index) = self.four_keys_index.remove(&target) else {
            return;
        };
        self.intensional_quads.remove(&target);

        Self::remove_nested_index_entry(
            &mut self.subject_predicate_index,
            target.subject,
            target.predicate,
            target_index,
        );
        Self::remove_nested_index_entry(
            &mut self.object_predicate_index,
            target.obj,
            target.predicate,
            target_index,
        );
        Self::remove_flat_index_entry(&mut self.predicate_index, target.predicate, target_index);
        Self::remove_flat_index_entry(&mut self.triple_id_index, target.triple_id, target_index);

        let last_index = self.quad_count - 1;
        if target_index != last_index {
            let moved_quad = self.quad_list[last_index];

            Self::relocate_nested_index_entry(
                &mut self.subject_predicate_index,
                moved_quad.subject,
                moved_quad.predicate,
                last_index,
                target_index,
            );
            Self::relocate_nested_index_entry(
                &mut self.object_predicate_index,
                moved_quad.obj,
                moved_quad.predicate,
                last_index,
                target_index,
            );
            Self::relocate_flat_index_entry(
                &mut self.predicate_index,
                moved_quad.predicate,
                last_index,
                target_index,
            );
            Self::relocate_flat_index_entry(
                &mut self.triple_id_index,
                moved_quad.triple_id,
                last_index,
                target_index,
            );

            self.quad_list[target_index] = moved_quad;
            self.four_keys_index.insert(moved_quad, target_index);
        }

        self.quad_list.pop();
        self.quad_count -= 1;
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

    pub fn get_quads_with_object(&self, object: GraphElementId) -> impl Iterator<Item = Quad> + '_ {
        self.object_predicate_index
            .get(&object)
            .into_iter()
            .flat_map(|m| m.values())
            .flat_map(|v| v.iter())
            .map(|&idx| self.get_quad_list_entry(idx))
    }

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
        self.get_quads_with_subject(subject)
            .filter(move |q| q.obj == object)
    }

    pub fn get_quads_with_id_subject(
        &self,
        id: GraphElementId,
        subject: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_subject(subject)
            .filter(move |q| q.triple_id == id)
    }

    pub fn get_quads_with_id_predicate(
        &self,
        id: GraphElementId,
        predicate: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_predicate(predicate)
            .filter(move |q| q.triple_id == id)
    }

    pub fn get_quads_with_id_object(
        &self,
        id: GraphElementId,
        object: GraphElementId,
    ) -> impl Iterator<Item = Quad> + '_ {
        self.get_quads_with_object(object)
            .filter(move |q| q.triple_id == id)
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

    /// Iterate over all quads. Reflects insertion order only if no
    /// `remove_quad` call has happened yet: `remove_quad` compacts
    /// `quad_list` via swap-removal, which relocates the previously-last
    /// quad into the removed slot rather than preserving relative order.
    /// See [`Self::remove_quad`].
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
    /// After removing a quad, every index must still correctly resolve the
    /// *remaining* quads — including a quad that shares an index bucket
    /// (same subject+predicate / object+predicate / predicate / graph) with
    /// the removed one, and the quad that `remove_quad`'s internal
    /// swap-removal relocates (the previously-last quad in `quad_list`).
    /// Guards against the point-removal implementation leaving stale or
    /// mis-pointed index entries. See
    /// [#535](https://github.com/daghovland/rdf-datalog/issues/535).
    #[test]
    fn test_remove_quad_keeps_indexes_consistent_for_shared_bucket_and_relocated_quad() {
        let mut table = QuadTable::new(10);
        // q1, q2 share subject+predicate (1,2) and predicate 2 and graph 0.
        let q1 = make_quad(0, 1, 2, 3);
        let q2 = make_quad(0, 1, 2, 4);
        // q3 is unrelated, added last -> it's the "moved" quad when q1
        // (not last) is removed via swap-based point removal.
        let q3 = make_quad(0, 5, 6, 7);
        table.add_quad(q1);
        table.add_quad(q2);
        table.add_quad(q3);

        table.remove_quad(q1);

        assert!(!table.contains(&q1), "q1 should be gone");
        assert!(table.contains(&q2), "q2 (bucket sibling) should remain");
        assert!(table.contains(&q3), "q3 (relocated quad) should remain");

        let subj1: Vec<Quad> = table.get_quads_with_subject(1).collect();
        assert_eq!(subj1, vec![q2], "subject index must drop q1 but keep q2");

        let pred2: Vec<Quad> = table.get_quads_with_predicate(2).collect();
        assert_eq!(pred2, vec![q2], "predicate index must drop q1 but keep q2");

        let subj5: Vec<Quad> = table.get_quads_with_subject(5).collect();
        assert_eq!(
            subj5,
            vec![q3],
            "relocated quad q3 must still be found via subject index"
        );

        let pred6: Vec<Quad> = table.get_quads_with_predicate(6).collect();
        assert_eq!(
            pred6,
            vec![q3],
            "relocated quad q3 must still be found via predicate index"
        );

        let graph0: HashSet<Quad> = table.get_graph(0).collect();
        assert_eq!(
            graph0,
            HashSet::from([q2, q3]),
            "graph index must reflect exactly the remaining quads"
        );

        assert_eq!(table.get_all_quads().count(), 2);
    }

    /// Removing a quad that was never present is a documented no-op: it
    /// must not panic, must not alter existing indexes, and repeated calls
    /// remain no-ops. See [#535](https://github.com/daghovland/rdf-datalog/issues/535).
    #[test]
    fn test_remove_quad_missing_quad_is_noop() {
        let mut table = QuadTable::new(10);
        let q1 = make_quad(0, 1, 2, 3);
        table.add_quad(q1);

        let absent = make_quad(0, 9, 9, 9);
        table.remove_quad(absent);

        assert!(table.contains(&q1), "existing quad must be unaffected");
        assert_eq!(table.get_all_quads().count(), 1);

        // Removing the same absent quad again is still a no-op.
        table.remove_quad(absent);
        assert!(table.contains(&q1));
        assert_eq!(table.get_all_quads().count(), 1);
    }

    /// Removing the single last-remaining quad must not underflow any
    /// bookkeeping (`quad_count`, `quad_list`) — the swap-based point
    /// removal has a degenerate case when the removed quad's own index
    /// equals the last index. See
    /// [#535](https://github.com/daghovland/rdf-datalog/issues/535).
    #[test]
    fn test_remove_quad_last_remaining_quad() {
        let mut table = QuadTable::new(10);
        let q1 = make_quad(0, 1, 2, 3);
        table.add_quad(q1);
        table.remove_quad(q1);
        assert!(!table.contains(&q1));
        assert_eq!(table.get_all_quads().count(), 0);
        assert_eq!(table.quad_count, 0);
        assert!(table.quad_list.is_empty());
    }

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

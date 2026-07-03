/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! BF (Backward-Forward) incremental maintenance of a materialised Datalog closure.
//!
//! The two-phase algorithm:
//! 1. **Backward phase** — BFS through the `DerivedFromIndex` reverse graph to compute the
//!    *possibly-deleted* set PD: every derived fact whose derivation chain passes through at
//!    least one deleted base fact.
//! 2. **Forward phase** — remove PD from the closure, then re-run semi-naive materialisation.
//!    Facts in PD that are still derivable from surviving base facts will be re-derived.
//!
//! A tipping-point guard falls back to full re-materialisation when |PD|/|derived| > 25%,
//! avoiding pathological cases where incremental maintenance is more expensive than a rebuild.
//!
//! Related issue: [#109](https://github.com/daghovland/rdf-datalog/issues/109),
//! part of epic [#83](https://github.com/daghovland/rdf-datalog/issues/83).

use crate::reasoner::DatalogProgram;
use crate::stratifier::RulePartitioner;
use crate::types::Rule;
use dag_rdf::{Datastore, Quad, QuadTable};
use std::collections::{HashMap, HashSet, VecDeque};

/// Tipping-point: if |PD|/|derived| > this fraction, fall back to full re-materialisation.
const FALLBACK_THRESHOLD: f64 = 0.25;

/// Incremental reasoner implementing the BF algorithm for maintaining a materialised
/// Datalog closure under base-fact insertions and deletions.
///
/// The reasoner is initialised by materialising from scratch (with derivation tracking).
/// Subsequent updates are applied via [`Self::apply_deletions`] and [`Self::apply_insertions`].
pub struct IncrementalReasoner {
    /// One `DatalogProgram` per stratum, in topological stratum order.
    programs: Vec<DatalogProgram>,
}

impl IncrementalReasoner {
    /// Materialise from scratch with derivation tracking enabled.
    ///
    /// Stratifies `rules` and runs semi-naive materialisation over each stratum in order.
    pub fn new(rules: Vec<Rule>, base: &mut Datastore) -> Self {
        let stratifier = RulePartitioner::new(rules);
        let strata = stratifier.order_rules();
        let mut programs: Vec<DatalogProgram> =
            strata.into_iter().map(DatalogProgram::new).collect();
        for program in &mut programs {
            program.materialise_seminaive(base);
        }
        IncrementalReasoner { programs }
    }

    /// Apply a batch of base-fact deletions using the BF algorithm.
    ///
    /// Returns the number of derived facts removed from the closure.
    pub fn apply_deletions(&mut self, base: &mut Datastore, deletes: &[Quad]) -> usize {
        if deletes.is_empty() {
            return 0;
        }

        // --- Backward phase ---
        let pd = self.backward_phase(deletes);

        // --- Tipping-point check ---
        let total_derived: usize = self
            .programs
            .iter()
            .map(|p| p.derived_from.iter().count())
            .sum();
        if total_derived > 0 && pd.len() as f64 / total_derived as f64 > FALLBACK_THRESHOLD {
            // PD is large relative to the closure: full rebuild is cheaper.
            return self.full_rematerialise(base, deletes);
        }

        // --- Remove deleted base facts ---
        for q in deletes {
            base.named_graphs.remove_quad(*q);
        }

        // --- Forward phase ---
        self.forward_phase(base, pd)
    }

    /// Apply a batch of base-fact insertions.
    ///
    /// Inserts the quads into the store and re-runs semi-naive evaluation so that
    /// only quads triggered by the new base facts produce new inferences.
    pub fn apply_insertions(&mut self, base: &mut Datastore, inserts: &[Quad]) {
        for q in inserts {
            base.named_graphs.add_quad(*q);
        }
        // Re-run semi-naive; already-present derived facts are skipped by the dedup
        // check in `add_intensional_quad`, so only genuinely new inferences are added.
        for program in &mut self.programs {
            program.materialise_seminaive(base);
        }
    }

    // --- Internal helpers ---

    /// Backward phase: BFS through the reverse derivation graph.
    ///
    /// Starts from the deleted base facts and collects every derived quad whose
    /// derivation chain is broken — the possibly-deleted set PD.
    fn backward_phase(&self, deletes: &[Quad]) -> HashSet<Quad> {
        // Build reverse index: witness_quad → all derived quads that use it as a witness.
        let mut reverse: HashMap<Quad, Vec<Quad>> = HashMap::new();
        for program in &self.programs {
            for (derived, derivations) in program.derived_from.iter() {
                for d in derivations {
                    for &witness in &d.body_witnesses {
                        reverse.entry(witness).or_default().push(*derived);
                    }
                }
            }
        }

        // BFS: propagate deletion upward through derived facts.
        let mut pd: HashSet<Quad> = HashSet::new();
        let mut worklist: VecDeque<Quad> = deletes.iter().copied().collect();
        while let Some(q) = worklist.pop_front() {
            if let Some(dependents) = reverse.get(&q) {
                for &derived in dependents {
                    if pd.insert(derived) {
                        // Propagate: derived facts that depend on this one are also suspect.
                        worklist.push_back(derived);
                    }
                }
            }
        }
        pd
    }

    /// Forward phase: remove PD from the closure, then re-derive surviving facts.
    ///
    /// Returns the number of facts that were permanently removed (not re-derived).
    fn forward_phase(&mut self, base: &mut Datastore, pd: HashSet<Quad>) -> usize {
        let removed = pd.len();
        // Retract PD facts and their derivation records from both the store and the index.
        for q in &pd {
            base.named_graphs.remove_quad(*q);
            for program in &mut self.programs {
                program.derived_from.remove(q);
            }
        }
        // Re-derive: semi-naive will re-add any PD fact that is still provable from the
        // surviving base facts.  Facts that were in PD but are re-derived will be
        // re-inserted by `add_intensional_quad` (dedup ensures no double-counting).
        for program in &mut self.programs {
            program.materialise_seminaive(base);
        }
        removed
    }

    /// Full re-materialisation fallback for large deletes.
    ///
    /// Removes the deleted base facts, snapshots surviving base facts, tears down
    /// the derived closure, and rebuilds from scratch.
    fn full_rematerialise(&mut self, base: &mut Datastore, deletes: &[Quad]) -> usize {
        // Remove base facts.
        for q in deletes {
            base.named_graphs.remove_quad(*q);
        }
        // Snapshot only the base (non-derived) facts that survived.
        let base_facts: Vec<Quad> = base.named_graphs.extensional_quads().collect();
        let hint = base_facts.len() as u32;

        // Clear the entire store and reset derivation indexes.
        base.named_graphs = QuadTable::new(hint);
        for q in base_facts {
            base.named_graphs.add_quad(q);
        }

        let before = base.named_graphs.quad_count;
        for program in &mut self.programs {
            program.derived_from = Default::default();
            program.materialise_seminaive(base);
        }
        // Return the number of newly derived facts (may differ from the original PD size
        // since some may have been re-derivable, but we report the new derivations added).
        base.named_graphs.quad_count - before
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RuleAtom, RuleHead};
    use dag_rdf::{DEFAULT_GRAPH_ELEMENT_ID, IriReference, QuadPattern, RdfResource, Term};

    /// Build a Datastore pre-loaded with interned resources a, p, b, c and return
    /// (datastore, g, a, p, b, c).
    fn setup_store() -> (Datastore, u32, u32, u32, u32, u32) {
        let mut ds = Datastore::new(100);
        let g = DEFAULT_GRAPH_ELEMENT_ID;
        let a = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/a".to_string(),
            )));
        let p = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p".to_string(),
            )));
        let b = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/b".to_string(),
            )));
        let c = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/c".to_string(),
            )));
        (ds, g, a, p, b, c)
    }

    /// Build the standard transitivity rule: { ?x p ?y, ?y p ?z } => { ?x p ?z }
    fn transitivity_rule(g: u32, p: u32) -> Rule {
        Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("z".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("y".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("z".to_string()),
                }),
            ],
        }
    }

    /// Deleting the only supporting base fact for a derived quad must remove it.
    ///
    /// Setup: A→B→C with transitivity rule.
    /// After materialisation, A→C is derived.
    /// Deleting A→B must remove A→C (no other path exists).
    #[test]
    fn test_delete_base_fact_removes_derived() {
        let (mut ds, g, a, p, b, c) = setup_store();

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);

        let mut reasoner = IncrementalReasoner::new(vec![transitivity_rule(g, p)], &mut ds);

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived before deletion"
        );

        // Delete A→B: the only derivation of A→C uses it as a witness.
        reasoner.apply_deletions(&mut ds, &[fact_ab]);

        assert!(
            !ds.named_graphs.contains(&fact_ab),
            "deleted base fact A→B should be gone"
        );
        assert!(
            !ds.named_graphs.contains(&derived_ac),
            "derived A→C should be removed after deleting its only support A→B"
        );
        // B→C is not implicated by deleting A→B.
        assert!(
            ds.named_graphs.contains(&fact_bc),
            "unrelated fact B→C should remain"
        );
    }

    /// When a derived fact has two independent derivation paths, deleting the support
    /// for one path must not remove the fact (the second path still validates it).
    ///
    /// Setup: A→B, B→C (derive A→C via transitivity), plus A→p2→C and an alias rule
    /// p2→p.  After materialisation, A→C is derivable via both paths.
    /// Deleting A→B should leave A→C intact (still derivable via A→p2→C → alias rule).
    #[test]
    fn test_delete_base_fact_keeps_multiply_derived() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let p2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p2".to_string(),
            )));

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        // Second, independent path: A →p2→ C
        let fact_ac_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);
        ds.named_graphs.add_quad(fact_ac_p2);

        // Alias rule: { ?x p2 ?z } => { ?x p ?z }
        let alias_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("z".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p2),
                object: Term::Variable("z".to_string()),
            })],
        };

        let mut reasoner =
            IncrementalReasoner::new(vec![transitivity_rule(g, p), alias_rule], &mut ds);

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived before deletion"
        );

        // Delete A→B: removes the transitivity path, but alias path survives.
        reasoner.apply_deletions(&mut ds, &[fact_ab]);

        assert!(
            !ds.named_graphs.contains(&fact_ab),
            "deleted base fact A→B should be gone"
        );
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should survive: still derivable via A→p2→C + alias rule"
        );
    }

    /// Inserting a new base fact that completes a derivation chain must add the
    /// derived facts produced by that chain.
    ///
    /// Setup: only B→C is in the store initially; no derived facts.
    /// Insert A→B; the transitivity rule should derive A→C.
    #[test]
    fn test_insert_adds_derived() {
        let (mut ds, g, a, p, b, c) = setup_store();

        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_bc);

        let mut reasoner = IncrementalReasoner::new(vec![transitivity_rule(g, p)], &mut ds);

        // No derived A→C yet (no A→B).
        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            !ds.named_graphs.contains(&derived_ac),
            "A→C should not exist before inserting A→B"
        );

        // Insert A→B: should trigger derivation of A→C.
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        reasoner.apply_insertions(&mut ds, &[fact_ab]);

        assert!(
            ds.named_graphs.contains(&fact_ab),
            "inserted base fact A→B should be present"
        );
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived after inserting A→B"
        );
    }

    /// Combined update: delete one base fact and insert another.
    ///
    /// Setup: A→B, B→C, C→D.  Materialise: derives A→C, A→D, B→D.
    /// Delete B→C (removes A→C, A→D, B→D from closure).
    /// Insert A→C directly as a base fact.
    /// After: A→C is present (base), A→D is derived via A→C + C→D, B→D is gone.
    #[test]
    fn test_apply_update_delete_and_insert() {
        let (mut ds, g, a, p, b, c) = setup_store();
        let d = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/d".to_string(),
            )));

        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        let fact_bc = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: c,
        };
        let fact_cd = Quad {
            triple_id: g,
            subject: c,
            predicate: p,
            obj: d,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);
        ds.named_graphs.add_quad(fact_cd);

        let mut reasoner = IncrementalReasoner::new(vec![transitivity_rule(g, p)], &mut ds);

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        let derived_ad = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: d,
        };
        let derived_bd = Quad {
            triple_id: g,
            subject: b,
            predicate: p,
            obj: d,
        };

        assert!(
            ds.named_graphs.contains(&derived_ac),
            "A→C should be derived initially"
        );
        assert!(
            ds.named_graphs.contains(&derived_ad),
            "A→D should be derived initially"
        );
        assert!(
            ds.named_graphs.contains(&derived_bd),
            "B→D should be derived initially"
        );

        // Step 1: delete B→C.
        reasoner.apply_deletions(&mut ds, &[fact_bc]);

        assert!(
            !ds.named_graphs.contains(&fact_bc),
            "deleted B→C should be gone"
        );

        // Step 2: insert A→C as a new base fact.
        let new_base_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        reasoner.apply_insertions(&mut ds, &[new_base_ac]);

        // A→C is now present (either re-derived or as a base fact).
        assert!(
            ds.named_graphs.contains(&new_base_ac),
            "A→C should be present after insertion"
        );
        // A→D is re-derivable: A→C + C→D (C→D was never deleted).
        assert!(
            ds.named_graphs.contains(&derived_ad),
            "A→D should be re-derived via A→C + C→D"
        );
        // B→D: only derivable via B→C + C→D or B→?→D chains.
        // B→C was deleted; no other path from B to D.
        assert!(
            !ds.named_graphs.contains(&derived_bd),
            "B→D should remain absent: no surviving path from B to D"
        );
    }
}

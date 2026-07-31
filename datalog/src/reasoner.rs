/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::datalog::{
    apply_substitution_quad, constant_quad_pattern, direct_wildcard_pattern, empty_substitution,
    evaluate, get_matches_for_rule, is_fact, is_safe_rule, wildcard_quad_pattern,
};
use crate::stratifier::RulePartitioner;
use crate::types::{
    Derivation, DerivedFromIndex, PartialRule, QuadWildcard, Rule, RuleAtom, RuleHead,
};
use dag_rdf::Datastore;
use std::collections::HashMap;
use std::fmt;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Error produced while materialising a Datalog program.
///
/// Currently the only variant: a genuine, correctly-derived logical
/// contradiction (a rule whose head is [`RuleHead::Contradiction`] had its
/// body satisfied). Previously this crashed the whole process via `panic!`;
/// see [#301](https://github.com/daghovland/rdf-datalog/issues/301).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningError {
    /// A `RuleHead::Contradiction` rule fired. The `String` describes the
    /// triggering rule (its `Display` output).
    Contradiction(String),
}

impl fmt::Display for ReasoningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReasoningError::Contradiction(rule) => {
                write!(f, "Contradiction during reasoning: {rule}")
            }
        }
    }
}

impl std::error::Error for ReasoningError {}

// ── DatalogProgram ────────────────────────────────────────────────────────────

pub struct DatalogProgram {
    pub rules: Vec<Rule>,
    rule_map: HashMap<QuadWildcard, Vec<PartialRule>>,
    /// Records for each derived quad how it was produced (rule + body witnesses).
    pub derived_from: DerivedFromIndex,
}

impl DatalogProgram {
    pub fn new(rules: Vec<Rule>) -> Self {
        for r in &rules {
            is_safe_rule(r); // panics on unsafe rule
        }
        // Single-pass build: one entry per body atom, using the canonical (exact)
        // wildcard pattern.  Sub-wildcard expansion happens on the FACT side in
        // get_rules_for_fact, so the rule only needs its direct pattern as a key.
        // Using all sub-wildcards here would index every rule under (*, *, *, *),
        // causing every fact to scan every rule — O(facts × rules) = catastrophic.
        let mut rule_map: HashMap<QuadWildcard, Vec<PartialRule>> = HashMap::new();
        for (rule_id, rule) in rules.iter().enumerate() {
            for atom in &rule.body {
                if let RuleAtom::PositivePattern(p) = atom {
                    let wc = direct_wildcard_pattern(p);
                    rule_map.entry(wc).or_default().push(PartialRule {
                        rule: rule.clone(),
                        match_pattern: p.clone(),
                        rule_id,
                    });
                }
            }
        }
        DatalogProgram {
            rules,
            rule_map,
            derived_from: DerivedFromIndex::new(),
        }
    }

    pub fn add_rule(&mut self, rule: Rule) {
        is_safe_rule(&rule);
        let rule_id = self.rules.len(); // will be the new index after push
        for atom in &rule.body {
            if let RuleAtom::PositivePattern(p) = atom {
                let wc = direct_wildcard_pattern(p);
                self.rule_map.entry(wc).or_default().push(PartialRule {
                    rule: rule.clone(),
                    match_pattern: p.clone(),
                    rule_id,
                });
            }
        }
        self.rules.push(rule);
    }

    fn get_rules_for_fact(&self, fact: &dag_rdf::Quad) -> Vec<crate::types::PartialRuleMatch> {
        wildcard_quad_pattern(&constant_quad_pattern(fact))
            .iter()
            .filter_map(|wc| self.rule_map.get(wc))
            .flatten()
            .flat_map(|pr| get_matches_for_rule(fact, pr))
            .collect()
    }

    fn get_facts(&self) -> Result<Vec<dag_rdf::Quad>, ReasoningError> {
        self.rules
            .iter()
            .filter(|r| is_fact(r))
            .map(|r| match &r.head {
                RuleHead::Contradiction => Err(ReasoningError::Contradiction(format!("{r}"))),
                RuleHead::NormalHead(p) => Ok(apply_substitution_quad(&empty_substitution(), p)),
            })
            .collect()
    }

    /// Return the ground facts encoded directly in rules (body-less rules).
    /// Callers that want to drive materialisation manually must seed these before
    /// calling `materialise_one_iteration`.
    ///
    /// Returns `Err(ReasoningError::Contradiction)` if a body-less
    /// `RuleHead::Contradiction` rule is present (an inconsistency asserted
    /// directly, with no body to evaluate) — see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301).
    pub fn materialise_seed_facts(&self) -> Result<Vec<dag_rdf::Quad>, ReasoningError> {
        self.get_facts()
    }

    /// Run one semi-naive iteration over `named_graphs`, starting from `delta_start`.
    ///
    /// Returns `(new_delta_start, inferred_count)` where `inferred_count` is the number
    /// of quads added this iteration.  Returns `None` when the fixpoint is reached
    /// (no new quads were produced in the previous iteration).
    ///
    /// Returns `Err(ReasoningError::Contradiction)` if a `RuleHead::Contradiction`
    /// rule's body is satisfied by the current store — a genuine, correctly-derived
    /// inconsistency. Callers must not treat a partially-applied iteration as
    /// having produced a sound closure; see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301).
    pub fn materialise_one_iteration(
        &mut self,
        datastore: &mut Datastore,
        delta_start: usize,
    ) -> Result<Option<(usize, usize)>, ReasoningError> {
        let delta_end = datastore.named_graphs.quad_count;
        if delta_start >= delta_end {
            return Ok(None); // fixpoint reached
        }

        let delta: Vec<dag_rdf::Quad> =
            datastore.named_graphs.quad_list[delta_start..delta_end].to_vec();

        for quad in &delta {
            for rule_match in self.get_rules_for_fact(quad) {
                // `get_rules_for_fact` only tells us that `quad` matches ONE
                // triggering atom of this rule's body — the other body atoms
                // (e.g. a property-edge atom guarding a conditional
                // contradiction such as `cls-maxc0`, see
                // https://github.com/daghovland/rdf-datalog/issues/298) are
                // NOT yet checked at this point. We must run the full join
                // via `evaluate()` before treating a `Contradiction` head as
                // triggered — panicking here on the bare triggering-atom
                // match would fire the contradiction unconditionally,
                // ignoring the rest of the rule body.
                let head_pattern = match &rule_match.partial_rule.rule.head {
                    RuleHead::Contradiction => {
                        if !evaluate(datastore, &rule_match).is_empty() {
                            return Err(ReasoningError::Contradiction(format!(
                                "{}",
                                rule_match.partial_rule.rule
                            )));
                        }
                        continue;
                    }
                    RuleHead::NormalHead(h) => h.clone(),
                };
                // evaluate() borrows datastore immutably and returns an owned Vec,
                // so the borrow is released before add_intensional_quad() is called.
                let rule = &rule_match.partial_rule.rule;
                let rule_id = rule_match.partial_rule.rule_id;
                let subs = evaluate(datastore, &rule_match);
                for sub in subs {
                    let derived = apply_substitution_quad(&sub, &head_pattern);
                    datastore.named_graphs.add_intensional_quad(derived);
                    // Always record this derivation path.  The BF backward phase
                    // needs all witnesses, not just the first one that created the
                    // fact.  Duplicate (rule_id, witnesses) pairs are suppressed.
                    let body_witnesses: Vec<dag_rdf::Quad> = rule
                        .body
                        .iter()
                        .filter_map(|atom| match atom {
                            RuleAtom::PositivePattern(p) => Some(apply_substitution_quad(&sub, p)),
                            _ => None,
                        })
                        .collect();
                    // record() deduplicates, so no need to check first.
                    self.derived_from.record(
                        derived,
                        Derivation {
                            rule_id,
                            body_witnesses,
                        },
                    );
                }
            }
        }

        let new_count = datastore.named_graphs.quad_count - delta_end;
        Ok(Some((delta_end, new_count)))
    }

    /// Semi-naive forward-chaining materialisation over the quad store.
    ///
    /// Each iteration evaluates rules only against the *delta* — quads newly
    /// added in the previous iteration — rather than scanning the whole store.
    /// Joins for non-triggering body atoms still use the full indexed store.
    /// This gives O(delta × rules) work per iteration instead of O(store × rules).
    ///
    /// Returns `Err(ReasoningError::Contradiction)` on a genuine, correctly-derived
    /// inconsistency, instead of panicking (see
    /// [#301](https://github.com/daghovland/rdf-datalog/issues/301)). The store may
    /// already contain some quads derived earlier in the same run when this
    /// happens; callers that need a clean rollback should restore/rebuild from the
    /// base facts (e.g. [`crate::IncrementalReasoner::rebuild_from_base`]) rather
    /// than trust the partially-materialised closure.
    pub fn materialise_seminaive(
        &mut self,
        datastore: &mut Datastore,
    ) -> Result<(), ReasoningError> {
        for quad in self.get_facts()? {
            datastore.named_graphs.add_quad(quad);
        }

        let mut delta_start: usize = 0;
        loop {
            match self.materialise_one_iteration(datastore, delta_start)? {
                None => break,
                Some((new_start, _)) => delta_start = new_start,
            }
        }
        Ok(())
    }

    /// Naive materialisation kept for regression comparison.
    #[allow(dead_code)]
    fn materialise_naive(&self, datastore: &mut Datastore) -> Result<(), ReasoningError> {
        for quad in self.get_facts()? {
            datastore.named_graphs.add_quad(quad);
        }
        let mut changed = true;
        while changed {
            changed = false;
            let quads: Vec<dag_rdf::Quad> = datastore.named_graphs.get_all_quads().collect();
            let mut new_quads: Vec<dag_rdf::Quad> = Vec::new();
            for quad in &quads {
                for rule_match in self.get_rules_for_fact(quad) {
                    // See the matching comment in `materialise_one_iteration`:
                    // the triggering-atom match alone does not prove the full
                    // rule body holds, so a `Contradiction` head must be
                    // re-checked via a full `evaluate()` join first.
                    let head_pattern = match &rule_match.partial_rule.rule.head {
                        RuleHead::Contradiction => {
                            if !evaluate(datastore, &rule_match).is_empty() {
                                return Err(ReasoningError::Contradiction(format!(
                                    "{}",
                                    rule_match.partial_rule.rule
                                )));
                            }
                            continue;
                        }
                        RuleHead::NormalHead(h) => h.clone(),
                    };
                    let subs = evaluate(datastore, &rule_match);
                    for sub in subs {
                        let new_quad = apply_substitution_quad(&sub, &head_pattern);
                        if !datastore.named_graphs.contains(&new_quad) {
                            new_quads.push(new_quad);
                        }
                    }
                }
            }
            for q in new_quads {
                if !datastore.named_graphs.contains(&q) {
                    datastore.named_graphs.add_quad(q);
                    changed = true;
                }
            }
        }
        Ok(())
    }
}

// ── Top-level evaluate ────────────────────────────────────────────────────────

/// Stratify `rules` and materialise each stratum in order over `datastore`.
///
/// Returns `Err(ReasoningError::Contradiction)` if any stratum derives a
/// genuine inconsistency (a `RuleHead::Contradiction` rule body is satisfied),
/// instead of panicking — see
/// [#301](https://github.com/daghovland/rdf-datalog/issues/301). `datastore`
/// may contain a partially-materialised closure in that case.
pub fn evaluate_rules(rules: Vec<Rule>, datastore: &mut Datastore) -> Result<(), ReasoningError> {
    let stratifier = RulePartitioner::new(rules);
    let stratification = stratifier.order_rules();
    for partition in stratification {
        let mut program = DatalogProgram::new(partition);
        program.materialise_seminaive(datastore)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RuleAtom, RuleHead};
    use dag_rdf::{
        DEFAULT_GRAPH_ELEMENT_ID, Datastore, IriReference, Quad, QuadPattern, RdfResource, Term,
    };

    /// Build a simple transitivity scenario:
    ///   Base facts: (g, a, p, b), (g, b, p, c)
    ///   Rule:       { ?x p ?y, ?y p ?z } => { ?x p ?z }
    /// After materialisation, (g, a, p, c) should be derived, and the base
    /// facts should remain base.
    #[test]
    fn test_reasoner_marks_inferred_as_derived() {
        let mut ds = Datastore::new(100);
        let g = DEFAULT_GRAPH_ELEMENT_ID;

        // Intern resources: a, p, b, c
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

        // Insert base facts directly (not via reasoner)
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

        // Transitivity rule: [?x, p, ?y], [?y, p, ?z] => [?x, p, ?z]
        let rule = Rule {
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
        };

        let mut program = DatalogProgram::new(vec![rule]);
        program.materialise_seminaive(&mut ds).unwrap();

        // The derived quad (a, p, c) should exist
        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };
        assert!(
            ds.named_graphs.contains(&derived_ac),
            "transitively inferred quad (a, p, c) should be present"
        );
        assert!(
            !ds.named_graphs.is_extensional(&derived_ac),
            "inferred quad (a, p, c) should be marked derived, not base"
        );

        // Original base facts should still be base
        assert!(
            ds.named_graphs.is_extensional(&fact_ab),
            "original fact (a, p, b) should remain base"
        );
        assert!(
            ds.named_graphs.is_extensional(&fact_bc),
            "original fact (b, p, c) should remain base"
        );
    }

    /// Helper: build a small Datastore with resources a, p, b, c and return
    /// (datastore, g, a, p, b, c) ready for rule tests.
    fn setup_abpc_store() -> (Datastore, u32, u32, u32, u32, u32) {
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

    /// Base quads added directly must have no derivation entry.
    #[test]
    fn test_base_quad_has_no_derivation() {
        let (mut ds, g, a, p, b, _c) = setup_abpc_store();
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        // No rules → program does nothing, index stays empty.
        let mut program = DatalogProgram::new(vec![]);
        program.materialise_seminaive(&mut ds).unwrap();

        assert!(
            !program.derived_from.has_derivation(&fact_ab),
            "base quad should have no derivation entry"
        );
    }

    /// Transitively derived quad must have a derivation with correct rule_id and witnesses.
    #[test]
    fn test_derived_quad_has_derivation() {
        let (mut ds, g, a, p, b, c) = setup_abpc_store();
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

        // Transitivity rule: [?x, p, ?y], [?y, p, ?z] => [?x, p, ?z]
        let rule = Rule {
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
        };

        let mut program = DatalogProgram::new(vec![rule]);
        program.materialise_seminaive(&mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };

        // The derivation index must record (a, p, c).
        assert!(
            program.derived_from.has_derivation(&derived_ac),
            "derived quad (a, p, c) should have a derivation entry"
        );

        let derivations = program.derived_from.derivations_for(&derived_ac);
        assert_eq!(derivations.len(), 1, "exactly one derivation path expected");

        let deriv = &derivations[0];
        assert_eq!(deriv.rule_id, 0, "should reference rule at index 0");
        // Body witnesses are (a, p, b) then (b, p, c)
        assert_eq!(
            deriv.body_witnesses,
            vec![fact_ab, fact_bc],
            "body witnesses should be the two base facts"
        );
    }

    /// When the same quad is derivable via two paths it should get two entries.
    #[test]
    fn test_multiple_derivation_paths() {
        // Facts: (a, p, b), (b, p, c), (a, p2, c)
        // Rule 1 (transitivity via p): [?x, p, ?y], [?y, p, ?z] => [?x, p, ?z]
        // Rule 2 (alias):              [?x, p2, ?z]              => [?x, p, ?z]
        // Both rules can derive (a, p, c); we want two derivation entries.
        let (mut ds, g, a, p, b, c) = setup_abpc_store();
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
        let fact_ac_p2 = Quad {
            triple_id: g,
            subject: a,
            predicate: p2,
            obj: c,
        };
        ds.named_graphs.add_quad(fact_ab);
        ds.named_graphs.add_quad(fact_bc);
        ds.named_graphs.add_quad(fact_ac_p2);

        // Rule 0: transitivity via p
        let rule_transit = Rule {
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
        };
        // Rule 1: alias p2 to p
        let rule_alias = Rule {
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

        let mut program = DatalogProgram::new(vec![rule_transit, rule_alias]);
        program.materialise_seminaive(&mut ds).unwrap();

        let derived_ac = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: c,
        };

        let derivations = program.derived_from.derivations_for(&derived_ac);
        assert!(
            derivations.len() >= 2,
            "expected at least 2 derivation paths for (a, p, c), got {}",
            derivations.len()
        );
    }

    /// A genuine, correctly-derived contradiction must surface as
    /// `Err(ReasoningError::Contradiction)` instead of panicking.
    ///
    /// Setup: fact (a, p, b); rule `{ ?x p ?y } => Contradiction`.
    /// The rule's body is satisfied by the fact, so materialisation must
    /// return an error rather than crash the process.
    /// See https://github.com/daghovland/rdf-datalog/issues/301.
    #[test]
    fn test_contradiction_returns_err_not_panic() {
        let (mut ds, g, a, p, b, _c) = setup_abpc_store();
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        let contradiction_rule = Rule {
            head: RuleHead::Contradiction,
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            })],
        };

        let mut program = DatalogProgram::new(vec![contradiction_rule]);
        let result = program.materialise_seminaive(&mut ds);

        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );
    }

    /// [`evaluate_rules`] (the top-level entry point used throughout the
    /// codebase) must also propagate the contradiction as `Err` rather than
    /// panicking.
    #[test]
    fn test_evaluate_rules_propagates_contradiction() {
        let (mut ds, g, a, p, b, _c) = setup_abpc_store();
        let fact_ab = Quad {
            triple_id: g,
            subject: a,
            predicate: p,
            obj: b,
        };
        ds.named_graphs.add_quad(fact_ab);

        let contradiction_rule = Rule {
            head: RuleHead::Contradiction,
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p),
                object: Term::Variable("y".to_string()),
            })],
        };

        let result = evaluate_rules(vec![contradiction_rule], &mut ds);
        assert!(
            matches!(result, Err(ReasoningError::Contradiction(_))),
            "expected a Contradiction error, got {result:?}"
        );
    }
}

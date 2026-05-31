/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::datalog::{
    apply_substitution_quad, constant_quad_pattern, empty_substitution, evaluate,
    get_matches_for_rule, is_fact, is_safe_rule, wildcard_quad_pattern,
};
use crate::stratifier::RulePartitioner;
use crate::types::{PartialRule, QuadWildcard, Rule, RuleAtom, RuleHead};
use dag_rdf::{Datastore, QuadTable};
use std::collections::HashMap;

// ── DatalogProgram ────────────────────────────────────────────────────────────

pub struct DatalogProgram {
    rules: Vec<Rule>,
    rule_map: HashMap<QuadWildcard, Vec<PartialRule>>,
}

impl DatalogProgram {
    pub fn new(rules: Vec<Rule>) -> Self {
        for r in &rules {
            is_safe_rule(r); // panics on unsafe rule
        }
        // Single-pass build: insert directly into one HashMap instead of
        // reducing via binary merges (which would be O(n²) for large rule sets).
        let mut rule_map: HashMap<QuadWildcard, Vec<PartialRule>> = HashMap::new();
        for rule in &rules {
            for atom in &rule.body {
                if let RuleAtom::PositivePattern(p) = atom {
                    for wc in wildcard_quad_pattern(p) {
                        rule_map.entry(wc).or_default().push(PartialRule {
                            rule: rule.clone(),
                            match_pattern: p.clone(),
                        });
                    }
                }
            }
        }
        DatalogProgram { rules, rule_map }
    }

    pub fn add_rule(&mut self, rule: Rule) {
        is_safe_rule(&rule);
        for atom in &rule.body {
            if let RuleAtom::PositivePattern(p) = atom {
                for wc in wildcard_quad_pattern(p) {
                    self.rule_map.entry(wc).or_default().push(PartialRule {
                        rule: rule.clone(),
                        match_pattern: p.clone(),
                    });
                }
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

    fn get_facts(&self) -> Vec<dag_rdf::Quad> {
        self.rules
            .iter()
            .filter(|r| is_fact(r))
            .map(|r| match &r.head {
                RuleHead::Contradiction => {
                    panic!("Contradiction in facts — inconsistency detected. Aborting.")
                }
                RuleHead::NormalHead(p) => apply_substitution_quad(&empty_substitution(), p),
            })
            .collect()
    }

    /// Semi-naive forward-chaining materialisation over the quad store.
    ///
    /// Each iteration evaluates rules only against the *delta* — quads newly
    /// added in the previous iteration — rather than scanning the whole store.
    /// Joins for non-triggering body atoms still use the full indexed store.
    /// This gives O(delta × rules) work per iteration instead of O(store × rules).
    pub fn materialise_seminaive(&self, named_graphs: &mut QuadTable) {
        for quad in self.get_facts() {
            named_graphs.add_quad(quad);
        }

        // delta_start tracks the index into quad_list where the current delta begins.
        // Initially the entire store is the delta (all input facts are "new").
        let mut delta_start: usize = 0;
        let mut iteration: usize = 0;

        loop {
            let delta_end = named_graphs.quad_count;
            if delta_start >= delta_end {
                break; // nothing new last round — fixpoint reached
            }

            let delta_size = delta_end - delta_start;
            log::debug!(
                "materialise: iteration {} — delta {} quads, store {} total",
                iteration,
                delta_size,
                delta_end
            );
            iteration += 1;

            // Snapshot the delta so we can mutate named_graphs during the loop.
            let delta: Vec<dag_rdf::Quad> = named_graphs.quad_list[delta_start..delta_end].to_vec();
            delta_start = delta_end;

            for quad in &delta {
                for rule_match in self.get_rules_for_fact(quad) {
                    let head_pattern = match &rule_match.partial_rule.rule.head {
                        RuleHead::Contradiction => panic!(
                            "Contradiction during reasoning: {}",
                            rule_match.partial_rule.rule
                        ),
                        RuleHead::NormalHead(h) => h.clone(),
                    };
                    for sub in evaluate(named_graphs, &rule_match) {
                        let new_quad = apply_substitution_quad(&sub, &head_pattern);
                        named_graphs.add_quad(new_quad); // dedup handled internally
                    }
                }
            }
        }
    }

    /// Naive materialisation kept for regression comparison.
    #[allow(dead_code)]
    fn materialise_naive(&self, named_graphs: &mut QuadTable) {
        for quad in self.get_facts() {
            named_graphs.add_quad(quad);
        }
        let mut changed = true;
        while changed {
            changed = false;
            let quads: Vec<dag_rdf::Quad> = named_graphs.get_all_quads().collect();
            let mut new_quads: Vec<dag_rdf::Quad> = Vec::new();
            for quad in &quads {
                for rule_match in self.get_rules_for_fact(quad) {
                    let head_pattern = match &rule_match.partial_rule.rule.head {
                        RuleHead::Contradiction => panic!(
                            "Contradiction during reasoning: {}",
                            rule_match.partial_rule.rule
                        ),
                        RuleHead::NormalHead(h) => h.clone(),
                    };
                    let subs = evaluate(named_graphs, &rule_match);
                    for sub in subs {
                        let new_quad = apply_substitution_quad(&sub, &head_pattern);
                        if !named_graphs.contains(&new_quad) {
                            new_quads.push(new_quad);
                        }
                    }
                }
            }
            for q in new_quads {
                if !named_graphs.contains(&q) {
                    named_graphs.add_quad(q);
                    changed = true;
                }
            }
        }
    }
}

// ── Top-level evaluate ────────────────────────────────────────────────────────

/// Stratify `rules` and materialise each stratum in order over `datastore`.
pub fn evaluate_rules(rules: Vec<Rule>, datastore: &mut Datastore) {
    let stratifier = RulePartitioner::new(rules);
    let stratification = stratifier.order_rules();
    for partition in stratification {
        let program = DatalogProgram::new(partition);
        program.materialise_seminaive(&mut datastore.named_graphs);
    }
}

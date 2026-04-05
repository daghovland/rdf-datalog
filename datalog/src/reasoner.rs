/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use std::collections::HashMap;
use dag_rdf::{Datastore, QuadTable};
use crate::datalog::{
    apply_substitution_quad, constant_quad_pattern, empty_substitution, evaluate,
    get_matches_for_rule, get_partial_matches, is_fact, is_safe_rule, merge_partial_match_maps,
    wildcard_quad_pattern,
};
use crate::stratifier::RulePartitioner;
use crate::types::{PartialRule, QuadWildcard, Rule, RuleHead};

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
        let rule_map = rules
            .iter()
            .map(get_partial_matches)
            .reduce(|a, b| merge_partial_match_maps(vec![a, b]))
            .unwrap_or_default();
        DatalogProgram { rules, rule_map }
    }

    pub fn add_rule(&mut self, rule: Rule) {
        is_safe_rule(&rule);
        let new_map = get_partial_matches(&rule);
        self.rule_map = merge_partial_match_maps(vec![
            std::mem::take(&mut self.rule_map),
            new_map,
        ]);
        self.rules.push(rule);
    }

    fn get_rules_for_fact(
        &self,
        fact: &dag_rdf::Quad,
    ) -> Vec<crate::types::PartialRuleMatch> {
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
            .filter_map(|r| match &r.head {
                RuleHead::Contradiction => {
                    panic!("Contradiction in facts — inconsistency detected. Aborting.")
                }
                RuleHead::NormalHead(p) => Some(apply_substitution_quad(&empty_substitution(), p)),
            })
            .collect()
    }

    /// Naive (forward-chaining) materialisation over the quad store.
    pub fn materialise_naive(&self, named_graphs: &mut QuadTable) {
        // First, add all ground facts from the rules
        for quad in self.get_facts() {
            named_graphs.add_quad(quad);
        }

        // Forward-chain until fixpoint
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
                    // evaluate already returns a Vec
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
        program.materialise_naive(&mut datastore.named_graphs);
    }
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Stratification of a Datalog program for safe negation.
//!
//! Implements topological ordering (Kahn's algorithm) of rules, detecting
//! cycles through negative edges (which would make the program non-stratifiable).
//! Based on chapters on negation in Abiteboul, Hull, Vianu: "Foundations of
//! Databases" (1995) and the rule-level variant from Motik et al.

use crate::reasoner::ReasoningError;
use crate::types::{Rule, RuleAtom, RuleHead};
use crate::unification::{PatternEdge, depending_rules};
use std::collections::{HashMap, VecDeque};

// ── OrderedRule ──────────────────────────────────────────────────────────────

#[derive(Debug)]
struct OrderedRule {
    rule: Rule,
    successors: Vec<PatternEdge>,
    num_predecessors: usize,
    uses_intensional_negative_edge: bool,
    output: bool,
}

fn create_ordered_rules(rules: &[Rule]) -> Vec<OrderedRule> {
    rules
        .iter()
        .map(|r| OrderedRule {
            rule: r.clone(),
            successors: Vec::new(),
            num_predecessors: 0,
            uses_intensional_negative_edge: false,
            output: false,
        })
        .collect()
}

// ── RulePartitioner ──────────────────────────────────────────────────────────

/// Creates a stratification of the datalog program.
pub struct RulePartitioner {
    rules: Vec<Rule>,
    rule_index: HashMap<Rule, usize>,
    ordered: Vec<OrderedRule>,
    ready_queue: VecDeque<usize>,
    next_queue: VecDeque<usize>,
    /// True when no rule has a NotPattern body atom — lets order_rules skip Kahn's algorithm.
    pure_positive: bool,
}

impl RulePartitioner {
    pub fn new(rules: Vec<Rule>) -> Self {
        let rules: Vec<Rule> = {
            let mut seen = std::collections::HashSet::new();
            rules
                .into_iter()
                .filter(|r| seen.insert(r.clone()))
                .collect()
        };

        // Fast path: if no rule has negation in its body, the program is pure-positive.
        // Stratification (Kahn's O(n²) dependency graph) is unnecessary — all rules form
        // one stratum and the semi-naive fixpoint converges regardless of rule order.
        let pure_positive = !rules
            .iter()
            .any(|r| r.body.iter().any(|a| matches!(a, RuleAtom::NotPattern(_))));

        if pure_positive {
            return RulePartitioner {
                rules,
                rule_index: HashMap::new(),
                ordered: Vec::new(),
                ready_queue: VecDeque::new(),
                next_queue: VecDeque::new(),
                pure_positive: true,
            };
        }

        let rule_index: HashMap<Rule, usize> = rules
            .iter()
            .enumerate()
            .map(|(i, r)| (r.clone(), i))
            .collect();

        let mut ordered = create_ordered_rules(&rules);

        // Build the dependency graph
        for i in 0..rules.len() {
            if let RuleHead::NormalHead(ref head_pattern) = rules[i].head {
                let deps = depending_rules(&rules, head_pattern);
                for edge in deps {
                    let dep_rule = edge.get_rule().clone();
                    if let Some(&dep_idx) = rule_index.get(&dep_rule) {
                        ordered[i].successors.push(edge);
                        ordered[dep_idx].num_predecessors += 1;
                    }
                }
            }
        }

        let ready_queue: VecDeque<usize> = (0..rules.len())
            .filter(|&i| ordered[i].num_predecessors == 0)
            .collect();

        RulePartitioner {
            rules,
            rule_index,
            ordered,
            ready_queue,
            next_queue: VecDeque::new(),
            pure_positive: false,
        }
    }

    fn update_successor(&mut self, _removed_idx: usize, edge: &PatternEdge) {
        let dep_rule = edge.get_rule().clone();
        if let Some(&dep_idx) = self.rule_index.get(&dep_rule) {
            if matches!(edge, PatternEdge::NegativePatternEdge(_)) {
                self.ordered[dep_idx].uses_intensional_negative_edge = true;
            }
            if !self.ordered[dep_idx].output {
                if self.ordered[dep_idx].num_predecessors == 0 {
                    log::error!(
                        "Stratification bug: num_predecessors underflow for rule {:?}",
                        self.ordered[dep_idx].rule
                    );
                    return;
                }
                self.ordered[dep_idx].num_predecessors -= 1;
                if self.ordered[dep_idx].num_predecessors == 0 {
                    self.ordered[dep_idx].output = true;
                    if self.ordered[dep_idx].uses_intensional_negative_edge {
                        self.next_queue.push_back(dep_idx);
                    } else {
                        self.ready_queue.push_back(dep_idx);
                    }
                }
            }
        }
    }

    fn get_partition(&mut self) -> Vec<Rule> {
        let mut partition = Vec::new();
        while let Some(idx) = self.ready_queue.pop_front() {
            let successors: Vec<PatternEdge> = self.ordered[idx].successors.clone();
            for edge in successors {
                self.update_successor(idx, &edge);
            }
            partition.push(self.ordered[idx].rule.clone());
        }
        partition
    }

    fn reset_stratification(&mut self) {
        for o in &mut self.ordered {
            o.uses_intensional_negative_edge = false;
        }
        while let Some(idx) = self.next_queue.pop_front() {
            self.ready_queue.push_back(idx);
        }
    }

    fn topological_sort_finished(&self) -> bool {
        self.ordered
            .iter()
            .all(|o| o.output || o.num_predecessors == 0)
    }

    /// Find a cycle through rules that still have predecessors and return
    /// those indices so the caller can break the cycle (for cyclic-but-positive rules).
    ///
    /// Returns `Err(ReasoningError::NotStratifiable)` if a negative dependency
    /// edge is found on a cycle — see
    /// [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    fn find_cycle(&self) -> Result<Option<Vec<usize>>, ReasoningError> {
        let candidates: Vec<usize> = (0..self.rules.len())
            .filter(|&i| !self.ordered[i].output && self.ordered[i].num_predecessors > 0)
            .collect();

        // DFS from each candidate to find a cycle
        for &start in &candidates {
            let mut visited = vec![false; self.rules.len()];
            let _stack = [start];
            let mut path = Vec::new();
            if self.dfs_cycle(start, &mut visited, &mut path)? {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    /// Returns `Err(ReasoningError::NotStratifiable)` instead of panicking
    /// when a negative dependency edge is found on a cycle — see
    /// [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    fn dfs_cycle(
        &self,
        idx: usize,
        visited: &mut Vec<bool>,
        path: &mut Vec<usize>,
    ) -> Result<bool, ReasoningError> {
        if visited[idx] {
            return Ok(path.contains(&idx));
        }
        visited[idx] = true;
        path.push(idx);
        for edge in &self.ordered[idx].successors {
            let dep = edge.get_rule();
            if let Some(&dep_idx) = self.rule_index.get(dep)
                && !self.ordered[dep_idx].output
            {
                if matches!(edge, PatternEdge::NegativePatternEdge(_)) {
                    let message = format!(
                        "Datalog program has a cycle with negation — not stratifiable! \
                         Cycle includes rule: {}",
                        self.rules[idx]
                    );
                    log::error!("{message}");
                    return Err(ReasoningError::NotStratifiable(format!(
                        "{}",
                        self.rules[idx]
                    )));
                }
                if self.dfs_cycle(dep_idx, visited, path)? {
                    return Ok(true);
                }
            }
        }
        path.pop();
        Ok(false)
    }

    fn handle_cycle(&mut self) -> Result<(), ReasoningError> {
        if let Some(cycle) = self.find_cycle()? {
            for idx in cycle {
                if !self.ordered[idx].output {
                    self.ordered[idx].output = true;
                    self.ready_queue.push_back(idx);
                }
            }
        }
        Ok(())
    }

    /// Return the stratified sequence of rule partitions. Each partition must
    /// be fully materialised before the next one can start.
    ///
    /// Returns `Err(ReasoningError::NotStratifiable)` if the program has a
    /// dependency cycle through a negative edge, instead of panicking — see
    /// [#363](https://github.com/daghovland/rdf-datalog/issues/363).
    pub fn order_rules(mut self) -> Result<Vec<Vec<Rule>>, ReasoningError> {
        // Pure-positive programs need no stratification — one stratum is always correct.
        if self.pure_positive {
            return Ok(if self.rules.is_empty() {
                vec![]
            } else {
                vec![self.rules]
            });
        }

        let mut stratification = Vec::new();

        if self.ready_queue.is_empty() {
            self.handle_cycle()?;
        }

        while !self.ready_queue.is_empty() {
            let partition = self.get_partition();
            stratification.push(partition);
            self.reset_stratification();
            if self.ready_queue.is_empty() && !self.topological_sort_finished() {
                self.handle_cycle()?;
            }
        }

        Ok(stratification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::{DEFAULT_GRAPH_ELEMENT_ID, QuadPattern, Term};

    const G: u32 = DEFAULT_GRAPH_ELEMENT_ID;
    // Distinct fixed resource ids standing in for the predicates `a`/`b`.
    const PRED_A: u32 = 100;
    const PRED_B: u32 = 101;

    fn pattern(predicate_id: u32) -> QuadPattern {
        QuadPattern {
            graph: Term::Resource(G),
            subject: Term::Variable("x".to_string()),
            predicate: Term::Resource(predicate_id),
            object: Term::Resource(G),
        }
    }

    /// `A(x) :- NOT B(x).` / `B(x) :- NOT A(x).` — a mutual-negation cycle.
    /// This must not be stratifiable: whichever rule fires first flips the
    /// other's negated premise.
    #[test]
    fn order_rules_returns_err_on_negation_cycle() {
        let rule_a = Rule {
            head: RuleHead::NormalHead(pattern(PRED_A)),
            body: vec![RuleAtom::NotPattern(pattern(PRED_B))],
        };
        let rule_b = Rule {
            head: RuleHead::NormalHead(pattern(PRED_B)),
            body: vec![RuleAtom::NotPattern(pattern(PRED_A))],
        };

        let partitioner = RulePartitioner::new(vec![rule_a, rule_b]);
        let result = partitioner.order_rules();

        assert!(
            matches!(
                result,
                Err(crate::reasoner::ReasoningError::NotStratifiable(_))
            ),
            "mutual-negation cycle must return Err(NotStratifiable), got {:?}",
            result
        );
    }

    /// `A(x) :- B(x).` / `C(x) :- NOT A(x).` — negation is used, but there is
    /// no cycle through it (A only depends positively on B; C negates A from
    /// a strictly later stratum). Must still succeed.
    #[test]
    fn order_rules_still_succeeds_on_stratifiable_program() {
        const PRED_C: u32 = 102;
        let rule_a = Rule {
            head: RuleHead::NormalHead(pattern(PRED_A)),
            body: vec![RuleAtom::PositivePattern(pattern(PRED_B))],
        };
        let rule_c = Rule {
            head: RuleHead::NormalHead(pattern(PRED_C)),
            body: vec![RuleAtom::NotPattern(pattern(PRED_A))],
        };

        let partitioner = RulePartitioner::new(vec![rule_a.clone(), rule_c.clone()]);
        let strata = partitioner
            .order_rules()
            .expect("stratifiable program should not error");

        assert_eq!(
            strata.len(),
            2,
            "expected 2 strata (A's stratum, then C's), got {}",
            strata.len()
        );
        assert!(
            strata[0].contains(&rule_a),
            "first stratum should contain rule A"
        );
        assert!(
            strata.last().unwrap().contains(&rule_c),
            "last stratum should contain rule C"
        );
    }
}

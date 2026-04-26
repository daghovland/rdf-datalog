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

use crate::types::{Rule, RuleHead};
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
    fn find_cycle(&self) -> Option<Vec<usize>> {
        let candidates: Vec<usize> = (0..self.rules.len())
            .filter(|&i| !self.ordered[i].output && self.ordered[i].num_predecessors > 0)
            .collect();

        // DFS from each candidate to find a cycle
        for &start in &candidates {
            let mut visited = vec![false; self.rules.len()];
            let _stack = [start];
            let mut path = Vec::new();
            if self.dfs_cycle(start, &mut visited, &mut path) {
                return Some(path);
            }
        }
        None
    }

    fn dfs_cycle(&self, idx: usize, visited: &mut Vec<bool>, path: &mut Vec<usize>) -> bool {
        if visited[idx] {
            return path.contains(&idx);
        }
        visited[idx] = true;
        path.push(idx);
        for edge in &self.ordered[idx].successors {
            let dep = edge.get_rule();
            if let Some(&dep_idx) = self.rule_index.get(dep)
                && !self.ordered[dep_idx].output
            {
                if matches!(edge, PatternEdge::NegativePatternEdge(_)) {
                    log::error!(
                        "Datalog program has a cycle with negation — not stratifiable! \
                         Cycle includes rule: {}",
                        self.rules[idx]
                    );
                    panic!("Datalog program has a cycle with negation and is not stratifiable!");
                }
                if self.dfs_cycle(dep_idx, visited, path) {
                    return true;
                }
            }
        }
        path.pop();
        false
    }

    fn handle_cycle(&mut self) {
        if let Some(cycle) = self.find_cycle() {
            for idx in cycle {
                if !self.ordered[idx].output {
                    self.ordered[idx].output = true;
                    self.ready_queue.push_back(idx);
                }
            }
        }
    }

    /// Return the stratified sequence of rule partitions. Each partition must
    /// be fully materialised before the next one can start.
    pub fn order_rules(mut self) -> Vec<Vec<Rule>> {
        let mut stratification = Vec::new();

        if self.ready_queue.is_empty() {
            self.handle_cycle();
        }

        while !self.ready_queue.is_empty() {
            let partition = self.get_partition();
            stratification.push(partition);
            self.reset_stratification();
            if self.ready_queue.is_empty() && !self.topological_sort_finished() {
                self.handle_cycle();
            }
        }

        stratification
    }
}

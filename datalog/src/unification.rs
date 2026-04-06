/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Unification utilities for the stratification algorithm.
//!
//! The method names follow <https://ojs.aaai.org/index.php/AAAI/article/view/9409>.

use crate::types::{Rule, RuleAtom, RuleHead};
use dag_rdf::{GraphElementId, QuadPattern, Term};
use std::collections::HashMap;

// ── Term / pattern unifiability ──────────────────────────────────────────────

fn variable_constant_unifiable(
    var: &str,
    resource: GraphElementId,
    constant_map: &mut HashMap<String, GraphElementId>,
    variable_map: &HashMap<String, String>,
) -> bool {
    let canonical = variable_map.get(var).map(|s| s.as_str()).unwrap_or(var);
    match constant_map.get(canonical) {
        None => {
            constant_map.insert(canonical.to_owned(), resource);
            true
        }
        Some(&r) => r == resource,
    }
}

fn terms_unifiable(
    t1: &Term,
    t2: &Term,
    constant_map: &mut HashMap<String, GraphElementId>,
    variable_map: &mut HashMap<String, String>,
) -> bool {
    match (t1, t2) {
        (Term::Variable(v), Term::Resource(r)) => {
            variable_constant_unifiable(v, *r, constant_map, variable_map)
        }
        (Term::Resource(r), Term::Variable(v)) => {
            variable_constant_unifiable(v, *r, constant_map, variable_map)
        }
        (Term::Resource(r1), Term::Resource(r2)) => r1 == r2,
        (Term::Variable(v1), Term::Variable(v2)) => {
            variable_map.insert(v1.clone(), v2.clone());
            true
        }
    }
}

/// True if two quad patterns can be unified (there exists a substitution that
/// makes them equal).
pub fn quad_patterns_unifiable(q1: &QuadPattern, q2: &QuadPattern) -> bool {
    let mut cm = HashMap::new();
    let mut vm = HashMap::new();
    terms_unifiable(&q1.subject, &q2.subject, &mut cm, &mut vm)
        && terms_unifiable(&q1.predicate, &q2.predicate, &mut cm, &mut vm)
        && terms_unifiable(&q1.object, &q2.object, &mut cm, &mut vm)
        && terms_unifiable(&q1.graph, &q2.graph, &mut cm, &mut vm)
}

// ── Dependency graph edges ───────────────────────────────────────────────────

/// An edge in the rule dependency graph.
/// Positive: rule A's head unifies with a positive body atom of rule B.
/// Negative: rule A's head unifies with a negated body atom of rule B.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternEdge {
    PositivePatternEdge(Rule),
    NegativePatternEdge(Rule),
}

impl PatternEdge {
    pub fn get_rule(&self) -> &Rule {
        match self {
            PatternEdge::PositivePatternEdge(r) | PatternEdge::NegativePatternEdge(r) => r,
        }
    }
}

/// Returns all rules that have a body atom (positive or negative) unifiable
/// with `head_pattern`. The edge is negative if the body atom is negated.
pub fn depending_rules(rules: &[Rule], head_pattern: &QuadPattern) -> Vec<PatternEdge> {
    let mut result = Vec::new();
    for rule in rules {
        let mut has_negative = false;
        let mut has_match = false;
        for atom in &rule.body {
            match atom {
                RuleAtom::PositivePattern(p) if quad_patterns_unifiable(head_pattern, p) => {
                    has_match = true;
                }
                RuleAtom::NotPattern(p) if quad_patterns_unifiable(head_pattern, p) => {
                    has_match = true;
                    has_negative = true;
                }
                _ => {}
            }
        }
        if has_match {
            if has_negative {
                result.push(PatternEdge::NegativePatternEdge(rule.clone()));
            } else {
                result.push(PatternEdge::PositivePatternEdge(rule.clone()));
            }
        }
    }
    result
}

/// Returns all rules whose head is unifiable with `pattern` (the intensional
/// — IDB — predicates for this pattern).
pub fn intentional_rules<'a>(
    rules: &'a [Rule],
    pattern: &QuadPattern,
) -> impl Iterator<Item = &'a Rule> {
    rules.iter().filter(move |rule| match &rule.head {
        RuleHead::Contradiction => false,
        RuleHead::NormalHead(hp) => quad_patterns_unifiable(pattern, hp),
    })
}

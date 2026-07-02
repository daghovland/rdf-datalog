/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use dag_rdf::{GraphElementId, QuadPattern, Term};
use sparql_parser::ast::Expression;
use std::collections::HashMap;
use std::fmt;

/// A position in a wildcard pattern — either a specific resource or a wildcard
/// matching any resource. Used in the rule-index for fast lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceOrWildcard {
    Resource(GraphElementId),
    Wildcard,
}

/// A quad pattern where every position is either a concrete ID or a wildcard.
/// Used as a key in the partial-rule index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QuadWildcard {
    pub graph: ResourceOrWildcard,
    pub subject: ResourceOrWildcard,
    pub predicate: ResourceOrWildcard,
    pub object: ResourceOrWildcard,
}

/// The head of a datalog rule — either a quad pattern to assert, or Contradiction
/// (signals an inconsistency if the body is satisfied).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RuleHead {
    NormalHead(QuadPattern),
    Contradiction,
}

impl RuleHead {
    pub fn get_variables(&self) -> Vec<&str> {
        match self {
            RuleHead::NormalHead(p) => p.get_variables(),
            RuleHead::Contradiction => vec![],
        }
    }
}

impl fmt::Display for RuleHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleHead::NormalHead(p) => write!(f, "{}", p),
            RuleHead::Contradiction => write!(f, "false"),
        }
    }
}

/// An atom in a rule body.
///
/// `FilterAtom(expr)` holds a SPARQL expression guard — the substitution passes
/// iff the expression evaluates to `true`.  Uses `sparql_parser::eval_expr_as_filter`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RuleAtom {
    PositivePattern(QuadPattern),
    NotPattern(QuadPattern),
    NotEqualsAtom(Term, Term),
    FilterAtom(Expression),
}

impl RuleAtom {
    pub fn get_variables(&self) -> Vec<&str> {
        match self {
            RuleAtom::PositivePattern(p) | RuleAtom::NotPattern(p) => p.get_variables(),
            RuleAtom::NotEqualsAtom(t1, t2) => {
                let mut vars = vec![];
                if let Term::Variable(v) = t1 {
                    vars.push(v.as_str());
                }
                if let Term::Variable(v) = t2 {
                    vars.push(v.as_str());
                }
                vars
            }
            RuleAtom::FilterAtom(_) => vec![],
        }
    }
}

impl fmt::Display for RuleAtom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleAtom::PositivePattern(p) => write!(f, "{}", p),
            RuleAtom::NotPattern(p) => write!(f, "not {}", p),
            RuleAtom::NotEqualsAtom(t1, t2) => write!(f, "{} != {}", t1, t2),
            RuleAtom::FilterAtom(expr) => write!(f, "FILTER({:?})", expr),
        }
    }
}

/// A complete datalog rule: `head :- body`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rule {
    pub head: RuleHead,
    pub body: Vec<RuleAtom>,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = self
            .body
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "{} :- {} .", self.head, body)
    }
}

/// A variable substitution mapping variable names to concrete resource IDs.
pub type Substitution = HashMap<String, GraphElementId>;

/// A rule together with the specific body atom that triggered a partial match.
#[derive(Debug, Clone)]
pub struct PartialRule {
    pub rule: Rule,
    pub match_pattern: QuadPattern,
    /// Index of this rule in the owning `DatalogProgram::rules` vector.
    pub rule_id: usize,
}

/// A partial match: a rule + the triggering pattern + current substitution.
#[derive(Debug, Clone)]
pub struct PartialRuleMatch {
    pub partial_rule: PartialRule,
    pub substitution: Substitution,
}

// ── DerivedFrom index ─────────────────────────────────────────────────────────

/// Records one way a derived quad was produced: which rule and which body quads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derivation {
    /// Index into `DatalogProgram::rules` of the rule that produced this fact.
    pub rule_id: usize,
    /// The concrete quads that matched the rule's positive body atoms (in body order).
    pub body_witnesses: Vec<dag_rdf::Quad>,
}

/// Maps each derived quad to all the ways it can be derived.
///
/// A single quad may appear in multiple entries when it is derivable via more
/// than one rule or via the same rule with different body witnesses.  The BF
/// backward phase uses this to decide whether a derived fact can survive after
/// some base facts are deleted.
#[derive(Debug, Clone, Default)]
pub struct DerivedFromIndex {
    index: std::collections::HashMap<dag_rdf::Quad, Vec<Derivation>>,
}

impl DerivedFromIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `derived_quad` was produced by `derivation`.
    ///
    /// Duplicate derivations (same rule_id and body_witnesses) are silently ignored
    /// so that semi-naive iteration does not double-count witnesses for the same quad.
    pub fn record(&mut self, derived_quad: dag_rdf::Quad, derivation: Derivation) {
        let entries = self.index.entry(derived_quad).or_default();
        if !entries.contains(&derivation) {
            entries.push(derivation);
        }
    }

    /// Return all derivations for `quad`, or an empty slice if it has none.
    pub fn derivations_for(&self, quad: &dag_rdf::Quad) -> &[Derivation] {
        self.index.get(quad).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// True iff `quad` has at least one recorded derivation.
    pub fn has_derivation(&self, quad: &dag_rdf::Quad) -> bool {
        self.index.contains_key(quad)
    }

    /// Remove all derivations for `quad` (e.g. when the quad is retracted).
    pub fn remove(&mut self, quad: &dag_rdf::Quad) {
        self.index.remove(quad);
    }

    /// Iterate over all (derived_quad, derivations) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&dag_rdf::Quad, &Vec<Derivation>)> {
        self.index.iter()
    }
}

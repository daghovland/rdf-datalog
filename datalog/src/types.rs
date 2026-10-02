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
    /// Reverse index: `rule_id -> { derived_quad -> count of recorded
    /// derivations of that quad using this rule_id }`. Lets
    /// [`crate::IncrementalReasoner`]'s negation-invalidation scan
    /// (`negation_invalidated_seeds`) look up exactly the derived quads a
    /// candidate `rule_id` could have produced, instead of scanning every
    /// entry in `index` — see
    /// [#683](https://github.com/daghovland/rdf-datalog/issues/683). The
    /// count (rather than a plain set) is needed because the same quad can
    /// be derived more than once by the same rule with different body
    /// witnesses; `unrecord` must not drop the quad from the reverse index
    /// while any such derivation still exists.
    by_rule: std::collections::HashMap<usize, std::collections::HashMap<dag_rdf::Quad, usize>>,
}

impl DerivedFromIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `derived_quad` was produced by `derivation`.
    ///
    /// Duplicate derivations (same rule_id and body_witnesses) are silently ignored
    /// so that semi-naive iteration does not double-count witnesses for the same quad.
    ///
    /// Returns `true` iff this was a genuinely new entry (not a duplicate) —
    /// callers implementing undo-log rollback (see
    /// [`crate::IncrementalReasoner::apply_insertions`]) use this to know
    /// exactly which entries this call added, so [`Self::unrecord`] can
    /// remove precisely those on failure without disturbing pre-existing
    /// derivations for the same quad. See
    /// [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    pub fn record(&mut self, derived_quad: dag_rdf::Quad, derivation: Derivation) -> bool {
        let entries = self.index.entry(derived_quad).or_default();
        if !entries.contains(&derivation) {
            let rule_id = derivation.rule_id;
            entries.push(derivation);
            *self
                .by_rule
                .entry(rule_id)
                .or_default()
                .entry(derived_quad)
                .or_insert(0) += 1;
            true
        } else {
            false
        }
    }

    /// Remove exactly one previously-`record`ed `(derived_quad, derivation)` entry.
    ///
    /// Unlike [`Self::remove`] (which drops *every* derivation for `quad`),
    /// this removes only the single matching entry, leaving any other
    /// derivations for the same quad intact. Used to undo a specific
    /// `record` call during rollback. If the quad's entry list becomes empty
    /// as a result, the key is dropped entirely. No-op if the entry is not
    /// present. See [#320](https://github.com/daghovland/rdf-datalog/issues/320).
    pub fn unrecord(&mut self, derived_quad: &dag_rdf::Quad, derivation: &Derivation) {
        let mut removed = false;
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.index.entry(*derived_quad)
        {
            let entries = e.get_mut();
            let before = entries.len();
            entries.retain(|d| d != derivation);
            removed = entries.len() != before;
            if entries.is_empty() {
                e.remove();
            }
        }
        if removed {
            self.decrement_by_rule(derivation.rule_id, derived_quad);
        }
    }

    /// Decrement (and, at zero, drop) the `by_rule[rule_id][quad]` count.
    fn decrement_by_rule(&mut self, rule_id: usize, quad: &dag_rdf::Quad) {
        if let std::collections::hash_map::Entry::Occupied(mut rule_entry) =
            self.by_rule.entry(rule_id)
        {
            let quads = rule_entry.get_mut();
            if let std::collections::hash_map::Entry::Occupied(mut count_entry) = quads.entry(*quad)
            {
                *count_entry.get_mut() -= 1;
                if *count_entry.get() == 0 {
                    count_entry.remove();
                }
            }
            if quads.is_empty() {
                rule_entry.remove();
            }
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
        if let Some(derivations) = self.index.remove(quad) {
            for d in &derivations {
                if let Some(quads) = self.by_rule.get_mut(&d.rule_id) {
                    quads.remove(quad);
                    if quads.is_empty() {
                        self.by_rule.remove(&d.rule_id);
                    }
                }
            }
        }
    }

    /// Iterate over all (derived_quad, derivations) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&dag_rdf::Quad, &Vec<Derivation>)> {
        self.index.iter()
    }

    /// Every derived quad that has (or had) at least one recorded derivation
    /// using `rule_id`, without scanning derivations for any other rule. The
    /// indexed counterpart to filtering [`Self::iter`] by
    /// `derivation.rule_id == rule_id` — see the `by_rule` field doc and
    /// [#683](https://github.com/daghovland/rdf-datalog/issues/683).
    pub fn quads_for_rule(&self, rule_id: usize) -> impl Iterator<Item = &dag_rdf::Quad> {
        self.by_rule
            .get(&rule_id)
            .into_iter()
            .flat_map(|m| m.keys())
    }
}

#[cfg(test)]
mod derived_from_index_tests {
    use super::*;
    use std::collections::HashSet;

    fn q(n: u32) -> dag_rdf::Quad {
        dag_rdf::Quad {
            triple_id: 0,
            subject: n,
            predicate: 100,
            obj: 200,
        }
    }

    fn derivation(rule_id: usize, witness: dag_rdf::Quad) -> Derivation {
        Derivation {
            rule_id,
            body_witnesses: vec![witness],
        }
    }

    /// `quads_for_rule` returns exactly the derived quads recorded under
    /// that `rule_id`, not quads derived by other rules — the core
    /// correctness property the index must have to replace the full
    /// `iter()` scan in `negation_invalidated_seeds`. See
    /// [#683](https://github.com/daghovland/rdf-datalog/issues/683).
    #[test]
    fn quads_for_rule_returns_only_matching_rule() {
        let mut idx = DerivedFromIndex::new();
        idx.record(q(1), derivation(0, q(10)));
        idx.record(q(2), derivation(1, q(11)));
        idx.record(q(3), derivation(0, q(12)));

        let rule0: HashSet<_> = idx.quads_for_rule(0).copied().collect();
        assert_eq!(rule0, HashSet::from([q(1), q(3)]));

        let rule1: HashSet<_> = idx.quads_for_rule(1).copied().collect();
        assert_eq!(rule1, HashSet::from([q(2)]));

        assert_eq!(idx.quads_for_rule(2).count(), 0);
    }

    /// A quad derived twice by the same rule (different witnesses) appears
    /// exactly once in `quads_for_rule` (it is a set of quads, not of
    /// derivations), and surviving the removal of one of the two
    /// derivations (`unrecord`) must not drop the quad from the index since
    /// the other derivation still uses that rule_id.
    #[test]
    fn quads_for_rule_dedups_and_survives_partial_unrecord() {
        let mut idx = DerivedFromIndex::new();
        let d_a = derivation(0, q(10));
        let d_b = derivation(0, q(11));
        idx.record(q(1), d_a.clone());
        idx.record(q(1), d_b.clone());

        let rule0: Vec<_> = idx.quads_for_rule(0).collect();
        assert_eq!(rule0, vec![&q(1)]);

        idx.unrecord(&q(1), &d_a);
        // One derivation remains (d_b), so q(1) must still be reachable.
        let rule0: Vec<_> = idx.quads_for_rule(0).collect();
        assert_eq!(rule0, vec![&q(1)]);
        assert!(idx.has_derivation(&q(1)));

        idx.unrecord(&q(1), &d_b);
        // No derivations left at all now.
        assert_eq!(idx.quads_for_rule(0).count(), 0);
        assert!(!idx.has_derivation(&q(1)));
    }

    /// `remove` (drop every derivation for a quad, e.g. on retraction) must
    /// also clear that quad out of every `rule_id` it was indexed under,
    /// including when two different rules both derived the same quad.
    #[test]
    fn remove_clears_quad_from_every_rule_bucket() {
        let mut idx = DerivedFromIndex::new();
        idx.record(q(1), derivation(0, q(10)));
        idx.record(q(1), derivation(1, q(11)));

        idx.remove(&q(1));

        assert_eq!(idx.quads_for_rule(0).count(), 0);
        assert_eq!(idx.quads_for_rule(1).count(), 0);
        assert!(!idx.has_derivation(&q(1)));
    }

    /// Recording a duplicate derivation (same rule_id + body_witnesses) is a
    /// no-op for the reverse index too — it must not inflate the recorded
    /// count so that a single later `unrecord` already drops the quad.
    #[test]
    fn duplicate_record_does_not_require_double_unrecord() {
        let mut idx = DerivedFromIndex::new();
        let d = derivation(0, q(10));
        assert!(idx.record(q(1), d.clone()));
        assert!(!idx.record(q(1), d.clone())); // duplicate, ignored

        idx.unrecord(&q(1), &d);
        assert_eq!(idx.quads_for_rule(0).count(), 0);
        assert!(!idx.has_derivation(&q(1)));
    }
}

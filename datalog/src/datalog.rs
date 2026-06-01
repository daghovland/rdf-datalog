/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::types::{
    PartialRule, PartialRuleMatch, QuadWildcard, ResourceOrWildcard, Rule, RuleAtom, Substitution,
};
use dag_rdf::{GraphElementId, Quad, QuadPattern, QuadTable, Term};
use std::collections::HashMap;

pub fn empty_substitution() -> Substitution {
    HashMap::new()
}

pub fn is_fact(rule: &Rule) -> bool {
    rule.body.is_empty()
}

/// Build a fully-ground QuadPattern from a concrete Quad.
pub fn constant_quad_pattern(quad: &Quad) -> QuadPattern {
    QuadPattern {
        graph: Term::Resource(quad.triple_id),
        subject: Term::Resource(quad.subject),
        predicate: Term::Resource(quad.predicate),
        object: Term::Resource(quad.obj),
    }
}

/// Compute the single canonical index key for a rule body atom.
///
/// Variables become Wildcard; concrete resource IDs stay as Resource.
/// This is used when INDEXING rule body atoms into the rule_map — one key per atom,
/// no sub-wildcards.  The fact-side lookup (`wildcard_quad_pattern`) expands concrete
/// positions into both Resource and Wildcard, so every rule body pattern is reachable.
pub fn direct_wildcard_pattern(quad: &QuadPattern) -> QuadWildcard {
    fn to_rw(term: &Term) -> ResourceOrWildcard {
        match term {
            Term::Variable(_) => ResourceOrWildcard::Wildcard,
            Term::Resource(r) => ResourceOrWildcard::Resource(*r),
        }
    }
    QuadWildcard {
        graph: to_rw(&quad.graph),
        subject: to_rw(&quad.subject),
        predicate: to_rw(&quad.predicate),
        object: to_rw(&quad.object),
    }
}

/// Generate all quad wildcard patterns for a quad pattern (up to 16 combinations).
/// Variables always become Wildcard; constants produce both Resource and Wildcard variants.
/// Used for FACT-SIDE lookup only — finds all rule body atoms whose canonical key matches.
pub fn wildcard_quad_pattern(quad: &QuadPattern) -> Vec<QuadWildcard> {
    fn expand(term: &Term) -> Vec<ResourceOrWildcard> {
        match term {
            Term::Variable(_) => vec![ResourceOrWildcard::Wildcard],
            Term::Resource(r) => vec![
                ResourceOrWildcard::Resource(*r),
                ResourceOrWildcard::Wildcard,
            ],
        }
    }
    let graphs = expand(&quad.graph);
    let subjects = expand(&quad.subject);
    let predicates = expand(&quad.predicate);
    let objects = expand(&quad.object);

    let mut result = Vec::new();
    for g in &graphs {
        for s in &subjects {
            for p in &predicates {
                for o in &objects {
                    result.push(QuadWildcard {
                        graph: g.clone(),
                        subject: s.clone(),
                        predicate: p.clone(),
                        object: o.clone(),
                    });
                }
            }
        }
    }
    result
}

/// Returns variables in the rule head that are not bound in any body atom.
pub fn get_unsafe_head_variables(rule: &Rule) -> Vec<String> {
    let body_vars: std::collections::HashSet<String> = rule
        .body
        .iter()
        .flat_map(|a| a.get_variables())
        .map(|s| s.to_owned())
        .collect();
    rule.head
        .get_variables()
        .into_iter()
        .filter(|v| !body_vars.contains(*v))
        .map(|v| v.to_owned())
        .collect()
}

pub fn is_safe_rule(rule: &Rule) -> bool {
    let unsafe_vars = get_unsafe_head_variables(rule);
    if unsafe_vars.is_empty() {
        true
    } else {
        panic!("Unsafe variables {:?} in rule: {}", unsafe_vars, rule)
    }
}

/// Try to extend substitution `sub` by unifying `resource` with `term`.
pub fn get_substitution(
    resource: GraphElementId,
    term: &Term,
    sub: Substitution,
) -> Option<Substitution> {
    match term {
        Term::Resource(r) => {
            if *r == resource {
                Some(sub)
            } else {
                None
            }
        }
        Term::Variable(v) => match sub.get(v) {
            Some(&r) if r == resource => Some(sub),
            Some(_) => None,
            None => {
                let mut sub = sub;
                sub.insert(v.clone(), resource);
                Some(sub)
            }
        },
    }
}

/// Try to build a complete substitution for `fact` matching `pattern`.
pub fn get_substitutions(
    sub: Substitution,
    fact: &Quad,
    pattern: &QuadPattern,
) -> Option<Substitution> {
    get_substitution(fact.triple_id, &pattern.graph, sub)
        .and_then(|s| get_substitution(fact.subject, &pattern.subject, s))
        .and_then(|s| get_substitution(fact.predicate, &pattern.predicate, s))
        .and_then(|s| get_substitution(fact.obj, &pattern.object, s))
}

/// Apply a substitution to a Term, returning the concrete ID.
pub fn apply_substitution_resource(sub: &Substitution, term: &Term) -> GraphElementId {
    match term {
        Term::Resource(r) => *r,
        Term::Variable(v) => *sub
            .get(v)
            .unwrap_or_else(|| panic!("Variable {} not in substitution — invalid rule", v)),
    }
}

/// Apply a substitution to a QuadPattern to produce a concrete Quad.
pub fn apply_substitution_quad(sub: &Substitution, pattern: &QuadPattern) -> Quad {
    Quad {
        triple_id: apply_substitution_resource(sub, &pattern.graph),
        subject: apply_substitution_resource(sub, &pattern.subject),
        predicate: apply_substitution_resource(sub, &pattern.predicate),
        obj: apply_substitution_resource(sub, &pattern.object),
    }
}

/// Replace each Variable in a Term with its substitution (if present).
fn get_mapped_resource(sub: &Substitution, term: &Term) -> Term {
    match term {
        Term::Resource(_) => term.clone(),
        Term::Variable(v) => match sub.get(v) {
            Some(r) => Term::Resource(*r),
            None => term.clone(),
        },
    }
}

/// Evaluate a single positive pattern against the quad table, yielding all
/// substitution extensions that satisfy the pattern.
pub fn evaluate_pattern<'a>(
    rdf: &'a QuadTable,
    pattern: &QuadPattern,
    sub: Substitution,
) -> Box<dyn Iterator<Item = Substitution> + 'a> {
    let mapped = QuadPattern {
        graph: get_mapped_resource(&sub, &pattern.graph),
        subject: get_mapped_resource(&sub, &pattern.subject),
        predicate: get_mapped_resource(&sub, &pattern.predicate),
        object: get_mapped_resource(&sub, &pattern.object),
    };

    // Clone the mapped terms so the iterator owns them without referencing `mapped`.
    let (mg, ms, mp, mo) = (
        mapped.graph.clone(),
        mapped.subject.clone(),
        mapped.predicate.clone(),
        mapped.object.clone(),
    );

    let quads: Vec<Quad> = match (&mg, &ms, &mp, &mo) {
        (Term::Resource(g), Term::Resource(s), Term::Variable(_), Term::Variable(_)) => {
            rdf.get_quads_with_id_subject(*g, *s).collect()
        }
        (Term::Resource(g), Term::Variable(_), Term::Resource(p), Term::Variable(_)) => {
            rdf.get_quads_with_id_predicate(*g, *p).collect()
        }
        (Term::Resource(g), Term::Variable(_), Term::Variable(_), Term::Resource(o)) => {
            rdf.get_quads_with_id_object(*g, *o).collect()
        }
        (Term::Resource(g), Term::Resource(s), Term::Resource(p), Term::Variable(_)) => rdf
            .get_quads_with_id_subject_predicate(*g, *s, *p)
            .collect(),
        (Term::Resource(g), Term::Variable(_), Term::Resource(p), Term::Resource(o)) => {
            rdf.get_quads_with_id_object_predicate(*g, *o, *p).collect()
        }
        (Term::Resource(g), Term::Resource(s), Term::Resource(p), Term::Resource(o)) => {
            let quad = Quad {
                triple_id: *g,
                subject: *s,
                predicate: *p,
                obj: *o,
            };
            if rdf.contains(&quad) {
                vec![quad]
            } else {
                vec![]
            }
        }
        (Term::Resource(g), Term::Resource(s), Term::Variable(_), Term::Resource(o)) => {
            rdf.get_quads_with_id_subject_object(*g, *s, *o).collect()
        }
        (Term::Resource(g), Term::Variable(_), Term::Variable(_), Term::Variable(_)) => {
            rdf.get_graph(*g).collect()
        }
        (Term::Variable(_), Term::Resource(s), Term::Variable(_), Term::Variable(_)) => {
            rdf.get_quads_with_subject(*s).collect()
        }
        (Term::Variable(_), Term::Variable(_), Term::Resource(p), Term::Variable(_)) => {
            rdf.get_quads_with_predicate(*p).collect()
        }
        (Term::Variable(_), Term::Variable(_), Term::Variable(_), Term::Resource(o)) => {
            rdf.get_quads_with_object(*o).collect()
        }
        (Term::Variable(_), Term::Resource(s), Term::Resource(p), Term::Variable(_)) => {
            rdf.get_quads_with_subject_predicate(*s, *p).collect()
        }
        (Term::Variable(_), Term::Variable(_), Term::Resource(p), Term::Resource(o)) => {
            rdf.get_quads_with_object_predicate(*o, *p).collect()
        }
        (Term::Variable(_), Term::Resource(s), Term::Resource(p), Term::Resource(o)) => rdf
            .get_quads_with_subject_predicate(*s, *p)
            .filter(|q| q.obj == *o)
            .collect(),
        (Term::Variable(_), Term::Resource(s), Term::Variable(_), Term::Resource(o)) => {
            rdf.get_quads_with_subject_object(*s, *o).collect()
        }
        (Term::Variable(_), Term::Variable(_), Term::Variable(_), Term::Variable(_)) => {
            rdf.get_all_quads().collect()
        }
    };

    let pattern = QuadPattern {
        graph: mg,
        subject: ms,
        predicate: mp,
        object: mo,
    };
    Box::new(
        quads
            .into_iter()
            .filter_map(move |q| get_substitutions(sub.clone(), &q, &pattern)),
    )
}

/// Evaluate all positive body atoms of a partial rule match, returning all
/// complete substitutions.
pub fn evaluate_positive(rdf: &QuadTable, rule_match: &PartialRuleMatch) -> Vec<Substitution> {
    let positive_patterns: Vec<QuadPattern> = rule_match
        .partial_rule
        .rule
        .body
        .iter()
        .filter_map(|a| match a {
            RuleAtom::PositivePattern(p) => Some(p.clone()),
            _ => None,
        })
        .collect();

    let mut subs = vec![rule_match.substitution.clone()];
    for pattern in positive_patterns {
        subs = subs
            .into_iter()
            .flat_map(|sub| evaluate_pattern(rdf, &pattern, sub))
            .collect();
    }
    subs
}

/// Full evaluation: positive atoms first, then filter by negated atoms.
pub fn evaluate(rdf: &QuadTable, rule_match: &PartialRuleMatch) -> Vec<Substitution> {
    let not_patterns: Vec<QuadPattern> = rule_match
        .partial_rule
        .rule
        .body
        .iter()
        .filter_map(|a| match a {
            RuleAtom::NotPattern(p) => Some(p.clone()),
            _ => None,
        })
        .collect();

    let pos = evaluate_positive(rdf, rule_match);

    if not_patterns.is_empty() {
        return pos;
    }

    pos.into_iter()
        .filter(|sub| {
            not_patterns
                .iter()
                .all(|np| evaluate_pattern(rdf, np, sub.clone()).next().is_none())
        })
        .collect()
}

/// Build a partial rule index: quad-wildcard → list of partial rules.
pub fn get_partial_matches(rule: &Rule) -> HashMap<QuadWildcard, Vec<PartialRule>> {
    let mut map: HashMap<QuadWildcard, Vec<PartialRule>> = HashMap::new();
    for atom in &rule.body {
        let pattern = match atom {
            RuleAtom::PositivePattern(p) => p,
            _ => continue,
        };
        let wc = direct_wildcard_pattern(pattern);
        map.entry(wc).or_default().push(PartialRule {
            rule: rule.clone(),
            match_pattern: pattern.clone(),
        });
    }
    map
}

/// Merge multiple partial-rule maps into one.
pub fn merge_partial_match_maps(
    maps: Vec<HashMap<QuadWildcard, Vec<PartialRule>>>,
) -> HashMap<QuadWildcard, Vec<PartialRule>> {
    let mut result: HashMap<QuadWildcard, Vec<PartialRule>> = HashMap::new();
    for map in maps {
        for (k, v) in map {
            result.entry(k).or_default().extend(v);
        }
    }
    result
}

/// For a given fact and a partial rule, return all PartialRuleMatches where
/// the fact matches the triggering pattern.
pub fn get_matches_for_rule(fact: &Quad, partial_rule: &PartialRule) -> Vec<PartialRuleMatch> {
    let sub = get_substitutions(empty_substitution(), fact, &partial_rule.match_pattern);
    match sub {
        Some(s) => vec![PartialRuleMatch {
            partial_rule: partial_rule.clone(),
            substitution: s,
        }],
        None => vec![],
    }
}

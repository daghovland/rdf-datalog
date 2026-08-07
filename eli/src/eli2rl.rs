/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Translation from ELI to datalog rules.
//!
//! Algorithm 1 from <https://arxiv.org/abs/2008.02232>.

use crate::axioms::{ComplexConcept, Formula, NormalizedConcept};
use dag_rdf::query::get_default_graph_pattern;
use dag_rdf::{GraphElementManager, RdfResource, Term};
use datalog::types::{Rule, RuleAtom, RuleHead};
use ingress::{IriReference, OWL_SAME_AS, RDF_TYPE};
use owl_ontology::{Class, FullIri, Individual, ObjectPropertyExpression};

// ── Pattern helpers ───────────────────────────────────────────────────────────

fn get_type_pattern(
    resources: &mut GraphElementManager,
    var: &str,
    cls: &Class,
) -> dag_rdf::QuadPattern {
    let FullIri(class_iri) = cls;
    get_default_graph_pattern(
        Term::Variable(var.to_owned()),
        Term::Resource(
            resources.add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_owned()))),
        ),
        Term::Resource(resources.add_node_resource(RdfResource::Iri(class_iri.clone()))),
    )
}

fn get_anonymous_type_pattern(
    resources: &mut GraphElementManager,
    var: &str,
) -> dag_rdf::QuadPattern {
    let anon = resources.create_unnamed_anon_resource();
    get_default_graph_pattern(
        Term::Variable(var.to_owned()),
        Term::Resource(
            resources.add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_owned()))),
        ),
        Term::Resource(anon),
    )
}

fn get_role_pattern(
    role_id: dag_rdf::GraphElementId,
    subject_var: &str,
    object_var: &str,
) -> dag_rdf::QuadPattern {
    get_default_graph_pattern(
        Term::Variable(subject_var.to_owned()),
        Term::Resource(role_id),
        Term::Variable(object_var.to_owned()),
    )
}

/// Resolves an `Individual` to its interned `GraphElementId`.
fn individual_to_id(
    resources: &mut GraphElementManager,
    individual: &Individual,
) -> dag_rdf::GraphElementId {
    match individual {
        Individual::NamedIndividual(FullIri(iri)) => {
            resources.add_node_resource(RdfResource::Iri(iri.clone()))
        }
        Individual::AnonymousIndividual(anon_id) => {
            resources.get_or_create_named_anon_resource(format!("{}", anon_id))
        }
    }
}

fn get_role_value_pattern(
    resources: &mut GraphElementManager,
    role_id: dag_rdf::GraphElementId,
    subject_var: &str,
    individual: &Individual,
) -> dag_rdf::QuadPattern {
    let obj_id = individual_to_id(resources, individual);
    get_default_graph_pattern(
        Term::Variable(subject_var.to_owned()),
        Term::Resource(role_id),
        Term::Resource(obj_id),
    )
}

/// Resolves a single (non-chain) `ObjectPropertyExpression` — i.e. a named
/// or anonymous property, or an inverse of one — into one quad pattern
/// `(subject_var, role, object_var)`. Used as the "one join atom" building
/// block both directly and as the last link of an `ObjectPropertyChain`.
///
/// Returns `None` if `prop` is itself an `ObjectPropertyChain`: OWL 2 does
/// not allow `ObjectPropertyChain` to nest inside another chain's element
/// list, so this is a defensive guard against a malformed ontology rather
/// than an expected case.
fn get_single_role_pattern(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    subject_var: &str,
    object_var: &str,
) -> Option<dag_rdf::QuadPattern> {
    match prop {
        ObjectPropertyExpression::NamedObjectProperty(FullIri(iri)) => {
            let role_id = resources.add_node_resource(RdfResource::Iri(iri.clone()));
            Some(get_role_pattern(role_id, subject_var, object_var))
        }
        ObjectPropertyExpression::AnonymousObjectProperty(id) => {
            let role_id = resources.add_node_resource(RdfResource::AnonymousBlankNode(*id));
            Some(get_role_pattern(role_id, subject_var, object_var))
        }
        ObjectPropertyExpression::InverseObjectProperty(inner) => {
            get_single_role_pattern(resources, inner, object_var, subject_var)
        }
        ObjectPropertyExpression::ObjectPropertyChain(_) => None,
    }
}

/// Resolves a single (non-chain) `ObjectPropertyExpression` into one quad
/// pattern whose object is fixed to `individual` rather than a free
/// variable — the `owl:hasValue` counterpart of [`get_single_role_pattern`].
///
/// Handles `InverseObjectProperty(NamedObjectProperty | AnonymousObjectProperty)`:
/// `ObjectHasValue(ObjectInverseOf(P), a)` denotes "the set of x such that
/// a P x", so `individual` is the pattern's subject and `subject_var` is
/// its object. `InverseObjectProperty` wrapping anything other than a
/// simple named/anonymous property (in particular a chain) is not
/// supported — see [issue #408](https://github.com/daghovland/rdf-datalog/issues/408)
/// for why this narrower scope is acceptable (OWL 2's RDF mapping does not
/// structurally permit `ObjectPropertyChain` as the direct target of
/// `ObjectInverseOf` in the first place).
fn get_single_obj_value_pattern(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    subject_var: &str,
    individual: &Individual,
) -> Option<dag_rdf::QuadPattern> {
    match prop {
        ObjectPropertyExpression::NamedObjectProperty(FullIri(iri)) => {
            let role_id = resources.add_node_resource(RdfResource::Iri(iri.clone()));
            Some(get_role_value_pattern(
                resources,
                role_id,
                subject_var,
                individual,
            ))
        }
        ObjectPropertyExpression::AnonymousObjectProperty(id) => {
            let role_id = resources.add_node_resource(RdfResource::AnonymousBlankNode(*id));
            Some(get_role_value_pattern(
                resources,
                role_id,
                subject_var,
                individual,
            ))
        }
        ObjectPropertyExpression::InverseObjectProperty(inner) => match inner.as_ref() {
            ObjectPropertyExpression::NamedObjectProperty(FullIri(iri)) => {
                let role_id = resources.add_node_resource(RdfResource::Iri(iri.clone()));
                let obj_id = individual_to_id(resources, individual);
                Some(get_default_graph_pattern(
                    Term::Resource(obj_id),
                    Term::Resource(role_id),
                    Term::Variable(subject_var.to_owned()),
                ))
            }
            ObjectPropertyExpression::AnonymousObjectProperty(id) => {
                let role_id = resources.add_node_resource(RdfResource::AnonymousBlankNode(*id));
                let obj_id = individual_to_id(resources, individual);
                Some(get_default_graph_pattern(
                    Term::Resource(obj_id),
                    Term::Resource(role_id),
                    Term::Variable(subject_var.to_owned()),
                ))
            }
            _ => {
                log::warn!(
                    "InverseObjectProperty wrapping an InverseObjectProperty or \
                     ObjectPropertyChain inside ObjectHasValue is not supported; \
                     skipping this axiom (issue #408)"
                );
                None
            }
        },
        ObjectPropertyExpression::ObjectPropertyChain(_) => None,
    }
}

/// Builds the join-atom chain for an `ObjectPropertyChain` of `n >= 1`
/// properties, shared by both [`get_obj_prop_pattern`]'s and
/// [`get_obj_value_pattern`]'s chain cases: the first `n - 1` links are
/// resolved as free-variable joins via `subject_var -p1-> v1 -p2-> ... ->
/// v(n-1)`, introducing fresh intermediate variables named
/// `{subject_var}_{var_hint}_chain{i}`; the final link is resolved by
/// `final_link` (a free variable for `get_obj_prop_pattern`, a fixed
/// individual for `get_obj_value_pattern`).
///
/// Returns `None` if `props` is empty (nothing sensible to join) or if any
/// link fails to translate (propagated from the recursive/final calls).
fn build_prop_chain<F>(
    resources: &mut GraphElementManager,
    props: &[ObjectPropertyExpression],
    subject_var: &str,
    var_hint: &str,
    final_link: F,
) -> Option<Vec<dag_rdf::QuadPattern>>
where
    F: FnOnce(
        &mut GraphElementManager,
        &ObjectPropertyExpression,
        &str,
    ) -> Option<dag_rdf::QuadPattern>,
{
    let n = props.len();
    if n == 0 {
        return None;
    }
    let mut patterns = Vec::new();
    let mut cur_var = subject_var.to_owned();
    for (i, link) in props[..n - 1].iter().enumerate() {
        let next_var = format!("{subject_var}_{var_hint}_chain{i}");
        patterns.extend(get_obj_prop_pattern(resources, link, &cur_var, &next_var)?);
        cur_var = next_var;
    }
    let last_pattern = final_link(resources, &props[n - 1], &cur_var)?;
    patterns.push(last_pattern);
    Some(patterns)
}

/// Resolves `prop` into a chain of one or more join atoms connecting
/// `subject_var` to `object_var`. A simple/named/anonymous property (or an
/// inverse of one) produces a single-element chain; an `ObjectPropertyChain`
/// of `p1, ..., pn` produces `n` atoms joined through `n - 1` fresh
/// intermediate variables.
///
/// Returns `None` for `InverseObjectProperty(ObjectPropertyChain(...))` —
/// correctly inverting a chain would require reversing its element order
/// and inverting each link, which is not implemented. See
/// [issue #408](https://github.com/daghovland/rdf-datalog/issues/408).
fn get_obj_prop_pattern(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    subject_var: &str,
    object_var: &str,
) -> Option<Vec<dag_rdf::QuadPattern>> {
    match prop {
        ObjectPropertyExpression::NamedObjectProperty(_)
        | ObjectPropertyExpression::AnonymousObjectProperty(_) => {
            Some(vec![get_single_role_pattern(
                resources,
                prop,
                subject_var,
                object_var,
            )?])
        }
        ObjectPropertyExpression::InverseObjectProperty(inner) => match inner.as_ref() {
            ObjectPropertyExpression::ObjectPropertyChain(_) => {
                log::warn!(
                    "InverseObjectProperty wrapping an ObjectPropertyChain is not \
                     supported; skipping this axiom (issue #408)"
                );
                None
            }
            _ => get_obj_prop_pattern(resources, inner, object_var, subject_var),
        },
        ObjectPropertyExpression::ObjectPropertyChain(props) => {
            if props.is_empty() {
                log::warn!("Empty ObjectPropertyChain; skipping this axiom (issue #408)");
                return None;
            }
            build_prop_chain(resources, props, subject_var, object_var, |r, link, var| {
                get_single_role_pattern(r, link, var, object_var)
            })
        }
    }
}

/// Resolves `prop` into a chain of one or more join atoms connecting
/// `subject_var` to the fixed `individual` — the `owl:hasValue`
/// counterpart of [`get_obj_prop_pattern`]. The **last** pattern's object
/// is `individual` rather than a free variable.
///
/// Returns `None` for `InverseObjectProperty(ObjectPropertyChain(...))`,
/// for the same reason as `get_obj_prop_pattern`. See
/// [issue #408](https://github.com/daghovland/rdf-datalog/issues/408).
fn get_obj_value_pattern(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    subject_var: &str,
    individual: &Individual,
) -> Option<Vec<dag_rdf::QuadPattern>> {
    match prop {
        ObjectPropertyExpression::NamedObjectProperty(_)
        | ObjectPropertyExpression::AnonymousObjectProperty(_) => {
            Some(vec![get_single_obj_value_pattern(
                resources,
                prop,
                subject_var,
                individual,
            )?])
        }
        ObjectPropertyExpression::InverseObjectProperty(inner) => match inner.as_ref() {
            ObjectPropertyExpression::ObjectPropertyChain(_) => {
                log::warn!(
                    "InverseObjectProperty wrapping an ObjectPropertyChain inside \
                     ObjectHasValue is not supported; skipping this axiom (issue #408)"
                );
                None
            }
            _ => Some(vec![get_single_obj_value_pattern(
                resources,
                prop,
                subject_var,
                individual,
            )?]),
        },
        ObjectPropertyExpression::ObjectPropertyChain(props) => {
            if props.is_empty() {
                log::warn!("Empty ObjectPropertyChain; skipping this axiom (issue #408)");
                return None;
            }
            build_prop_chain(resources, props, subject_var, "hv", |r, link, var| {
                get_single_obj_value_pattern(r, link, var, individual)
            })
        }
    }
}

// ── ELI translation (Algorithm 1) ────────────────────────────────────────────

fn translate_eli(
    resources: &mut GraphElementManager,
    concept: &ComplexConcept,
    var_name: &str,
    clause: usize,
) -> Option<Vec<dag_rdf::QuadPattern>> {
    match concept {
        ComplexConcept::AtomicConcept(FullIri(iri)) => Some(vec![get_type_pattern(
            resources,
            var_name,
            &FullIri(iri.clone()),
        )]),
        ComplexConcept::Intersection(clauses) => {
            let mut result = Vec::new();
            for (i, c) in clauses.iter().enumerate() {
                result.extend(translate_eli(resources, c, var_name, i + 1)?);
            }
            Some(result)
        }
        ComplexConcept::SomeValuesFrom(role, inner_concept) => {
            let new_var = format!("{}_{}", var_name, clause);
            let role_triples = get_obj_prop_pattern(resources, role, var_name, &new_var)?;
            let concept_triples = translate_eli(resources, inner_concept, &new_var, 1)?;
            Some(role_triples.into_iter().chain(concept_triples).collect())
        }
        ComplexConcept::Top => Some(vec![]),
    }
}

fn translate_simple_subclass(
    resources: &mut GraphElementManager,
    sub: &ComplexConcept,
    sup: &Class,
) -> Option<Rule> {
    Some(Rule {
        head: RuleHead::NormalHead(get_type_pattern(resources, "X", sup)),
        body: translate_eli(resources, sub, "X", 1)?
            .into_iter()
            .map(RuleAtom::PositivePattern)
            .collect(),
    })
}

fn translate_empty_intersection(
    resources: &mut GraphElementManager,
    sub_concepts: &[Class],
) -> Option<Rule> {
    Some(Rule {
        head: RuleHead::Contradiction,
        body: sub_concepts
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    })
}

fn get_atomic_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    concept_name: &Class,
) -> Option<Vec<Rule>> {
    Some(vec![Rule {
        head: RuleHead::NormalHead(get_type_pattern(resources, "X", concept_name)),
        body: sub_conjunction
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    }])
}

fn get_atomic_anonymous_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
) -> Option<Vec<Rule>> {
    Some(vec![Rule {
        head: RuleHead::NormalHead(get_anonymous_type_pattern(resources, "X")),
        body: sub_conjunction
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    }])
}

fn get_universal_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    prop: &ObjectPropertyExpression,
    concept_name: &Class,
) -> Option<Vec<Rule>> {
    let role_atoms = get_obj_prop_pattern(resources, prop, "X", "Y")?
        .into_iter()
        .map(RuleAtom::PositivePattern);
    let type_head = get_type_pattern(resources, "Y", concept_name);
    let body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .chain(role_atoms)
        .collect();
    Some(vec![Rule {
        head: RuleHead::NormalHead(type_head),
        body,
    }])
}

fn get_at_most_one_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    prop: &ObjectPropertyExpression,
) -> Option<Vec<Rule>> {
    // W3C OWL-RL rule prp-fp: if p is functional and p(X,Y1) and p(X,Y2)
    // then sameAs(Y1,Y2).  No negation guard — matches the monotonic W3C spec
    // rule and avoids a stratification cycle (the negated body and the head
    // would both mention sameAs).  Firing when Y1=Y2 just derives sameAs(Y,Y),
    // which is harmless reflexivity.
    let same_as_iri = IriReference(OWL_SAME_AS.to_owned());
    let p1 = get_obj_prop_pattern(resources, prop, "X", "Y1")?;
    let p2 = get_obj_prop_pattern(resources, prop, "X", "Y2")?;
    let mut body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .collect();
    body.extend(p1.into_iter().map(RuleAtom::PositivePattern));
    body.extend(p2.into_iter().map(RuleAtom::PositivePattern));
    // `owl:sameAs` is always a fixed, synthetic named property here (never a
    // chain), and a rule head is structurally a single atom — build it
    // directly via `get_role_pattern` rather than threading the `Vec` that
    // `get_obj_prop_pattern` returns through head position.
    let same_as_role_id = resources.add_node_resource(RdfResource::Iri(same_as_iri));
    Some(vec![Rule {
        head: RuleHead::NormalHead(get_role_pattern(same_as_role_id, "Y1", "Y2")),
        body,
    }])
}

/// `C ⊑ ≤0 R` — OWL 2 RL/RDF rule `cls-maxc0`:
/// `T(?u, rdf:type, ?x), T(?x, owl:maxCardinality, "0"), T(?x, owl:onProperty, ?p),
/// T(?u, ?p, ?y) → false`.
///
/// The contradiction is conditional on the `R`-edge actually existing in the
/// body (`prop(X, Y)` for a fresh `Y`), combining the sub_conjunction's
/// type-membership atoms (as in `translate_empty_intersection`) with a
/// property-edge atom (as in `get_universal_normalized_rule`) — unlike the
/// unconditional `Bottom` case, an instance of `C` with zero `R`-successors
/// does NOT trigger this rule. See
/// <https://github.com/daghovland/rdf-datalog/issues/298>.
fn get_at_most_zero_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    prop: &ObjectPropertyExpression,
) -> Option<Vec<Rule>> {
    let role_atoms = get_obj_prop_pattern(resources, prop, "X", "Y")?
        .into_iter()
        .map(RuleAtom::PositivePattern);
    let body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .chain(role_atoms)
        .collect();
    Some(vec![Rule {
        head: RuleHead::Contradiction,
        body,
    }])
}

fn get_object_has_value_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    prop: &ObjectPropertyExpression,
    individual: &Individual,
) -> Option<Vec<Rule>> {
    // A rule head is structurally a single atom, but with chains/inverse
    // properties `get_obj_value_pattern` can now return more than one
    // pattern. Split it: all but the last pattern become body atoms
    // (introducing the chain's intermediate variables), and the last
    // pattern — the one whose object is fixed to `individual` — becomes the
    // head. For a simple (non-chain, non-inverse) property this always
    // returns exactly one pattern, so the body addition is empty and the
    // head is that single pattern, matching the pre-#408 behavior exactly.
    let mut patterns = get_obj_value_pattern(resources, prop, "X", individual)?;
    let head_pattern = patterns
        .pop()
        .expect("get_obj_value_pattern always returns a non-empty Vec");
    let mut body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .collect();
    body.extend(patterns.into_iter().map(RuleAtom::PositivePattern));
    Some(vec![Rule {
        head: RuleHead::NormalHead(head_pattern),
        body,
    }])
}

/// Translate a single normalized ELI formula into datalog rules.
///
/// Returns `None` if any sub-construct is not yet ELI-translatable — the
/// only remaining case is `InverseObjectProperty` wrapping an
/// `ObjectPropertyChain` (inside an existential or `owl:hasValue`), see
/// [issue #408](https://github.com/daghovland/rdf-datalog/issues/408).
/// The whole formula is skipped conservatively rather than partially
/// applied.
fn generate_axiom_rl(resources: &mut GraphElementManager, formula: &Formula) -> Option<Vec<Rule>> {
    match formula {
        Formula::DirectlyTranslatableConceptInclusion {
            subclass_disjunction,
            superclass_conjunction,
        } => {
            let mut rules = Vec::new();
            for sub in subclass_disjunction {
                for sup in superclass_conjunction {
                    rules.push(translate_simple_subclass(resources, sub, sup)?);
                }
            }
            Some(rules)
        }
        Formula::NormalizedConceptInclusion {
            subclass_conjunction,
            superclass,
        } => match superclass {
            NormalizedConcept::Bottom => {
                translate_empty_intersection(resources, subclass_conjunction).map(|rule| vec![rule])
            }
            NormalizedConcept::AtomicNamedConcept(cls) => {
                get_atomic_normalized_rule(resources, subclass_conjunction, cls)
            }
            NormalizedConcept::AtomicAnonymousConcept => {
                get_atomic_anonymous_normalized_rule(resources, subclass_conjunction)
            }
            NormalizedConcept::AllValuesFrom(prop, cls) => {
                get_universal_normalized_rule(resources, subclass_conjunction, prop, cls)
            }
            NormalizedConcept::ObjectHasValue(prop, individual) => {
                get_object_has_value_normalized_rule(
                    resources,
                    subclass_conjunction,
                    prop,
                    individual,
                )
            }
            NormalizedConcept::AtMostOneValueFrom(prop) => {
                get_at_most_one_normalized_rule(resources, subclass_conjunction, prop)
            }
            NormalizedConcept::AtMostZeroValueFrom(prop) => {
                get_at_most_zero_normalized_rule(resources, subclass_conjunction, prop)
            }
        },
    }
}

/// Translate a list of ELI formulas into datalog rules.
///
/// Returns `None` if any formula fails to translate (e.g. because it uses a
/// legal-but-unimplemented OWL 2 construct — see
/// [issue #408](https://github.com/daghovland/rdf-datalog/issues/408) for
/// the one remaining case) — the whole set of formulas (i.e. the whole
/// originating axiom) is skipped conservatively rather than partially
/// applied.
pub fn generate_tbox_rl(
    resources: &mut GraphElementManager,
    formulas: impl IntoIterator<Item = Formula>,
) -> Option<Vec<Rule>> {
    formulas
        .into_iter()
        .map(|f| generate_axiom_rl(resources, &f))
        .collect::<Option<Vec<Vec<Rule>>>>()
        .map(|rules| rules.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingress::IriReference;
    use owl_ontology::{ClassAxiom, ClassExpression};

    fn class(iri: &str) -> Class {
        FullIri(IriReference(iri.to_owned()))
    }

    fn named_prop(iri: &str) -> ObjectPropertyExpression {
        ObjectPropertyExpression::NamedObjectProperty(FullIri(IriReference(iri.to_owned())))
    }

    // ── get_obj_prop_pattern ──────────────────────────────────────────────

    #[test]
    fn get_obj_prop_pattern_named_property_returns_some() {
        let mut resources = GraphElementManager::new(10);
        let prop = named_prop("https://example.org/p");
        let result = get_obj_prop_pattern(&mut resources, &prop, "X", "Y");
        assert!(result.is_some());
    }

    #[test]
    fn get_obj_prop_pattern_property_chain_returns_two_link_chain() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let result = get_obj_prop_pattern(&mut resources, &chain, "X", "Y").unwrap();
        assert_eq!(result.len(), 2, "2-link chain must produce 2 quad patterns");

        let p1_id = resources.add_node_resource(RdfResource::Iri(IriReference(
            "https://example.org/p1".to_owned(),
        )));
        let p2_id = resources.add_node_resource(RdfResource::Iri(IriReference(
            "https://example.org/p2".to_owned(),
        )));

        // First pattern: (X, p1, <fresh>)
        assert_eq!(result[0].subject, Term::Variable("X".to_owned()));
        assert_eq!(result[0].predicate, Term::Resource(p1_id));
        let fresh_var = match &result[0].object {
            Term::Variable(v) => v.clone(),
            other => panic!("expected a fresh variable, got {other:?}"),
        };

        // Second pattern: (<same fresh var>, p2, Y)
        assert_eq!(result[1].subject, Term::Variable(fresh_var));
        assert_eq!(result[1].predicate, Term::Resource(p2_id));
        assert_eq!(result[1].object, Term::Variable("Y".to_owned()));
    }

    #[test]
    fn get_obj_prop_pattern_empty_chain_returns_none() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![]);
        let result = get_obj_prop_pattern(&mut resources, &chain, "X", "Y");
        assert!(result.is_none(), "empty chain has nothing to join");
    }

    #[test]
    fn get_obj_prop_pattern_inverse_of_chain_returns_none() {
        // InverseObjectProperty wrapping an ObjectPropertyChain stays
        // unimplemented (out of scope for #408): correctly inverting a chain
        // would require reversing element order and inverting each link, not
        // just swapping the two endpoint variables.
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![named_prop(
            "https://example.org/p1",
        )]);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(chain));
        let result = get_obj_prop_pattern(&mut resources, &inverse, "X", "Y");
        assert!(result.is_none());
    }

    // ── get_obj_value_pattern ─────────────────────────────────────────────

    #[test]
    fn get_obj_value_pattern_named_property_returns_some() {
        let mut resources = GraphElementManager::new(10);
        let prop = named_prop("https://example.org/p");
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let result = get_obj_value_pattern(&mut resources, &prop, "X", &individual);
        assert!(result.is_some());
    }

    #[test]
    fn get_obj_value_pattern_inverse_produces_swapped_pattern() {
        // ObjectHasValue(ObjectInverseOf(p), i) means "x such that i p x":
        // individual is the pattern's subject, subject_var is its object.
        let mut resources = GraphElementManager::new(10);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(named_prop(
            "https://example.org/p",
        )));
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let result = get_obj_value_pattern(&mut resources, &inverse, "X", &individual).unwrap();
        assert_eq!(result.len(), 1);

        let p_id = resources.add_node_resource(RdfResource::Iri(IriReference(
            "https://example.org/p".to_owned(),
        )));
        let i_id = resources.add_node_resource(RdfResource::Iri(IriReference(
            "https://example.org/i".to_owned(),
        )));
        assert_eq!(result[0].subject, Term::Resource(i_id));
        assert_eq!(result[0].predicate, Term::Resource(p_id));
        assert_eq!(result[0].object, Term::Variable("X".to_owned()));
    }

    #[test]
    fn get_obj_value_pattern_inverse_of_chain_returns_none() {
        // Out of scope for #408, same reasoning as get_obj_prop_pattern.
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![named_prop(
            "https://example.org/p1",
        )]);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(chain));
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let result = get_obj_value_pattern(&mut resources, &inverse, "X", &individual);
        assert!(result.is_none());
    }

    #[test]
    fn get_obj_value_pattern_property_chain_produces_two_link_chain_ending_at_individual() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let result = get_obj_value_pattern(&mut resources, &chain, "X", &individual).unwrap();
        assert_eq!(result.len(), 2, "2-link chain must produce 2 quad patterns");

        let p1_id = resources.add_node_resource(RdfResource::Iri(IriReference(
            "https://example.org/p1".to_owned(),
        )));
        let p2_id = resources.add_node_resource(RdfResource::Iri(IriReference(
            "https://example.org/p2".to_owned(),
        )));
        let i_id = resources.add_node_resource(RdfResource::Iri(IriReference(
            "https://example.org/i".to_owned(),
        )));

        // First pattern: (X, p1, <fresh>)
        assert_eq!(result[0].subject, Term::Variable("X".to_owned()));
        assert_eq!(result[0].predicate, Term::Resource(p1_id));
        let fresh_var = match &result[0].object {
            Term::Variable(v) => v.clone(),
            other => panic!("expected a fresh variable, got {other:?}"),
        };

        // Second (last) pattern: (<same fresh var>, p2, i) — fixed individual.
        assert_eq!(result[1].subject, Term::Variable(fresh_var));
        assert_eq!(result[1].predicate, Term::Resource(p2_id));
        assert_eq!(result[1].object, Term::Resource(i_id));
    }

    #[test]
    fn get_obj_value_pattern_empty_chain_returns_none() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![]);
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let result = get_obj_value_pattern(&mut resources, &chain, "X", &individual);
        assert!(result.is_none());
    }

    // ── translate_eli / generate_axiom_rl regression ────────────────────

    #[test]
    fn translate_eli_ordinary_axiom_still_produces_some() {
        let mut resources = GraphElementManager::new(10);
        let concept = ComplexConcept::AtomicConcept(class("https://example.org/A"));
        let result = translate_eli(&mut resources, &concept, "X", 1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn translate_eli_some_values_from_with_chain_produces_chain_plus_concept_patterns() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let concept = ComplexConcept::SomeValuesFrom(
            chain,
            Box::new(ComplexConcept::AtomicConcept(class(
                "https://example.org/A",
            ))),
        );
        let result = translate_eli(&mut resources, &concept, "X", 1).unwrap();
        // 2 role-chain patterns + 1 rdf:type pattern for the inner concept.
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn translate_eli_some_values_from_with_inverse_of_chain_returns_none() {
        // The still-unsupported nested case must propagate None, not panic.
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![named_prop(
            "https://example.org/p1",
        )]);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(chain));
        let concept = ComplexConcept::SomeValuesFrom(
            inverse,
            Box::new(ComplexConcept::AtomicConcept(class(
                "https://example.org/A",
            ))),
        );
        let result = translate_eli(&mut resources, &concept, "X", 1);
        assert!(result.is_none());
    }

    #[test]
    fn generate_axiom_rl_ordinary_axiom_produces_some_rules() {
        let mut resources = GraphElementManager::new(10);
        let formula = Formula::NormalizedConceptInclusion {
            subclass_conjunction: vec![class("https://example.org/A")],
            superclass: NormalizedConcept::AtomicNamedConcept(class("https://example.org/B")),
        };
        let result = generate_axiom_rl(&mut resources, &formula);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn generate_axiom_rl_object_has_value_with_inverse_produces_rule() {
        let mut resources = GraphElementManager::new(10);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(named_prop(
            "https://example.org/p",
        )));
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let formula = Formula::NormalizedConceptInclusion {
            subclass_conjunction: vec![class("https://example.org/A")],
            superclass: NormalizedConcept::ObjectHasValue(inverse, individual),
        };
        let result = generate_axiom_rl(&mut resources, &formula);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn generate_axiom_rl_object_has_value_with_inverse_of_chain_returns_none() {
        // Still-unsupported nested case: regression check that it stays None
        // rather than panicking.
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![named_prop(
            "https://example.org/p1",
        )]);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(chain));
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let formula = Formula::NormalizedConceptInclusion {
            subclass_conjunction: vec![class("https://example.org/A")],
            superclass: NormalizedConcept::ObjectHasValue(inverse, individual),
        };
        let result = generate_axiom_rl(&mut resources, &formula);
        assert!(result.is_none());
    }

    #[test]
    fn generate_tbox_rl_ordinary_formulas_produce_rules() {
        let mut resources = GraphElementManager::new(10);
        let formula = Formula::NormalizedConceptInclusion {
            subclass_conjunction: vec![class("https://example.org/A")],
            superclass: NormalizedConcept::AtomicNamedConcept(class("https://example.org/B")),
        };
        let result = generate_tbox_rl(&mut resources, vec![formula]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn generate_tbox_rl_still_unsupported_construct_skips_axiom() {
        // The only remaining unsupported construct: InverseObjectProperty
        // wrapping an ObjectPropertyChain inside ObjectHasValue.
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![named_prop(
            "https://example.org/p1",
        )]);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(chain));
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let formula = Formula::NormalizedConceptInclusion {
            subclass_conjunction: vec![class("https://example.org/A")],
            superclass: NormalizedConcept::ObjectHasValue(inverse, individual),
        };
        let result = generate_tbox_rl(&mut resources, vec![formula]);
        assert!(result.is_none());
    }

    // ── integration: eli::owl2datalog via crate::owl2datalog ────────────

    #[test]
    fn owl2datalog_existential_with_property_chain_produces_rules() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ObjectSomeValuesFrom(
                chain,
                Box::new(ClassExpression::ClassName(class("https://example.org/B"))),
            ),
            ClassExpression::ClassName(class("https://example.org/A")),
        );
        let result = crate::owl2datalog(&mut resources, &axiom);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn owl2datalog_object_has_value_with_inverse_produces_rules() {
        let mut resources = GraphElementManager::new(10);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(named_prop(
            "https://example.org/p",
        )));
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ClassName(class("https://example.org/A")),
            ClassExpression::ObjectHasValue(
                inverse,
                Individual::NamedIndividual(FullIri(IriReference(
                    "https://example.org/i".to_owned(),
                ))),
            ),
        );
        let result = crate::owl2datalog(&mut resources, &axiom);
        // The full extractor normalizes a SubClassOf axiom through
        // concept-representative rules (unrelated to this PR's scope), so
        // the exact rule count isn't a stable/meaningful assertion here —
        // what matters is that it now translates at all instead of
        // returning None, which the materialisation-level tests below
        // verify end to end.
        assert!(result.is_some());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn owl2datalog_existential_with_inverse_of_chain_does_not_panic() {
        // Regression check for the still-unsupported nested case: PR #407
        // established this must skip the axiom (None), not panic. Confirms
        // that narrowing the scope of #408 to exclude InverseObjectProperty
        // wrapping an ObjectPropertyChain didn't reintroduce the panic.
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![named_prop(
            "https://example.org/p1",
        )]);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(chain));
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ObjectSomeValuesFrom(
                inverse,
                Box::new(ClassExpression::ClassName(class("https://example.org/B"))),
            ),
            ClassExpression::ClassName(class("https://example.org/A")),
        );
        let result = crate::owl2datalog(&mut resources, &axiom);
        assert_eq!(result, None);
    }

    // ── materialisation-level: rules generated actually derive correct facts ──

    /// Helper: intern an IRI resource in `resources` and return its id.
    fn iri(resources: &mut GraphElementManager, s: &str) -> dag_rdf::GraphElementId {
        resources.add_node_resource(RdfResource::Iri(IriReference(s.to_owned())))
    }

    #[test]
    fn materialise_two_hop_property_chain_existential_derives_type() {
        // SubClassOf(A, ObjectSomeValuesFrom(ObjectPropertyChain(p1 p2), B))
        // over `a p1 m . m p2 b . b rdf:type B` should derive `a rdf:type A`.
        let mut ds = dag_rdf::Datastore::new(100);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ObjectSomeValuesFrom(
                chain,
                Box::new(ClassExpression::ClassName(class("https://example.org/B"))),
            ),
            ClassExpression::ClassName(class("https://example.org/A")),
        );
        let rules = crate::owl2datalog(&mut ds.resources, &axiom).expect("axiom should translate");

        let a = iri(&mut ds.resources, "https://example.org/a");
        let m = iri(&mut ds.resources, "https://example.org/m");
        let b_ind = iri(&mut ds.resources, "https://example.org/b");
        let p1 = iri(&mut ds.resources, "https://example.org/p1");
        let p2 = iri(&mut ds.resources, "https://example.org/p2");
        let rdf_type = iri(&mut ds.resources, RDF_TYPE);
        let cls_a = iri(&mut ds.resources, "https://example.org/A");
        let cls_b = iri(&mut ds.resources, "https://example.org/B");

        ds.add_triple(dag_rdf::ingress::Triple {
            subject: a,
            predicate: p1,
            obj: m,
        });
        ds.add_triple(dag_rdf::ingress::Triple {
            subject: m,
            predicate: p2,
            obj: b_ind,
        });
        ds.add_triple(dag_rdf::ingress::Triple {
            subject: b_ind,
            predicate: rdf_type,
            obj: cls_b,
        });

        datalog::reasoner::evaluate_rules(rules, &mut ds).expect("materialisation should succeed");

        assert!(
            ds.contains_triple(&dag_rdf::ingress::Triple {
                subject: a,
                predicate: rdf_type,
                obj: cls_a,
            }),
            "a rdf:type A should be derived via the 2-hop property chain"
        );
    }

    #[test]
    fn materialise_two_hop_property_chain_existential_negative_control() {
        // Same rule, but the data does NOT satisfy the chain (missing the
        // p2 hop) — a rdf:type A must NOT be derived.
        let mut ds = dag_rdf::Datastore::new(100);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ObjectSomeValuesFrom(
                chain,
                Box::new(ClassExpression::ClassName(class("https://example.org/B"))),
            ),
            ClassExpression::ClassName(class("https://example.org/A")),
        );
        let rules = crate::owl2datalog(&mut ds.resources, &axiom).expect("axiom should translate");

        let a = iri(&mut ds.resources, "https://example.org/a");
        let m = iri(&mut ds.resources, "https://example.org/m");
        let p1 = iri(&mut ds.resources, "https://example.org/p1");
        let rdf_type = iri(&mut ds.resources, RDF_TYPE);
        let cls_a = iri(&mut ds.resources, "https://example.org/A");

        // Only the first hop exists; no p2 edge, no B-membership.
        ds.add_triple(dag_rdf::ingress::Triple {
            subject: a,
            predicate: p1,
            obj: m,
        });

        datalog::reasoner::evaluate_rules(rules, &mut ds).expect("materialisation should succeed");

        assert!(
            !ds.contains_triple(&dag_rdf::ingress::Triple {
                subject: a,
                predicate: rdf_type,
                obj: cls_a,
            }),
            "a rdf:type A must not be derived when the chain is not fully satisfied"
        );
    }

    #[test]
    fn materialise_inverse_in_has_value_derives_property_edge() {
        // ObjectHasValue only ever appears in super-class position (see
        // `extractor.rs`'s `eli_class_extractor`, which does not handle
        // ObjectHasValue), so `SubClassOf(A, ObjectHasValue(ObjectInverseOf(p), i))`
        // compiles to the class -> property direction (OWL 2 RL's cls-hv1,
        // not cls-hv2): "every A has i as its inverse-p value", i.e.
        // `A(X) => i p X`. Given `a rdf:type A`, `i p a` should be derived.
        let mut ds = dag_rdf::Datastore::new(100);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(named_prop(
            "https://example.org/p",
        )));
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ClassName(class("https://example.org/A")),
            ClassExpression::ObjectHasValue(
                inverse,
                Individual::NamedIndividual(FullIri(IriReference(
                    "https://example.org/i".to_owned(),
                ))),
            ),
        );
        let rules = crate::owl2datalog(&mut ds.resources, &axiom).expect("axiom should translate");

        let a = iri(&mut ds.resources, "https://example.org/a");
        let i_ind = iri(&mut ds.resources, "https://example.org/i");
        let p = iri(&mut ds.resources, "https://example.org/p");
        let rdf_type = iri(&mut ds.resources, RDF_TYPE);
        let cls_a = iri(&mut ds.resources, "https://example.org/A");

        ds.add_triple(dag_rdf::ingress::Triple {
            subject: a,
            predicate: rdf_type,
            obj: cls_a,
        });

        datalog::reasoner::evaluate_rules(rules, &mut ds).expect("materialisation should succeed");

        assert!(
            ds.contains_triple(&dag_rdf::ingress::Triple {
                subject: i_ind,
                predicate: p,
                obj: a,
            }),
            "i p a should be derived via the inverse-in-hasValue rule"
        );
    }

    #[test]
    fn materialise_inverse_in_has_value_negative_control() {
        // Without `a rdf:type A` as a premise, the rule body is unsatisfied
        // and `i p a` must not be derived.
        let mut ds = dag_rdf::Datastore::new(100);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(named_prop(
            "https://example.org/p",
        )));
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ClassName(class("https://example.org/A")),
            ClassExpression::ObjectHasValue(
                inverse,
                Individual::NamedIndividual(FullIri(IriReference(
                    "https://example.org/i".to_owned(),
                ))),
            ),
        );
        let rules = crate::owl2datalog(&mut ds.resources, &axiom).expect("axiom should translate");

        let a = iri(&mut ds.resources, "https://example.org/a");
        let i_ind = iri(&mut ds.resources, "https://example.org/i");
        let p = iri(&mut ds.resources, "https://example.org/p");

        // No `a rdf:type A` fact asserted at all.

        datalog::reasoner::evaluate_rules(rules, &mut ds).expect("materialisation should succeed");

        assert!(
            !ds.contains_triple(&dag_rdf::ingress::Triple {
                subject: i_ind,
                predicate: p,
                obj: a,
            }),
            "i p a must not be derived without a rdf:type A as a premise"
        );
    }

    #[test]
    fn materialise_property_chain_in_has_value_derives_property_edge() {
        // Same class -> property direction as the inverse case above:
        // `SubClassOf(A, ObjectHasValue(ObjectPropertyChain(p1 p2), i))`
        // compiles to body = [A(X), X p1 v0] (the chain's non-final links,
        // introducing intermediate variable v0), head = (v0, p2, i) (the
        // chain's final link, fixed to the individual). Given `a rdf:type A`
        // and `a p1 m`, `m p2 i` should be derived.
        let mut ds = dag_rdf::Datastore::new(100);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ClassName(class("https://example.org/A")),
            ClassExpression::ObjectHasValue(
                chain,
                Individual::NamedIndividual(FullIri(IriReference(
                    "https://example.org/i".to_owned(),
                ))),
            ),
        );
        let rules = crate::owl2datalog(&mut ds.resources, &axiom).expect("axiom should translate");

        let a = iri(&mut ds.resources, "https://example.org/a");
        let m = iri(&mut ds.resources, "https://example.org/m");
        let i_ind = iri(&mut ds.resources, "https://example.org/i");
        let p1 = iri(&mut ds.resources, "https://example.org/p1");
        let p2 = iri(&mut ds.resources, "https://example.org/p2");
        let rdf_type = iri(&mut ds.resources, RDF_TYPE);
        let cls_a = iri(&mut ds.resources, "https://example.org/A");

        ds.add_triple(dag_rdf::ingress::Triple {
            subject: a,
            predicate: rdf_type,
            obj: cls_a,
        });
        ds.add_triple(dag_rdf::ingress::Triple {
            subject: a,
            predicate: p1,
            obj: m,
        });

        datalog::reasoner::evaluate_rules(rules, &mut ds).expect("materialisation should succeed");

        assert!(
            ds.contains_triple(&dag_rdf::ingress::Triple {
                subject: m,
                predicate: p2,
                obj: i_ind,
            }),
            "m p2 i should be derived via the 2-hop property chain in ObjectHasValue"
        );
    }

    #[test]
    fn materialise_property_chain_in_has_value_negative_control() {
        // Without the `a p1 m` premise, the rule body is unsatisfied and
        // `m p2 i` must not be derived even though `a rdf:type A` holds.
        let mut ds = dag_rdf::Datastore::new(100);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let axiom = ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ClassName(class("https://example.org/A")),
            ClassExpression::ObjectHasValue(
                chain,
                Individual::NamedIndividual(FullIri(IriReference(
                    "https://example.org/i".to_owned(),
                ))),
            ),
        );
        let rules = crate::owl2datalog(&mut ds.resources, &axiom).expect("axiom should translate");

        let a = iri(&mut ds.resources, "https://example.org/a");
        let m = iri(&mut ds.resources, "https://example.org/m");
        let i_ind = iri(&mut ds.resources, "https://example.org/i");
        let p2 = iri(&mut ds.resources, "https://example.org/p2");
        let rdf_type = iri(&mut ds.resources, RDF_TYPE);
        let cls_a = iri(&mut ds.resources, "https://example.org/A");

        // Only the class membership holds; the p1 hop is missing.
        ds.add_triple(dag_rdf::ingress::Triple {
            subject: a,
            predicate: rdf_type,
            obj: cls_a,
        });

        datalog::reasoner::evaluate_rules(rules, &mut ds).expect("materialisation should succeed");

        assert!(
            !ds.contains_triple(&dag_rdf::ingress::Triple {
                subject: m,
                predicate: p2,
                obj: i_ind,
            }),
            "m p2 i must not be derived without the a p1 m premise"
        );
    }
}

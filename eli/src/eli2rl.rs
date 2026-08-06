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

fn get_role_value_pattern(
    resources: &mut GraphElementManager,
    role_id: dag_rdf::GraphElementId,
    subject_var: &str,
    individual: &Individual,
) -> dag_rdf::QuadPattern {
    let obj_id = match individual {
        Individual::NamedIndividual(FullIri(iri)) => {
            resources.add_node_resource(RdfResource::Iri(iri.clone()))
        }
        Individual::AnonymousIndividual(anon_id) => {
            resources.get_or_create_named_anon_resource(format!("{}", anon_id))
        }
    };
    get_default_graph_pattern(
        Term::Variable(subject_var.to_owned()),
        Term::Resource(role_id),
        Term::Resource(obj_id),
    )
}

/// Returns `None` if `prop` contains an `ObjectPropertyChain`, which is
/// legal OWL 2 syntax but not yet supported inside an existential
/// (`owl:someValuesFrom`) by the ELI translation. See
/// <https://github.com/daghovland/rdf-datalog/issues/363>.
fn get_obj_prop_pattern(
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
            get_obj_prop_pattern(resources, inner, object_var, subject_var)
        }
        ObjectPropertyExpression::ObjectPropertyChain(_) => {
            log::warn!(
                "Property chain in existential not yet supported; skipping this axiom (issue #363)"
            );
            None
        }
    }
}

/// Returns `None` if `prop` is an `InverseObjectProperty` or an
/// `ObjectPropertyChain`, both legal OWL 2 syntax but not yet supported
/// inside `owl:hasValue` by the ELI translation. See
/// <https://github.com/daghovland/rdf-datalog/issues/363>.
fn get_obj_value_pattern(
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
        ObjectPropertyExpression::InverseObjectProperty(_) => {
            log::warn!(
                "Inverse ObjectHasValue not yet supported; skipping this axiom (issue #363)"
            );
            None
        }
        ObjectPropertyExpression::ObjectPropertyChain(_) => {
            log::warn!(
                "Property chain in ObjectHasValue not yet supported; skipping this axiom (issue #363)"
            );
            None
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
            let role_triple = get_obj_prop_pattern(resources, role, var_name, &new_var)?;
            let concept_triples = translate_eli(resources, inner_concept, &new_var, 1)?;
            Some(
                std::iter::once(role_triple)
                    .chain(concept_triples)
                    .collect(),
            )
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
    let role_atom = RuleAtom::PositivePattern(get_obj_prop_pattern(resources, prop, "X", "Y")?);
    let type_head = get_type_pattern(resources, "Y", concept_name);
    let body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .chain(std::iter::once(role_atom))
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
    let same_as = ObjectPropertyExpression::NamedObjectProperty(FullIri(same_as_iri));
    let p1 = get_obj_prop_pattern(resources, prop, "X", "Y1")?;
    let p2 = get_obj_prop_pattern(resources, prop, "X", "Y2")?;
    let mut body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .collect();
    body.push(RuleAtom::PositivePattern(p1));
    body.push(RuleAtom::PositivePattern(p2));
    Some(vec![Rule {
        head: RuleHead::NormalHead(get_obj_prop_pattern(resources, &same_as, "Y1", "Y2")?),
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
    let role_atom = RuleAtom::PositivePattern(get_obj_prop_pattern(resources, prop, "X", "Y")?);
    let body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .chain(std::iter::once(role_atom))
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
    Some(vec![Rule {
        head: RuleHead::NormalHead(get_obj_value_pattern(resources, prop, "X", individual)?),
        body: sub_conjunction
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    }])
}

/// Translate a single normalized ELI formula into datalog rules.
///
/// Returns `None` if any sub-construct is not yet ELI-translatable (e.g. an
/// `ObjectPropertyChain` inside an existential, or an inverse/chain property
/// inside `owl:hasValue`, see [issue #363](https://github.com/daghovland/rdf-datalog/issues/363)).
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
/// legal-but-unimplemented OWL 2 construct, see
/// [issue #363](https://github.com/daghovland/rdf-datalog/issues/363)) — the
/// whole set of formulas (i.e. the whole originating axiom) is skipped
/// conservatively rather than partially applied.
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
    fn get_obj_prop_pattern_property_chain_returns_none() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let result = get_obj_prop_pattern(&mut resources, &chain, "X", "Y");
        assert!(
            result.is_none(),
            "property chain in existential should be skipped, not translated"
        );
    }

    #[test]
    fn get_obj_prop_pattern_inverse_of_chain_returns_none() {
        // InverseObjectProperty recurses into get_obj_prop_pattern with swapped
        // args; a chain nested inside an inverse must still propagate None.
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
    fn get_obj_value_pattern_inverse_returns_none() {
        let mut resources = GraphElementManager::new(10);
        let inverse = ObjectPropertyExpression::InverseObjectProperty(Box::new(named_prop(
            "https://example.org/p",
        )));
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let result = get_obj_value_pattern(&mut resources, &inverse, "X", &individual);
        assert!(
            result.is_none(),
            "inverse property in ObjectHasValue should be skipped, not translated"
        );
    }

    #[test]
    fn get_obj_value_pattern_property_chain_returns_none() {
        let mut resources = GraphElementManager::new(10);
        let chain = ObjectPropertyExpression::ObjectPropertyChain(vec![
            named_prop("https://example.org/p1"),
            named_prop("https://example.org/p2"),
        ]);
        let individual =
            Individual::NamedIndividual(FullIri(IriReference("https://example.org/i".to_owned())));
        let result = get_obj_value_pattern(&mut resources, &chain, "X", &individual);
        assert!(
            result.is_none(),
            "property chain in ObjectHasValue should be skipped, not translated"
        );
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
    fn translate_eli_some_values_from_with_chain_returns_none() {
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
    fn generate_axiom_rl_object_has_value_with_inverse_returns_none() {
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
    fn generate_tbox_rl_unsupported_construct_skips_axiom() {
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
        let result = generate_tbox_rl(&mut resources, vec![formula]);
        assert!(result.is_none());
    }

    // ── integration: eli::owl2datalog via crate::owl2datalog ────────────

    #[test]
    fn owl2datalog_existential_with_property_chain_does_not_panic() {
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
        // Must not panic; the axiom is skipped entirely (None, contributing
        // zero rules) since property chains inside existentials are not yet
        // ELI-translatable.
        let result = crate::owl2datalog(&mut resources, &axiom);
        assert_eq!(result, None);
    }

    #[test]
    fn owl2datalog_object_has_value_with_inverse_does_not_panic() {
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
        // The axiom is skipped entirely (None), since the ObjectHasValue
        // sub-formula with an inverse property is not yet ELI-translatable;
        // the conservative interpretation skips the whole axiom rather than
        // partially applying it.
        let result = crate::owl2datalog(&mut resources, &axiom);
        assert_eq!(result, None);
    }
}

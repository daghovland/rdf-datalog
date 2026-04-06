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

fn get_obj_prop_pattern(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    subject_var: &str,
    object_var: &str,
) -> dag_rdf::QuadPattern {
    match prop {
        ObjectPropertyExpression::NamedObjectProperty(FullIri(iri)) => {
            let role_id = resources.add_node_resource(RdfResource::Iri(iri.clone()));
            get_role_pattern(role_id, subject_var, object_var)
        }
        ObjectPropertyExpression::AnonymousObjectProperty(id) => {
            let role_id = resources.add_node_resource(RdfResource::AnonymousBlankNode(*id));
            get_role_pattern(role_id, subject_var, object_var)
        }
        ObjectPropertyExpression::InverseObjectProperty(inner) => {
            get_obj_prop_pattern(resources, inner, object_var, subject_var)
        }
        ObjectPropertyExpression::ObjectPropertyChain(_) => {
            panic!("Property chain in existential not yet supported")
        }
    }
}

fn get_obj_value_pattern(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    subject_var: &str,
    individual: &Individual,
) -> dag_rdf::QuadPattern {
    match prop {
        ObjectPropertyExpression::NamedObjectProperty(FullIri(iri)) => {
            let role_id = resources.add_node_resource(RdfResource::Iri(iri.clone()));
            get_role_value_pattern(resources, role_id, subject_var, individual)
        }
        ObjectPropertyExpression::AnonymousObjectProperty(id) => {
            let role_id = resources.add_node_resource(RdfResource::AnonymousBlankNode(*id));
            get_role_value_pattern(resources, role_id, subject_var, individual)
        }
        ObjectPropertyExpression::InverseObjectProperty(_) => {
            panic!("Inverse ObjectHasValue not yet supported")
        }
        ObjectPropertyExpression::ObjectPropertyChain(_) => {
            panic!("Property chain in ObjectHasValue not yet supported")
        }
    }
}

// ── ELI translation (Algorithm 1) ────────────────────────────────────────────

fn translate_eli(
    resources: &mut GraphElementManager,
    concept: &ComplexConcept,
    var_name: &str,
    clause: usize,
) -> Vec<dag_rdf::QuadPattern> {
    match concept {
        ComplexConcept::AtomicConcept(FullIri(iri)) => {
            vec![get_type_pattern(resources, var_name, &FullIri(iri.clone()))]
        }
        ComplexConcept::Intersection(clauses) => clauses
            .iter()
            .enumerate()
            .flat_map(|(i, c)| translate_eli(resources, c, var_name, i + 1))
            .collect(),
        ComplexConcept::SomeValuesFrom(role, inner_concept) => {
            let new_var = format!("{}_{}", var_name, clause);
            let role_triple = get_obj_prop_pattern(resources, role, var_name, &new_var);
            let concept_triples = translate_eli(resources, inner_concept, &new_var, 1);
            std::iter::once(role_triple)
                .chain(concept_triples)
                .collect()
        }
        ComplexConcept::Top => vec![],
    }
}

fn translate_simple_subclass(
    resources: &mut GraphElementManager,
    sub: &ComplexConcept,
    sup: &Class,
) -> Rule {
    Rule {
        head: RuleHead::NormalHead(get_type_pattern(resources, "X", sup)),
        body: translate_eli(resources, sub, "X", 1)
            .into_iter()
            .map(RuleAtom::PositivePattern)
            .collect(),
    }
}

fn translate_empty_intersection(
    resources: &mut GraphElementManager,
    sub_concepts: &[Class],
) -> Rule {
    Rule {
        head: RuleHead::Contradiction,
        body: sub_concepts
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    }
}

fn get_atomic_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    concept_name: &Class,
) -> Vec<Rule> {
    vec![Rule {
        head: RuleHead::NormalHead(get_type_pattern(resources, "X", concept_name)),
        body: sub_conjunction
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    }]
}

fn get_atomic_anonymous_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
) -> Vec<Rule> {
    vec![Rule {
        head: RuleHead::NormalHead(get_anonymous_type_pattern(resources, "X")),
        body: sub_conjunction
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    }]
}

fn get_universal_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    prop: &ObjectPropertyExpression,
    concept_name: &Class,
) -> Vec<Rule> {
    let role_atom = RuleAtom::PositivePattern(get_obj_prop_pattern(resources, prop, "X", "Y"));
    let type_head = get_type_pattern(resources, "Y", concept_name);
    let body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .chain(std::iter::once(role_atom))
        .collect();
    vec![Rule {
        head: RuleHead::NormalHead(type_head),
        body,
    }]
}

fn get_at_most_one_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    prop: &ObjectPropertyExpression,
) -> Vec<Rule> {
    let same_as_iri = IriReference(OWL_SAME_AS.to_owned());
    let same_as = ObjectPropertyExpression::NamedObjectProperty(FullIri(same_as_iri));
    let not_same_as = RuleAtom::NotPattern(get_obj_prop_pattern(resources, &same_as, "Y1", "Y2"));
    let p1 = get_obj_prop_pattern(resources, prop, "X", "Y1");
    let p2 = get_obj_prop_pattern(resources, prop, "X", "Y2");
    let mut body: Vec<RuleAtom> = sub_conjunction
        .iter()
        .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
        .collect();
    body.push(RuleAtom::PositivePattern(p1));
    body.push(RuleAtom::PositivePattern(p2));
    body.push(not_same_as);
    vec![Rule {
        head: RuleHead::NormalHead(get_obj_prop_pattern(resources, &same_as, "Y1", "Y2")),
        body,
    }]
}

fn get_object_has_value_normalized_rule(
    resources: &mut GraphElementManager,
    sub_conjunction: &[Class],
    prop: &ObjectPropertyExpression,
    individual: &Individual,
) -> Vec<Rule> {
    vec![Rule {
        head: RuleHead::NormalHead(get_obj_value_pattern(resources, prop, "X", individual)),
        body: sub_conjunction
            .iter()
            .map(|cls| RuleAtom::PositivePattern(get_type_pattern(resources, "X", cls)))
            .collect(),
    }]
}

fn generate_axiom_rl(resources: &mut GraphElementManager, formula: &Formula) -> Vec<Rule> {
    match formula {
        Formula::DirectlyTranslatableConceptInclusion {
            subclass_disjunction,
            superclass_conjunction,
        } => subclass_disjunction
            .iter()
            .flat_map(|sub| {
                superclass_conjunction
                    .iter()
                    .map(|sup| translate_simple_subclass(resources, sub, sup))
                    .collect::<Vec<_>>()
            })
            .collect(),
        Formula::NormalizedConceptInclusion {
            subclass_conjunction,
            superclass,
        } => match superclass {
            NormalizedConcept::Bottom => vec![translate_empty_intersection(
                resources,
                subclass_conjunction,
            )],
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
        },
    }
}

/// Translate a list of ELI formulas into datalog rules.
pub fn generate_tbox_rl(
    resources: &mut GraphElementManager,
    formulas: impl IntoIterator<Item = Formula>,
) -> Vec<Rule> {
    formulas
        .into_iter()
        .flat_map(|f| generate_axiom_rl(resources, &f))
        .collect()
}

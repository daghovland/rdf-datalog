/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Extraction and normalization of ELI axioms from OWL 2 class axioms.
//!
//! Normalization follows Section 4.2 of <https://arxiv.org/pdf/2008.02232> and
//! the structural transformation in <https://www.ijcai.org/Proceedings/09/Papers/336.pdf>.

use ingress::IriReference;
use owl_ontology::{Class, ClassAxiom, ClassExpression, FullIri, ObjectPropertyExpression};
use crate::axioms::{ComplexConcept, Formula, NormalizedConcept};

/// Build a synthetic IRI to represent a complex concept (A_C in the paper).
fn concept_representative(concept: &ClassExpression) -> Class {
    let hash = format!("{:?}", concept).len(); // stable enough for a representative IRI
    FullIri(IriReference(format!(
        "https://github.org/daghovland/DagSemTools/ConceptRepresentative/{}",
        hash
    )))
}

/// Try to extract a `ComplexConcept` from a class expression in sub-concept position.
fn eli_class_extractor(expr: &ClassExpression) -> Option<ComplexConcept> {
    match expr {
        ClassExpression::ClassName(cls) => Some(ComplexConcept::AtomicConcept(cls.clone())),
        ClassExpression::ObjectIntersectionOf(classes) => {
            let parts: Option<Vec<ComplexConcept>> =
                classes.iter().map(eli_class_extractor).collect();
            parts.map(ComplexConcept::Intersection)
        }
        ClassExpression::ObjectSomeValuesFrom(role, cls) => eli_class_extractor(cls)
            .map(|c| ComplexConcept::SomeValuesFrom(role.clone(), Box::new(c))),
        ClassExpression::ObjectMinQualifiedCardinality(card, role, cls) if *card == 1u32.into() => {
            eli_class_extractor(cls)
                .map(|c| ComplexConcept::SomeValuesFrom(role.clone(), Box::new(c)))
        }
        _ => None,
    }
}

/// Extract ELI sub-concepts from a union (may produce multiple items).
fn eli_sub_class_extractor(expr: &ClassExpression) -> Vec<Option<ComplexConcept>> {
    match expr {
        ClassExpression::ObjectUnionOf(exprs) => {
            exprs.iter().flat_map(eli_sub_class_extractor).collect()
        }
        e => vec![eli_class_extractor(e)],
    }
}

/// Try to extract atomic super-concepts from a class expression.
fn eli_super_class_extractor(expr: &ClassExpression) -> Option<Vec<Class>> {
    match expr {
        ClassExpression::ObjectIntersectionOf(exprs) => {
            let parts: Option<Vec<Vec<Class>>> =
                exprs.iter().map(eli_super_class_extractor).collect();
            parts.map(|v| v.into_iter().flatten().collect())
        }
        ClassExpression::ClassName(cls) => Some(vec![cls.clone()]),
        _ => None,
    }
}

fn flatten_option_list<T>(opts: Vec<Option<T>>) -> Option<Vec<T>> {
    opts.into_iter().collect()
}

// ── Normalization ─────────────────────────────────────────────────────────────

fn sub_concept_some_values_from(
    obj_prop: &ObjectPropertyExpression,
    cls: &ClassExpression,
    concept: &ClassExpression,
) -> (Vec<ClassExpression>, Vec<ClassExpression>, Vec<Formula>) {
    let repr = concept_representative(concept);
    (
        vec![cls.clone()],
        vec![],
        vec![Formula::NormalizedConceptInclusion {
            subclass_conjunction: vec![concept_representative(cls)],
            superclass: NormalizedConcept::AllValuesFrom(
                ObjectPropertyExpression::InverseObjectProperty(Box::new(obj_prop.clone())),
                repr,
            ),
        }],
    )
}

fn concept_positive_occurrence_normalization(
    concept: &ClassExpression,
) -> Vec<Formula> {
    let (pos, neg, mut formulas) = match concept {
        ClassExpression::ObjectComplementOf(inner) => (
            vec![],
            vec![inner.as_ref().clone()],
            vec![Formula::NormalizedConceptInclusion {
                subclass_conjunction: vec![
                    concept_representative(concept),
                    concept_representative(inner),
                ],
                superclass: NormalizedConcept::Bottom,
            }],
        ),
        ClassExpression::ObjectIntersectionOf(exprs) => (
            exprs.clone(),
            vec![],
            exprs
                .iter()
                .map(|e| Formula::NormalizedConceptInclusion {
                    subclass_conjunction: vec![concept_representative(concept)],
                    superclass: NormalizedConcept::AtomicNamedConcept(concept_representative(e)),
                })
                .collect(),
        ),
        ClassExpression::ObjectHasValue(prop, individual) => (
            vec![],
            vec![],
            vec![Formula::NormalizedConceptInclusion {
                subclass_conjunction: vec![concept_representative(concept)],
                superclass: NormalizedConcept::ObjectHasValue(prop.clone(), individual.clone()),
            }],
        ),
        _ => {
            let repr = concept_representative(concept);
            let (pos, neg, super_concepts): (Vec<ClassExpression>, Vec<ClassExpression>, Vec<NormalizedConcept>) =
                match concept {
                    ClassExpression::ClassName(cls) => (vec![], vec![], vec![NormalizedConcept::AtomicNamedConcept(cls.clone())]),
                    ClassExpression::AnonymousClass(_) => (vec![], vec![], vec![NormalizedConcept::AtomicAnonymousConcept]),
                    ClassExpression::ObjectUnionOf(_) => {
                        log::warn!("Invalid OWL 2 RL: Union in superclass position not allowed");
                        (vec![], vec![], vec![])
                    }
                    ClassExpression::ObjectAllValuesFrom(prop, inner) => (
                        vec![],
                        vec![inner.as_ref().clone()],
                        vec![NormalizedConcept::AllValuesFrom(prop.clone(), concept_representative(inner))],
                    ),
                    ClassExpression::ObjectMaxCardinality(card, prop) if *card == 0u32.into() => {
                        (vec![], vec![], vec![NormalizedConcept::Bottom])
                    }
                    ClassExpression::ObjectMaxCardinality(card, prop) if *card == 1u32.into() => {
                        (vec![], vec![], vec![NormalizedConcept::AtMostOneValueFrom(prop.clone())])
                    }
                    ClassExpression::ObjectMaxCardinality(card, _) => {
                        log::warn!("Invalid OWL 2 RL: ObjectMaxCardinality on superConcept only allowed with cardinality 0 or 1");
                        (vec![], vec![], vec![])
                    }
                    _ => {
                        log::warn!("Unhandled super-concept expression: {:?}", concept);
                        (vec![], vec![], vec![])
                    }
                };
            let formulas = super_concepts
                .into_iter()
                .map(|sc| Formula::NormalizedConceptInclusion {
                    subclass_conjunction: vec![repr.clone()],
                    superclass: sc,
                })
                .collect();
            (pos, neg, formulas)
        }
    };

    for p in &pos { formulas.extend(concept_positive_occurrence_normalization(p)); }
    for n in &neg { formulas.extend(concept_negative_occurrence_normalization(n)); }
    formulas
}

fn concept_negative_occurrence_normalization(concept: &ClassExpression) -> Vec<Formula> {
    let repr = concept_representative(concept);
    let main_repr = NormalizedConcept::AtomicNamedConcept(repr.clone());

    let (pos, neg, mut formulas) = match concept {
        ClassExpression::ObjectUnionOf(exprs) => (
            vec![],
            exprs.clone(),
            exprs
                .iter()
                .map(|e| Formula::NormalizedConceptInclusion {
                    subclass_conjunction: vec![concept_representative(e)],
                    superclass: main_repr.clone(),
                })
                .collect(),
        ),
        ClassExpression::ObjectSomeValuesFrom(prop, inner) => {
            sub_concept_some_values_from(prop, inner, concept)
        }
        ClassExpression::ObjectMinQualifiedCardinality(card, prop, inner) if *card == 1u32.into() => {
            sub_concept_some_values_from(prop, inner, concept)
        }
        ClassExpression::ObjectMinQualifiedCardinality(_, _, _) => {
            log::warn!("Invalid OWL 2 RL: ObjectMinQualifiedCardinality only allowed with cardinality 1");
            (vec![], vec![], vec![])
        }
        ClassExpression::ObjectComplementOf(inner) => (
            vec![inner.as_ref().clone()],
            vec![],
            vec![Formula::NormalizedConceptInclusion {
                subclass_conjunction: vec![concept_representative(concept), concept_representative(inner)],
                superclass: NormalizedConcept::Bottom,
            }],
        ),
        ClassExpression::ObjectHasValue(_, _) => {
            log::error!("objectHasValue in negative position not yet implemented");
            (vec![], vec![], vec![])
        }
        _ => {
            let (pos, neg, sub_conjunctions): (_, _, Vec<Vec<Class>>) = match concept {
                ClassExpression::ClassName(cls) => (vec![], vec![], vec![vec![cls.clone()]]),
                ClassExpression::AnonymousClass(_) => (vec![], vec![], vec![vec![repr.clone()]]),
                ClassExpression::ObjectIntersectionOf(exprs) => (
                    vec![],
                    exprs.clone(),
                    vec![exprs.iter().map(concept_representative).collect()],
                ),
                ClassExpression::ObjectAllValuesFrom(_, _) => {
                    log::warn!("ObjectAllValuesFrom not allowed on subconcept in OWL 2 RL");
                    (vec![], vec![], vec![])
                }
                _ => {
                    log::warn!("Unhandled sub-concept expression: {:?}", concept);
                    (vec![], vec![], vec![])
                }
            };
            let fmls = sub_conjunctions
                .into_iter()
                .map(|sub| Formula::NormalizedConceptInclusion {
                    subclass_conjunction: sub,
                    superclass: NormalizedConcept::AtomicNamedConcept(repr.clone()),
                })
                .collect();
            (pos, neg, fmls)
        }
    };

    for p in &pos { formulas.extend(concept_positive_occurrence_normalization(p)); }
    for n in &neg { formulas.extend(concept_negative_occurrence_normalization(n)); }
    formulas
}

fn sub_class_axiom_normalization(axiom: &ClassAxiom) -> Vec<Formula> {
    match axiom {
        ClassAxiom::SubClassOf(_, sub, sup) => {
            let mut result = vec![Formula::NormalizedConceptInclusion {
                subclass_conjunction: vec![concept_representative(sub)],
                superclass: NormalizedConcept::AtomicNamedConcept(concept_representative(sup)),
            }];
            result.extend(concept_positive_occurrence_normalization(sup));
            result.extend(concept_negative_occurrence_normalization(sub));
            result
        }
        ClassAxiom::DisjointClasses(_, _) => todo!("DisjointClasses normalization"),
        ClassAxiom::EquivalentClasses(_, _) => todo!("EquivalentClasses normalization"),
        ClassAxiom::DisjointUnion(_, _, exprs) => {
            log::warn!("Invalid OWL 2 RL: DisjointUnion not allowed");
            vec![]
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Extract ELI formulas from a `ClassAxiom`, or `None` if the axiom is not
/// an ELI-expressible class axiom.
pub fn eli_axiom_extractor(axiom: &ClassAxiom) -> Option<Vec<Formula>> {
    match axiom {
        ClassAxiom::SubClassOf(_, sub, sup) => {
            let sub_opts = eli_sub_class_extractor(sub);
            let sub_expr = flatten_option_list(sub_opts);
            let super_expr = eli_super_class_extractor(sup);
            match (sub_expr, super_expr) {
                (Some(sub_e), Some(sup_e)) => Some(vec![Formula::DirectlyTranslatableConceptInclusion {
                    subclass_disjunction: sub_e,
                    superclass_conjunction: sup_e,
                }]),
                _ => Some(sub_class_axiom_normalization(axiom)),
            }
        }
        ClassAxiom::EquivalentClasses(_, classes) => {
            // A ≡ B ↔ A ⊑ B ∧ B ⊑ A
            let pairs: Vec<Formula> = classes
                .iter()
                .flat_map(|a| {
                    classes.iter().filter(move |b| *b != a).filter_map(move |b| {
                        let axiom = ClassAxiom::SubClassOf(vec![], a.clone(), b.clone());
                        eli_axiom_extractor(&axiom)
                    })
                })
                .flatten()
                .collect();
            if pairs.is_empty() { None } else { Some(pairs) }
        }
        _ => None,
    }
}

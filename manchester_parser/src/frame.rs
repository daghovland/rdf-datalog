/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Entity frames (`Class:`, `ObjectProperty:`, `DataProperty:`, `Individual:`,
//! `AnnotationProperty:`) and top-level `misc` axioms. Each frame parser
//! returns `Vec<Axiom>` — one frame typically expands to several axioms (one
//! per section-list item), per `docs/plans/MANCHESTER_SYNTAX_PLAN.md`.
//!
//! `DisjointUnionOf:` and `HasKey:` (class frame), `SubPropertyChain:`
//! (object property frame) are deferred; see
//! [#157](https://github.com/daghovland/rdf-datalog/issues/157).

use crate::annotation::{annotations_section, opt_leading_annotations};
use crate::class_expr::description;
use crate::data_range::data_range;
use crate::individual::individual;
use crate::iri::{ParserContext, iri};
use crate::literal::literal;
use crate::property_expr::object_property_expression;
use crate::tokens::{keyword, punct};
use nom::IResult;
use nom::branch::alt;
use nom::multi::{many0, separated_list1};
use owl_ontology::{
    Annotation, Assertion, ClassAxiom, ClassExpression, DataPropertyAxiom, DataRange, Entity,
    FullIri, Individual, ObjectPropertyAxiom, ObjectPropertyExpression,
};

type Axiom = owl_ontology::Axiom;

/// `X { ',' X }` where each element may have a leading `Annotations: ...`.
fn annotated_list<'a, T, P>(
    ctx: &'a ParserContext,
    mut item: P,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<(Vec<Annotation>, T)>>
where
    P: FnMut(&'a str) -> IResult<&'a str, T> + 'a,
{
    move |input: &'a str| {
        separated_list1(punct(','), |i: &'a str| {
            let (i, anns) = opt_leading_annotations(ctx)(i)?;
            let (i, val) = item(i)?;
            Ok((i, (anns, val)))
        })(input)
    }
}

// ── Class: frame ─────────────────────────────────────────────────────────

enum ClassSection {
    Annotations(Vec<Annotation>),
    SubClassOf(Vec<(Vec<Annotation>, ClassExpression)>),
    EquivalentTo(Vec<(Vec<Annotation>, ClassExpression)>),
    DisjointWith(Vec<(Vec<Annotation>, ClassExpression)>),
}

fn class_section<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassSection> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(annotations_section(ctx), ClassSection::Annotations),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("SubClassOf:"),
                    annotated_list(ctx, description(ctx)),
                ),
                ClassSection::SubClassOf,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("EquivalentTo:"),
                    annotated_list(ctx, description(ctx)),
                ),
                ClassSection::EquivalentTo,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("DisjointWith:"),
                    annotated_list(ctx, description(ctx)),
                ),
                ClassSection::DisjointWith,
            ),
        ))(input)
    }
}

pub(crate) fn class_frame<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Axiom>> {
    move |input: &'a str| {
        let (input, _) = keyword("Class:")(input)?;
        let (input, class_iri) = iri(ctx)(input)?;
        let (input, sections) = many0(class_section(ctx))(input)?;

        let mut decl_annotations = Vec::new();
        let mut axioms = Vec::new();
        for section in sections {
            match section {
                ClassSection::Annotations(anns) => decl_annotations.extend(anns),
                ClassSection::SubClassOf(list) => {
                    for (anns, expr) in list {
                        axioms.push(Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                            anns,
                            ClassExpression::ClassName(class_iri.clone()),
                            expr,
                        )));
                    }
                }
                ClassSection::EquivalentTo(list) => {
                    for (anns, expr) in list {
                        axioms.push(Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                            anns,
                            vec![ClassExpression::ClassName(class_iri.clone()), expr],
                        )));
                    }
                }
                ClassSection::DisjointWith(list) => {
                    for (anns, expr) in list {
                        axioms.push(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                            anns,
                            vec![ClassExpression::ClassName(class_iri.clone()), expr],
                        )));
                    }
                }
            }
        }
        axioms.insert(
            0,
            Axiom::AxiomDeclaration((decl_annotations, Entity::ClassDeclaration(class_iri))),
        );
        Ok((input, axioms))
    }
}

// ── ObjectProperty: frame ────────────────────────────────────────────────

fn object_property_characteristic<'a>(
    ctx: &'a ParserContext,
    prop: ObjectPropertyExpression,
) -> impl FnMut(&'a str) -> IResult<&'a str, ObjectPropertyAxiom> + 'a {
    move |input: &'a str| {
        let (input, anns) = opt_leading_annotations(ctx)(input)?;
        alt((
            nom::combinator::map(keyword("InverseFunctional"), {
                let prop = prop.clone();
                let anns = anns.clone();
                move |_| {
                    ObjectPropertyAxiom::InverseFunctionalObjectProperty(anns.clone(), prop.clone())
                }
            }),
            nom::combinator::map(keyword("Functional"), {
                let prop = prop.clone();
                let anns = anns.clone();
                move |_| ObjectPropertyAxiom::FunctionalObjectProperty(anns.clone(), prop.clone())
            }),
            nom::combinator::map(keyword("Transitive"), {
                let prop = prop.clone();
                let anns = anns.clone();
                move |_| ObjectPropertyAxiom::TransitiveObjectProperty(anns.clone(), prop.clone())
            }),
            nom::combinator::map(keyword("Symmetric"), {
                let prop = prop.clone();
                let anns = anns.clone();
                move |_| ObjectPropertyAxiom::SymmetricObjectProperty(anns.clone(), prop.clone())
            }),
            nom::combinator::map(keyword("Asymmetric"), {
                let prop = prop.clone();
                let anns = anns.clone();
                move |_| ObjectPropertyAxiom::AsymmetricObjectProperty(anns.clone(), prop.clone())
            }),
            nom::combinator::map(keyword("Reflexive"), {
                let prop = prop.clone();
                let anns = anns.clone();
                move |_| ObjectPropertyAxiom::ReflexiveObjectProperty(anns.clone(), prop.clone())
            }),
            nom::combinator::map(keyword("Irreflexive"), {
                let prop = prop.clone();
                move |_| ObjectPropertyAxiom::IrreflexiveObjectProperty(anns.clone(), prop.clone())
            }),
        ))(input)
    }
}

enum ObjectPropertySection {
    Annotations(Vec<Annotation>),
    Domain(Vec<(Vec<Annotation>, ClassExpression)>),
    Range(Vec<(Vec<Annotation>, ClassExpression)>),
    Characteristics(Vec<ObjectPropertyAxiom>),
    SubPropertyOf(Vec<(Vec<Annotation>, ObjectPropertyExpression)>),
    EquivalentTo(Vec<(Vec<Annotation>, ObjectPropertyExpression)>),
    DisjointWith(Vec<(Vec<Annotation>, ObjectPropertyExpression)>),
    InverseOf(Vec<(Vec<Annotation>, ObjectPropertyExpression)>),
}

fn object_property_section<'a>(
    ctx: &'a ParserContext,
    self_prop: ObjectPropertyExpression,
) -> impl FnMut(&'a str) -> IResult<&'a str, ObjectPropertySection> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(annotations_section(ctx), ObjectPropertySection::Annotations),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Domain:"), annotated_list(ctx, description(ctx))),
                ObjectPropertySection::Domain,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Range:"), annotated_list(ctx, description(ctx))),
                ObjectPropertySection::Range,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("Characteristics:"),
                    separated_list1(
                        punct(','),
                        object_property_characteristic(ctx, self_prop.clone()),
                    ),
                ),
                ObjectPropertySection::Characteristics,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("SubPropertyOf:"),
                    annotated_list(ctx, object_property_expression(ctx)),
                ),
                ObjectPropertySection::SubPropertyOf,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("EquivalentTo:"),
                    annotated_list(ctx, object_property_expression(ctx)),
                ),
                ObjectPropertySection::EquivalentTo,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("DisjointWith:"),
                    annotated_list(ctx, object_property_expression(ctx)),
                ),
                ObjectPropertySection::DisjointWith,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("InverseOf:"),
                    annotated_list(ctx, object_property_expression(ctx)),
                ),
                ObjectPropertySection::InverseOf,
            ),
        ))(input)
    }
}

pub(crate) fn object_property_frame<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Axiom>> {
    move |input: &'a str| {
        let (input, _) = keyword("ObjectProperty:")(input)?;
        let (input, prop_iri) = iri(ctx)(input)?;
        let self_prop = ObjectPropertyExpression::NamedObjectProperty(prop_iri.clone());
        let (input, sections) = many0(object_property_section(ctx, self_prop.clone()))(input)?;

        let mut decl_annotations = Vec::new();
        let mut axioms = Vec::new();
        for section in sections {
            match section {
                ObjectPropertySection::Annotations(anns) => decl_annotations.extend(anns),
                ObjectPropertySection::Domain(list) => {
                    for (_anns, expr) in list {
                        // ObjectPropertyDomain carries no annotation slot in
                        // the target model; per-item Annotations: are parsed
                        // (for forward-compat) but discarded.
                        axioms.push(Axiom::AxiomObjectPropertyAxiom(
                            ObjectPropertyAxiom::ObjectPropertyDomain(self_prop.clone(), expr),
                        ));
                    }
                }
                ObjectPropertySection::Range(list) => {
                    for (_anns, expr) in list {
                        axioms.push(Axiom::AxiomObjectPropertyAxiom(
                            ObjectPropertyAxiom::ObjectPropertyRange(self_prop.clone(), expr),
                        ));
                    }
                }
                ObjectPropertySection::Characteristics(cs) => {
                    for c in cs {
                        axioms.push(Axiom::AxiomObjectPropertyAxiom(c));
                    }
                }
                ObjectPropertySection::SubPropertyOf(list) => {
                    for (anns, super_prop) in list {
                        axioms.push(Axiom::AxiomObjectPropertyAxiom(
                            ObjectPropertyAxiom::SubObjectPropertyOf(
                                anns,
                                owl_ontology::SubPropertyExpression::SubObjectPropertyExpression(
                                    self_prop.clone(),
                                ),
                                super_prop,
                            ),
                        ));
                    }
                }
                ObjectPropertySection::EquivalentTo(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomObjectPropertyAxiom(
                            ObjectPropertyAxiom::EquivalentObjectProperties(
                                anns,
                                vec![self_prop.clone(), other],
                            ),
                        ));
                    }
                }
                ObjectPropertySection::DisjointWith(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomObjectPropertyAxiom(
                            ObjectPropertyAxiom::DisjointObjectProperties(
                                anns,
                                vec![self_prop.clone(), other],
                            ),
                        ));
                    }
                }
                ObjectPropertySection::InverseOf(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomObjectPropertyAxiom(
                            ObjectPropertyAxiom::InverseObjectProperties(
                                anns,
                                self_prop.clone(),
                                other,
                            ),
                        ));
                    }
                }
            }
        }
        axioms.insert(
            0,
            Axiom::AxiomDeclaration((
                decl_annotations,
                Entity::ObjectPropertyDeclaration(prop_iri),
            )),
        );
        Ok((input, axioms))
    }
}

// ── DataProperty: frame ──────────────────────────────────────────────────

enum DataPropertySection {
    Annotations(Vec<Annotation>),
    Domain(Vec<(Vec<Annotation>, ClassExpression)>),
    Range(Vec<(Vec<Annotation>, DataRange)>),
    Functional(Vec<Annotation>),
    SubPropertyOf(Vec<(Vec<Annotation>, FullIri)>),
    EquivalentTo(Vec<(Vec<Annotation>, FullIri)>),
    DisjointWith(Vec<(Vec<Annotation>, FullIri)>),
}

fn data_property_section<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataPropertySection> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(annotations_section(ctx), DataPropertySection::Annotations),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Domain:"), annotated_list(ctx, description(ctx))),
                DataPropertySection::Domain,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Range:"), annotated_list(ctx, data_range(ctx))),
                DataPropertySection::Range,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("Characteristics:"),
                    nom::sequence::preceded(opt_leading_annotations(ctx), keyword("Functional")),
                ),
                |_| DataPropertySection::Functional(Vec::new()),
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("SubPropertyOf:"), annotated_list(ctx, iri(ctx))),
                DataPropertySection::SubPropertyOf,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("EquivalentTo:"), annotated_list(ctx, iri(ctx))),
                DataPropertySection::EquivalentTo,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("DisjointWith:"), annotated_list(ctx, iri(ctx))),
                DataPropertySection::DisjointWith,
            ),
        ))(input)
    }
}

pub(crate) fn data_property_frame<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Axiom>> {
    move |input: &'a str| {
        let (input, _) = keyword("DataProperty:")(input)?;
        let (input, prop_iri) = iri(ctx)(input)?;
        let (input, sections) = many0(data_property_section(ctx))(input)?;

        let mut decl_annotations = Vec::new();
        let mut axioms = Vec::new();
        for section in sections {
            match section {
                DataPropertySection::Annotations(anns) => decl_annotations.extend(anns),
                DataPropertySection::Domain(list) => {
                    for (anns, expr) in list {
                        axioms.push(Axiom::AxiomDataPropertyAxiom(
                            DataPropertyAxiom::DataPropertyDomain(anns, prop_iri.clone(), expr),
                        ));
                    }
                }
                DataPropertySection::Range(list) => {
                    for (anns, dr) in list {
                        axioms.push(Axiom::AxiomDataPropertyAxiom(
                            DataPropertyAxiom::DataPropertyRange(anns, prop_iri.clone(), dr),
                        ));
                    }
                }
                DataPropertySection::Functional(anns) => {
                    axioms.push(Axiom::AxiomDataPropertyAxiom(
                        DataPropertyAxiom::FunctionalDataProperty(anns, prop_iri.clone()),
                    ));
                }
                DataPropertySection::SubPropertyOf(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomDataPropertyAxiom(
                            DataPropertyAxiom::SubDataPropertyOf(anns, prop_iri.clone(), other),
                        ));
                    }
                }
                DataPropertySection::EquivalentTo(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomDataPropertyAxiom(
                            DataPropertyAxiom::EquivalentDataProperties(
                                anns,
                                vec![prop_iri.clone(), other],
                            ),
                        ));
                    }
                }
                DataPropertySection::DisjointWith(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomDataPropertyAxiom(
                            DataPropertyAxiom::DisjointDataProperties(
                                anns,
                                vec![prop_iri.clone(), other],
                            ),
                        ));
                    }
                }
            }
        }
        axioms.insert(
            0,
            Axiom::AxiomDeclaration((decl_annotations, Entity::DataPropertyDeclaration(prop_iri))),
        );
        Ok((input, axioms))
    }
}

// ── Individual: frame ────────────────────────────────────────────────────

enum IndividualSection {
    Annotations(Vec<Annotation>),
    Types(Vec<(Vec<Annotation>, ClassExpression)>),
    Facts(Vec<(Vec<Annotation>, bool, FullIri, FactTarget)>),
    SameAs(Vec<(Vec<Annotation>, Individual)>),
    DifferentFrom(Vec<(Vec<Annotation>, Individual)>),
}

enum FactTarget {
    Obj(Individual),
    Data(ingress::GraphElement),
}

fn fact<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, (bool, FullIri, FactTarget)> {
    move |input: &'a str| {
        let (input, negated) = nom::combinator::opt(keyword("not"))(input)?;
        let (input, prop) = iri(ctx)(input)?;
        let (rest_ws, ()) = crate::tokens::sp(input)?;
        if crate::class_expr::literal_follows(rest_ws) {
            let (input, lit) = literal(ctx)(input)?;
            Ok((input, (negated.is_some(), prop, FactTarget::Data(lit))))
        } else {
            let (input, ind) = individual(ctx)(input)?;
            Ok((input, (negated.is_some(), prop, FactTarget::Obj(ind))))
        }
    }
}

fn individual_section<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, IndividualSection> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(annotations_section(ctx), IndividualSection::Annotations),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Types:"), annotated_list(ctx, description(ctx))),
                IndividualSection::Types,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Facts:"), annotated_list(ctx, fact(ctx))),
                |list: Vec<(Vec<Annotation>, (bool, FullIri, FactTarget))>| {
                    IndividualSection::Facts(
                        list.into_iter()
                            .map(|(anns, (neg, prop, target))| (anns, neg, prop, target))
                            .collect(),
                    )
                },
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("SameAs:"), annotated_list(ctx, individual(ctx))),
                IndividualSection::SameAs,
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("DifferentFrom:"),
                    annotated_list(ctx, individual(ctx)),
                ),
                IndividualSection::DifferentFrom,
            ),
        ))(input)
    }
}

pub(crate) fn individual_frame<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Axiom>> {
    move |input: &'a str| {
        let (input, _) = keyword("Individual:")(input)?;
        let (input, ind) = individual(ctx)(input)?;
        let (input, sections) = many0(individual_section(ctx))(input)?;

        let mut decl_annotations = Vec::new();
        let mut axioms = Vec::new();
        for section in sections {
            match section {
                IndividualSection::Annotations(anns) => decl_annotations.extend(anns),
                IndividualSection::Types(list) => {
                    for (anns, expr) in list {
                        axioms.push(Axiom::AxiomAssertion(Assertion::ClassAssertion(
                            anns,
                            expr,
                            ind.clone(),
                        )));
                    }
                }
                IndividualSection::Facts(list) => {
                    for (anns, negated, prop, target) in list {
                        match target {
                            FactTarget::Obj(other) => {
                                let prop_expr = ObjectPropertyExpression::NamedObjectProperty(prop);
                                axioms.push(Axiom::AxiomAssertion(if negated {
                                    Assertion::NegativeObjectPropertyAssertion(
                                        anns,
                                        prop_expr,
                                        ind.clone(),
                                        other,
                                    )
                                } else {
                                    Assertion::ObjectPropertyAssertion(
                                        anns,
                                        prop_expr,
                                        ind.clone(),
                                        other,
                                    )
                                }));
                            }
                            FactTarget::Data(lit) => {
                                axioms.push(Axiom::AxiomAssertion(if negated {
                                    Assertion::NegativeDataPropertyAssertion(
                                        anns,
                                        prop,
                                        ind.clone(),
                                        lit,
                                    )
                                } else {
                                    Assertion::DataPropertyAssertion(anns, prop, ind.clone(), lit)
                                }));
                            }
                        }
                    }
                }
                IndividualSection::SameAs(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomAssertion(Assertion::SameIndividual(
                            anns,
                            vec![ind.clone(), other],
                        )));
                    }
                }
                IndividualSection::DifferentFrom(list) => {
                    for (anns, other) in list {
                        axioms.push(Axiom::AxiomAssertion(Assertion::DifferentIndividuals(
                            anns,
                            vec![ind.clone(), other],
                        )));
                    }
                }
            }
        }
        axioms.insert(
            0,
            Axiom::AxiomDeclaration((decl_annotations, Entity::NamedIndividualDeclaration(ind))),
        );
        Ok((input, axioms))
    }
}

// ── AnnotationProperty: frame ────────────────────────────────────────────

enum AnnotationPropertySection {
    Annotations(Vec<Annotation>),
    Domain(Vec<(Vec<Annotation>, FullIri)>),
    Range(Vec<(Vec<Annotation>, FullIri)>),
    SubPropertyOf(Vec<(Vec<Annotation>, FullIri)>),
}

fn annotation_property_section<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, AnnotationPropertySection> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                annotations_section(ctx),
                AnnotationPropertySection::Annotations,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Domain:"), annotated_list(ctx, iri(ctx))),
                AnnotationPropertySection::Domain,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("Range:"), annotated_list(ctx, iri(ctx))),
                AnnotationPropertySection::Range,
            ),
            nom::combinator::map(
                nom::sequence::preceded(keyword("SubPropertyOf:"), annotated_list(ctx, iri(ctx))),
                AnnotationPropertySection::SubPropertyOf,
            ),
        ))(input)
    }
}

pub(crate) fn annotation_property_frame<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Axiom>> {
    move |input: &'a str| {
        let (input, _) = keyword("AnnotationProperty:")(input)?;
        let (input, prop_iri) = iri(ctx)(input)?;
        let (input, sections) = many0(annotation_property_section(ctx))(input)?;

        let mut decl_annotations = Vec::new();
        let mut axioms = Vec::new();
        for section in sections {
            match section {
                AnnotationPropertySection::Annotations(anns) => decl_annotations.extend(anns),
                AnnotationPropertySection::Domain(list) => {
                    for (anns, target) in list {
                        axioms.push(Axiom::AxiomAnnotationAxiom(
                            owl_ontology::AnnotationAxiom::AnnotationPropertyDomain(
                                anns,
                                prop_iri.clone(),
                                target,
                            ),
                        ));
                    }
                }
                AnnotationPropertySection::Range(list) => {
                    for (anns, target) in list {
                        axioms.push(Axiom::AxiomAnnotationAxiom(
                            owl_ontology::AnnotationAxiom::AnnotationPropertyRange(
                                anns,
                                prop_iri.clone(),
                                target,
                            ),
                        ));
                    }
                }
                AnnotationPropertySection::SubPropertyOf(list) => {
                    for (anns, super_prop) in list {
                        axioms.push(Axiom::AxiomAnnotationAxiom(
                            owl_ontology::AnnotationAxiom::SubAnnotationPropertyOf(
                                anns,
                                prop_iri.clone(),
                                super_prop,
                            ),
                        ));
                    }
                }
            }
        }
        axioms.insert(
            0,
            Axiom::AxiomDeclaration((
                decl_annotations,
                Entity::AnnotationPropertyDeclaration(prop_iri),
            )),
        );
        Ok((input, axioms))
    }
}

// ── Top-level misc ───────────────────────────────────────────────────────

/// `misc` — top-level axioms not tied to any single entity frame.
pub(crate) fn misc<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("EquivalentClasses:"),
                    nom::sequence::pair(
                        opt_annotations(ctx),
                        separated_list1(punct(','), description(ctx)),
                    ),
                ),
                |(anns, list)| Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(anns, list)),
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("DisjointClasses:"),
                    nom::sequence::pair(
                        opt_annotations(ctx),
                        separated_list1(punct(','), description(ctx)),
                    ),
                ),
                |(anns, list)| Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(anns, list)),
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("SameIndividual:"),
                    nom::sequence::pair(
                        opt_annotations(ctx),
                        separated_list1(punct(','), individual(ctx)),
                    ),
                ),
                |(anns, list)| Axiom::AxiomAssertion(Assertion::SameIndividual(anns, list)),
            ),
            nom::combinator::map(
                nom::sequence::preceded(
                    keyword("DifferentIndividuals:"),
                    nom::sequence::pair(
                        opt_annotations(ctx),
                        separated_list1(punct(','), individual(ctx)),
                    ),
                ),
                |(anns, list)| Axiom::AxiomAssertion(Assertion::DifferentIndividuals(anns, list)),
            ),
            equivalent_or_disjoint_properties(ctx, "EquivalentProperties:", true),
            equivalent_or_disjoint_properties(ctx, "DisjointProperties:", false),
        ))(input)
    }
}

fn opt_annotations<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Annotation>> {
    move |input: &'a str| match annotations_section(ctx)(input) {
        Ok((rest, anns)) => Ok((rest, anns)),
        Err(_) => Ok((input, Vec::new())),
    }
}

/// `'EquivalentProperties:'|'DisjointProperties:' annotations (objectProperty2List | dataProperty2List)`.
///
/// Disambiguated via the same pre-scanned data-property table used by
/// `class_expr::restriction` — see that module's docs.
fn equivalent_or_disjoint_properties<'a>(
    ctx: &'a ParserContext,
    kw: &'static str,
    equivalent: bool,
) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> + 'a {
    move |input: &'a str| {
        let (input, (anns, first, rest)) = nom::sequence::preceded(
            keyword(kw),
            nom::sequence::tuple((
                opt_annotations(ctx),
                iri(ctx),
                many0(nom::sequence::preceded(punct(','), iri(ctx))),
            )),
        )(input)?;
        let mut all = vec![first.clone()];
        all.extend(rest);
        let axiom = if ctx.is_known_data_property(&(first.0).0) {
            if equivalent {
                Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::EquivalentDataProperties(
                    anns, all,
                ))
            } else {
                Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DisjointDataProperties(anns, all))
            }
        } else {
            let all = all
                .into_iter()
                .map(ObjectPropertyExpression::NamedObjectProperty)
                .collect();
            if equivalent {
                Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::EquivalentObjectProperties(
                    anns, all,
                ))
            } else {
                Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::DisjointObjectProperties(
                    anns, all,
                ))
            }
        };
        Ok((input, axiom))
    }
}

/// Any recognized frame or misc axiom, expanded to zero or more `Axiom`s.
pub(crate) fn any_frame<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Axiom>> {
    move |input: &'a str| {
        alt((
            class_frame(ctx),
            object_property_frame(ctx),
            data_property_frame(ctx),
            individual_frame(ctx),
            annotation_property_frame(ctx),
            nom::combinator::map(misc(ctx), |a| vec![a]),
        ))(input)
    }
}

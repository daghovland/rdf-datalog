/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Every axiom-keyword production (`Declaration`, class/object-property/
//! data-property axioms, `DatatypeDefinition`, `HasKey`, assertions,
//! annotation axioms) -> `owl_ontology::Axiom`, plus the top-level `axiom`
//! dispatcher `Ontology(...)`'s body is `many0`-parsed with.
//!
//! See `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`'s "Phases" section
//! for the #180-mandated tier (phases 1-10, all in this file) vs. the
//! append-only phase-11 extensions (`ObjectPropertyChain` as a
//! `SubObjectPropertyOf` LHS, `HasKey`) marked below.

use crate::annotation::{annotation_subject, annotation_value_as_graph_element, axiom_annotations};
use crate::class_expr::class_expression;
use crate::data_range::data_range;
use crate::individual::individual;
use crate::iri::{ParserContext, iri};
use crate::literal::literal;
use crate::property_expr::{data_property_expression, object_property_expression};
use crate::tokens::{many0_no_sep, many1_no_sep, paren_form, punct};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use owl_ontology::{
    Assertion, Axiom, ClassAxiom, DataPropertyAxiom, Entity, ObjectPropertyAxiom,
    SubPropertyExpression,
};

// ---------------------------------------------------------------------
// Declaration (phase 2)
// ---------------------------------------------------------------------

fn entity<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Entity> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(paren_form("Class", iri(ctx)), Entity::ClassDeclaration),
            nom::combinator::map(
                paren_form("Datatype", iri(ctx)),
                Entity::DatatypeDeclaration,
            ),
            nom::combinator::map(
                paren_form("ObjectProperty", iri(ctx)),
                Entity::ObjectPropertyDeclaration,
            ),
            nom::combinator::map(
                paren_form("DataProperty", iri(ctx)),
                Entity::DataPropertyDeclaration,
            ),
            nom::combinator::map(
                paren_form("AnnotationProperty", iri(ctx)),
                Entity::AnnotationPropertyDeclaration,
            ),
            nom::combinator::map(paren_form("NamedIndividual", iri(ctx)), |i| {
                Entity::NamedIndividualDeclaration(owl_ontology::Individual::NamedIndividual(i))
            }),
        ))
        .parse(input)
    }
}

fn declaration<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        nom::combinator::map(
            paren_form("Declaration", (axiom_annotations(ctx), entity(ctx))),
            Axiom::AxiomDeclaration,
        )
        .parse(input)
    }
}

// ---------------------------------------------------------------------
// Class axioms (phase 5)
// ---------------------------------------------------------------------

fn class_axiom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form(
                    "SubClassOf",
                    (
                        axiom_annotations(ctx),
                        class_expression(ctx),
                        class_expression(ctx),
                    ),
                ),
                |(anns, sub, sup)| Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(anns, sub, sup)),
            ),
            nom::combinator::map(
                paren_form(
                    "EquivalentClasses",
                    (axiom_annotations(ctx), many1_no_sep(class_expression(ctx))),
                ),
                |(anns, ces)| Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(anns, ces)),
            ),
            nom::combinator::map(
                paren_form(
                    "DisjointClasses",
                    (axiom_annotations(ctx), many1_no_sep(class_expression(ctx))),
                ),
                |(anns, ces)| Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(anns, ces)),
            ),
            nom::combinator::map(
                paren_form(
                    "DisjointUnion",
                    (
                        axiom_annotations(ctx),
                        iri(ctx),
                        many1_no_sep(class_expression(ctx)),
                    ),
                ),
                |(anns, class, ces)| {
                    Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(anns, class, ces))
                },
            ),
        ))
        .parse(input)
    }
}

// ---------------------------------------------------------------------
// Object property axioms (phase 6; ObjectPropertyChain LHS is phase 11)
// ---------------------------------------------------------------------

fn sub_object_property_expression<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, SubPropertyExpression> {
    move |input: &'a str| {
        alt((
            // Phase 11: property chains.
            nom::combinator::map(
                paren_form(
                    "ObjectPropertyChain",
                    many1_no_sep(object_property_expression(ctx)),
                ),
                SubPropertyExpression::PropertyExpressionChain,
            ),
            nom::combinator::map(
                object_property_expression(ctx),
                SubPropertyExpression::SubObjectPropertyExpression,
            ),
        ))
        .parse(input)
    }
}

/// `ObjectPropertyDomain`/`ObjectPropertyRange` have no `Vec<Annotation>`
/// slot on `owl_ontology::ObjectPropertyAxiom` (unlike every other variant
/// in that enum) — see `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`'s
/// "Type-model gaps" section. Any `axiomAnnotations` parsed here are dropped
/// with a `log::warn!` when non-empty.
fn warn_if_annotations_dropped(keyword: &str, anns: &[owl_ontology::Annotation]) {
    if !anns.is_empty() {
        log::warn!(
            "owl_functional_parser: dropping {} axiomAnnotations on {keyword}(...) -- \
             owl_ontology::ObjectPropertyAxiom::{keyword} has no Vec<Annotation> slot (#180)",
            anns.len()
        );
    }
}

fn object_property_axiom<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form(
                    "SubObjectPropertyOf",
                    (
                        axiom_annotations(ctx),
                        sub_object_property_expression(ctx),
                        object_property_expression(ctx),
                    ),
                ),
                |(anns, sub, sup)| {
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
                        anns, sub, sup,
                    ))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "EquivalentObjectProperties",
                    (
                        axiom_annotations(ctx),
                        many1_no_sep(object_property_expression(ctx)),
                    ),
                ),
                |(anns, ps)| {
                    Axiom::AxiomObjectPropertyAxiom(
                        ObjectPropertyAxiom::EquivalentObjectProperties(anns, ps),
                    )
                },
            ),
            nom::combinator::map(
                paren_form(
                    "DisjointObjectProperties",
                    (
                        axiom_annotations(ctx),
                        many1_no_sep(object_property_expression(ctx)),
                    ),
                ),
                |(anns, ps)| {
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::DisjointObjectProperties(
                        anns, ps,
                    ))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectPropertyDomain",
                    (
                        axiom_annotations(ctx),
                        object_property_expression(ctx),
                        class_expression(ctx),
                    ),
                ),
                |(anns, p, c)| {
                    warn_if_annotations_dropped("ObjectPropertyDomain", &anns);
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyDomain(p, c))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectPropertyRange",
                    (
                        axiom_annotations(ctx),
                        object_property_expression(ctx),
                        class_expression(ctx),
                    ),
                ),
                |(anns, p, c)| {
                    warn_if_annotations_dropped("ObjectPropertyRange", &anns);
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyRange(p, c))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "InverseObjectProperties",
                    (
                        axiom_annotations(ctx),
                        object_property_expression(ctx),
                        object_property_expression(ctx),
                    ),
                ),
                |(anns, p1, p2)| {
                    Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::InverseObjectProperties(
                        anns, p1, p2,
                    ))
                },
            ),
            unary_object_property_axiom(ctx, "FunctionalObjectProperty", |anns, p| {
                ObjectPropertyAxiom::FunctionalObjectProperty(anns, p)
            }),
            unary_object_property_axiom(ctx, "InverseFunctionalObjectProperty", |anns, p| {
                ObjectPropertyAxiom::InverseFunctionalObjectProperty(anns, p)
            }),
            unary_object_property_axiom(ctx, "ReflexiveObjectProperty", |anns, p| {
                ObjectPropertyAxiom::ReflexiveObjectProperty(anns, p)
            }),
            unary_object_property_axiom(ctx, "IrreflexiveObjectProperty", |anns, p| {
                ObjectPropertyAxiom::IrreflexiveObjectProperty(anns, p)
            }),
            unary_object_property_axiom(ctx, "SymmetricObjectProperty", |anns, p| {
                ObjectPropertyAxiom::SymmetricObjectProperty(anns, p)
            }),
            unary_object_property_axiom(ctx, "AsymmetricObjectProperty", |anns, p| {
                ObjectPropertyAxiom::AsymmetricObjectProperty(anns, p)
            }),
            unary_object_property_axiom(ctx, "TransitiveObjectProperty", |anns, p| {
                ObjectPropertyAxiom::TransitiveObjectProperty(anns, p)
            }),
        ))
        .parse(input)
    }
}

fn unary_object_property_axiom<'a>(
    ctx: &'a ParserContext,
    keyword: &'static str,
    build: fn(
        Vec<owl_ontology::Annotation>,
        owl_ontology::ObjectPropertyExpression,
    ) -> ObjectPropertyAxiom,
) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        nom::combinator::map(
            paren_form(
                keyword,
                (axiom_annotations(ctx), object_property_expression(ctx)),
            ),
            move |(anns, p)| Axiom::AxiomObjectPropertyAxiom(build(anns, p)),
        )
        .parse(input)
    }
}

// ---------------------------------------------------------------------
// Data property axioms + DatatypeDefinition (phase 7)
// ---------------------------------------------------------------------

fn data_property_axiom<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form(
                    "SubDataPropertyOf",
                    (
                        axiom_annotations(ctx),
                        data_property_expression(ctx),
                        data_property_expression(ctx),
                    ),
                ),
                |(anns, sub, sup)| {
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::SubDataPropertyOf(
                        anns, sub, sup,
                    ))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "EquivalentDataProperties",
                    (
                        axiom_annotations(ctx),
                        many1_no_sep(data_property_expression(ctx)),
                    ),
                ),
                |(anns, ps)| {
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::EquivalentDataProperties(
                        anns, ps,
                    ))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "DisjointDataProperties",
                    (
                        axiom_annotations(ctx),
                        many1_no_sep(data_property_expression(ctx)),
                    ),
                ),
                |(anns, ps)| {
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DisjointDataProperties(
                        anns, ps,
                    ))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "DataPropertyDomain",
                    (
                        axiom_annotations(ctx),
                        data_property_expression(ctx),
                        class_expression(ctx),
                    ),
                ),
                |(anns, p, c)| {
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyDomain(anns, p, c))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "DataPropertyRange",
                    (
                        axiom_annotations(ctx),
                        data_property_expression(ctx),
                        data_range(ctx),
                    ),
                ),
                |(anns, p, dr)| {
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyRange(anns, p, dr))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "FunctionalDataProperty",
                    (axiom_annotations(ctx), data_property_expression(ctx)),
                ),
                |(anns, p)| {
                    Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::FunctionalDataProperty(
                        anns, p,
                    ))
                },
            ),
        ))
        .parse(input)
    }
}

fn datatype_definition<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        nom::combinator::map(
            paren_form(
                "DatatypeDefinition",
                (axiom_annotations(ctx), iri(ctx), data_range(ctx)),
            ),
            |(anns, dt, dr)| Axiom::AxiomDatatypeDefinition(anns, dt, dr),
        )
        .parse(input)
    }
}

/// Phase 11: `HasKey`.
fn has_key<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        nom::combinator::map(
            paren_form(
                "HasKey",
                (
                    axiom_annotations(ctx),
                    class_expression(ctx),
                    nom::sequence::delimited(
                        punct('('),
                        many0_no_sep(object_property_expression(ctx)),
                        punct(')'),
                    ),
                    nom::sequence::delimited(
                        punct('('),
                        many0_no_sep(data_property_expression(ctx)),
                        punct(')'),
                    ),
                ),
            ),
            |(anns, ce, ops, dps)| Axiom::AxiomHasKey(anns, ce, ops, dps),
        )
        .parse(input)
    }
}

// ---------------------------------------------------------------------
// Assertions (phase 8)
// ---------------------------------------------------------------------

fn assertion<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form(
                    "SameIndividual",
                    (axiom_annotations(ctx), many1_no_sep(individual(ctx))),
                ),
                |(anns, inds)| Axiom::AxiomAssertion(Assertion::SameIndividual(anns, inds)),
            ),
            nom::combinator::map(
                paren_form(
                    "DifferentIndividuals",
                    (axiom_annotations(ctx), many1_no_sep(individual(ctx))),
                ),
                |(anns, inds)| Axiom::AxiomAssertion(Assertion::DifferentIndividuals(anns, inds)),
            ),
            nom::combinator::map(
                paren_form(
                    "ClassAssertion",
                    (
                        axiom_annotations(ctx),
                        class_expression(ctx),
                        individual(ctx),
                    ),
                ),
                |(anns, ce, ind)| Axiom::AxiomAssertion(Assertion::ClassAssertion(anns, ce, ind)),
            ),
            nom::combinator::map(
                paren_form(
                    "NegativeObjectPropertyAssertion",
                    (
                        axiom_annotations(ctx),
                        object_property_expression(ctx),
                        individual(ctx),
                        individual(ctx),
                    ),
                ),
                |(anns, p, i1, i2)| {
                    Axiom::AxiomAssertion(Assertion::NegativeObjectPropertyAssertion(
                        anns, p, i1, i2,
                    ))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectPropertyAssertion",
                    (
                        axiom_annotations(ctx),
                        object_property_expression(ctx),
                        individual(ctx),
                        individual(ctx),
                    ),
                ),
                |(anns, p, i1, i2)| {
                    Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(anns, p, i1, i2))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "NegativeDataPropertyAssertion",
                    (
                        axiom_annotations(ctx),
                        data_property_expression(ctx),
                        individual(ctx),
                        literal(ctx),
                    ),
                ),
                |(anns, p, i, l)| {
                    Axiom::AxiomAssertion(Assertion::NegativeDataPropertyAssertion(anns, p, i, l))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "DataPropertyAssertion",
                    (
                        axiom_annotations(ctx),
                        data_property_expression(ctx),
                        individual(ctx),
                        literal(ctx),
                    ),
                ),
                |(anns, p, i, l)| {
                    Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(anns, p, i, l))
                },
            ),
        ))
        .parse(input)
    }
}

// ---------------------------------------------------------------------
// Annotation axioms (phase 9)
// ---------------------------------------------------------------------

fn annotation_axiom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form(
                    "AnnotationAssertion",
                    (
                        axiom_annotations(ctx),
                        iri(ctx),
                        annotation_subject(ctx),
                        annotation_value_as_graph_element(ctx),
                    ),
                ),
                |(anns, prop, subj, val)| {
                    Axiom::AxiomAnnotationAxiom(owl_ontology::AnnotationAxiom::AnnotationAssertion(
                        anns, prop, subj, val,
                    ))
                },
            ),
            nom::combinator::map(
                paren_form(
                    "SubAnnotationPropertyOf",
                    (axiom_annotations(ctx), iri(ctx), iri(ctx)),
                ),
                |(anns, sub, sup)| {
                    Axiom::AxiomAnnotationAxiom(
                        owl_ontology::AnnotationAxiom::SubAnnotationPropertyOf(anns, sub, sup),
                    )
                },
            ),
            nom::combinator::map(
                paren_form(
                    "AnnotationPropertyDomain",
                    (axiom_annotations(ctx), iri(ctx), iri(ctx)),
                ),
                |(anns, prop, dom)| {
                    Axiom::AxiomAnnotationAxiom(
                        owl_ontology::AnnotationAxiom::AnnotationPropertyDomain(anns, prop, dom),
                    )
                },
            ),
            nom::combinator::map(
                paren_form(
                    "AnnotationPropertyRange",
                    (axiom_annotations(ctx), iri(ctx), iri(ctx)),
                ),
                |(anns, prop, rng)| {
                    Axiom::AxiomAnnotationAxiom(
                        owl_ontology::AnnotationAxiom::AnnotationPropertyRange(anns, prop, rng),
                    )
                },
            ),
        ))
        .parse(input)
    }
}

// ---------------------------------------------------------------------
// Top-level dispatcher
// ---------------------------------------------------------------------

/// `axiom ::= Declaration | ClassAxiom | ObjectPropertyAxiom | DataPropertyAxiom`
/// `        | DatatypeDefinition | HasKey | Assertion | AnnotationAxiom`
pub(crate) fn axiom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Axiom> {
    move |input: &'a str| {
        alt((
            declaration(ctx),
            class_axiom(ctx),
            object_property_axiom(ctx),
            data_property_axiom(ctx),
            datatype_definition(ctx),
            has_key(ctx),
            assertion(ctx),
            annotation_axiom(ctx),
        ))
        .parse(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_default_prefix() -> ParserContext {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        ctx
    }

    #[test]
    fn parses_all_six_declaration_kinds() {
        let ctx = ctx_with_default_prefix();
        for (src, expect) in [
            ("Declaration(Class(:Pizza))", "Class"),
            ("Declaration(Datatype(:MyDatatype))", "Datatype"),
            ("Declaration(ObjectProperty(:hasTopping))", "ObjectProperty"),
            ("Declaration(DataProperty(:hasAge))", "DataProperty"),
            (
                "Declaration(AnnotationProperty(:comment))",
                "AnnotationProperty",
            ),
            ("Declaration(NamedIndividual(:fido))", "NamedIndividual"),
        ] {
            let (_, ax) = axiom(&ctx)(src).unwrap();
            match (&ax, expect) {
                (Axiom::AxiomDeclaration((_, Entity::ClassDeclaration(_))), "Class") => {}
                (Axiom::AxiomDeclaration((_, Entity::DatatypeDeclaration(_))), "Datatype") => {}
                (
                    Axiom::AxiomDeclaration((_, Entity::ObjectPropertyDeclaration(_))),
                    "ObjectProperty",
                ) => {}
                (
                    Axiom::AxiomDeclaration((_, Entity::DataPropertyDeclaration(_))),
                    "DataProperty",
                ) => {}
                (
                    Axiom::AxiomDeclaration((_, Entity::AnnotationPropertyDeclaration(_))),
                    "AnnotationProperty",
                ) => {}
                (
                    Axiom::AxiomDeclaration((_, Entity::NamedIndividualDeclaration(_))),
                    "NamedIndividual",
                ) => {}
                (other, exp) => panic!("{src}: expected {exp} declaration, got {other:?}"),
            }
        }
    }

    #[test]
    fn parses_sub_class_of() {
        let ctx = ctx_with_default_prefix();
        let (_, ax) = axiom(&ctx)("SubClassOf(:Dog :Animal)").unwrap();
        assert_eq!(
            ax,
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                owl_ontology::ClassExpression::ClassName(owl_ontology::FullIri(
                    ingress::IriReference("http://example.org/Dog".to_string())
                )),
                owl_ontology::ClassExpression::ClassName(owl_ontology::FullIri(
                    ingress::IriReference("http://example.org/Animal".to_string())
                )),
            ))
        );
    }

    #[test]
    fn parses_equivalent_and_disjoint_classes() {
        let ctx = ctx_with_default_prefix();
        let (_, ax) = axiom(&ctx)("EquivalentClasses(:Dog :Canine)").unwrap();
        match ax {
            Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(_, ces)) => {
                assert_eq!(ces.len(), 2)
            }
            other => panic!("expected EquivalentClasses, got {other:?}"),
        }
        let (_, ax2) = axiom(&ctx)("DisjointClasses(:Dog :Cat)").unwrap();
        match ax2 {
            Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(_, ces)) => {
                assert_eq!(ces.len(), 2)
            }
            other => panic!("expected DisjointClasses, got {other:?}"),
        }
    }

    #[test]
    fn parses_disjoint_union() {
        let ctx = ctx_with_default_prefix();
        let (_, ax) = axiom(&ctx)("DisjointUnion(:Animal :Dog :Cat)").unwrap();
        match ax {
            Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(_, class, ces)) => {
                assert_eq!(class.0.0, "http://example.org/Animal");
                assert_eq!(ces.len(), 2);
            }
            other => panic!("expected DisjointUnion, got {other:?}"),
        }
    }

    #[test]
    fn parses_object_property_axioms() {
        let ctx = ctx_with_default_prefix();
        let (_, ax) = axiom(&ctx)("ObjectPropertyDomain(:hasTopping :Pizza)").unwrap();
        assert!(matches!(
            ax,
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyDomain(_, _))
        ));
        let (_, ax2) = axiom(&ctx)("TransitiveObjectProperty(:hasPart)").unwrap();
        assert!(matches!(
            ax2,
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::TransitiveObjectProperty(_, _))
        ));
        let (_, ax3) = axiom(&ctx)("SubObjectPropertyOf(:hasDog :hasPet)").unwrap();
        assert!(matches!(
            ax3,
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
                _,
                SubPropertyExpression::SubObjectPropertyExpression(_),
                _
            ))
        ));
    }

    #[test]
    fn parses_sub_object_property_of_with_chain() {
        let ctx = ctx_with_default_prefix();
        let (_, ax) = axiom(&ctx)(
            "SubObjectPropertyOf(ObjectPropertyChain(:hasParent :hasParent) :hasGrandparent)",
        )
        .unwrap();
        match ax {
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
                _,
                SubPropertyExpression::PropertyExpressionChain(chain),
                _,
            )) => assert_eq!(chain.len(), 2),
            other => panic!("expected chain SubObjectPropertyOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_data_property_axioms_and_datatype_definition() {
        let ctx = ctx_with_default_prefix();
        ctx.declare_prefix("xsd", ingress::XSD);
        let (_, ax) = axiom(&ctx)("FunctionalDataProperty(:hasAge)").unwrap();
        assert!(matches!(
            ax,
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::FunctionalDataProperty(_, _))
        ));
        let (_, ax2) = axiom(&ctx)("DataPropertyRange(:hasAge xsd:integer)").unwrap();
        assert!(matches!(
            ax2,
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyRange(_, _, _))
        ));
        let (_, ax3) = axiom(&ctx)("DatatypeDefinition(:AdultAge xsd:integer)").unwrap();
        assert!(matches!(ax3, Axiom::AxiomDatatypeDefinition(_, _, _)));
    }

    #[test]
    fn parses_has_key() {
        let ctx = ctx_with_default_prefix();
        let (_, ax) = axiom(&ctx)("HasKey(:Person (:hasSSN) ())").unwrap();
        match ax {
            Axiom::AxiomHasKey(_, _, ops, dps) => {
                assert_eq!(ops.len(), 1);
                assert_eq!(dps.len(), 0);
            }
            other => panic!("expected HasKey, got {other:?}"),
        }
    }

    #[test]
    fn parses_all_assertion_kinds() {
        let ctx = ctx_with_default_prefix();
        assert!(matches!(
            axiom(&ctx)("ClassAssertion(:Dog :fido)").unwrap().1,
            Axiom::AxiomAssertion(Assertion::ClassAssertion(_, _, _))
        ));
        assert!(matches!(
            axiom(&ctx)("ObjectPropertyAssertion(:hasPet :alice :fido)")
                .unwrap()
                .1,
            Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(_, _, _, _))
        ));
        assert!(matches!(
            axiom(&ctx)("NegativeObjectPropertyAssertion(:hasPet :alice :rex)")
                .unwrap()
                .1,
            Axiom::AxiomAssertion(Assertion::NegativeObjectPropertyAssertion(_, _, _, _))
        ));
        assert!(matches!(
            axiom(&ctx)("DataPropertyAssertion(:hasAge :alice \"30\")")
                .unwrap()
                .1,
            Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(_, _, _, _))
        ));
        assert!(matches!(
            axiom(&ctx)("NegativeDataPropertyAssertion(:hasAge :alice \"5\")")
                .unwrap()
                .1,
            Axiom::AxiomAssertion(Assertion::NegativeDataPropertyAssertion(_, _, _, _))
        ));
        assert!(matches!(
            axiom(&ctx)("SameIndividual(:alice :alicia)").unwrap().1,
            Axiom::AxiomAssertion(Assertion::SameIndividual(_, _))
        ));
        assert!(matches!(
            axiom(&ctx)("DifferentIndividuals(:alice :bob)").unwrap().1,
            Axiom::AxiomAssertion(Assertion::DifferentIndividuals(_, _))
        ));
    }

    #[test]
    fn parses_annotation_axioms() {
        let ctx = ctx_with_default_prefix();
        ctx.declare_prefix("rdfs", ingress::RDFS);
        let (_, ax) = axiom(&ctx)("AnnotationAssertion(rdfs:label :Pizza \"Pizza\")").unwrap();
        assert!(matches!(
            ax,
            Axiom::AxiomAnnotationAxiom(owl_ontology::AnnotationAxiom::AnnotationAssertion(
                _,
                _,
                _,
                _
            ))
        ));
        let (_, ax2) = axiom(&ctx)("SubAnnotationPropertyOf(rdfs:label rdfs:comment)").unwrap();
        assert!(matches!(
            ax2,
            Axiom::AxiomAnnotationAxiom(owl_ontology::AnnotationAxiom::SubAnnotationPropertyOf(
                _,
                _,
                _
            ))
        ));
    }

    #[test]
    fn axiom_annotations_attach_to_a_later_axiom() {
        let ctx = ctx_with_default_prefix();
        ctx.declare_prefix("rdfs", ingress::RDFS);
        let (_, ax) =
            axiom(&ctx)("SubClassOf(Annotation(rdfs:label \"why\") :Dog :Animal)").unwrap();
        match ax {
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(anns, _, _)) => {
                assert_eq!(anns.len(), 1);
            }
            other => panic!("expected SubClassOf with annotations, got {other:?}"),
        }
    }
}

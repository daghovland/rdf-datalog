/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `Annotation ::= 'Annotation' '(' annotationAnnotations AnnotationProperty AnnotationValue ')'`
//! `axiomAnnotations ::= { Annotation }`, `annotationAnnotations ::= { Annotation }`
//!
//! Meta-annotations (an `Annotation(...)` nested inside another
//! `Annotation(...)`'s own `annotationAnnotations`) are parsed but their
//! payload is discarded: `owl_ontology::Annotation` is a flat
//! `(AnnotationProperty, AnnotationValue)` pair with no slot for annotations
//! on an annotation, the same limitation `manchester_parser` documents (see
//! its `annotation.rs` module docs, #157) — parsing them (rather than
//! failing) still lets a document that happens to use meta-annotations parse
//! successfully, matching this crate's general policy of dropping
//! unrepresentable input with a warning rather than rejecting the whole
//! document.

use crate::iri::{ParserContext, iri, node_id};
use crate::literal::literal;
use crate::tokens::{many0_no_sep, paren_form};
use ingress::{GraphElement, RdfResource};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use owl_ontology::{Annotation, AnnotationValue, Individual};

/// `AnnotationValue ::= AnonymousIndividual | IRI | Literal`
fn annotation_value<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, AnnotationValue> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(literal(ctx), AnnotationValue::LiteralAnnotation),
            nom::combinator::map(node_id, |label: String| {
                AnnotationValue::IndividualAnnotation(Individual::AnonymousIndividual(
                    ctx.anon_individual_for_label(&label),
                ))
            }),
            nom::combinator::map(iri(ctx), AnnotationValue::IriAnnotation),
        ))
        .parse(input)
    }
}

/// `Annotation ::= 'Annotation' '(' annotationAnnotations AnnotationProperty AnnotationValue ')'`
pub(crate) fn annotation<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Annotation> {
    move |input: &'a str| {
        paren_form("Annotation", |input| {
            let (input, _meta) = many0_no_sep(annotation(ctx)).parse(input)?;
            let (input, prop) = iri(ctx)(input)?;
            let (input, value) = annotation_value(ctx)(input)?;
            Ok((input, (prop, value)))
        })
        .parse(input)
    }
}

/// `axiomAnnotations ::= { Annotation }`
pub(crate) fn axiom_annotations<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Annotation>> {
    move |input: &'a str| many0_no_sep(annotation(ctx)).parse(input)
}

/// Lower an `AnnotationSubject ::= IRI | AnonymousIndividual` into the
/// `GraphElement` slot `owl_ontology::AnnotationAxiom::AnnotationAssertion`
/// actually carries.
pub(crate) fn annotation_subject<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, GraphElement> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(node_id, |label: String| {
                GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(
                    ctx.anon_individual_for_label(&label),
                ))
            }),
            nom::combinator::map(iri(ctx), |i| {
                GraphElement::NodeOrEdge(RdfResource::Iri(i.0))
            }),
        ))
        .parse(input)
    }
}

/// Lower an `AnnotationValue` into the `GraphElement` slot
/// `AnnotationAssertion`'s value carries: an IRI or anonymous individual
/// becomes a resource `GraphElement`, a literal stays a literal `GraphElement`.
pub(crate) fn annotation_value_as_graph_element<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, GraphElement> {
    move |input: &'a str| {
        alt((
            literal(ctx),
            nom::combinator::map(node_id, |label: String| {
                GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(
                    ctx.anon_individual_for_label(&label),
                ))
            }),
            nom::combinator::map(iri(ctx), |i| {
                GraphElement::NodeOrEdge(RdfResource::Iri(i.0))
            }),
        ))
        .parse(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_annotation_with_literal_value() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("rdfs", ingress::RDFS);
        let (_, (prop, value)) = annotation(&ctx)("Annotation(rdfs:label \"Pizza\")").unwrap();
        let owl_ontology::FullIri(ingress::IriReference(prop_iri)) = &prop;
        assert_eq!(*prop_iri, format!("{}label", ingress::RDFS));
        match value {
            AnnotationValue::LiteralAnnotation(_) => {}
            other => panic!("expected LiteralAnnotation, got {other:?}"),
        }
    }

    #[test]
    fn parses_axiom_annotations_empty_and_nonempty() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("rdfs", ingress::RDFS);
        let (rest, anns) = axiom_annotations(&ctx)("rest").unwrap();
        assert!(anns.is_empty());
        assert_eq!(rest, "rest");

        let (rest2, anns2) = axiom_annotations(&ctx)("Annotation(rdfs:label \"x\") rest").unwrap();
        assert_eq!(anns2.len(), 1);
        assert_eq!(rest2, "rest");
    }
}

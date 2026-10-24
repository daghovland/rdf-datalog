/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `annotation ::= annotationPropertyIRI annotationTarget`
//! `annotations ::= 'Annotations:' annotationAnnotatedList`
//! `annotationAnnotatedList ::= [annotations] annotation { ',' [annotations] annotation }`
//!
//! `annotationTarget` is one of an IRI, an (anonymous) individual, or a
//! literal. Meta-annotations (annotations on an annotation) are supported
//! via the `[annotations]` in `annotationAnnotatedList`: each list item may
//! carry its own leading `Annotations:` block, recursively — see
//! [#695](https://github.com/daghovland/rdf-datalog/issues/695).

use crate::iri::{ParserContext, iri, node_id};
use crate::literal::literal;
use crate::tokens::{keyword, punct};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use nom::multi::separated_list1;
use owl_ontology::{Annotation, AnnotationValue, Individual};

/// A single `[annotations] annotationProperty annotationTarget` item: an
/// optional leading `Annotations:` block (meta-annotations on this one
/// annotation, recursively), followed by the property+target pair.
pub(crate) fn annotation<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Annotation> {
    move |input: &'a str| {
        let (input, meta) = opt_leading_annotations(ctx)(input)?;
        let (input, prop) = iri(ctx)(input)?;
        let (input, value) = alt((
            nom::combinator::map(literal(ctx), AnnotationValue::LiteralAnnotation),
            nom::combinator::map(node_id, |label: String| {
                AnnotationValue::IndividualAnnotation(Individual::AnonymousIndividual(
                    ctx.anon_individual_for_label(&label),
                ))
            }),
            nom::combinator::map(iri(ctx), AnnotationValue::IriAnnotation),
        ))
        .parse(input)?;
        Ok((input, Annotation::with_meta(prop, value, meta)))
    }
}

/// An optional `'Annotations:' annotation { ',' annotation }` prefix, as
/// found before most annotated-list items. Returns an empty `Vec` if absent.
pub(crate) fn opt_leading_annotations<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Annotation>> {
    move |input: &'a str| match nom::sequence::preceded(
        keyword("Annotations:"),
        separated_list1(punct(','), annotation(ctx)),
    )
    .parse(input)
    {
        Ok((rest, anns)) => Ok((rest, anns)),
        Err(_) => Ok((input, Vec::new())),
    }
}

/// A standalone `'Annotations:' annotation { ',' annotation }` frame
/// section (used for e.g. a `Class:` frame's own `Annotations:` section,
/// which annotates the entity's declaration).
pub(crate) fn annotations_section<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Annotation>> {
    move |input: &'a str| {
        nom::sequence::preceded(
            keyword("Annotations:"),
            separated_list1(punct(','), annotation(ctx)),
        )
        .parse(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_annotations_section_with_literal_and_iri_targets() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        ctx.declare_prefix("rdfs", ingress::RDFS);
        let (_, anns) =
            annotations_section(&ctx)("Annotations: rdfs:label \"Pizza\", rdfs:seeAlso Other")
                .unwrap();
        assert_eq!(anns.len(), 2);
        let owl_ontology::FullIri(ingress::IriReference(prop_iri)) = &anns[0].property;
        assert_eq!(*prop_iri, format!("{}label", ingress::RDFS));
        match &anns[0].value {
            AnnotationValue::LiteralAnnotation(_) => {}
            other => panic!("expected LiteralAnnotation, got {other:?}"),
        }
        match &anns[1].value {
            AnnotationValue::IriAnnotation(_) => {}
            other => panic!("expected IriAnnotation, got {other:?}"),
        }
        assert!(anns[0].annotations.is_empty());
    }

    #[test]
    fn parses_nested_meta_annotations() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        ctx.declare_prefix("ex", "http://example.org/");
        let (rest, anns) = annotations_section(&ctx)(
            "Annotations: Annotations: ex:source ex:Textbook ex:comment \"hello\"",
        )
        .unwrap();
        assert_eq!(rest, "");
        assert_eq!(anns.len(), 1);
        let outer = &anns[0];
        let owl_ontology::FullIri(ingress::IriReference(prop_iri)) = &outer.property;
        assert_eq!(*prop_iri, "http://example.org/comment");
        match &outer.value {
            AnnotationValue::LiteralAnnotation(_) => {}
            other => panic!("expected LiteralAnnotation, got {other:?}"),
        }
        assert_eq!(outer.annotations.len(), 1);
        let meta = &outer.annotations[0];
        let owl_ontology::FullIri(ingress::IriReference(meta_prop_iri)) = &meta.property;
        assert_eq!(*meta_prop_iri, "http://example.org/source");
        match &meta.value {
            AnnotationValue::IriAnnotation(_) => {}
            other => panic!("expected IriAnnotation, got {other:?}"),
        }
        assert!(meta.annotations.is_empty());
    }
}

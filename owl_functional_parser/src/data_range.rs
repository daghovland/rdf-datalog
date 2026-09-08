/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `DataRange ::= Datatype | DataIntersectionOf | DataUnionOf | DataComplementOf`
//! `           | DataOneOf | DatatypeRestriction`
//!
//! Phase 3 (issue #180's mandated tier) only needs the bare `Datatype` case;
//! the compound forms (`DataIntersectionOf`/`DataUnionOf`/`DataComplementOf`/
//! `DataOneOf`/`DatatypeRestriction`) are phase 11 (see
//! `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`), included here rather
//! than in a separate module since they're all one recursive `DataRange`
//! grammar.

use crate::iri::{ParserContext, iri};
use crate::literal::literal;
use crate::tokens::{many1_no_sep, paren_form};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use owl_ontology::DataRange;

pub(crate) fn data_range<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    move |input: &'a str| {
        alt((
            compound_data_range(ctx),
            nom::combinator::map(iri(ctx), DataRange::NamedDataRange),
        ))
        .parse(input)
    }
}

/// The keyword-prefixed `DataRange` alternatives only -- excludes the bare
/// `Datatype` case. Used by `class_expr.rs`'s `DataSomeValuesFrom`/
/// `DataAllValuesFrom` parsing to disambiguate a trailing bare IRI as either
/// another `DataPropertyExpression` or the final `DataRange`: a bare IRI is
/// syntactically identical in both positions, so that disambiguation can't
/// rely on `data_range` (which would also accept the bare form) -- it needs
/// this compound-only parser plus its own "is this the last item" lookahead.
pub(crate) fn compound_data_range<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form("DataIntersectionOf", many1_no_sep(data_range(ctx))),
                DataRange::DataIntersectionOf,
            ),
            nom::combinator::map(
                paren_form("DataUnionOf", many1_no_sep(data_range(ctx))),
                DataRange::DataUnionOf,
            ),
            nom::combinator::map(paren_form("DataComplementOf", data_range(ctx)), |inner| {
                DataRange::DataComplementOf(Box::new(inner))
            }),
            nom::combinator::map(
                paren_form("DataOneOf", many1_no_sep(literal(ctx))),
                DataRange::DataOneOf,
            ),
            nom::combinator::map(
                paren_form("DatatypeRestriction", datatype_restriction_body(ctx)),
                |(dt, facets)| DataRange::DatatypeRestriction(dt, facets),
            ),
        ))
        .parse(input)
    }
}

/// `DataPropertyExpression { DataPropertyExpression } DataRange` -- the
/// shared body of `DataSomeValuesFrom`/`DataAllValuesFrom`. A bare IRI is
/// ambiguous between "one more data property" and "the final data range"
/// (both are just `iri(ctx)`), so this greedily takes compound data ranges
/// or IRIs immediately followed by `)` as the terminal `DataRange`, and
/// every other bare IRI as a property.
pub(crate) fn data_properties_then_range<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, (Vec<owl_ontology::DataProperty>, DataRange)> {
    move |input: &'a str| {
        let mut props = Vec::new();
        let mut rest = input;
        loop {
            if let Ok((next, dr)) = compound_data_range(ctx)(rest) {
                return Ok((next, (props, dr)));
            }
            let (next, i) = iri(ctx)(rest)?;
            if next.starts_with(')') {
                return Ok((next, (props, DataRange::NamedDataRange(i))));
            }
            props.push(i);
            rest = next;
        }
    }
}

/// One `(facet, value)` pair inside a `DatatypeRestriction(...)`.
type FacetValue = (owl_ontology::DataProperty, ingress::GraphElement);

/// `Datatype constrainingFacet restrictionValue { constrainingFacet restrictionValue }`
/// — the body of `DatatypeRestriction(...)`. `owl_ontology::DataRange::DatatypeRestriction`
/// models each `(facet, value)` pair as `(DataProperty, GraphElement)` — the
/// facet IRI reuses the `DataProperty` type alias (both are plain `FullIri`).
fn datatype_restriction_body<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, (owl_ontology::Datatype, Vec<FacetValue>)> {
    move |input: &'a str| {
        let (input, dt) = iri(ctx)(input)?;
        let (input, facets) = many1_no_sep(|i| {
            let (i, facet) = iri(ctx)(i)?;
            let (i, value) = literal(ctx)(i)?;
            Ok((i, (facet, value)))
        })
        .parse(input)?;
        Ok((input, (dt, facets)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_named_datatype() {
        let ctx = ParserContext::new();
        let (_, dr) = data_range(&ctx)("xsd:integer").unwrap();
        assert_eq!(
            dr,
            DataRange::NamedDataRange(owl_ontology::FullIri(ingress::IriReference(format!(
                "{}integer",
                ingress::XSD
            ))))
        );
    }

    #[test]
    fn parses_data_intersection_and_union_and_complement() {
        let ctx = ParserContext::new();
        let (_, dr) = data_range(&ctx)("DataIntersectionOf(xsd:integer xsd:string)").unwrap();
        assert_eq!(
            dr,
            DataRange::DataIntersectionOf(vec![
                DataRange::NamedDataRange(owl_ontology::FullIri(ingress::IriReference(format!(
                    "{}integer",
                    ingress::XSD
                )))),
                DataRange::NamedDataRange(owl_ontology::FullIri(ingress::IriReference(format!(
                    "{}string",
                    ingress::XSD
                )))),
            ])
        );
        let (_, dr2) = data_range(&ctx)("DataComplementOf(xsd:integer)").unwrap();
        assert_eq!(
            dr2,
            DataRange::DataComplementOf(Box::new(DataRange::NamedDataRange(
                owl_ontology::FullIri(ingress::IriReference(format!("{}integer", ingress::XSD)))
            )))
        );
    }

    #[test]
    fn parses_data_one_of() {
        let ctx = ParserContext::new();
        let (_, dr) = data_range(&ctx)("DataOneOf(\"a\" \"b\")").unwrap();
        match dr {
            DataRange::DataOneOf(vals) => assert_eq!(vals.len(), 2),
            other => panic!("expected DataOneOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_datatype_restriction() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("xsd", ingress::XSD);
        let (_, dr) = data_range(&ctx)(
            "DatatypeRestriction(xsd:integer xsd:minInclusive \"0\" xsd:maxInclusive \"10\")",
        )
        .unwrap();
        match dr {
            DataRange::DatatypeRestriction(dt, facets) => {
                assert_eq!(dt.0.0, format!("{}integer", ingress::XSD));
                assert_eq!(facets.len(), 2);
            }
            other => panic!("expected DatatypeRestriction, got {other:?}"),
        }
    }
}

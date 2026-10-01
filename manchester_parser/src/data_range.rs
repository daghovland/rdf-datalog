/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! The data-range precedence ladder, mirroring `class_expr.rs`'s
//! `description`/`conjunction`/`primary`/`atomic` for class expressions:
//!
//! ```text
//! dataRange       ::= dataConjunction 'or' dataConjunction { 'or' dataConjunction } | dataConjunction
//! dataConjunction ::= dataPrimary 'and' dataPrimary { 'and' dataPrimary } | dataPrimary
//! dataPrimary     ::= [ 'not' ] dataAtomic
//! dataAtomic      ::= Datatype [ '[' facet restrictionValue (',' facet restrictionValue)* ']' ]
//!                    | '{' literal (',' literal)* '}'
//!                    | '(' dataRange ')'
//! ```
//!
//! See [#501](https://github.com/daghovland/rdf-datalog/issues/501)'s
//! addendum in `docs/plans/MANCHESTER_SYNTAX_PLAN.md` for the facet-token ->
//! facet-IRI mapping table and other design notes.

use crate::iri::{ParserContext, iri};
use crate::literal::literal;
use crate::tokens::{keyword, punct, tok};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use nom::multi::{many0, separated_list1};
use nom::sequence::delimited;
use owl_ontology::{DataProperty, DataRange, FullIri};

/// `dataRange`
pub(crate) fn data_range<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    move |input: &'a str| {
        let (input, first) = data_conjunction(ctx)(input)?;
        let (input, mut rest) = many0(nom::sequence::preceded(
            keyword("or"),
            data_conjunction(ctx),
        ))
        .parse(input)?;
        if rest.is_empty() {
            Ok((input, first))
        } else {
            let mut all = vec![first];
            all.append(&mut rest);
            Ok((input, DataRange::DataUnionOf(all)))
        }
    }
}

/// `dataConjunction`
fn data_conjunction<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    move |input: &'a str| {
        let (input, first) = data_primary(ctx)(input)?;
        let (input, mut rest) =
            many0(nom::sequence::preceded(keyword("and"), data_primary(ctx))).parse(input)?;
        if rest.is_empty() {
            Ok((input, first))
        } else {
            let mut all = vec![first];
            all.append(&mut rest);
            Ok((input, DataRange::DataIntersectionOf(all)))
        }
    }
}

/// `dataPrimary ::= [ 'not' ] dataAtomic`
fn data_primary<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    move |input: &'a str| {
        let (input, negated) = nom::combinator::opt(keyword("not")).parse(input)?;
        let (input, inner) = data_atomic(ctx)(input)?;
        if negated.is_some() {
            Ok((input, DataRange::DataComplementOf(Box::new(inner))))
        } else {
            Ok((input, inner))
        }
    }
}

/// `dataAtomic ::= Datatype [ '[' facet restrictionValue {',' facet restrictionValue} ']' ]`
/// `           | '{' literal {',' literal} '}' | '(' dataRange ')'`
fn data_atomic<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                delimited(
                    punct('{'),
                    separated_list1(punct(','), literal(ctx)),
                    punct('}'),
                ),
                DataRange::DataOneOf,
            ),
            delimited(punct('('), data_range(ctx), punct(')')),
            datatype_with_optional_facets(ctx),
        ))
        .parse(input)
    }
}

/// `Datatype [ '[' facet restrictionValue {',' facet restrictionValue} ']' ]` —
/// a bare named datatype, optionally followed by a bracketed facet list. The
/// only lookahead this grammar needs: `[` can never start anything else that
/// could follow a `Datatype` here, so no backtracking is required.
fn datatype_with_optional_facets<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    move |input: &'a str| {
        let (input, dt) = iri(ctx)(input)?;
        match punct('[')(input) {
            Ok((input, _)) => {
                let (input, facets) =
                    separated_list1(punct(','), facet_restriction(ctx)).parse(input)?;
                let (input, _) = punct(']')(input)?;
                Ok((input, DataRange::DatatypeRestriction(dt, facets)))
            }
            Err(_) => Ok((input, DataRange::NamedDataRange(dt))),
        }
    }
}

/// One `facet restrictionValue` pair, e.g. `>= 0` or `minLength 1`.
fn facet_restriction<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, (DataProperty, ingress::GraphElement)> {
    move |input: &'a str| {
        let (input, f) = facet(input)?;
        let (input, value) = literal(ctx)(input)?;
        Ok((input, (f, value)))
    }
}

/// A single symbolic punctuation token (e.g. `<=`), consuming trailing
/// whitespace/comments — `tokens::keyword` assumes identifier characters so
/// can't be reused here. Tried longest-first at the call site (`<=` before
/// `<`, `>=` before `>`).
fn sym<'a>(s: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &'a str| tok(nom::bytes::complete::tag(s))(input)
}

/// `constrainingFacet`, mapped to its canonical XSD (or, for `langRange`,
/// RDF) facet IRI. See the #501 addendum in
/// `docs/plans/MANCHESTER_SYNTAX_PLAN.md` for the full mapping table.
fn facet(input: &str) -> IResult<&str, FullIri> {
    alt((
        nom::combinator::map(sym("<="), |_| facet_iri("maxInclusive")),
        nom::combinator::map(sym(">="), |_| facet_iri("minInclusive")),
        nom::combinator::map(sym("<"), |_| facet_iri("maxExclusive")),
        nom::combinator::map(sym(">"), |_| facet_iri("minExclusive")),
        nom::combinator::map(keyword("minLength"), |_| facet_iri("minLength")),
        nom::combinator::map(keyword("maxLength"), |_| facet_iri("maxLength")),
        nom::combinator::map(keyword("length"), |_| facet_iri("length")),
        nom::combinator::map(keyword("pattern"), |_| facet_iri("pattern")),
        nom::combinator::map(keyword("langRange"), |_| {
            FullIri(ingress::IriReference(format!("{}langRange", ingress::RDF)))
        }),
    ))
    .parse(input)
}

fn facet_iri(local: &str) -> FullIri {
    FullIri(ingress::IriReference(format!("{}{local}", ingress::XSD)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_datatype() {
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
    fn parses_intersection_union_complement() {
        let ctx = ParserContext::new();
        let (_, dr) = data_range(&ctx)("xsd:integer and xsd:positiveInteger").unwrap();
        match dr {
            DataRange::DataIntersectionOf(items) => assert_eq!(items.len(), 2),
            other => panic!("expected DataIntersectionOf, got {other:?}"),
        }
        let (_, dr) = data_range(&ctx)("xsd:integer or xsd:string").unwrap();
        match dr {
            DataRange::DataUnionOf(items) => assert_eq!(items.len(), 2),
            other => panic!("expected DataUnionOf, got {other:?}"),
        }
        let (_, dr) = data_range(&ctx)("not xsd:boolean").unwrap();
        match dr {
            DataRange::DataComplementOf(_) => {}
            other => panic!("expected DataComplementOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_one_of() {
        let ctx = ParserContext::new();
        let (_, dr) = data_range(&ctx)("{ \"a\", \"b\" }").unwrap();
        match dr {
            DataRange::DataOneOf(vals) => assert_eq!(vals.len(), 2),
            other => panic!("expected DataOneOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_parenthesized_and_nested() {
        let ctx = ParserContext::new();
        let (_, dr) = data_range(&ctx)("(xsd:integer or xsd:string) and not xsd:boolean").unwrap();
        match dr {
            DataRange::DataIntersectionOf(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(items[0], DataRange::DataUnionOf(_)));
                assert!(matches!(items[1], DataRange::DataComplementOf(_)));
            }
            other => panic!("expected DataIntersectionOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_datatype_restriction_facets() {
        let ctx = ParserContext::new();
        let (_, dr) = data_range(&ctx)("xsd:integer[>= 0, < 100]").unwrap();
        match dr {
            DataRange::DatatypeRestriction(dt, facets) => {
                assert_eq!(dt.0.0, format!("{}integer", ingress::XSD));
                assert_eq!(facets.len(), 2);
                assert_eq!(facets[0].0.0.0, format!("{}minInclusive", ingress::XSD));
                assert_eq!(facets[1].0.0.0, format!("{}maxExclusive", ingress::XSD));
            }
            other => panic!("expected DatatypeRestriction, got {other:?}"),
        }
    }
}

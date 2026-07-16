/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! The class-expression precedence ladder:
//!
//! ```text
//! description ::= conjunction 'or' conjunction { 'or' conjunction } | conjunction
//! conjunction ::= primary 'and' primary { 'and' primary } | primary
//! primary     ::= [ 'not' ] ( restriction | atomic )
//! restriction ::= objectPropertyExpression ( 'some' | 'only' ) primary
//!               | objectPropertyExpression 'value' individual
//!               | objectPropertyExpression 'Self'
//!               | objectPropertyExpression ( 'min' | 'max' | 'exactly' ) nonNegativeInteger [ primary ]
//!               | dataPropertyExpression ( 'some' | 'only' ) dataPrimary
//!               | dataPropertyExpression 'value' literal
//!               | dataPropertyExpression ( 'min' | 'max' | 'exactly' ) nonNegativeInteger [ dataPrimary ]
//! atomic      ::= classIRI | '{' individualList '}' | '(' description ')'
//! ```
//!
//! The `classIRI 'that' [ 'not' ] restriction { 'and' [ 'not' ] restriction }`
//! alternative of `conjunction` is deferred; see
//! [#157](https://github.com/daghovland/rdf-datalog/issues/157).
//!
//! ## Object- vs data-property restriction disambiguation
//!
//! `hasTopping some Mozzarella` (object) and `hasAge some xsd:integer` (data)
//! are syntactically identical after the property name (`IRI 'some' IRI`);
//! Manchester Syntax relies on knowing each property's declared punning to
//! disambiguate. `manchester_parser` resolves this with a lightweight
//! pre-scan (see `ParserContext::is_known_data_property` /
//! `lib.rs::prescan_data_properties`): before parsing frame bodies, the
//! document is scanned once for `DataProperty:` frame headers and their
//! (prefix-resolved) IRIs are recorded. A restriction's property defaults to
//! "object" unless its IRI was recorded as a data property. `value`
//! restrictions instead disambiguate structurally (a literal token vs an
//! individual token never overlap), which is exact regardless of pre-scan.

use crate::individual::individual;
use crate::iri::{ParserContext, iri};
use crate::literal::literal;
use crate::property_expr::object_property_expression;
use crate::tokens::{keyword, punct, sp, tok};
use nom::IResult;
use nom::branch::alt;
use nom::multi::{many0, separated_list1};
use nom::sequence::delimited;
use num_bigint::BigInt;
use owl_ontology::{ClassExpression, DataRange};

fn unsigned_integer(input: &str) -> IResult<&str, BigInt> {
    let end = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    if end == 0 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    let n = BigInt::parse_bytes(&input.as_bytes()[..end], 10).ok_or_else(|| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    tok(|i: &str| Ok((&i[end..], ())))(input).map(|(rest, ())| (rest, n))
}

/// True if the remaining input, after skipping whitespace, starts with a
/// token that can only begin a `literal` (`"`, a digit, or a signed digit) —
/// as opposed to an `individual` (an IRI form or `_:nodeID`).
pub(crate) fn literal_follows(input: &str) -> bool {
    let (rest, ()) = sp(input).unwrap_or((input, ()));
    match rest.chars().next() {
        Some('"') => true,
        Some(c) if c.is_ascii_digit() => true,
        Some('+') | Some('-') => rest[1..].starts_with(|c: char| c.is_ascii_digit()),
        _ => false,
    }
}

/// `description`
pub(crate) fn description<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        let (input, first) = conjunction(ctx)(input)?;
        let (input, mut rest) =
            many0(nom::sequence::preceded(keyword("or"), conjunction(ctx)))(input)?;
        if rest.is_empty() {
            Ok((input, first))
        } else {
            let mut all = vec![first];
            all.append(&mut rest);
            Ok((input, ClassExpression::ObjectUnionOf(all)))
        }
    }
}

/// `conjunction` (without the `classIRI 'that' ...` sugar; see module docs).
fn conjunction<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        let (input, first) = primary(ctx)(input)?;
        let (input, mut rest) =
            many0(nom::sequence::preceded(keyword("and"), primary(ctx)))(input)?;
        if rest.is_empty() {
            Ok((input, first))
        } else {
            let mut all = vec![first];
            all.append(&mut rest);
            Ok((input, ClassExpression::ObjectIntersectionOf(all)))
        }
    }
}

/// `primary ::= [ 'not' ] ( restriction | atomic )`
fn primary<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        let (input, negated) = nom::combinator::opt(keyword("not"))(input)?;
        let (input, inner) = alt((restriction(ctx), atomic(ctx)))(input)?;
        if negated.is_some() {
            Ok((input, ClassExpression::ObjectComplementOf(Box::new(inner))))
        } else {
            Ok((input, inner))
        }
    }
}

fn object_restriction_tail<'a>(
    ctx: &'a ParserContext,
    prop: owl_ontology::ObjectPropertyExpression,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> + 'a {
    move |input: &'a str| {
        alt((
            nom::combinator::map(nom::sequence::preceded(keyword("some"), primary(ctx)), {
                let prop = prop.clone();
                move |filler| ClassExpression::ObjectSomeValuesFrom(prop.clone(), Box::new(filler))
            }),
            nom::combinator::map(nom::sequence::preceded(keyword("only"), primary(ctx)), {
                let prop = prop.clone();
                move |filler| ClassExpression::ObjectAllValuesFrom(prop.clone(), Box::new(filler))
            }),
            nom::combinator::map(
                nom::sequence::preceded(keyword("value"), individual(ctx)),
                {
                    let prop = prop.clone();
                    move |ind| ClassExpression::ObjectHasValue(prop.clone(), ind)
                },
            ),
            nom::combinator::map(keyword("Self"), {
                let prop = prop.clone();
                move |_| ClassExpression::ObjectHasSelf(prop.clone())
            }),
            {
                let prop = prop.clone();
                move |input: &'a str| {
                    let (input, n) =
                        nom::sequence::preceded(keyword("min"), unsigned_integer)(input)?;
                    let (input, filler) = nom::combinator::opt(primary(ctx))(input)?;
                    Ok((
                        input,
                        match filler {
                            Some(f) => ClassExpression::ObjectMinQualifiedCardinality(
                                n,
                                prop.clone(),
                                Box::new(f),
                            ),
                            None => ClassExpression::ObjectMinCardinality(n, prop.clone()),
                        },
                    ))
                }
            },
            {
                let prop = prop.clone();
                move |input: &'a str| {
                    let (input, n) =
                        nom::sequence::preceded(keyword("max"), unsigned_integer)(input)?;
                    let (input, filler) = nom::combinator::opt(primary(ctx))(input)?;
                    Ok((
                        input,
                        match filler {
                            Some(f) => ClassExpression::ObjectMaxQualifiedCardinality(
                                n,
                                prop.clone(),
                                Box::new(f),
                            ),
                            None => ClassExpression::ObjectMaxCardinality(n, prop.clone()),
                        },
                    ))
                }
            },
            {
                let prop = prop.clone();
                move |input: &'a str| {
                    let (input, n) =
                        nom::sequence::preceded(keyword("exactly"), unsigned_integer)(input)?;
                    let (input, filler) = nom::combinator::opt(primary(ctx))(input)?;
                    Ok((
                        input,
                        match filler {
                            Some(f) => ClassExpression::ObjectExactQualifiedCardinality(
                                n,
                                prop.clone(),
                                Box::new(f),
                            ),
                            None => ClassExpression::ObjectExactCardinality(n, prop.clone()),
                        },
                    ))
                }
            },
        ))(input)
    }
}

fn data_restriction_tail<'a>(
    ctx: &'a ParserContext,
    prop: owl_ontology::FullIri,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> + 'a {
    move |input: &'a str| {
        alt((
            nom::combinator::map(nom::sequence::preceded(keyword("some"), data_range(ctx)), {
                let prop = prop.clone();
                move |dr: DataRange| ClassExpression::DataSomeValuesFrom(vec![prop.clone()], dr)
            }),
            nom::combinator::map(nom::sequence::preceded(keyword("only"), data_range(ctx)), {
                let prop = prop.clone();
                move |dr: DataRange| ClassExpression::DataAllValuesFrom(vec![prop.clone()], dr)
            }),
            nom::combinator::map(nom::sequence::preceded(keyword("value"), literal(ctx)), {
                let prop = prop.clone();
                move |lit| ClassExpression::DataHasValue(prop.clone(), lit)
            }),
            {
                let prop = prop.clone();
                move |input: &'a str| {
                    let (input, n) =
                        nom::sequence::preceded(keyword("min"), unsigned_integer)(input)?;
                    let (input, filler) = nom::combinator::opt(data_range(ctx))(input)?;
                    Ok((
                        input,
                        match filler {
                            Some(f) => {
                                ClassExpression::DataMinQualifiedCardinality(n, prop.clone(), f)
                            }
                            None => ClassExpression::DataMinCardinality(n, prop.clone()),
                        },
                    ))
                }
            },
            {
                let prop = prop.clone();
                move |input: &'a str| {
                    let (input, n) =
                        nom::sequence::preceded(keyword("max"), unsigned_integer)(input)?;
                    let (input, filler) = nom::combinator::opt(data_range(ctx))(input)?;
                    Ok((
                        input,
                        match filler {
                            Some(f) => {
                                ClassExpression::DataMaxQualifiedCardinality(n, prop.clone(), f)
                            }
                            None => ClassExpression::DataMaxCardinality(n, prop.clone()),
                        },
                    ))
                }
            },
            {
                let prop = prop.clone();
                move |input: &'a str| {
                    let (input, n) =
                        nom::sequence::preceded(keyword("exactly"), unsigned_integer)(input)?;
                    let (input, filler) = nom::combinator::opt(data_range(ctx))(input)?;
                    Ok((
                        input,
                        match filler {
                            Some(f) => {
                                ClassExpression::DataExactQualifiedCardinality(n, prop.clone(), f)
                            }
                            None => ClassExpression::DataExactCardinality(n, prop.clone()),
                        },
                    ))
                }
            },
        ))(input)
    }
}

/// `restriction` — parses the property expression, then dispatches to the
/// object- or data-property restriction tail. See module docs for the
/// disambiguation strategy.
fn restriction<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        // `inverse P` can only be an object property expression.
        if let Ok((rest, prop)) = nom::sequence::preceded(
            nom::combinator::peek(keyword("inverse")),
            object_property_expression(ctx),
        )(input)
        {
            return object_restriction_tail(ctx, prop)(rest);
        }
        let (rest, name) = iri(ctx)(input)?;
        // `value` disambiguates structurally regardless of the pre-scanned
        // property-kind table.
        if let Ok((after_value, _)) = keyword("value")(rest) {
            if literal_follows(after_value) {
                return nom::combinator::map(literal(ctx), |lit| {
                    ClassExpression::DataHasValue(name.clone(), lit)
                })(after_value);
            }
            return nom::combinator::map(individual(ctx), |ind| {
                ClassExpression::ObjectHasValue(
                    owl_ontology::ObjectPropertyExpression::NamedObjectProperty(name.clone()),
                    ind,
                )
            })(after_value);
        }
        if ctx.is_known_data_property(&(name.0).0) {
            data_restriction_tail(ctx, name)(rest)
        } else {
            object_restriction_tail(
                ctx,
                owl_ontology::ObjectPropertyExpression::NamedObjectProperty(name),
            )(rest)
        }
    }
}

/// `atomic ::= classIRI | '{' individualList '}' | '(' description ')'`
fn atomic<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                delimited(
                    punct('{'),
                    separated_list1(punct(','), individual(ctx)),
                    punct('}'),
                ),
                ClassExpression::ObjectOneOf,
            ),
            delimited(punct('('), description(ctx), punct(')')),
            nom::combinator::map(iri(ctx), ClassExpression::ClassName),
        ))(input)
    }
}

fn data_range<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    crate::data_range::data_range(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use owl_ontology::{FullIri, ObjectPropertyExpression};

    fn ctx_with_default() -> ParserContext {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        ctx
    }

    fn cls(name: &str) -> ClassExpression {
        ClassExpression::ClassName(FullIri(ingress::IriReference(format!(
            "http://example.org/{name}"
        ))))
    }

    #[test]
    fn parses_atomic_class() {
        let ctx = ctx_with_default();
        let (_, e) = description(&ctx)("Pizza").unwrap();
        assert_eq!(e, cls("Pizza"));
    }

    #[test]
    fn parses_and_or_not() {
        let ctx = ctx_with_default();
        let (_, e) = description(&ctx)("Pizza and Food").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectIntersectionOf(vec![cls("Pizza"), cls("Food")])
        );
        let (_, e) = description(&ctx)("Pizza or Food").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectUnionOf(vec![cls("Pizza"), cls("Food")])
        );
        let (_, e) = description(&ctx)("not Pizza").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectComplementOf(Box::new(cls("Pizza")))
        );
    }

    #[test]
    fn parses_object_some_only_value_self() {
        let ctx = ctx_with_default();
        let prop = ObjectPropertyExpression::NamedObjectProperty(FullIri(ingress::IriReference(
            "http://example.org/hasTopping".to_string(),
        )));
        let (_, e) = description(&ctx)("hasTopping some Mozzarella").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectSomeValuesFrom(prop.clone(), Box::new(cls("Mozzarella")))
        );
        let (_, e) = description(&ctx)("hasTopping only Mozzarella").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectAllValuesFrom(prop.clone(), Box::new(cls("Mozzarella")))
        );
        let (_, e) = description(&ctx)("hasTopping Self").unwrap();
        assert_eq!(e, ClassExpression::ObjectHasSelf(prop.clone()));
    }

    #[test]
    fn parses_object_cardinalities_qualified_and_unqualified() {
        let ctx = ctx_with_default();
        let prop = ObjectPropertyExpression::NamedObjectProperty(FullIri(ingress::IriReference(
            "http://example.org/hasTopping".to_string(),
        )));
        let (_, e) = description(&ctx)("hasTopping min 2").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectMinCardinality(BigInt::from(2), prop.clone())
        );
        let (_, e) = description(&ctx)("hasTopping min 2 Mozzarella").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectMinQualifiedCardinality(
                BigInt::from(2),
                prop.clone(),
                Box::new(cls("Mozzarella"))
            )
        );
        let (_, e) = description(&ctx)("hasTopping exactly 1 Mozzarella").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectExactQualifiedCardinality(
                BigInt::from(1),
                prop,
                Box::new(cls("Mozzarella"))
            )
        );
    }

    #[test]
    fn parses_one_of_and_parens() {
        let ctx = ctx_with_default();
        let (_, e) = description(&ctx)("(Pizza or Food) and Cheap").unwrap();
        assert_eq!(
            e,
            ClassExpression::ObjectIntersectionOf(vec![
                ClassExpression::ObjectUnionOf(vec![cls("Pizza"), cls("Food")]),
                cls("Cheap"),
            ])
        );
        let (_, e) = description(&ctx)("{ Alice, Bob }").unwrap();
        match e {
            ClassExpression::ObjectOneOf(inds) => assert_eq!(inds.len(), 2),
            other => panic!("expected ObjectOneOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_data_value_disambiguated_from_object_value() {
        let ctx = ctx_with_default();
        let (_, e) = description(&ctx)("hasAge value \"42\"^^xsd:integer").unwrap();
        match e {
            ClassExpression::DataHasValue(_, _) => {}
            other => panic!("expected DataHasValue, got {other:?}"),
        }
        let (_, e) = description(&ctx)("hasFriend value Alice").unwrap();
        match e {
            ClassExpression::ObjectHasValue(_, _) => {}
            other => panic!("expected ObjectHasValue, got {other:?}"),
        }
    }

    #[test]
    fn parses_data_some_when_property_known_as_data() {
        let ctx = ctx_with_default();
        ctx.mark_data_property("http://example.org/hasAge");
        let (_, e) = description(&ctx)("hasAge some xsd:integer").unwrap();
        match e {
            ClassExpression::DataSomeValuesFrom(_, _) => {}
            other => panic!("expected DataSomeValuesFrom, got {other:?}"),
        }
    }
}

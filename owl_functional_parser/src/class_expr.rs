/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `ClassExpression` — every keyword form from
//! [the OWL 2 spec §8](https://www.w3.org/TR/owl2-syntax/#Class_Expressions).
//! Unlike Manchester Syntax's `description`/`conjunction`/`primary`
//! precedence ladder, every construct here names its own keyword, so this is
//! a flat `alt(...)` dispatch on the leading token, not a precedence climb.

use crate::data_range::data_range;
use crate::individual::individual;
use crate::iri::{ParserContext, iri};
use crate::property_expr::{data_property_expression, object_property_expression};
use crate::tokens::{many1_no_sep, non_negative_integer, paren_form};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use nom::combinator::opt;
use owl_ontology::ClassExpression;

pub(crate) fn class_expression<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form("ObjectIntersectionOf", many1_no_sep(class_expression(ctx))),
                ClassExpression::ObjectIntersectionOf,
            ),
            nom::combinator::map(
                paren_form("ObjectUnionOf", many1_no_sep(class_expression(ctx))),
                ClassExpression::ObjectUnionOf,
            ),
            nom::combinator::map(
                paren_form("ObjectComplementOf", class_expression(ctx)),
                |inner| ClassExpression::ObjectComplementOf(Box::new(inner)),
            ),
            nom::combinator::map(
                paren_form("ObjectOneOf", many1_no_sep(individual(ctx))),
                ClassExpression::ObjectOneOf,
            ),
            object_restriction(ctx),
            data_restriction(ctx),
            nom::combinator::map(iri(ctx), ClassExpression::ClassName),
        ))
        .parse(input)
    }
}

fn object_restriction<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form(
                    "ObjectSomeValuesFrom",
                    (object_property_expression(ctx), class_expression(ctx)),
                ),
                |(p, c)| ClassExpression::ObjectSomeValuesFrom(p, Box::new(c)),
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectAllValuesFrom",
                    (object_property_expression(ctx), class_expression(ctx)),
                ),
                |(p, c)| ClassExpression::ObjectAllValuesFrom(p, Box::new(c)),
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectHasValue",
                    (object_property_expression(ctx), individual(ctx)),
                ),
                |(p, i)| ClassExpression::ObjectHasValue(p, i),
            ),
            nom::combinator::map(
                paren_form("ObjectHasSelf", object_property_expression(ctx)),
                ClassExpression::ObjectHasSelf,
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectMinCardinality",
                    (
                        non_negative_integer,
                        object_property_expression(ctx),
                        opt(class_expression(ctx)),
                    ),
                ),
                |(n, p, filler)| match filler {
                    Some(c) => ClassExpression::ObjectMinQualifiedCardinality(n, p, Box::new(c)),
                    None => ClassExpression::ObjectMinCardinality(n, p),
                },
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectMaxCardinality",
                    (
                        non_negative_integer,
                        object_property_expression(ctx),
                        opt(class_expression(ctx)),
                    ),
                ),
                |(n, p, filler)| match filler {
                    Some(c) => ClassExpression::ObjectMaxQualifiedCardinality(n, p, Box::new(c)),
                    None => ClassExpression::ObjectMaxCardinality(n, p),
                },
            ),
            nom::combinator::map(
                paren_form(
                    "ObjectExactCardinality",
                    (
                        non_negative_integer,
                        object_property_expression(ctx),
                        opt(class_expression(ctx)),
                    ),
                ),
                |(n, p, filler)| match filler {
                    Some(c) => ClassExpression::ObjectExactQualifiedCardinality(n, p, Box::new(c)),
                    None => ClassExpression::ObjectExactCardinality(n, p),
                },
            ),
        ))
        .parse(input)
    }
}

fn data_restriction<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ClassExpression> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form(
                    "DataSomeValuesFrom",
                    crate::data_range::data_properties_then_range(ctx),
                ),
                |(props, dr)| ClassExpression::DataSomeValuesFrom(props, dr),
            ),
            nom::combinator::map(
                paren_form(
                    "DataAllValuesFrom",
                    crate::data_range::data_properties_then_range(ctx),
                ),
                |(props, dr)| ClassExpression::DataAllValuesFrom(props, dr),
            ),
            nom::combinator::map(
                paren_form(
                    "DataHasValue",
                    (data_property_expression(ctx), crate::literal::literal(ctx)),
                ),
                |(p, l)| ClassExpression::DataHasValue(p, l),
            ),
            nom::combinator::map(
                paren_form(
                    "DataMinCardinality",
                    (
                        non_negative_integer,
                        data_property_expression(ctx),
                        opt(data_range(ctx)),
                    ),
                ),
                |(n, p, filler)| match filler {
                    Some(dr) => ClassExpression::DataMinQualifiedCardinality(n, p, dr),
                    None => ClassExpression::DataMinCardinality(n, p),
                },
            ),
            nom::combinator::map(
                paren_form(
                    "DataMaxCardinality",
                    (
                        non_negative_integer,
                        data_property_expression(ctx),
                        opt(data_range(ctx)),
                    ),
                ),
                |(n, p, filler)| match filler {
                    Some(dr) => ClassExpression::DataMaxQualifiedCardinality(n, p, dr),
                    None => ClassExpression::DataMaxCardinality(n, p),
                },
            ),
            nom::combinator::map(
                paren_form(
                    "DataExactCardinality",
                    (
                        non_negative_integer,
                        data_property_expression(ctx),
                        opt(data_range(ctx)),
                    ),
                ),
                |(n, p, filler)| match filler {
                    Some(dr) => ClassExpression::DataExactQualifiedCardinality(n, p, dr),
                    None => ClassExpression::DataExactCardinality(n, p),
                },
            ),
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
    fn parses_class_name() {
        let ctx = ctx_with_default_prefix();
        let (_, ce) = class_expression(&ctx)(":Pizza").unwrap();
        assert_eq!(
            ce,
            ClassExpression::ClassName(owl_ontology::FullIri(ingress::IriReference(
                "http://example.org/Pizza".to_string()
            )))
        );
    }

    #[test]
    fn parses_intersection_union_complement() {
        let ctx = ctx_with_default_prefix();
        let (_, ce) = class_expression(&ctx)("ObjectIntersectionOf(:Food :Pizza)").unwrap();
        match ce {
            ClassExpression::ObjectIntersectionOf(v) => assert_eq!(v.len(), 2),
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        }
        let (_, ce2) = class_expression(&ctx)("ObjectUnionOf(:Food :Pizza)").unwrap();
        match ce2 {
            ClassExpression::ObjectUnionOf(v) => assert_eq!(v.len(), 2),
            other => panic!("expected ObjectUnionOf, got {other:?}"),
        }
        let (_, ce3) = class_expression(&ctx)("ObjectComplementOf(:Pizza)").unwrap();
        match ce3 {
            ClassExpression::ObjectComplementOf(_) => {}
            other => panic!("expected ObjectComplementOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_object_one_of() {
        let ctx = ctx_with_default_prefix();
        let (_, ce) = class_expression(&ctx)("ObjectOneOf(:Alice :Bob)").unwrap();
        match ce {
            ClassExpression::ObjectOneOf(v) => assert_eq!(v.len(), 2),
            other => panic!("expected ObjectOneOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_object_some_and_all_values_from() {
        let ctx = ctx_with_default_prefix();
        let (_, ce) = class_expression(&ctx)("ObjectSomeValuesFrom(:hasTopping :Topping)").unwrap();
        match ce {
            ClassExpression::ObjectSomeValuesFrom(_, _) => {}
            other => panic!("expected ObjectSomeValuesFrom, got {other:?}"),
        }
        let (_, ce2) = class_expression(&ctx)("ObjectAllValuesFrom(:hasTopping :Topping)").unwrap();
        match ce2 {
            ClassExpression::ObjectAllValuesFrom(_, _) => {}
            other => panic!("expected ObjectAllValuesFrom, got {other:?}"),
        }
    }

    #[test]
    fn parses_object_has_value_and_has_self() {
        let ctx = ctx_with_default_prefix();
        let (_, ce) = class_expression(&ctx)("ObjectHasValue(:hasTopping :Mushroom)").unwrap();
        match ce {
            ClassExpression::ObjectHasValue(_, _) => {}
            other => panic!("expected ObjectHasValue, got {other:?}"),
        }
        let (_, ce2) = class_expression(&ctx)("ObjectHasSelf(:likes)").unwrap();
        match ce2 {
            ClassExpression::ObjectHasSelf(_) => {}
            other => panic!("expected ObjectHasSelf, got {other:?}"),
        }
    }

    #[test]
    fn parses_object_cardinalities_qualified_and_unqualified() {
        let ctx = ctx_with_default_prefix();
        let (_, ce) = class_expression(&ctx)("ObjectMinCardinality(1 :hasTopping)").unwrap();
        assert!(
            matches!(ce, ClassExpression::ObjectMinCardinality(n, _) if n == num_bigint::BigInt::from(1))
        );

        let (_, ce2) =
            class_expression(&ctx)("ObjectMinCardinality(1 :hasTopping :Topping)").unwrap();
        assert!(matches!(
            ce2,
            ClassExpression::ObjectMinQualifiedCardinality(_, _, _)
        ));

        let (_, ce3) = class_expression(&ctx)("ObjectMaxCardinality(3 :hasTopping)").unwrap();
        assert!(matches!(ce3, ClassExpression::ObjectMaxCardinality(_, _)));

        let (_, ce4) = class_expression(&ctx)("ObjectExactCardinality(2 :hasTopping)").unwrap();
        assert!(matches!(ce4, ClassExpression::ObjectExactCardinality(_, _)));
    }

    #[test]
    fn parses_data_restrictions() {
        let ctx = ctx_with_default_prefix();
        ctx.declare_prefix("xsd", ingress::XSD);
        let (_, ce) = class_expression(&ctx)("DataSomeValuesFrom(:hasAge xsd:integer)").unwrap();
        match ce {
            ClassExpression::DataSomeValuesFrom(props, _) => assert_eq!(props.len(), 1),
            other => panic!("expected DataSomeValuesFrom, got {other:?}"),
        }
        let (_, ce2) = class_expression(&ctx)("DataHasValue(:hasAge \"42\")").unwrap();
        match ce2 {
            ClassExpression::DataHasValue(_, _) => {}
            other => panic!("expected DataHasValue, got {other:?}"),
        }
        let (_, ce3) = class_expression(&ctx)("DataMinCardinality(1 :hasAge)").unwrap();
        assert!(matches!(ce3, ClassExpression::DataMinCardinality(_, _)));
        let (_, ce4) = class_expression(&ctx)("DataMinCardinality(1 :hasAge xsd:integer)").unwrap();
        assert!(matches!(
            ce4,
            ClassExpression::DataMinQualifiedCardinality(_, _, _)
        ));
    }

    #[test]
    fn parses_nested_class_expression() {
        let ctx = ctx_with_default_prefix();
        let (_, ce) = class_expression(&ctx)(
            "ObjectIntersectionOf(:Food ObjectSomeValuesFrom(:hasTopping :Topping))",
        )
        .unwrap();
        match ce {
            ClassExpression::ObjectIntersectionOf(v) => {
                assert_eq!(v.len(), 2);
                assert!(matches!(v[1], ClassExpression::ObjectSomeValuesFrom(_, _)));
            }
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        }
    }
}

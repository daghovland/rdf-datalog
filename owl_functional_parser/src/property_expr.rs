/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `ObjectPropertyExpression ::= ObjectProperty | InverseObjectProperty`
//! `InverseObjectProperty ::= 'ObjectInverseOf' '(' ObjectProperty ')'`
//! `DataPropertyExpression ::= DataProperty`
//!
//! `propertyExpressionChain ::= 'ObjectPropertyChain' '(' ObjectPropertyExpression
//! ObjectPropertyExpression { ObjectPropertyExpression } ')'` (phase 11,
//! see `axiom.rs::sub_object_property_of`).

use crate::iri::{ParserContext, iri};
use crate::tokens::paren_form;
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use owl_ontology::{DataProperty, ObjectPropertyExpression};

pub(crate) fn object_property_expression<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ObjectPropertyExpression> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(
                paren_form("ObjectInverseOf", object_property_expression(ctx)),
                |inner| ObjectPropertyExpression::InverseObjectProperty(Box::new(inner)),
            ),
            nom::combinator::map(iri(ctx), ObjectPropertyExpression::NamedObjectProperty),
        ))
        .parse(input)
    }
}

/// `DataPropertyExpression ::= DataProperty` — always a bare IRI, no
/// wrapper keyword.
pub(crate) fn data_property_expression<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataProperty> {
    iri(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_inverse_object_property() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        let (_, named) = object_property_expression(&ctx)(":hasTopping").unwrap();
        assert_eq!(
            named,
            ObjectPropertyExpression::NamedObjectProperty(owl_ontology::FullIri(
                ingress::IriReference("http://example.org/hasTopping".to_string())
            ))
        );
        let (_, inv) = object_property_expression(&ctx)("ObjectInverseOf(:hasTopping)").unwrap();
        assert_eq!(
            inv,
            ObjectPropertyExpression::InverseObjectProperty(Box::new(
                ObjectPropertyExpression::NamedObjectProperty(owl_ontology::FullIri(
                    ingress::IriReference("http://example.org/hasTopping".to_string())
                ))
            ))
        );
    }
}

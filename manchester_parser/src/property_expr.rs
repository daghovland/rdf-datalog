/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `objectPropertyExpression ::= objectPropertyIRI | 'inverse' objectPropertyIRI`
//! `dataPropertyExpression   ::= dataPropertyIRI`

use crate::iri::{ParserContext, iri};
use crate::tokens::keyword;
use nom::IResult;
use nom::branch::alt;
use owl_ontology::ObjectPropertyExpression;

pub(crate) fn object_property_expression<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, ObjectPropertyExpression> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(nom::sequence::preceded(keyword("inverse"), iri(ctx)), |p| {
                ObjectPropertyExpression::InverseObjectProperty(Box::new(
                    ObjectPropertyExpression::NamedObjectProperty(p),
                ))
            }),
            nom::combinator::map(iri(ctx), ObjectPropertyExpression::NamedObjectProperty),
        ))(input)
    }
}

// `dataPropertyExpression ::= dataPropertyIRI` is just a resolved IRI in this
// data model (no wrapper type analogous to `ObjectPropertyExpression`), so
// callers use `iri::iri` directly rather than a dedicated function here.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_inverse_object_property() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        let (_, p) = object_property_expression(&ctx)("hasTopping").unwrap();
        assert_eq!(
            p,
            ObjectPropertyExpression::NamedObjectProperty(owl_ontology::FullIri(
                ingress::IriReference("http://example.org/hasTopping".to_string())
            ))
        );
        let (_, inv) = object_property_expression(&ctx)("inverse hasTopping").unwrap();
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

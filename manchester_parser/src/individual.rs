/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `individual ::= individualIRI | nodeID`

use crate::iri::{ParserContext, iri, node_id};
use nom::IResult;
use nom::branch::alt;
use owl_ontology::Individual;

pub(crate) fn individual<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Individual> {
    move |input: &'a str| {
        alt((
            nom::combinator::map(node_id, |label: String| {
                Individual::AnonymousIndividual(ctx.anon_individual_for_label(&label))
            }),
            nom::combinator::map(iri(ctx), Individual::NamedIndividual),
        ))(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_anonymous_individual() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        let (_, named) = individual(&ctx)("Alice").unwrap();
        assert_eq!(
            named,
            Individual::NamedIndividual(owl_ontology::FullIri(ingress::IriReference(
                "http://example.org/Alice".to_string()
            )))
        );
        let (_, anon1) = individual(&ctx)("_:x").unwrap();
        let (_, anon2) = individual(&ctx)("_:x").unwrap();
        assert_eq!(anon1, anon2);
    }
}

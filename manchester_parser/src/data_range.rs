/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `dataRange` — this implementation only supports the bare named-datatype
//! alternative of `dataAtomic` (`dataRange ::= Datatype`). Compound data
//! ranges (`and`/`or`/`not`/`{literalList}`) and `datatypeRestriction`
//! (facets) are deferred; see
//! [#157](https://github.com/daghovland/rdf-datalog/issues/157) and
//! `docs/plans/MANCHESTER_SYNTAX_PLAN.md`.

use crate::iri::{ParserContext, iri};
use nom::IResult;
use owl_ontology::DataRange;

pub(crate) fn data_range<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, DataRange> {
    nom::combinator::map(iri(ctx), DataRange::NamedDataRange)
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
}

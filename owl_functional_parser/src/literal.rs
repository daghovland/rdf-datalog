/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `Literal ::= typedLiteral | stringLiteralNoLanguage | stringLiteralWithLanguage`
//! `typedLiteral ::= lexicalForm '^^' Datatype`
//!
//! Unlike Manchester Syntax, Functional-Style Syntax has no bare numeric
//! literal shorthand (`42`, `3.5`) — every literal is a quoted string,
//! optionally typed or language-tagged. Produces an [`ingress::GraphElement`]
//! (always the `GraphLiteral` variant).

use crate::iri::{ParserContext, iri};
use crate::tokens::{sp, tok};
use ingress::{GraphElement, RdfLiteral};
use nom::IResult;

/// A quoted string, `"..."`, with `\"` and `\\` escapes.
fn quoted_string(input: &str) -> IResult<&str, String> {
    let (input, _) = nom::character::complete::char('"')(input)?;
    let mut out = String::new();
    let mut chars = input.char_indices();
    loop {
        match chars.next() {
            None => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Tag,
                )));
            }
            Some((i, '"')) => {
                let rest = &input[i + 1..];
                let (rest, ()) = sp(rest)?;
                return Ok((rest, out));
            }
            Some((_, '\\')) => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, c)) => out.push(c),
                None => {
                    return Err(nom::Err::Error(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::Tag,
                    )));
                }
            },
            Some((_, c)) => out.push(c),
        }
    }
}

/// `@` languageTag, e.g. `@en`.
fn language_tag(input: &str) -> IResult<&str, String> {
    let (input, _) = nom::character::complete::char('@')(input)?;
    tok(|i: &str| {
        let end = i
            .find(|c: char| !(c.is_alphanumeric() || c == '-'))
            .unwrap_or(i.len());
        if end == 0 {
            return Err(nom::Err::Error(nom::error::Error::new(
                i,
                nom::error::ErrorKind::Alpha,
            )));
        }
        Ok((&i[end..], i[..end].to_string()))
    })(input)
}

/// Full `Literal` production, resolved against `ctx`'s prefixes for the
/// `^^Datatype` case.
pub(crate) fn literal<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, GraphElement> {
    move |input: &'a str| {
        let (rest, s) = quoted_string(input)?;
        if let Some(rest2) = rest.strip_prefix("^^") {
            let (rest2, ()) = sp(rest2)?;
            let (rest2, dt) = iri(ctx)(rest2)?;
            return Ok((
                rest2,
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                    type_iri: dt.0,
                    literal: s,
                }),
            ));
        }
        if let Ok((rest2, lang)) = language_tag(rest) {
            return Ok((
                rest2,
                GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, literal: s }),
            ));
        }
        Ok((
            rest,
            GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_string() {
        let ctx = ParserContext::new();
        let (_, lit) = literal(&ctx)("\"hello\" rest").unwrap();
        assert_eq!(
            lit,
            GraphElement::GraphLiteral(RdfLiteral::LiteralString("hello".to_string()))
        );
    }

    #[test]
    fn parses_typed_literal() {
        let ctx = ParserContext::new();
        let (_, lit) = literal(&ctx)("\"42\"^^xsd:integer rest").unwrap();
        assert_eq!(
            lit,
            GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                type_iri: ingress::IriReference(format!("{}integer", ingress::XSD)),
                literal: "42".to_string(),
            })
        );
    }

    #[test]
    fn parses_language_tagged_literal() {
        let ctx = ParserContext::new();
        let (_, lit) = literal(&ctx)("\"Pizza\"@en rest").unwrap();
        assert_eq!(
            lit,
            GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
                lang: "en".to_string(),
                literal: "Pizza".to_string(),
            })
        );
    }
}

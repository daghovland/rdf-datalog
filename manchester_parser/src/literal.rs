/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `literal ::= typedLiteral | stringLiteralNoLanguage | stringLiteralWithLanguage`
//! `         | integerLiteral | decimalLiteral | floatingPointLiteral`
//!
//! Produces an [`ingress::GraphElement`] (always the `GraphLiteral` variant).

use crate::iri::{ParserContext, iri};
use crate::tokens::{sp, tok};
use ingress::{GraphElement, RdfLiteral};
use nom::IResult;
use num_bigint::BigInt;
use ordered_float::OrderedFloat;
use rust_decimal::Decimal;
use std::str::FromStr;

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

fn number_str(input: &str) -> IResult<&str, &str> {
    let mut end = 0;
    let bytes = input.as_bytes();
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits_start {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    if end < bytes.len() && bytes[end] == b'.' {
        let dot = end;
        let mut frac_end = end + 1;
        while frac_end < bytes.len() && bytes[frac_end].is_ascii_digit() {
            frac_end += 1;
        }
        if frac_end > dot + 1 {
            end = frac_end;
        }
    }
    Ok((&input[end..], &input[..end]))
}

/// `integerLiteral | decimalLiteral | floatingPointLiteral`. A trailing `f`/`F`
/// marks a float; a `.` with no trailing `f` marks a decimal; otherwise an
/// integer.
fn numeric_literal(input: &str) -> IResult<&str, RdfLiteral> {
    let (rest, num) = number_str(input)?;
    let is_decimal = num.contains('.');
    if let Some(rest2) = rest.strip_prefix(['f', 'F']) {
        let (rest2, ()) = sp(rest2)?;
        let value: f64 = num.parse().map_err(|_| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Float))
        })?;
        return Ok((rest2, RdfLiteral::FloatLiteral(OrderedFloat(value))));
    }
    let (rest, ()) = sp(rest)?;
    if is_decimal {
        let value = Decimal::from_str(num).map_err(|_| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Float))
        })?;
        Ok((rest, RdfLiteral::DecimalLiteral(value)))
    } else {
        let value = BigInt::from_str(num).map_err(|_| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
        })?;
        Ok((rest, RdfLiteral::IntegerLiteral(value)))
    }
}

/// Full `literal` production, resolved against `ctx`'s prefixes for the
/// `^^Datatype` case.
pub(crate) fn literal<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, GraphElement> {
    move |input: &'a str| {
        if input.starts_with('"') {
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
            return Ok((
                rest,
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)),
            ));
        }
        let (rest, lit) = numeric_literal(input)?;
        Ok((rest, GraphElement::GraphLiteral(lit)))
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
    fn parses_integer_and_decimal_and_float() {
        let ctx = ParserContext::new();
        assert_eq!(
            literal(&ctx)("42 ").unwrap().1,
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(42)))
        );
        assert_eq!(
            literal(&ctx)("3.5 ").unwrap().1,
            GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(
                Decimal::from_str("3.5").unwrap()
            ))
        );
        assert_eq!(
            literal(&ctx)("3.5f ").unwrap().1,
            GraphElement::GraphLiteral(RdfLiteral::FloatLiteral(OrderedFloat(3.5)))
        );
    }
}

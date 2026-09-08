/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Low-level tokenizing helpers: whitespace/comment skipping, identifier
//! character classes, and the `'Keyword' '(' inner ')'` combinator that
//! every Functional-Style Syntax production is built from.
//!
//! See [`docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`](../../../docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md).

use nom::IResult;
use nom::Parser;

/// Skip zero or more whitespace characters or `#`-to-end-of-line comments.
/// The W3C grammar itself defines no comment syntax; `#` comments are a
/// widely used convention (mirroring `manchester_parser::tokens::sp`) that
/// costs nothing to support and matches this codebase's other parsers.
pub(crate) fn sp(input: &str) -> IResult<&str, ()> {
    let mut rest = input;
    loop {
        let ws_end = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        rest = &rest[ws_end..];
        if rest.starts_with('#') {
            let line_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
            rest = &rest[line_end..];
            continue;
        }
        break;
    }
    Ok((rest, ()))
}

/// Run `inner`, then skip trailing whitespace/comments.
pub(crate) fn tok<'a, O, F>(mut inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    move |input: &'a str| {
        let (input, out) = inner(input)?;
        let (input, ()) = sp(input)?;
        Ok((input, out))
    }
}

/// Characters valid inside an identifier (prefix name or local name), a
/// conservative superset of the Turtle `PN_CHARS` rule.
pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
}

/// Match a single punctuation character (e.g. `(`, `)`, `=`), consuming
/// trailing whitespace/comments.
pub(crate) fn punct<'a>(c: char) -> impl FnMut(&'a str) -> IResult<&'a str, char> {
    move |input: &'a str| tok(nom::character::complete::char(c))(input)
}

/// Match a bare keyword name (e.g. `SubClassOf`), requiring the next
/// character to be `(` (functional syntax never uses a keyword bare — every
/// production is immediately followed by its parenthesized argument list),
/// so no separate word-boundary check is needed beyond that.
pub(crate) fn keyword<'a>(word: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &'a str| {
        if let Some(rest) = input.strip_prefix(word) {
            let (rest_sp, ()) = sp(rest)?;
            if rest_sp.starts_with('(') {
                return Ok((rest_sp, word));
            }
        }
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    }
}

/// `'Name' '(' inner ')'`, the fundamental shape of every Functional-Style
/// Syntax production. Skips whitespace/comments after the keyword, after
/// `(`, and after `)`.
///
/// `inner` takes anything implementing `nom::Parser` (not just a bare
/// `FnMut`) so a tuple of sub-parsers -- nom 8's `Parser` blanket impl for
/// tuples up to arity 21 -- can be passed directly, e.g.
/// `paren_form("Foo", (a_parser, b_parser))`, without a separate
/// `nom::sequence::tuple` wrapper.
pub(crate) fn paren_form<'a, O, F>(
    name: &'static str,
    mut inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: Parser<&'a str, Output = O, Error = nom::error::Error<&'a str>>,
{
    move |input: &'a str| {
        let (input, _) = keyword(name)(input)?;
        let (input, _) = punct('(')(input)?;
        let (input, out) = inner.parse(input)?;
        let (input, _) = punct(')')(input)?;
        Ok((input, out))
    }
}

/// A non-negative integer (used for cardinality restriction bounds).
pub(crate) fn non_negative_integer(input: &str) -> IResult<&str, num_bigint::BigInt> {
    tok(|i: &str| {
        let end = i.find(|c: char| !c.is_ascii_digit()).unwrap_or(i.len());
        if end == 0 {
            return Err(nom::Err::Error(nom::error::Error::new(
                i,
                nom::error::ErrorKind::Digit,
            )));
        }
        let value = num_bigint::BigInt::parse_bytes(&i.as_bytes()[..end], 10).ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(i, nom::error::ErrorKind::Digit))
        })?;
        Ok((&i[end..], value))
    })(input)
}

/// One or more `P`s, in sequence, with no separator (the functional-syntax
/// convention for e.g. `ClassExpression ClassExpression { ClassExpression }`
/// argument lists — no commas between siblings, unlike Manchester).
pub(crate) fn many1_no_sep<'a, O, F>(mut p: F) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<O>>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    move |input: &'a str| {
        let mut out = Vec::new();
        let mut rest = input;
        loop {
            match p(rest) {
                Ok((next, item)) => {
                    out.push(item);
                    rest = next;
                }
                Err(_) if !out.is_empty() => break,
                Err(e) => return Err(e),
            }
        }
        Ok((rest, out))
    }
}

/// Zero or more `P`s, in sequence, with no separator.
pub(crate) fn many0_no_sep<'a, O, F>(mut p: F) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<O>>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    move |input: &'a str| {
        let mut out = Vec::new();
        let mut rest = input;
        while let Ok((next, item)) = p.parse(rest) {
            out.push(item);
            rest = next;
        }
        Ok((rest, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_requires_following_open_paren() {
        assert!(keyword("Class")("ClassAssertion(...)").is_err());
        assert!(keyword("Class")("Class(:Pizza)").is_ok());
    }

    #[test]
    fn paren_form_parses_and_strips_delimiters() {
        let (rest, inner) = paren_form("Foo", |i| nom::bytes::complete::tag("bar")(i))
            .parse("Foo(bar) rest")
            .unwrap();
        assert_eq!(inner, "bar");
        assert_eq!(rest, "rest");
    }
}

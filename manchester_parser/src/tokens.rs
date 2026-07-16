/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Low-level tokenizing helpers shared across the Manchester Syntax parser:
//! whitespace/comment skipping, word-boundary-aware keyword matching, and
//! identifier character classes.
//!
//! See [`docs/plans/MANCHESTER_SYNTAX_PLAN.md`](../../../docs/plans/MANCHESTER_SYNTAX_PLAN.md).

use nom::IResult;

/// Skip zero or more whitespace characters or `#`-to-end-of-line comments.
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

/// Characters valid inside a Manchester Syntax identifier (prefix name or
/// local name), a conservative superset of the Turtle `PN_CHARS` rule.
pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

/// Match a bare identifier (used for prefix names, local names, and as the
/// fallback grammar production for keywords), consuming trailing whitespace.
pub(crate) fn identifier(input: &str) -> IResult<&str, &str> {
    let mut chars = input.char_indices();
    match chars.next() {
        Some((_, c)) if is_ident_start(c) => {}
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Alpha,
            )));
        }
    }
    let end = chars
        .find(|(_, c)| !is_ident_char(*c))
        .map(|(i, _)| i)
        .unwrap_or(input.len());
    tok(|i| Ok((&i[end..], &i[..end])))(input)
}

/// Match an exact keyword, requiring a following non-identifier character (or
/// end of input) so that e.g. `and`/`or`/`not`/`some`/`only` don't match a
/// prefix of a longer identifier. Consumes trailing whitespace/comments.
pub(crate) fn keyword<'a>(word: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &'a str| {
        if let Some(rest) = input.strip_prefix(word) {
            let boundary_ok = match rest.chars().next() {
                None => true,
                Some(c) => !is_ident_char(c),
            };
            if boundary_ok {
                return tok(|i: &'a str| Ok((&i[word.len()..], &i[..word.len()])))(input);
            }
        }
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    }
}

/// Match a single punctuation character (e.g. `:`, `(`, `)`, `{`, `}`, `,`),
/// consuming trailing whitespace/comments.
pub(crate) fn punct<'a>(c: char) -> impl FnMut(&'a str) -> IResult<&'a str, char> {
    move |input: &'a str| tok(nom::character::complete::char(c))(input)
}

/// Frame- and section-header keywords (always written with a trailing `:`
/// in `.omn` source, e.g. `Class:`, `SubClassOf:`). These are reserved: a
/// bare, unprefixed name cannot equal one of these words, since it would
/// otherwise be ambiguous with the start of the next frame/section whenever
/// it follows an optional grammar position (e.g. an unqualified cardinality
/// restriction's absent filler, `hasTopping min 2` immediately followed by
/// `Class: NextThing ...`). A name equal to one of these words is still
/// reachable via an explicit prefix, e.g. `:Class`.
const RESERVED_SIMPLE_NAMES: &[&str] = &[
    "Prefix",
    "Ontology",
    "Import",
    "Class",
    "ObjectProperty",
    "DataProperty",
    "AnnotationProperty",
    "Individual",
    "Datatype",
    "Annotations",
    "SubClassOf",
    "EquivalentTo",
    "DisjointWith",
    "DisjointUnionOf",
    "HasKey",
    "Domain",
    "Range",
    "Characteristics",
    "SubPropertyOf",
    "InverseOf",
    "SubPropertyChain",
    "Types",
    "Facts",
    "SameAs",
    "DifferentFrom",
    "EquivalentClasses",
    "DisjointClasses",
    "EquivalentProperties",
    "DisjointProperties",
    "SameIndividual",
    "DifferentIndividuals",
    "Rule",
];

/// Whether `name` is one of the reserved frame/section keywords (see
/// [`RESERVED_SIMPLE_NAMES`]) and therefore cannot be resolved as a bare
/// `simpleIRI`.
pub(crate) fn is_reserved_simple_name(name: &str) -> bool {
    RESERVED_SIMPLE_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_does_not_match_prefix_of_identifier() {
        assert!(keyword("and")("android").is_err());
        assert!(keyword("and")("and Foo").is_ok());
        assert!(keyword("and")("and(Foo)").is_ok());
    }

    #[test]
    fn identifier_reads_local_name() {
        let (rest, id) = identifier("hasTopping some Pizza").unwrap();
        assert_eq!(id, "hasTopping");
        assert_eq!(rest, "some Pizza");
    }
}

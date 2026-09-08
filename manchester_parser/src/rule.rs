/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SWRL `Rule:` frames.
//!
//! `rule       ::= 'Rule:' [annotations] atomList '->' atomList`
//! `atomList   ::= atom { ',' atom }`
//! `atom       ::= classAtom | genericAtom`
//! `classAtom  ::= description '(' atomArg ')'`
//! `genericAtom ::= iri '(' atomArg { ',' atomArg } ')'`
//! `atomArg    ::= '?' identifier | literal | individual`
//!
//! See the "`Rule:` SWRL frames" addendum in
//! `docs/plans/MANCHESTER_SYNTAX_PLAN.md` for why `classAtom` is tried first
//! (every unary atom parses as `Atom::ClassAtom`) and why there is no
//! reachable arity-1 `Atom::BuiltInAtom`.

use crate::annotation::opt_leading_annotations;
use crate::class_expr::description;
use crate::individual::individual;
use crate::iri::{ParserContext, iri};
use crate::literal::literal;
use crate::tokens::{is_ident_char, punct, tok};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use nom::multi::separated_list1;
use owl_ontology::{Atom, AtomArg, SwrlRule};

/// `'?' identifier`, e.g. `?p`.
fn variable_arg(input: &str) -> IResult<&str, AtomArg> {
    let (input, _) = nom::character::complete::char('?')(input)?;
    tok(|i: &str| {
        let end = i.find(|c: char| !is_ident_char(c)).unwrap_or(i.len());
        if end == 0 {
            return Err(nom::Err::Error(nom::error::Error::new(
                i,
                nom::error::ErrorKind::Alpha,
            )));
        }
        Ok((&i[end..], i[..end].to_string()))
    })
    .parse(input)
    .map(|(rest, name)| (rest, AtomArg::Variable(name)))
}

/// `atomArg ::= '?' identifier | literal | individual`.
fn atom_arg<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, AtomArg> {
    move |input: &'a str| {
        alt((
            variable_arg,
            nom::combinator::map(literal(ctx), AtomArg::Literal),
            nom::combinator::map(individual(ctx), AtomArg::Individual),
        ))
        .parse(input)
    }
}

/// `classAtom ::= description '(' atomArg ')'`.
fn class_atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        let (input, cls) = description(ctx)(input)?;
        let (input, _) = punct('(')(input)?;
        let (input, arg) = atom_arg(ctx)(input)?;
        let (input, _) = punct(')')(input)?;
        Ok((input, Atom::ClassAtom(cls, arg)))
    }
}

/// `genericAtom ::= iri '(' atomArg { ',' atomArg } ')'`. Arity 2 is
/// represented as `Atom::PropertyAtom`; any other arity (0, 3+ — arity 1 is
/// always consumed by `class_atom` first) as `Atom::BuiltInAtom`.
fn generic_atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        let (input, pred) = iri(ctx)(input)?;
        let (input, _) = punct('(')(input)?;
        let (input, args) = separated_list1(punct(','), atom_arg(ctx)).parse(input)?;
        let (input, _) = punct(')')(input)?;
        let atom = if let [a, b] = args.as_slice() {
            Atom::PropertyAtom(pred, a.clone(), b.clone())
        } else {
            Atom::BuiltInAtom(pred, args)
        };
        Ok((input, atom))
    }
}

/// `atom ::= classAtom | genericAtom`.
fn atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| alt((class_atom(ctx), generic_atom(ctx))).parse(input)
}

/// `atomList ::= atom { ',' atom }`.
fn atom_list<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Atom>> {
    move |input: &'a str| separated_list1(punct(','), atom(ctx)).parse(input)
}

/// `'->'`, the SWRL implication arrow.
fn arrow(input: &str) -> IResult<&str, &str> {
    tok(nom::bytes::complete::tag("->")).parse(input)
}

/// `rule ::= 'Rule:' [annotations] atomList '->' atomList`.
pub(crate) fn rule_frame<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, SwrlRule> {
    move |input: &'a str| {
        let (input, _) = crate::tokens::keyword("Rule:")(input)?;
        let (input, annotations) = opt_leading_annotations(ctx)(input)?;
        let (input, body) = atom_list(ctx)(input)?;
        let (input, _) = arrow(input)?;
        let (input, head) = atom_list(ctx)(input)?;
        Ok((
            input,
            SwrlRule {
                annotations,
                body,
                head,
            },
        ))
    }
}

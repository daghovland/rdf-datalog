/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SWRL `DLSafeRule(...)` parsing (issue
//! [#625](https://github.com/daghovland/rdf-datalog/issues/625)).
//!
//! `DLSafeRule` is **not** part of the W3C OWL 2 Functional-Style Syntax
//! specification (<https://www.w3.org/TR/owl2-syntax/> defines no
//! `Rule`/`DLSafeRule`/`Body`/`Head`/`Atom` grammar anywhere, including its
//! §11 "Global Restrictions on Axioms in OWL 2 DL", which covers property
//! hierarchy/axiom-closure restrictions, not rules) — it's a convention from
//! the OWL API / Protégé for serializing SWRL rules in functional-style
//! syntax. See `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`'s "SWRL
//! `DLSafeRule(...)` (#625)" section for the grammar this targets and the
//! mapping onto `owl_ontology::SwrlRule`/`Atom`/`AtomArg` (added by
//! [#498](https://github.com/daghovland/rdf-datalog/issues/498)/PR #636 for
//! `manchester_parser`'s `Rule:` frames, reused directly here — no new
//! `owl_ontology` types needed beyond three `Atom` variants for atom kinds
//! that Manchester's concrete syntax can't reach at all:
//! `DataRangeAtom`/`SameIndividualAtom`/`DifferentIndividualsAtom`).
//!
//! ```text
//! DLSafeRule ::= 'DLSafeRule' '(' axiomAnnotations 'Body' '(' {Atom} ')' 'Head' '(' {Atom} ')' ')'
//! Atom       ::= ClassAtom | DataRangeAtom | ObjectPropertyAtom | DataPropertyAtom
//!              | BuiltInAtom | SameIndividualAtom | DifferentIndividualsAtom
//! IArg       ::= Individual | 'Variable' '(' IRI ')'
//! DArg       ::= Literal | 'Variable' '(' IRI ')'
//! ```

use crate::annotation::axiom_annotations;
use crate::class_expr::class_expression;
use crate::data_range::data_range;
use crate::individual::individual;
use crate::iri::{ParserContext, iri};
use crate::literal::literal;
use crate::property_expr::{data_property_expression, object_property_expression};
use crate::tokens::{many0_no_sep, paren_form};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use owl_ontology::{Atom, AtomArg, ObjectPropertyExpression, SwrlRule};

/// `'Variable' '(' IRI ')'`, e.g. `Variable(:x)`. Stores the full resolved
/// IRI string (unlike Manchester's `Rule:` frames, which store the bare
/// name after `?` — see `AtomArg::Variable`'s doc comment).
fn variable_arg<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, AtomArg> {
    move |input: &'a str| {
        nom::combinator::map(paren_form("Variable", iri(ctx)), |i| {
            AtomArg::Variable(i.0.0)
        })
        .parse(input)
    }
}

/// `IArg ::= Individual | 'Variable' '(' IRI ')'`.
fn i_arg<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, AtomArg> {
    move |input: &'a str| {
        alt((
            variable_arg(ctx),
            nom::combinator::map(individual(ctx), AtomArg::Individual),
        ))
        .parse(input)
    }
}

/// `DArg ::= Literal | 'Variable' '(' IRI ')'`. Tried variable-first: a bare
/// `Variable(...)` is not itself a valid `Literal` production, so order
/// doesn't strictly matter here, but matches `i_arg`'s shape for symmetry.
fn d_arg<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, AtomArg> {
    move |input: &'a str| {
        alt((
            variable_arg(ctx),
            nom::combinator::map(literal(ctx), AtomArg::Literal),
        ))
        .parse(input)
    }
}

/// `ClassAtom ::= 'ClassAtom' '(' ClassExpression IArg ')'`.
fn class_atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        paren_form("ClassAtom", (class_expression(ctx), i_arg(ctx)))
            .parse(input)
            .map(|(rest, (ce, arg))| (rest, Atom::ClassAtom(ce, arg)))
    }
}

/// `DataRangeAtom ::= 'DataRangeAtom' '(' DataRange DArg ')'`.
fn data_range_atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        paren_form("DataRangeAtom", (data_range(ctx), d_arg(ctx)))
            .parse(input)
            .map(|(rest, (dr, arg))| (rest, Atom::DataRangeAtom(dr, arg)))
    }
}

/// Peel off any number of `ObjectInverseOf(...)` wrappers, returning the
/// innermost named property and whether the total nesting depth is odd
/// (i.e. whether the two atom arguments end up swapped).
fn unwrap_inverse(ope: ObjectPropertyExpression) -> (ObjectPropertyExpression, bool) {
    match ope {
        ObjectPropertyExpression::InverseObjectProperty(inner) => {
            let (base, swapped) = unwrap_inverse(*inner);
            (base, !swapped)
        }
        other => (other, false),
    }
}

/// `ObjectPropertyAtom ::= 'ObjectPropertyAtom' '(' ObjectPropertyExpression IArg IArg ')'`.
///
/// `owl_ontology::Atom::PropertyAtom` carries a plain `Iri` predicate, not a
/// general `ObjectPropertyExpression` (matching Manchester's `Rule:` frames,
/// which have the same restriction). `ObjectInverseOf(P)(x, y)` is
/// semantically `P(y, x)`, so an inverse property atom is represented by
/// swapping the two arguments onto the underlying named property — exact,
/// no new type needed. A non-atomic property expression (there is none in
/// OWL 2 besides `ObjectInverseOf`) would have nothing to fall back to, but
/// `ObjectPropertyExpression` has no other non-atomic variant.
fn object_property_atom<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        let (rest, (ope, a, b)) = paren_form(
            "ObjectPropertyAtom",
            (object_property_expression(ctx), i_arg(ctx), i_arg(ctx)),
        )
        .parse(input)?;
        let (base, swapped) = unwrap_inverse(ope);
        // `object_property_expression` only ever produces `NamedObjectProperty`
        // or `InverseObjectProperty` (see property_expr.rs) -- there is no
        // concrete syntax for `AnonymousObjectProperty` in any OWL 2 serialization,
        // so this is unreachable in practice; fail the parse rather than
        // synthesize a bogus predicate IRI if it ever were reached.
        let ObjectPropertyExpression::NamedObjectProperty(pred) = base else {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        };
        let atom = if swapped {
            Atom::PropertyAtom(pred, b, a)
        } else {
            Atom::PropertyAtom(pred, a, b)
        };
        Ok((rest, atom))
    }
}

/// `DataPropertyAtom ::= 'DataPropertyAtom' '(' DataProperty IArg DArg ')'`.
/// `DataPropertyExpression` is always a bare IRI, so this maps directly onto
/// `Atom::PropertyAtom`.
fn data_property_atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        paren_form(
            "DataPropertyAtom",
            (data_property_expression(ctx), i_arg(ctx), d_arg(ctx)),
        )
        .parse(input)
        .map(|(rest, (dp, a, b))| (rest, Atom::PropertyAtom(dp, a, b)))
    }
}

/// `BuiltInAtom ::= 'BuiltInAtom' '(' IRI DArg {DArg} ')'`. Unlike
/// Manchester's `generic_atom` (which must guess arity-2 `PropertyAtom` vs.
/// built-in from arity alone, since its syntax is ambiguous), functional
/// syntax's `BuiltInAtom` keyword is unambiguous at every arity including 2.
fn built_in_atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        paren_form(
            "BuiltInAtom",
            (iri(ctx), many0_no_sep(d_arg(ctx))),
        )
        .parse(input)
        .map(|(rest, (pred, args))| (rest, Atom::BuiltInAtom(pred, args)))
    }
}

/// `SameIndividualAtom ::= 'SameIndividualAtom' '(' IArg IArg ')'`.
fn same_individual_atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        paren_form("SameIndividualAtom", (i_arg(ctx), i_arg(ctx)))
            .parse(input)
            .map(|(rest, (a, b))| (rest, Atom::SameIndividualAtom(a, b)))
    }
}

/// `DifferentIndividualsAtom ::= 'DifferentIndividualsAtom' '(' IArg IArg ')'`.
fn different_individuals_atom<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        paren_form("DifferentIndividualsAtom", (i_arg(ctx), i_arg(ctx)))
            .parse(input)
            .map(|(rest, (a, b))| (rest, Atom::DifferentIndividualsAtom(a, b)))
    }
}

/// `Atom ::= ClassAtom | DataRangeAtom | ObjectPropertyAtom | DataPropertyAtom
///         | BuiltInAtom | SameIndividualAtom | DifferentIndividualsAtom`.
///
/// Every alternative starts with a distinct keyword, so no ambiguity/ordering
/// concern (unlike Manchester's `class_atom`-tried-first rule).
fn atom<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Atom> {
    move |input: &'a str| {
        alt((
            class_atom(ctx),
            data_range_atom(ctx),
            object_property_atom(ctx),
            data_property_atom(ctx),
            built_in_atom(ctx),
            same_individual_atom(ctx),
            different_individuals_atom(ctx),
        ))
        .parse(input)
    }
}

/// `'Body' '(' {Atom} ')'`.
fn body<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Atom>> {
    move |input: &'a str| paren_form("Body", many0_no_sep(atom(ctx))).parse(input)
}

/// `'Head' '(' {Atom} ')'`.
fn head<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Atom>> {
    move |input: &'a str| paren_form("Head", many0_no_sep(atom(ctx))).parse(input)
}

/// `DLSafeRule ::= 'DLSafeRule' '(' axiomAnnotations Body Head ')'`.
pub(crate) fn dl_safe_rule<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> IResult<&'a str, SwrlRule> {
    move |input: &'a str| {
        paren_form(
            "DLSafeRule",
            (axiom_annotations(ctx), body(ctx), head(ctx)),
        )
        .parse(input)
        .map(|(rest, (annotations, body, head))| {
            (
                rest,
                SwrlRule {
                    annotations,
                    body,
                    head,
                },
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ParserContext {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        ctx
    }

    #[test]
    fn parses_class_atom() {
        let (_, a) = class_atom(&ctx())("ClassAtom(:Person Variable(:x))").unwrap();
        assert!(matches!(a, Atom::ClassAtom(_, AtomArg::Variable(_))));
    }

    #[test]
    fn parses_object_property_atom_with_inverse_swaps_args() {
        let (_, a) = object_property_atom(&ctx())(
            "ObjectPropertyAtom(ObjectInverseOf(:hasParent) Variable(:x) Variable(:y))",
        )
        .unwrap();
        match a {
            Atom::PropertyAtom(_, AtomArg::Variable(first), AtomArg::Variable(second)) => {
                assert!(first.ends_with('y'));
                assert!(second.ends_with('x'));
            }
            other => panic!("expected swapped PropertyAtom, got {other:?}"),
        }
    }

    #[test]
    fn parses_built_in_atom_at_arity_two() {
        let (_, a) =
            built_in_atom(&ctx())("BuiltInAtom(:eq Variable(:x) Variable(:y))").unwrap();
        assert!(matches!(a, Atom::BuiltInAtom(_, args) if args.len() == 2));
    }

    #[test]
    fn parses_empty_body_and_head() {
        let (_, b) = body(&ctx())("Body()").unwrap();
        assert!(b.is_empty());
        let (_, h) = head(&ctx())("Head()").unwrap();
        assert!(h.is_empty());
    }

    #[test]
    fn parses_dl_safe_rule() {
        let (_, rule) = dl_safe_rule(&ctx())(
            "DLSafeRule(Body(ClassAtom(:Person Variable(:x))) Head(ClassAtom(:Human Variable(:x))))",
        )
        .unwrap();
        assert_eq!(rule.body.len(), 1);
        assert_eq!(rule.head.len(), 1);
        assert!(rule.annotations.is_empty());
    }
}

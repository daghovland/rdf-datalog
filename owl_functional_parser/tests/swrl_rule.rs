/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for SWRL `DLSafeRule(...)` parsing (issue
//! [#625](https://github.com/daghovland/rdf-datalog/issues/625)). See
//! `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`'s "SWRL
//! `DLSafeRule(...)` (#625)" section for the grammar and the mapping onto
//! `owl_ontology::SwrlRule`/`Atom`/`AtomArg` (added by #498/PR #636 for
//! `manchester_parser`'s `Rule:` frames, reused directly here).

use owl_ontology::{Atom, AtomArg, ClassExpression, Individual};

const PREFIX: &str = "Prefix(:=<http://example.org/pizza#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nPrefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n";

fn parse_body(body: &str) -> owl_ontology::Ontology {
    let src = format!("{PREFIX}Ontology({body})");
    owl_functional_parser::parse(&src).unwrap_or_else(|e| panic!("parse failed: {e}\nsrc: {src}"))
}

#[test]
fn parses_class_atom_rule() {
    let onto = parse_body(
        "DLSafeRule(Body(ClassAtom(:Person Variable(:x))) Head(ClassAtom(:Human Variable(:x))))",
    );
    assert_eq!(onto.rules.len(), 1);
    let rule = &onto.rules[0];
    assert_eq!(rule.body.len(), 1);
    assert_eq!(rule.head.len(), 1);
    match &rule.body[0] {
        Atom::ClassAtom(ClassExpression::ClassName(_), AtomArg::Variable(v)) => {
            assert!(v.ends_with('x'));
        }
        other => panic!("expected ClassAtom, got {other:?}"),
    }
}

#[test]
fn parses_object_property_atom_rule() {
    let onto = parse_body(
        "DLSafeRule(\
            Body(ObjectPropertyAtom(:hasParent Variable(:x) Variable(:y))) \
            Head(ObjectPropertyAtom(:hasAncestor Variable(:x) Variable(:y))))",
    );
    let rule = &onto.rules[0];
    match &rule.body[0] {
        Atom::PropertyAtom(_, AtomArg::Variable(_), AtomArg::Variable(_)) => {}
        other => panic!("expected PropertyAtom, got {other:?}"),
    }
}

#[test]
fn object_property_atom_with_inverse_swaps_args() {
    // ObjectPropertyAtom(ObjectInverseOf(:hasParent) ?x ?y) is semantically
    // hasParent(y, x) -- no dedicated Atom variant for inverse properties,
    // so the parser swaps the two arguments onto the plain (non-inverse)
    // predicate instead, per the plan doc's mapping.
    let onto = parse_body(
        "DLSafeRule(\
            Body(ObjectPropertyAtom(ObjectInverseOf(:hasParent) Variable(:x) Variable(:y))) \
            Head(ClassAtom(:Person Variable(:x))))",
    );
    let rule = &onto.rules[0];
    match &rule.body[0] {
        Atom::PropertyAtom(_, AtomArg::Variable(a), AtomArg::Variable(b)) => {
            assert!(a.ends_with('y'));
            assert!(b.ends_with('x'));
        }
        other => panic!("expected swapped PropertyAtom, got {other:?}"),
    }
}

#[test]
fn parses_data_property_atom_rule() {
    let onto = parse_body(
        "DLSafeRule(\
            Body(DataPropertyAtom(:hasAge Variable(:x) Variable(:a))) \
            Head(BuiltInAtom(<http://www.w3.org/2003/11/swrlb#greaterThan> Variable(:a) \"18\"^^xsd:integer)))",
    );
    let rule = &onto.rules[0];
    match &rule.body[0] {
        Atom::PropertyAtom(_, AtomArg::Variable(_), AtomArg::Variable(_)) => {}
        other => panic!("expected DataPropertyAtom as PropertyAtom, got {other:?}"),
    }
    match &rule.head[0] {
        Atom::BuiltInAtom(_, args) => assert_eq!(args.len(), 2),
        other => panic!("expected BuiltInAtom, got {other:?}"),
    }
}

#[test]
fn parses_builtin_atom_at_arity_two_stays_builtin() {
    // Unlike Manchester's Rule: frames (#498), functional syntax is never
    // ambiguous about arity-2 predicates -- BuiltInAtom(...) with two
    // arguments must stay a BuiltInAtom, not collapse into PropertyAtom.
    let onto = parse_body(
        "DLSafeRule(\
            Body() \
            Head(BuiltInAtom(<http://www.w3.org/2003/11/swrlb#equal> Variable(:x) Variable(:y))))",
    );
    let rule = &onto.rules[0];
    assert!(rule.body.is_empty());
    match &rule.head[0] {
        Atom::BuiltInAtom(_, args) => assert_eq!(args.len(), 2),
        other => panic!("expected BuiltInAtom to stay BuiltInAtom at arity 2, got {other:?}"),
    }
}

#[test]
fn parses_data_range_atom_rule() {
    let onto = parse_body(
        "DLSafeRule(Body(DataRangeAtom(xsd:integer Variable(:a))) Head(ClassAtom(:Person Variable(:x))))",
    );
    let rule = &onto.rules[0];
    match &rule.body[0] {
        Atom::DataRangeAtom(_, AtomArg::Variable(_)) => {}
        other => panic!("expected DataRangeAtom, got {other:?}"),
    }
}

#[test]
fn parses_same_and_different_individuals_atom_rule() {
    let onto = parse_body(
        "DLSafeRule(\
            Body(SameIndividualAtom(Variable(:x) :alice) DifferentIndividualsAtom(Variable(:x) :bob)) \
            Head(ClassAtom(:Person Variable(:x))))",
    );
    let rule = &onto.rules[0];
    assert!(matches!(rule.body[0], Atom::SameIndividualAtom(_, _)));
    assert!(matches!(rule.body[1], Atom::DifferentIndividualsAtom(_, _)));
}

#[test]
fn parses_ground_individual_args_not_just_variables() {
    let onto = parse_body("DLSafeRule(Body() Head(ObjectPropertyAtom(:hasParent :alice :bob)))");
    let rule = &onto.rules[0];
    match &rule.head[0] {
        Atom::PropertyAtom(_, AtomArg::Individual(Individual::NamedIndividual(_)), _) => {}
        other => panic!("expected ground individual arg, got {other:?}"),
    }
}

#[test]
fn rule_with_leading_annotations() {
    let onto = parse_body(
        "DLSafeRule(Annotation(rdfs:label \"my rule\") Body() Head(ClassAtom(:Person Variable(:x))))",
    );
    assert_eq!(onto.rules.len(), 1);
    assert_eq!(onto.rules[0].annotations.len(), 1);
}

#[test]
fn rule_interleaved_with_axioms() {
    let onto = parse_body(
        "\
        Declaration(Class(:Person))\n\
        DLSafeRule(Body(ClassAtom(:Person Variable(:x))) Head(ClassAtom(:Human Variable(:x))))\n\
        Declaration(Class(:Human))\n\
        ",
    );
    assert_eq!(onto.axioms.len(), 2);
    assert_eq!(onto.rules.len(), 1);
}

#[test]
fn multiple_rules() {
    let onto = parse_body(
        "\
        DLSafeRule(Body(ClassAtom(:Person Variable(:x))) Head(ClassAtom(:Human Variable(:x))))\n\
        DLSafeRule(Body(ClassAtom(:Human Variable(:x))) Head(ClassAtom(:Mortal Variable(:x))))\n\
        ",
    );
    assert_eq!(onto.rules.len(), 2);
}

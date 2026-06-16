/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SHACL (Shapes Constraint Language) validation test suite.
//!
//! Each test covers a code example from the W3C SHACL specification:
//! <https://www.w3.org/TR/shacl/>
//!
//! All tests except `shacl_testdata_parses` are `#[ignore]` because SHACL
//! validation is not yet implemented (see `docs/plans/SHACL_PLAN.md` and the `shacl`
//! crate). The test data files in `tests/testdata/shacl_*.ttl` are valid
//! Turtle and are verified to parse by `shacl_testdata_parses`.
//!
//! Test naming: `spec_s{section}_{constraint}` where `section` mirrors the
//! W3C SHACL specification section number.
//!
//! Reference: <https://www.w3.org/TR/shacl/>

use dag_rdf::Datastore;
use dagalog::load_file;
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn load(file: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata(file)).expect("test data file should parse as Turtle");
    ds
}

// ── Parse guard (not ignored) ─────────────────────────────────────────────────

/// Load every SHACL test data file to confirm all parse as valid Turtle.
/// This test is never ignored so that malformed test data is caught by CI.
#[test]
fn shacl_testdata_parses() {
    let files = [
        "shacl_s1_intro_data.ttl",
        "shacl_s1_intro_shapes.ttl",
        "shacl_s2_target_node_data.ttl",
        "shacl_s2_target_node_shapes.ttl",
        "shacl_s2_target_class_data.ttl",
        "shacl_s2_target_class_shapes.ttl",
        "shacl_s2_target_implicit_data.ttl",
        "shacl_s2_target_implicit_shapes.ttl",
        "shacl_s2_target_subjects_data.ttl",
        "shacl_s2_target_subjects_shapes.ttl",
        "shacl_s2_target_objects_data.ttl",
        "shacl_s2_target_objects_shapes.ttl",
        "shacl_s4_class_data.ttl",
        "shacl_s4_class_shapes.ttl",
        "shacl_s4_datatype_data.ttl",
        "shacl_s4_datatype_shapes.ttl",
        "shacl_s4_nodekind_data.ttl",
        "shacl_s4_nodekind_shapes.ttl",
        "shacl_s4_mincount_data.ttl",
        "shacl_s4_mincount_shapes.ttl",
        "shacl_s4_maxcount_data.ttl",
        "shacl_s4_maxcount_shapes.ttl",
        "shacl_s4_range_data.ttl",
        "shacl_s4_range_shapes.ttl",
        "shacl_s4_minlength_data.ttl",
        "shacl_s4_minlength_shapes.ttl",
        "shacl_s4_maxlength_data.ttl",
        "shacl_s4_maxlength_shapes.ttl",
        "shacl_s4_pattern_data.ttl",
        "shacl_s4_pattern_shapes.ttl",
        "shacl_s4_languagein_data.ttl",
        "shacl_s4_languagein_shapes.ttl",
        "shacl_s4_uniquelang_data.ttl",
        "shacl_s4_uniquelang_shapes.ttl",
        "shacl_s4_equals_data.ttl",
        "shacl_s4_equals_shapes.ttl",
        "shacl_s4_disjoint_data.ttl",
        "shacl_s4_disjoint_shapes.ttl",
        "shacl_s4_lessthan_data.ttl",
        "shacl_s4_lessthan_shapes.ttl",
        "shacl_s4_lessthanorequals_data.ttl",
        "shacl_s4_lessthanorequals_shapes.ttl",
        "shacl_s4_not_data.ttl",
        "shacl_s4_not_shapes.ttl",
        "shacl_s4_and_data.ttl",
        "shacl_s4_and_shapes.ttl",
        "shacl_s4_and_datatype_data.ttl",
        "shacl_s4_and_datatype_shapes.ttl",
        "shacl_s4_or_data.ttl",
        "shacl_s4_or_shapes.ttl",
        "shacl_s4_xone_data.ttl",
        "shacl_s4_xone_shapes.ttl",
        "shacl_s4_node_data.ttl",
        "shacl_s4_node_shapes.ttl",
        "shacl_s4_qualified_data.ttl",
        "shacl_s4_qualified_shapes.ttl",
        "shacl_s4_closed_data.ttl",
        "shacl_s4_closed_shapes.ttl",
        "shacl_s4_hasvalue_data.ttl",
        "shacl_s4_hasvalue_shapes.ttl",
        "shacl_s4_in_data.ttl",
        "shacl_s4_in_shapes.ttl",
    ];
    for f in &files {
        let _ = load(f);
    }
}

// ── §1  Introduction ──────────────────────────────────────────────────────────

/// SHACL §1.4 — Introductory PersonShape example.
///
/// Source: <https://www.w3.org/TR/shacl/#shacl-example>
///
/// The `PersonShape` constrains all `ex:Person` instances with:
/// - `sh:maxCount 1` and `sh:pattern "^\d{3}-\d{2}-\d{4}$"` on `ex:ssn`
/// - `sh:class ex:Company` and `sh:nodeKind sh:IRI` on `ex:worksFor`
/// - `sh:closed true` (only declared properties permitted)
///
/// Expected: 4 violations —
/// `ex:Alice` (ssn pattern), `ex:Bob` (ssn maxCount),
/// `ex:Calvin` (worksFor class), `ex:Calvin` (birthDate closed).
#[test]
fn spec_s1_4_intro_person_shape_violations() {
    let data = load("shacl_s1_intro_data.ttl");
    let shapes = load("shacl_s1_intro_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms, "data graph must not conform");
    assert_eq!(report.results.len(), 4, "expected 4 violations");
}

// ── §2  Target Declarations ───────────────────────────────────────────────────

/// SHACL §2.1.3.1 — `sh:targetNode` selects only the named nodes.
///
/// Source: <https://www.w3.org/TR/shacl/#targetNode>
///
/// The shape targets only `ex:Alice`. `ex:Alice` has no `ex:name` → 1 violation.
/// `ex:Bob` also has no `ex:name` but is not targeted → no violation for `ex:Bob`.
#[test]
fn spec_s2_1_1_target_node() {
    let data = load("shacl_s2_target_node_data.ttl");
    let shapes = load("shacl_s2_target_node_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Alice is targeted; 1 violation expected"
    );
}

/// SHACL §2.1.3.2 — `sh:targetClass` selects all instances of a class.
///
/// Source: <https://www.w3.org/TR/shacl/#targetClass>
///
/// `ex:Alice` and `ex:Bob` are `ex:Person`; `ex:NewYork` is `ex:Place` (not targeted).
/// `ex:Alice` has no `ex:name` → 1 violation. `ex:Bob` has `ex:name` → conforms.
#[test]
fn spec_s2_1_2_target_class() {
    let data = load("shacl_s2_target_class_data.ttl");
    let shapes = load("shacl_s2_target_class_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Alice (a Person) has no name"
    );
}

/// SHACL §2.1.3.3 — Implicit class target: a class that is also an `sh:NodeShape`.
///
/// Source: <https://www.w3.org/TR/shacl/#implicit-targetClass>
///
/// `ex:Person` is declared as both `rdfs:Class` and `sh:NodeShape`, so all
/// `ex:Person` instances are automatically targeted. `ex:Alice` has no `ex:name` →
/// 1 violation. `ex:NewYork` is `ex:Place` → not targeted.
#[test]
fn spec_s2_1_3_target_implicit_class() {
    let data = load("shacl_s2_target_implicit_data.ttl");
    let shapes = load("shacl_s2_target_implicit_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Alice is targeted and has no name"
    );
}

/// SHACL §2.1.3.4 — `sh:targetSubjectsOf` targets nodes that appear as subjects.
///
/// Source: <https://www.w3.org/TR/shacl/#targetSubjectsOf>
///
/// `ex:Alice ex:knows ex:Bob` → `ex:Alice` is targeted (subject of `ex:knows`).
/// The shape requires `sh:nodeKind sh:IRI`. `ex:Alice` is an IRI → conforms.
/// `ex:Bob` uses `ex:livesIn`, not `ex:knows` → not targeted.
#[test]
fn spec_s2_1_4_target_subjects_of() {
    let data = load("shacl_s2_target_subjects_data.ttl");
    let shapes = load("shacl_s2_target_subjects_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        report.conforms,
        "ex:Alice is an IRI → sh:nodeKind sh:IRI satisfied"
    );
    assert_eq!(report.results.len(), 0);
}

/// SHACL §2.1.3.5 — `sh:targetObjectsOf` targets nodes that appear as objects.
///
/// Source: <https://www.w3.org/TR/shacl/#targetObjectsOf>
///
/// Objects of `ex:knows` are targeted. `ex:Alice` (IRI object) → conforms.
/// `"Bob"` (literal object) → fails `sh:nodeKind sh:IRI` → 1 violation.
#[test]
fn spec_s2_1_5_target_objects_of() {
    let data = load("shacl_s2_target_objects_data.ttl");
    let shapes = load("shacl_s2_target_objects_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "the literal \"Bob\" object violates sh:nodeKind sh:IRI"
    );
}

// ── §4.1  Value Type Constraint Components ────────────────────────────────────

/// SHACL §4.1.1 — `sh:class`: value nodes must be instances of the given class.
///
/// Source: <https://www.w3.org/TR/shacl/#ClassConstraintComponent>
///
/// `ClassExampleShape` targets `ex:Alice`, `ex:Bob`, `ex:Carol` and requires
/// `ex:address` values to be typed `ex:PostalAddress`.
/// `ex:Carol`'s address blank node lacks `rdf:type ex:PostalAddress` → 1 violation.
#[test]
fn spec_s4_1_1_class() {
    let data = load("shacl_s4_class_data.ttl");
    let shapes = load("shacl_s4_class_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Carol's address violates sh:class"
    );
}

/// SHACL §4.1.2 — `sh:datatype`: value nodes must have the specified RDF datatype.
///
/// Source: <https://www.w3.org/TR/shacl/#DatatypeConstraintComponent>
///
/// `DatatypeExampleShape` requires `ex:age` to be `xsd:integer`.
/// `ex:Bob` has a plain literal; `ex:Carol` has `xsd:int` (not `xsd:integer`) →
/// 2 violations. `ex:Alice` has `xsd:integer` → conforms.
#[test]
fn spec_s4_1_2_datatype() {
    let data = load("shacl_s4_datatype_data.ttl");
    let shapes = load("shacl_s4_datatype_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        2,
        "ex:Bob and ex:Carol each produce 1 violation"
    );
}

/// SHACL §4.1.3 — `sh:nodeKind`: value nodes must be of the specified node kind.
///
/// Source: <https://www.w3.org/TR/shacl/#NodeKindConstraintComponent>
///
/// `NodeKindExampleShape` targets objects of `ex:knows` and requires `sh:IRI`.
/// `ex:Alice` (object of `ex:Bob ex:knows ex:Alice`) is an IRI → conforms.
/// `"Bob"` (object of `ex:Alice ex:knows "Bob"`) is a literal → 1 violation.
#[test]
fn spec_s4_1_3_nodekind() {
    let data = load("shacl_s4_nodekind_data.ttl");
    let shapes = load("shacl_s4_nodekind_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "the literal \"Bob\" object violates sh:nodeKind sh:IRI"
    );
}

// ── §4.2  Cardinality Constraint Components ───────────────────────────────────

/// SHACL §4.2.1 — `sh:minCount`: at least N values must be present.
///
/// Source: <https://www.w3.org/TR/shacl/#MinCountConstraintComponent>
///
/// `MinCountExampleShape` requires at least 1 `ex:name` value.
/// `ex:Alice` has `ex:name "Alice"` → conforms.
/// `ex:Bob` has only `ex:givenName` (no `ex:name`) → 1 violation.
#[test]
fn spec_s4_2_1_mincount() {
    let data = load("shacl_s4_mincount_data.ttl");
    let shapes = load("shacl_s4_mincount_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob has no ex:name → minCount 1 violated"
    );
}

/// SHACL §4.2.2 — `sh:maxCount`: at most N values may be present.
///
/// Source: <https://www.w3.org/TR/shacl/#MaxCountConstraintComponent>
///
/// `MaxCountExampleShape` requires at most 1 `ex:birthDate` value.
/// `ex:Bob` has 1 `ex:birthDate` → conforms.
/// `ex:Carol` has 2 `ex:birthDate` values → 1 violation.
#[test]
fn spec_s4_2_2_maxcount() {
    let data = load("shacl_s4_maxcount_data.ttl");
    let shapes = load("shacl_s4_maxcount_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Carol has 2 birthDate values → maxCount 1 violated"
    );
}

// ── §4.3  Value Range Constraint Components ───────────────────────────────────

/// SHACL §4.3 — `sh:minInclusive` and `sh:maxInclusive` (NumericRangeExampleShape).
///
/// Source: <https://www.w3.org/TR/shacl/#core-components-range>
///
/// Covers `sh:minInclusive` (§4.3.2) and `sh:maxInclusive` (§4.3.4).
/// `ex:Bob` age 23 → within [0, 150] → conforms.
/// `ex:Alice` age 220 → exceeds `sh:maxInclusive 150` → 1 violation.
/// `ex:Ted` age `"twenty one"` → non-numeric; range comparison inapplicable → conforms.
#[test]
fn spec_s4_3_value_range() {
    let data = load("shacl_s4_range_data.ttl");
    let shapes = load("shacl_s4_range_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Alice (age 220) exceeds maxInclusive 150"
    );
}

// ── §4.4  String-based Constraint Components ──────────────────────────────────

/// SHACL §4.4.1 — `sh:minLength`: string value must have at least N characters.
///
/// Source: <https://www.w3.org/TR/shacl/#MinLengthConstraintComponent>
///
/// `MinLengthExampleShape` requires `sh:minLength 4` on `ex:name`.
/// `ex:Alice` `"Al"` (len 2) and `ex:Carol` `"Car"` (len 3) → 2 violations.
/// `ex:Bob` `"Robert"` (len 6) → conforms.
#[test]
fn spec_s4_4_1_minlength() {
    let data = load("shacl_s4_minlength_data.ttl");
    let shapes = load("shacl_s4_minlength_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        2,
        "ex:Alice and ex:Carol are too short"
    );
}

/// SHACL §4.4.2 — `sh:maxLength`: string value must have at most N characters.
///
/// Source: <https://www.w3.org/TR/shacl/#MaxLengthConstraintComponent>
///
/// `MaxLengthExampleShape` requires `sh:maxLength 5` on `ex:name`.
/// `ex:Bob` `"Robert"` (len 6) → 1 violation.
/// `ex:Alice` `"Alice"` (len 5) and `ex:Carol` `"Carol"` (len 5) → conforms.
#[test]
fn spec_s4_4_2_maxlength() {
    let data = load("shacl_s4_maxlength_data.ttl");
    let shapes = load("shacl_s4_maxlength_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Bob's name exceeds maxLength 5"
    );
}

/// SHACL §4.4.3 — `sh:pattern`: string value must match the given regex.
///
/// Source: <https://www.w3.org/TR/shacl/#PatternConstraintComponent>
///
/// `PatternExampleShape` requires `ex:bCode` to match `^B\d{4}$`.
/// `ex:Alice` `"B1234"` → matches → conforms.
/// `ex:Bob` `"B123X"` → last char is not a digit → 1 violation.
#[test]
fn spec_s4_4_3_pattern() {
    let data = load("shacl_s4_pattern_data.ttl");
    let shapes = load("shacl_s4_pattern_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob's bCode does not match pattern"
    );
}

/// SHACL §4.4.4 — `sh:languageIn`: language tag must be in the given list.
///
/// Source: <https://www.w3.org/TR/shacl/#LanguageInConstraintComponent>
///
/// `LanguageInExampleShape` requires `ex:label` language tags to be in `("en" "de")`.
/// `ex:Alice` `@en` and `ex:Carol` `@de` → conforms.
/// `ex:Bob` `@fr` → `"fr"` not in `("en" "de")` → 1 violation.
#[test]
fn spec_s4_4_4_languagein() {
    let data = load("shacl_s4_languagein_data.ttl");
    let shapes = load("shacl_s4_languagein_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob's @fr label is not in (en, de)"
    );
}

/// SHACL §4.4.5 — `sh:uniqueLang`: no two values may share the same language tag.
///
/// Source: <https://www.w3.org/TR/shacl/#UniqueLangConstraintComponent>
///
/// `UniqueLangExampleShape` requires unique language tags on `ex:label`.
/// `ex:Alice` has two `@en` labels → 1 violation.
/// `ex:Bob` has `@en` and `@de` → distinct → conforms.
#[test]
fn spec_s4_4_5_uniquelang() {
    let data = load("shacl_s4_uniquelang_data.ttl");
    let shapes = load("shacl_s4_uniquelang_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Alice has two @en labels → uniqueLang violated"
    );
}

// ── §4.5  Property Pair Constraint Components ─────────────────────────────────

/// SHACL §4.5.1 — `sh:equals`: value sets for two properties must be identical.
///
/// Source: <https://www.w3.org/TR/shacl/#EqualsConstraintComponent>
///
/// `EqualsExampleShape` requires `{ex:firstName} = {ex:givenName}`.
/// `ex:Alice` both `"Alice"` → equal → conforms.
/// `ex:Bob` `firstName "Bob"` vs `givenName "Bobby"` → not equal → 1 violation.
#[test]
fn spec_s4_5_1_equals() {
    let data = load("shacl_s4_equals_data.ttl");
    let shapes = load("shacl_s4_equals_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob firstName ≠ givenName → sh:equals violated"
    );
}

/// SHACL §4.5.2 — `sh:disjoint`: value sets for two properties must not overlap.
///
/// Source: <https://www.w3.org/TR/shacl/#DisjointConstraintComponent>
///
/// `DisjointExampleShape` requires `{ex:prefLabel} ∩ {ex:altLabel} = ∅`.
/// `ex:Alice` `"Alice"` vs `"Alicia"` → disjoint → conforms.
/// `ex:Bob` both have `"Bob"` → shared value → 1 violation.
#[test]
fn spec_s4_5_2_disjoint() {
    let data = load("shacl_s4_disjoint_data.ttl");
    let shapes = load("shacl_s4_disjoint_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob has \"Bob\" as both prefLabel and altLabel"
    );
}

/// SHACL §4.5.3 — `sh:lessThan`: each path value must be strictly less than each
/// value of the comparison property.
///
/// Source: <https://www.w3.org/TR/shacl/#LessThanConstraintComponent>
///
/// `LessThanExampleShape` requires `ex:startDate < ex:endDate`.
/// `ex:Alice` 2020-01-01 < 2020-12-31 → conforms.
/// `ex:Bob` 2020-06-01 > 2020-01-01 → 1 violation.
#[test]
fn spec_s4_5_3_lessthan() {
    let data = load("shacl_s4_lessthan_data.ttl");
    let shapes = load("shacl_s4_lessthan_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob startDate > endDate → sh:lessThan violated"
    );
}

/// SHACL §4.5.4 — `sh:lessThanOrEquals`: each path value must be ≤ each value of
/// the comparison property.
///
/// Source: <https://www.w3.org/TR/shacl/#LessThanOrEqualsConstraintComponent>
///
/// `LessThanOrEqualsExampleShape` requires `ex:startDate ≤ ex:endDate`.
/// `ex:Alice` equal dates → ≤ satisfied → conforms.
/// `ex:Bob` start > end → 1 violation.
#[test]
fn spec_s4_5_4_lessthanorequals() {
    let data = load("shacl_s4_lessthanorequals_data.ttl");
    let shapes = load("shacl_s4_lessthanorequals_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob startDate > endDate → sh:lessThanOrEquals violated"
    );
}

// ── §4.6  Logical Constraint Components ──────────────────────────────────────

/// SHACL §4.6.1 — `sh:not`: the node must NOT conform to the given shape.
///
/// Source: <https://www.w3.org/TR/shacl/#core-components-logical>
///
/// `NotExampleShape` requires nodes to NOT be instances of `ex:LegalPerson`.
/// `ex:Alice` is an `ex:LegalPerson` → conforms to the negated shape → 1 violation.
/// `ex:Bob` is an `ex:NaturalPerson` → does not conform → `sh:not` satisfied.
#[test]
fn spec_s4_6_1_not() {
    let data = load("shacl_s4_not_data.ttl");
    let shapes = load("shacl_s4_not_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Alice is a LegalPerson → sh:not violated"
    );
}

/// SHACL §4.6.2 — `sh:and`: the node must conform to ALL shapes in the list.
///
/// Source: <https://www.w3.org/TR/shacl/#core-components-logical>
///
/// `AndExampleShape` requires both `ex:firstName` and `ex:lastName` (each minCount 1).
/// `ex:Alice` has both → conforms.
/// `ex:Bob` has only `ex:firstName` → fails the second sub-shape → 1 violation.
#[test]
fn spec_s4_6_2_and() {
    let data = load("shacl_s4_and_data.ttl");
    let shapes = load("shacl_s4_and_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob lacks ex:lastName → sh:and violated"
    );
}

/// Regression: `sh:and` must enforce ALL constraint types in inner shapes,
/// not just `sh:minCount`.
///
/// The shape requires `ex:age` to be `xsd:integer` (inside `sh:and`).
/// `ex:Alice` has an integer age → conforms.
/// `ex:Bob` has a string age → violates the `sh:datatype` constraint.
///
/// With the bug, the datatype violation inside `sh:and` is silently ignored
/// and the report incorrectly says the graph conforms.
#[test]
fn spec_s4_6_2_and_with_datatype_constraint() {
    let data = load("shacl_s4_and_datatype_data.ttl");
    let shapes = load("shacl_s4_and_datatype_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "ex:Bob has wrong datatype for ex:age — sh:and must catch datatype violations"
    );
    assert_eq!(
        report.results.len(),
        1,
        "exactly one violation expected (ex:Bob's ex:age has wrong datatype)"
    );
}

/// SHACL §4.6.3 — `sh:or`: the node must conform to AT LEAST ONE shape in the list.
///
/// Source: <https://www.w3.org/TR/shacl/#core-components-logical>
///
/// `OrExampleShape` requires nodes to be `ex:Employee` OR `ex:Customer`.
/// `ex:Alice` (Employee) and `ex:Bob` (Customer) → conforms.
/// `ex:Carol` (Supplier) → neither matches → 1 violation.
#[test]
fn spec_s4_6_3_or() {
    let data = load("shacl_s4_or_data.ttl");
    let shapes = load("shacl_s4_or_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Carol is neither Employee nor Customer"
    );
}

/// SHACL §4.6.4 — `sh:xone`: the node must conform to EXACTLY ONE shape in the list.
///
/// Source: <https://www.w3.org/TR/shacl/#core-components-logical>
///
/// `XoneExampleShape` requires exactly one of `ex:Employee` or `ex:Customer`.
/// `ex:Alice` (Employee only) → exactly one → conforms.
/// `ex:Bob` (Employee AND Customer) → two match → 1 violation.
/// `ex:Carol` (Supplier) → zero match → 1 violation.
#[test]
fn spec_s4_6_4_xone() {
    let data = load("shacl_s4_xone_data.ttl");
    let shapes = load("shacl_s4_xone_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        2,
        "ex:Bob (two matches) and ex:Carol (zero matches)"
    );
}

// ── §4.7  Shape-based Constraint Components ───────────────────────────────────

/// SHACL §4.7.1 — `sh:node`: values must conform to the referenced node shape.
///
/// Source: <https://www.w3.org/TR/shacl/#NodeConstraintComponent>
///
/// `NodeExampleShape` requires each `ex:address` value to conform to `ex:AddressShape`,
/// which itself requires `ex:city` (minCount 1).
/// `ex:Alice`'s address has `ex:city` → conforms to `ex:AddressShape` → conforms.
/// `ex:Bob`'s address has only `ex:zip` → fails `ex:AddressShape` → 1 violation.
#[test]
fn spec_s4_7_1_node() {
    let data = load("shacl_s4_node_data.ttl");
    let shapes = load("shacl_s4_node_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob's address lacks ex:city → sh:node violated"
    );
}

/// SHACL §4.7.3 — `sh:qualifiedValueShape` with `sh:qualifiedMinCount`.
///
/// Source: <https://www.w3.org/TR/shacl/#QualifiedValueShapeConstraintComponent>
///
/// `QualifiedExampleShape` requires at least 2 `ex:parent` values of kind `sh:IRI`.
/// `ex:Alice` has IRI parents `ex:Mom` and `ex:Dad` → 2 qualifying values → conforms.
/// `ex:Bob` has only `ex:Mom` → 1 qualifying value < 2 → 1 violation.
#[test]
fn spec_s4_7_3_qualified_value_shape() {
    let data = load("shacl_s4_qualified_data.ttl");
    let shapes = load("shacl_s4_qualified_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob has only 1 IRI parent; qualifiedMinCount 2 violated"
    );
}

// ── §4.8  Other Constraint Components ────────────────────────────────────────

/// SHACL §4.8.1 — `sh:closed`: only properties declared in `sh:property` are permitted.
///
/// Source: <https://www.w3.org/TR/shacl/#ClosedConstraintComponent>
///
/// `ClosedExampleShape` (closed, ignoring `rdf:type`) permits only `ex:name`.
/// `ex:Fido` has only `ex:name` → conforms.
/// `ex:Rex` has `ex:name` and `ex:breed` → `ex:breed` is forbidden → 1 violation.
#[test]
fn spec_s4_8_1_closed() {
    let data = load("shacl_s4_closed_data.ttl");
    let shapes = load("shacl_s4_closed_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Rex has ex:breed which is not permitted by closed shape"
    );
}

/// SHACL §4.8.2 — `sh:hasValue`: the value set must include the specified value.
///
/// Source: <https://www.w3.org/TR/shacl/#HasValueConstraintComponent>
///
/// `HasValueExampleShape` requires `ex:role` to include `ex:Admin`.
/// `ex:Alice` has `ex:Admin` and `ex:Editor` → includes `ex:Admin` → conforms.
/// `ex:Bob` has only `ex:Editor` → missing `ex:Admin` → 1 violation.
#[test]
fn spec_s4_8_2_has_value() {
    let data = load("shacl_s4_hasvalue_data.ttl");
    let shapes = load("shacl_s4_hasvalue_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob lacks ex:Admin in ex:role → sh:hasValue violated"
    );
}

/// SHACL §4.8.3 — `sh:in`: each value must be one of the listed values.
///
/// Source: <https://www.w3.org/TR/shacl/#InConstraintComponent>
///
/// `InExampleShape` requires `ex:status` to be one of `(ex:Pending ex:Active ex:Closed)`.
/// `ex:Alice` `ex:Active` and `ex:Bob` `ex:Pending` → in list → conforms.
/// `ex:Carol` `ex:Unknown` → not in list → 1 violation.
#[test]
fn spec_s4_8_3_in() {
    let data = load("shacl_s4_in_data.ttl");
    let shapes = load("shacl_s4_in_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Carol's status ex:Unknown is not in the allowed list"
    );
}

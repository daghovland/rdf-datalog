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
//! The test data files in `tests/testdata/shacl_*.ttl` are valid
//! Turtle and are verified to parse by `shacl_testdata_parses`.
//!
//! Test naming: `spec_s{section}_{constraint}` where `section` mirrors the
//! W3C SHACL specification section number.
//!
//! Reference: <https://www.w3.org/TR/shacl/>
//!
//! Run just this file: `cargo test --test shacl_suite`

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
        "shacl_s261_lexical_form_data.ttl",
        "shacl_s261_lexical_form_shapes.ttl",
        "shacl_s4_class_data.ttl",
        "shacl_s4_class_shapes.ttl",
        "shacl_s4_datatype_data.ttl",
        "shacl_s4_datatype_shapes.ttl",
        "shacl_s4_datatype_langstring_data.ttl",
        "shacl_s4_datatype_xsdstring_shapes.ttl",
        "shacl_s4_datatype_langstring_shapes.ttl",
        "shacl_s4_nodekind_data.ttl",
        "shacl_s4_nodekind_shapes.ttl",
        "shacl_s4_mincount_data.ttl",
        "shacl_s4_mincount_shapes.ttl",
        "shacl_s4_maxcount_data.ttl",
        "shacl_s4_maxcount_shapes.ttl",
        "shacl_s4_mincount_n_data.ttl",
        "shacl_s4_mincount_n_shapes.ttl",
        "shacl_s4_maxcount_n_data.ttl",
        "shacl_s4_maxcount_n_shapes.ttl",
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
        "shacl_s4_exclusive_data.ttl",
        "shacl_s4_exclusive_shapes.ttl",
        "shacl_s4_property_ref_data.ttl",
        "shacl_s4_property_ref_shapes.ttl",
        "shacl_s4_qualified_max_data.ttl",
        "shacl_s4_qualified_max_shapes.ttl",
        "shacl_s4_node_level_datatype_data.ttl",
        "shacl_s4_node_level_datatype_shapes.ttl",
        "shacl_s4_node_level_in_data.ttl",
        "shacl_s4_node_level_in_shapes.ttl",
        "shacl_s4_node_level_class_data.ttl",
        "shacl_s4_node_level_class_shapes.ttl",
        "shacl_s4_node_level_hasvalue_data.ttl",
        "shacl_s4_node_level_hasvalue_shapes.ttl",
        "shacl_s3_severity_data.ttl",
        "shacl_s3_severity_shapes.ttl",
        "shacl_s258_or_data.ttl",
        "shacl_s258_or_shapes.ttl",
        "shacl_s258_not_data.ttl",
        "shacl_s258_not_shapes.ttl",
        "shacl_s258_node_data.ttl",
        "shacl_s258_node_shapes.ttl",
        "shacl_s258_xone_data.ttl",
        "shacl_s258_xone_shapes.ttl",
        "shacl_s258_qualified_data.ttl",
        "shacl_s258_qualified_shapes.ttl",
        "shacl_s262_deactivated_data.ttl",
        "shacl_s262_deactivated_shapes.ttl",
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

/// Regression test for issue #259 — `sh:datatype xsd:string` must not conflate
/// `rdf:langString` (language-tagged literals) with `xsd:string` (plain literals).
///
/// `ex:Dave ex:name "hello"@en` is language-tagged, so its datatype is
/// `rdf:langString`, not `xsd:string` → violates.
/// `ex:Erin ex:name "hello"` is a plain literal, so its datatype is
/// `xsd:string` → conforms.
#[test]
fn regression_259_datatype_xsd_string_excludes_lang_tagged() {
    let data = load("shacl_s4_datatype_langstring_data.ttl");
    let shapes = load("shacl_s4_datatype_xsdstring_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Dave (lang-tagged) should violate sh:datatype xsd:string"
    );
}

/// Regression test for issue #259 — `sh:datatype rdf:langString` must not
/// accept a plain (non-language-tagged) literal.
///
/// `ex:Dave ex:name "hello"@en` is language-tagged → datatype is
/// `rdf:langString` → conforms.
/// `ex:Erin ex:name "hello"` is a plain literal, so its datatype is
/// `xsd:string`, not `rdf:langString` → violates.
#[test]
fn regression_259_datatype_langstring_excludes_plain_string() {
    let data = load("shacl_s4_datatype_langstring_data.ttl");
    let shapes = load("shacl_s4_datatype_langstring_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "only ex:Erin (plain string) should violate sh:datatype rdf:langString"
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

/// Regression test for issue #256 — `sh:maxCount N` with `N > 1` must require
/// `N + 1` distinct values to fire a violation, not just 2 (the bug: the old
/// translation hardcoded a 2-distinct-value check regardless of `N`).
///
/// `MaxCount2ExampleShape` (`sh:maxCount 2`): `ex:Dave2` has exactly 2
/// distinct `ex:tag` values (conforms), `ex:Eve2` has 3 (violates),
/// `ex:Frank2` has 1 (conforms).
/// `MaxCount3ExampleShape` (`sh:maxCount 3`): `ex:Dave3` has exactly 3
/// (conforms), `ex:Eve3` has 4 (violates), `ex:Frank3` has 1 (conforms).
#[test]
fn regression_issue_256_maxcount_n() {
    let data = load("shacl_s4_maxcount_n_data.ttl");
    let shapes = load("shacl_s4_maxcount_n_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        2,
        "only ex:Eve2 (3 > maxCount 2) and ex:Eve3 (4 > maxCount 3) should violate; \
         nodes with exactly N or fewer than N values must conform, got: {:?}",
        report.results
    );
    let focus_nodes: Vec<&str> = report
        .results
        .iter()
        .filter_map(|r| r.focus_node.as_deref())
        .collect();
    assert!(
        focus_nodes.iter().any(|f| f.contains("Eve2")),
        "expected a violation for ex:Eve2, got {focus_nodes:?}"
    );
    assert!(
        focus_nodes.iter().any(|f| f.contains("Eve3")),
        "expected a violation for ex:Eve3, got {focus_nodes:?}"
    );
}

/// Regression test for issue #256 — `sh:minCount N` with `N > 1` must fire a
/// violation when fewer than `N` distinct values are present (the bug: the
/// old translation emitted zero rules for `N > 1`, silently never violating).
///
/// `MinCount2ExampleShape` (`sh:minCount 2`): `ex:Gina2` has exactly 2
/// distinct `ex:tag` values (conforms), `ex:Hank2` has 1 (violates),
/// `ex:Ivy2` has 3 (conforms — no upper bound from minCount).
/// `MinCount3ExampleShape` (`sh:minCount 3`): `ex:Gina3` has exactly 3
/// (conforms), `ex:Hank3` has 2 (violates), `ex:Ivy3` has 4 (conforms).
#[test]
fn regression_issue_256_mincount_n() {
    let data = load("shacl_s4_mincount_n_data.ttl");
    let shapes = load("shacl_s4_mincount_n_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        2,
        "only ex:Hank2 (1 < minCount 2) and ex:Hank3 (2 < minCount 3) should violate; \
         nodes with exactly N or more than N values must conform, got: {:?}",
        report.results
    );
    let focus_nodes: Vec<&str> = report
        .results
        .iter()
        .filter_map(|r| r.focus_node.as_deref())
        .collect();
    assert!(
        focus_nodes.iter().any(|f| f.contains("Hank2")),
        "expected a violation for ex:Hank2, got {focus_nodes:?}"
    );
    assert!(
        focus_nodes.iter().any(|f| f.contains("Hank3")),
        "expected a violation for ex:Hank3, got {focus_nodes:?}"
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

// ── §4.3 (exclusive bounds) ───────────────────────────────────────────────────

/// SHACL §4.3.1 and §4.3.3 — `sh:minExclusive` and `sh:maxExclusive`.
///
/// Source: <https://www.w3.org/TR/shacl/#core-components-range>
///
/// `ExclusiveRangeShape` requires `ex:age` to be strictly within (0, 150).
/// The boundary values themselves are violations (exclusive bounds).
/// `ex:Alice` age 0  → not strictly > 0 → violates `sh:minExclusive`.
/// `ex:Bob`   age 23 → within (0, 150) → conforms.
/// `ex:Carol` age 150 → not strictly < 150 → violates `sh:maxExclusive`.
#[test]
fn spec_s4_3_exclusive_range() {
    let data = load("shacl_s4_exclusive_data.ttl");
    let shapes = load("shacl_s4_exclusive_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        2,
        "ex:Alice (age 0) and ex:Carol (age 150) each violate exclusive bounds"
    );
}

// ── §4.7.2 Standalone sh:property reference ───────────────────────────────────

/// SHACL §4.7.2 — `sh:property` referencing a named `sh:PropertyShape` by IRI.
///
/// Source: <https://www.w3.org/TR/shacl/#PropertyShapes>
///
/// `ex:PersonShape` references `ex:NamePropertyShape` by IRI (not an inline blank node).
/// `ex:NamePropertyShape` declares `sh:path ex:name ; sh:minCount 1`.
/// `ex:Alice` has `ex:name "Alice"` → conforms.
/// `ex:Bob` has no `ex:name` → minCount 1 violated → 1 violation.
#[test]
fn spec_s4_7_2_property_shape_ref() {
    let data = load("shacl_s4_property_ref_data.ttl");
    let shapes = load("shacl_s4_property_ref_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Bob has no ex:name → sh:minCount 1 via named PropertyShape violated"
    );
}

// ── §4.7.3 sh:qualifiedMaxCount ──────────────────────────────────────────────

/// SHACL §4.7.3 — `sh:qualifiedValueShape` with `sh:qualifiedMaxCount`.
///
/// Source: <https://www.w3.org/TR/shacl/#QualifiedValueShapeConstraintComponent>
///
/// `QualifiedMaxShape` requires at most 1 `ex:parent` value of kind `sh:IRI`.
/// `ex:Alice` has IRI parents `ex:Mom` and `ex:Dad` → 2 qualifying values > 1 → violation.
/// `ex:Bob` has only `ex:Mom` → 1 qualifying value ≤ 1 → conforms.
#[test]
fn spec_s4_7_3_qualified_max_count() {
    let data = load("shacl_s4_qualified_max_data.ttl");
    let shapes = load("shacl_s4_qualified_max_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!report.conforms);
    assert_eq!(
        report.results.len(),
        1,
        "ex:Alice has 2 IRI parents; qualifiedMaxCount 1 violated"
    );
}

// ── Issue #260 — node-level (pathless) value constraints ─────────────────────
//
// A shape may carry value constraints directly (no sh:path), which apply to the
// focus node itself rather than to a path-traversed value.
// See: https://github.com/daghovland/rdf-datalog/issues/260

/// Issue #260 — node-level `sh:datatype` (no `sh:path`) applies to the focus node.
///
/// `ex:n` is an IRI (via `ex:n a ex:Thing`), not an `xsd:integer` literal, so the
/// focus node itself must fail `sh:datatype xsd:integer` → 1 violation.
#[test]
fn regression_issue_260_node_level_datatype() {
    let data = load("shacl_s4_node_level_datatype_data.ttl");
    let shapes = load("shacl_s4_node_level_datatype_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "node-level sh:datatype must be checked against the focus node itself"
    );
    assert_eq!(report.results.len(), 1);
}

/// Issue #260 — node-level `sh:in` (no `sh:path`) applies to the focus node.
///
/// `ex:n` is neither `ex:A` nor `ex:B` → the focus node itself violates `sh:in`.
#[test]
fn regression_issue_260_node_level_in() {
    let data = load("shacl_s4_node_level_in_data.ttl");
    let shapes = load("shacl_s4_node_level_in_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "node-level sh:in must be checked against the focus node itself"
    );
    assert_eq!(report.results.len(), 1);
}

/// Issue #260 — node-level `sh:class` (no `sh:path`) applies to the focus node.
///
/// `ex:n` is `rdf:type ex:Thing`, not `ex:Person` → the focus node itself
/// violates `sh:class ex:Person`. Note: `ParsedShape::node_class` was parsed
/// but never read by either evaluator prior to this fix — this test confirms
/// it is now actually enforced (folded into the generic node-level mechanism).
#[test]
fn regression_issue_260_node_level_class() {
    let data = load("shacl_s4_node_level_class_data.ttl");
    let shapes = load("shacl_s4_node_level_class_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "node-level sh:class must be checked against the focus node itself"
    );
    assert_eq!(report.results.len(), 1);
}

/// Issue #260 — node-level `sh:hasValue` (no `sh:path`) applies to the focus node.
///
/// `ex:n` targeted directly; the focus node itself is not `ex:Expected` → violation.
#[test]
fn regression_issue_260_node_level_hasvalue() {
    let data = load("shacl_s4_node_level_hasvalue_data.ttl");
    let shapes = load("shacl_s4_node_level_hasvalue_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !report.conforms,
        "node-level sh:hasValue must be checked against the focus node itself"
    );
    assert_eq!(report.results.len(), 1);
}

// ── §3.5  Severity ────────────────────────────────────────────────────────────
//
// Regression tests for issue #263: `sh:severity` was ignored and every result
// was hardcoded to `Severity::Violation`. Source: <https://www.w3.org/TR/shacl/#severity>

/// A shape with `sh:severity sh:Warning` must produce results with
/// `Severity::Warning`, not the hardcoded `Severity::Violation`.
#[test]
fn regression_issue_263_severity_warning() {
    let data = load("shacl_s3_severity_data.ttl");
    let shapes = load("shacl_s3_severity_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    let warn_result = report
        .results
        .iter()
        .find(|r| r.focus_node.as_deref() == Some("http://example.com/ns#nWarn"))
        .expect("ex:nWarn should have a validation result (missing ex:v)");
    assert_eq!(warn_result.severity, shacl::Severity::Warning);
}

/// A shape with `sh:severity sh:Info` must produce results with `Severity::Info`.
#[test]
fn regression_issue_263_severity_info() {
    let data = load("shacl_s3_severity_data.ttl");
    let shapes = load("shacl_s3_severity_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    let info_result = report
        .results
        .iter()
        .find(|r| r.focus_node.as_deref() == Some("http://example.com/ns#nInfo"))
        .expect("ex:nInfo should have a validation result (missing ex:v)");
    assert_eq!(info_result.severity, shacl::Severity::Info);
}

/// A shape with no `sh:severity` declared must default to `Severity::Violation`
/// (guards against a regression in the common, unset case).
#[test]
fn regression_issue_263_severity_default() {
    let data = load("shacl_s3_severity_data.ttl");
    let shapes = load("shacl_s3_severity_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    let default_result = report
        .results
        .iter()
        .find(|r| r.focus_node.as_deref() == Some("http://example.com/ns#nDefault"))
        .expect("ex:nDefault should have a validation result (missing ex:v)");
    assert_eq!(default_result.severity, shacl::Severity::Violation);
}

/// `report_to_turtle` must emit the actual severity per result, not a hardcoded
/// `sh:Violation` for every result.
#[test]
fn regression_issue_263_severity_in_turtle_report() {
    let data = load("shacl_s3_severity_data.ttl");
    let shapes = load("shacl_s3_severity_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    let turtle = shacl::report_to_turtle(&report);
    assert!(
        turtle.contains("sh:resultSeverity sh:Warning"),
        "turtle report should contain sh:Warning severity:\n{turtle}"
    );
    assert!(
        turtle.contains("sh:resultSeverity sh:Info"),
        "turtle report should contain sh:Info severity:\n{turtle}"
    );
    assert!(
        turtle.contains("sh:resultSeverity sh:Violation"),
        "turtle report should contain sh:Violation severity for the default shape:\n{turtle}"
    );
}

// ── Issue #258 — shared inner-shape conformance checker ──────────────────────
//
// sh:or/sh:not (translate.rs::inner_ok_rules) and sh:node/sh:xone/
// sh:qualifiedValueShape (evaluate.rs::node_conforms_to_inner) previously
// evaluated inner-shape conformance through hand-rolled mini-checkers that only
// understood sh:class / sh:nodeKind / sh:property[sh:minCount>=1]. Any other
// constraint (sh:datatype, sh:pattern, sh:in, sh:hasValue, sh:maxCount, ...) was
// silently ignored, producing false positives (sh:or), false negatives (sh:not),
// or mis-counted qualifying values (sh:node/sh:xone/sh:qualifiedValueShape).
// See: https://github.com/daghovland/rdf-datalog/issues/258

/// True iff `report` contains a violation whose focus node is `iri`.
fn has_violation(report: &shacl::ValidationReport, iri: &str) -> bool {
    report
        .results
        .iter()
        .any(|r| r.focus_node.as_deref() == Some(iri))
}

const EX: &str = "http://example.com/ns#";

fn ex(local: &str) -> String {
    format!("{EX}{local}")
}

// ── sh:or ─────────────────────────────────────────────────────────────────────

/// Issue's concrete repro: `sh:or([sh:nodeKind sh:IRI], [sh:nodeKind sh:Literal])`
/// must correctly recognize an IRI-valued disjunct as conforming (previously
/// `inner_ok_rules` did not support `sh:nodeKind` at all, so neither disjunct was
/// ever derived "ok" and the node was always reported as violating).
#[test]
fn regression_issue_258_or_nodekind() {
    let data = load("shacl_s258_or_data.ttl");
    let shapes = load("shacl_s258_or_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("orNkOk")),
        "orNkOk's ex:v is an IRI — the sh:nodeKind sh:IRI disjunct conforms"
    );
    assert!(
        has_violation(&report, &ex("orNkBad")),
        "orNkBad's ex:v is a blank node — neither disjunct conforms"
    );
}

#[test]
fn regression_issue_258_or_datatype() {
    let data = load("shacl_s258_or_data.ttl");
    let shapes = load("shacl_s258_or_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("orDtOk")));
    assert!(has_violation(&report, &ex("orDtBad")));
}

#[test]
fn regression_issue_258_or_pattern() {
    let data = load("shacl_s258_or_data.ttl");
    let shapes = load("shacl_s258_or_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("orPatOk")));
    assert!(has_violation(&report, &ex("orPatBad")));
}

#[test]
fn regression_issue_258_or_in() {
    let data = load("shacl_s258_or_data.ttl");
    let shapes = load("shacl_s258_or_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("orInOk")));
    assert!(has_violation(&report, &ex("orInBad")));
}

#[test]
fn regression_issue_258_or_hasvalue() {
    let data = load("shacl_s258_or_data.ttl");
    let shapes = load("shacl_s258_or_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("orHvOk")));
    assert!(has_violation(&report, &ex("orHvBad")));
}

#[test]
fn regression_issue_258_or_maxcount() {
    let data = load("shacl_s258_or_data.ttl");
    let shapes = load("shacl_s258_or_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("orMcOk")));
    assert!(has_violation(&report, &ex("orMcBad")));
}

// ── sh:not ────────────────────────────────────────────────────────────────────

/// Issue's concrete repro: `sh:not [sh:nodeKind sh:IRI]` must fire when the inner
/// shape genuinely holds (previously `inner_ok_rules` never derived "ok" for
/// `sh:nodeKind`, so `sh:not`'s violation rule — which requires "ok" to be true
/// first — never fired, a false negative).
#[test]
fn regression_issue_258_not_nodekind() {
    let data = load("shacl_s258_not_data.ttl");
    let shapes = load("shacl_s258_not_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("notNkBad")),
        "notNkBad's ex:v is an IRI — the inner shape conforms, so sh:not violates"
    );
    assert!(
        !has_violation(&report, &ex("notNkOk")),
        "notNkOk's ex:v is a blank node — the inner shape does not conform, so sh:not is satisfied"
    );
}

#[test]
fn regression_issue_258_not_datatype() {
    let data = load("shacl_s258_not_data.ttl");
    let shapes = load("shacl_s258_not_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(has_violation(&report, &ex("notDtBad")));
    assert!(!has_violation(&report, &ex("notDtOk")));
}

#[test]
fn regression_issue_258_not_pattern() {
    let data = load("shacl_s258_not_data.ttl");
    let shapes = load("shacl_s258_not_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(has_violation(&report, &ex("notPatBad")));
    assert!(!has_violation(&report, &ex("notPatOk")));
}

#[test]
fn regression_issue_258_not_in() {
    let data = load("shacl_s258_not_data.ttl");
    let shapes = load("shacl_s258_not_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(has_violation(&report, &ex("notInBad")));
    assert!(!has_violation(&report, &ex("notInOk")));
}

#[test]
fn regression_issue_258_not_hasvalue() {
    let data = load("shacl_s258_not_data.ttl");
    let shapes = load("shacl_s258_not_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(has_violation(&report, &ex("notHvBad")));
    assert!(!has_violation(&report, &ex("notHvOk")));
}

#[test]
fn regression_issue_258_not_maxcount() {
    let data = load("shacl_s258_not_data.ttl");
    let shapes = load("shacl_s258_not_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(has_violation(&report, &ex("notMcBad")));
    assert!(!has_violation(&report, &ex("notMcOk")));
}

// ── sh:node ───────────────────────────────────────────────────────────────────

#[test]
fn regression_issue_258_node_datatype() {
    let data = load("shacl_s258_node_data.ttl");
    let shapes = load("shacl_s258_node_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("ndDtOk")));
    assert!(has_violation(&report, &ex("ndDtBad")));
}

#[test]
fn regression_issue_258_node_pattern() {
    let data = load("shacl_s258_node_data.ttl");
    let shapes = load("shacl_s258_node_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("ndPatOk")));
    assert!(has_violation(&report, &ex("ndPatBad")));
}

#[test]
fn regression_issue_258_node_in() {
    let data = load("shacl_s258_node_data.ttl");
    let shapes = load("shacl_s258_node_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("ndInOk")));
    assert!(has_violation(&report, &ex("ndInBad")));
}

#[test]
fn regression_issue_258_node_hasvalue() {
    let data = load("shacl_s258_node_data.ttl");
    let shapes = load("shacl_s258_node_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("ndHvOk")));
    assert!(has_violation(&report, &ex("ndHvBad")));
}

#[test]
fn regression_issue_258_node_maxcount() {
    let data = load("shacl_s258_node_data.ttl");
    let shapes = load("shacl_s258_node_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("ndMcOk")));
    assert!(has_violation(&report, &ex("ndMcBad")));
}

// ── sh:xone ───────────────────────────────────────────────────────────────────

#[test]
fn regression_issue_258_xone_datatype() {
    let data = load("shacl_s258_xone_data.ttl");
    let shapes = load("shacl_s258_xone_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("xoDtOk")));
    assert!(has_violation(&report, &ex("xoDtBad")));
}

#[test]
fn regression_issue_258_xone_pattern() {
    let data = load("shacl_s258_xone_data.ttl");
    let shapes = load("shacl_s258_xone_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("xoPatOk")));
    assert!(has_violation(&report, &ex("xoPatBad")));
}

#[test]
fn regression_issue_258_xone_in() {
    let data = load("shacl_s258_xone_data.ttl");
    let shapes = load("shacl_s258_xone_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("xoInOk")));
    assert!(has_violation(&report, &ex("xoInBad")));
}

#[test]
fn regression_issue_258_xone_hasvalue() {
    let data = load("shacl_s258_xone_data.ttl");
    let shapes = load("shacl_s258_xone_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("xoHvOk")));
    assert!(has_violation(&report, &ex("xoHvBad")));
}

#[test]
fn regression_issue_258_xone_maxcount() {
    let data = load("shacl_s258_xone_data.ttl");
    let shapes = load("shacl_s258_xone_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("xoMcOk")));
    assert!(has_violation(&report, &ex("xoMcBad")));
}

// ── sh:qualifiedValueShape ────────────────────────────────────────────────────

/// Issue's concrete repro, verbatim: `sh:qualifiedValueShape [sh:datatype
/// xsd:integer] ; sh:qualifiedMinCount 2` with two non-integer string values
/// must count 0 qualifying values and violate (previously the datatype inner
/// was ignored so both values were wrongly counted as qualifying).
#[test]
fn regression_issue_258_qualified_datatype() {
    let data = load("shacl_s258_qualified_data.ttl");
    let shapes = load("shacl_s258_qualified_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("qvDtOk")));
    assert!(has_violation(&report, &ex("qvDtBad")));
}

#[test]
fn regression_issue_258_qualified_pattern() {
    let data = load("shacl_s258_qualified_data.ttl");
    let shapes = load("shacl_s258_qualified_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("qvPatOk")));
    assert!(has_violation(&report, &ex("qvPatBad")));
}

#[test]
fn regression_issue_258_qualified_in() {
    let data = load("shacl_s258_qualified_data.ttl");
    let shapes = load("shacl_s258_qualified_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("qvInOk")));
    assert!(has_violation(&report, &ex("qvInBad")));
}

#[test]
fn regression_issue_258_qualified_hasvalue() {
    let data = load("shacl_s258_qualified_data.ttl");
    let shapes = load("shacl_s258_qualified_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("qvHvOk")));
    assert!(has_violation(&report, &ex("qvHvBad")));
}

#[test]
fn regression_issue_258_qualified_maxcount() {
    let data = load("shacl_s258_qualified_data.ttl");
    let shapes = load("shacl_s258_qualified_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(!has_violation(&report, &ex("qvMcOk")));
    assert!(has_violation(&report, &ex("qvMcBad")));
}

// ── sh:deactivated ──────────────────────────────────────────────────────────
//
// Per SHACL §3, a shape with sh:deactivated true must produce no validation
// results at all, from any of its constraints, even if the data would
// otherwise violate them. Previously sh:deactivated was not handled anywhere
// in the shacl crate, so every constraint on a deactivated shape was still
// evaluated. See https://github.com/daghovland/rdf-datalog/issues/262

/// Issue's concrete repro, verbatim: a deactivated node shape with
/// sh:targetNode + sh:property[sh:minCount 1] must conform even though the
/// focus node has no value for the path at all.
#[test]
fn regression_issue_262_node_shape_deactivated() {
    let data = load("shacl_s262_deactivated_data.ttl");
    let shapes = load("shacl_s262_deactivated_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nNodeDeactivated")),
        "the node shape is sh:deactivated, so sh:minCount 1 must never be checked"
    );
}

/// A deactivated sh:property block nested inside an otherwise-active node
/// shape must also produce no results, independent of the parent shape.
#[test]
fn regression_issue_262_property_shape_deactivated() {
    let data = load("shacl_s262_deactivated_data.ttl");
    let shapes = load("shacl_s262_deactivated_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nPropDeactivated")),
        "the sh:property block itself is sh:deactivated, so sh:minCount 1 must never be checked"
    );
}

/// Regression guard: the same sh:minCount 1 constraint on a shape that is
/// NOT deactivated must still correctly violate.
#[test]
fn regression_issue_262_active_shape_still_violates() {
    let data = load("shacl_s262_deactivated_data.ttl");
    let shapes = load("shacl_s262_deactivated_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nActive")),
        "this shape is not deactivated, so sh:minCount 1 must still be checked and violate"
    );
}

// ── lexical_form for IRI and blank-node value nodes ─────────────────────────
//
// Per the normative SHACL §4.4.1-4.4.3 text (verified against the W3C SHACL
// spec's own SPARQL definitions, which test `str($value)` guarded by
// `!isBlank($value)`): sh:minLength/sh:maxLength/sh:pattern "can be applied
// to any literals and IRIs, but not to blank nodes" - an IRI value node is
// tested against its own string form, while a blank node value node ALWAYS
// produces a violation regardless of the bound/pattern.
//
// `lexical_form` previously returned None for every non-literal (including
// IRIs), and the string-constraint evaluators treated None as "skip this
// value node" instead of "test the IRI string" / "always violate for blank
// nodes", so a non-matching IRI or any blank node silently conformed.
// See https://github.com/daghovland/rdf-datalog/issues/261

#[test]
fn regression_issue_261_pattern_iri_match_conforms() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nIriPatternMatch")),
        "an IRI value node whose string form matches sh:pattern must \
         conform - IRIs are tested by their own string form"
    );
}

#[test]
fn regression_issue_261_pattern_iri_non_match_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nIriPatternNonMatch")),
        "the issue's original repro: an IRI value node whose string form \
         does not match sh:pattern must violate instead of being silently \
         skipped"
    );
}

#[test]
fn regression_issue_261_pattern_blank_node_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nBlankPattern")),
        "a blank-node value node must always violate sh:pattern per SHACL \
         §4.4.3, even against a pattern that matches everything"
    );
}

#[test]
fn regression_issue_261_pattern_literal_match_conforms() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nLiteralPatternMatch")),
        "control case: a literal value node whose lexical form matches \
         sh:pattern must still conform - the fix must not break literals"
    );
}

#[test]
fn regression_issue_261_pattern_literal_non_match_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nLiteralPatternNonMatch")),
        "control case: a literal value node whose lexical form does not \
         match sh:pattern must still violate"
    );
}

#[test]
fn regression_issue_261_min_length_iri_ok_conforms() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nIriMinLenOk")),
        "an IRI value node long enough to satisfy sh:minLength must \
         conform - IRIs are tested by their own string form"
    );
}

#[test]
fn regression_issue_261_min_length_iri_too_short_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nIriMinLenTooShort")),
        "an IRI value node too short (string form) for sh:minLength must \
         violate"
    );
}

#[test]
fn regression_issue_261_min_length_blank_node_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nBlankMinLen")),
        "a blank-node value node must always violate sh:minLength per SHACL \
         §4.4.1, even with the loosest possible bound (0)"
    );
}

#[test]
fn regression_issue_261_min_length_literal_ok_conforms() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nLiteralMinLenOk")),
        "control case: a literal long enough to satisfy sh:minLength must \
         still conform"
    );
}

#[test]
fn regression_issue_261_min_length_literal_too_short_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nLiteralMinLenTooShort")),
        "control case: a literal too short for sh:minLength must still \
         violate"
    );
}

#[test]
fn regression_issue_261_max_length_iri_ok_conforms() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nIriMaxLenOk")),
        "an IRI value node short enough to satisfy sh:maxLength must \
         conform - IRIs are tested by their own string form"
    );
}

#[test]
fn regression_issue_261_max_length_iri_too_long_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nIriMaxLenTooLong")),
        "an IRI value node too long (string form) for sh:maxLength must \
         violate"
    );
}

#[test]
fn regression_issue_261_max_length_blank_node_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nBlankMaxLen")),
        "a blank-node value node must always violate sh:maxLength per SHACL \
         §4.4.2, even with a very generous bound (1000)"
    );
}

#[test]
fn regression_issue_261_max_length_literal_ok_conforms() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        !has_violation(&report, &ex("nLiteralMaxLenOk")),
        "control case: a literal short enough to satisfy sh:maxLength must \
         still conform"
    );
}

#[test]
fn regression_issue_261_max_length_literal_too_long_violates() {
    let data = load("shacl_s261_lexical_form_data.ttl");
    let shapes = load("shacl_s261_lexical_form_shapes.ttl");
    let report = shacl::validate(&data, &shapes).expect("validation must not error");
    assert!(
        has_violation(&report, &ex("nLiteralMaxLenTooLong")),
        "control case: a literal too long for sh:maxLength must still \
         violate"
    );
}

//! Regression tests for #208: `FILTER`'s `=`/`!=` (and `IN`/`NOT IN`, which
//! SPARQL 1.1 §17.4.1.9 defines in terms of `=`) must numerically/booleanly
//! normalize across the two `RdfLiteral` representations this codebase uses
//! for the same RDF value:
//!
//! - literals parsed directly from SPARQL query text (e.g. bare `42`, `true`)
//!   become `RdfLiteral::TypedLiteral { type_iri, literal }` (see
//!   `parse_numeric_literal`/`parse_boolean_literal` in `sparql_parser::lib`,
//!   which deliberately match what the `turtle` crate produces for parsed
//!   Turtle data), while
//! - *computed* results (unary minus, binary arithmetic, `ABS`/`CEIL`/
//!   `FLOOR`/`ROUND`, string predicates like `STRSTARTS`, xsd casts) produce
//!   the native enum variants instead (`IntegerLiteral`, `DecimalLiteral`,
//!   `DoubleLiteral`, `FloatLiteral`, `BooleanLiteral`).
//!
//! A raw Rust `==` sees these as different enum variants even when they
//! denote the same value, so e.g. `FILTER((1 + 1) = 2)` wrongly filtered out
//! every solution. `<`/`>`/`<=`/`>=` already normalize via
//! `compare_graph_elements` and are unaffected; only `=`/`!=`/`IN`/`NOT IN`
//! had the bug.
//!
//! See <https://github.com/daghovland/rdf-datalog/issues/208>.

use dag_rdf::Datastore;
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult};
use std::collections::HashMap;

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
    }
}

/// Run a `SELECT` query over an empty datastore and return the number of
/// result rows. Every query in this file follows the shape
/// `SELECT ?x WHERE { BIND(1 AS ?x) FILTER(<condition>) }`, so the row count
/// is 1 if `<condition>` held and 0 if it didn't.
fn row_count(sparql: &str) -> usize {
    let ds = Datastore::new(100);
    let (_, query) = parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
    match execute(&query, &ds, NetworkPolicy::Deny).expect("execute should succeed") {
        QueryResult::Select(r) => r.rows.len(),
        _ => panic!("expected SELECT"),
    }
}

// ── `=` : computed value vs. parsed literal (the core bug) ───────────────────

#[test]
fn filter_eq_computed_arithmetic_matches_parsed_literal() {
    // (1 + 1) is IntegerLiteral(2) (computed); `2` is TypedLiteral{xsd:integer,"2"}
    // (parsed from query text). These must compare equal.
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER((1 + 1) = 2) }");
    assert_eq!(
        n, 1,
        "(1 + 1) = 2 should hold once numeric normalization is fixed"
    );
}

#[test]
fn filter_eq_computed_function_matches_parsed_literal() {
    // ABS(-5) is IntegerLiteral(5) (computed); `5` is a parsed TypedLiteral.
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER(ABS(-5) = 5) }");
    assert_eq!(
        n, 1,
        "ABS(-5) = 5 should hold once numeric normalization is fixed"
    );
}

// ── `>` : unaffected baseline, must stay passing (regression guard) ───────────

#[test]
fn filter_gt_computed_arithmetic_regression_baseline() {
    // Already routes through compare_graph_elements today; must keep working.
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER((1 + 1) > 1) }");
    assert_eq!(
        n, 1,
        "(1 + 1) > 1 already worked before this fix and must keep working"
    );
}

// ── `!=` : both directions ────────────────────────────────────────────────────

#[test]
fn filter_ne_computed_arithmetic_true_when_values_differ() {
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER((1 + 1) != 3) }");
    assert_eq!(
        n, 1,
        "(1 + 1) != 3 should hold: 2 and 3 are genuinely different values"
    );
}

#[test]
fn filter_ne_computed_arithmetic_false_when_values_equal() {
    // Before the fix: IntegerLiteral(2) != TypedLiteral{xsd:integer,"2"} was
    // (wrongly) `true` via raw `!=`, purely because the variants differ, even
    // though both denote 2. Must become 0 rows once fixed.
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER((1 + 1) != 2) }");
    assert_eq!(
        n, 0,
        "(1 + 1) != 2 should NOT hold: both sides denote the same value 2"
    );
}

// ── Boolean equality: BooleanLiteral (computed) vs TypedLiteral{xsd:boolean} (parsed) ─

#[test]
fn filter_eq_computed_boolean_matches_parsed_true() {
    // STRSTARTS(...) evaluates to native BooleanLiteral(true); `true` in query
    // text parses to TypedLiteral{xsd:boolean,"true"} (parse_boolean_literal).
    let n =
        row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER(STRSTARTS(\"hello\", \"he\") = true) }");
    assert_eq!(
        n, 1,
        "STRSTARTS(...) = true should hold once boolean normalization is fixed"
    );
}

#[test]
fn filter_eq_computed_boolean_negative_case() {
    // Sanity check: booleans must not become trivially always-equal.
    let n =
        row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER(STRSTARTS(\"hello\", \"zz\") = true) }");
    assert_eq!(
        n, 0,
        "STRSTARTS(\"hello\", \"zz\") is false, so = true must not hold"
    );
}

// ── Same-variant sanity: two native values of the identical variant ──────────

#[test]
fn filter_eq_same_native_variant_still_correct() {
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER(ABS(-5) = ABS(-5)) }");
    assert_eq!(
        n, 1,
        "two identical computed IntegerLiterals must still compare equal"
    );
}

// ── Negative case: unequal computed value vs. parsed literal ─────────────────

#[test]
fn filter_eq_unequal_values_not_equal() {
    // Guards against a fix that makes `=` always return true for any pair of
    // numeric-looking literals regardless of value.
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER((1 + 1) = 3) }");
    assert_eq!(
        n, 0,
        "(1 + 1) = 3 must not hold: 2 and 3 are different values"
    );
}

// ── IN / NOT IN: SPARQL defines these in terms of `=` (§17.4.1.9), same bug shape ─

#[test]
fn filter_in_computed_value_matches_parsed_literal() {
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER((1 + 1) IN (2, 3)) }");
    assert_eq!(n, 1, "(1 + 1) IN (2, 3) should hold: 2 is in the list");
}

#[test]
fn filter_not_in_computed_value_excludes_match() {
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER((1 + 1) NOT IN (2, 3)) }");
    assert_eq!(
        n, 0,
        "(1 + 1) NOT IN (2, 3) must not hold: 2 is in the list"
    );
}

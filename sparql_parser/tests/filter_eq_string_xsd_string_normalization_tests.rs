//! Regression tests for #360: `FILTER`'s `=`/`!=` (and `IN`/`NOT IN`, which
//! SPARQL 1.1 §17.4.1.9 defines in terms of `=`) must treat a simple/plain
//! string literal (`"foo"`, parsed to `RdfLiteral::LiteralString`) and an
//! explicitly `xsd:string`-typed literal (`"foo"^^xsd:string`, parsed to
//! `RdfLiteral::TypedLiteral { type_iri: xsd:string, .. }`) as the *same
//! value* — RDF 1.1 §5.1 defines these as equivalent in the value space, and
//! SPARQL's `=` operator (unlike `sameTerm`) is value equality, not term
//! identity.
//!
//! Investigating #360 (see `docs/plans/XSD_STRING_LITERAL_360_PLAN.md`)
//! found that `sparql_parser`'s own query-text literal grammar
//! (`parse_string_literal` in `sparql_parser::lib`) already keeps
//! `"foo"^^xsd:string` as a distinct `TypedLiteral` rather than collapsing it
//! the way `turtle::convert_literal` does for parsed Turtle data — so this
//! gap is reachable *today*, entirely within SPARQL query syntax, with no
//! Turtle involved at all. `values_equal` (`sparql_parser/src/execute.rs`)
//! already normalizes numeric and boolean cross-representation mismatches
//! (#208) but has no equivalent case for strings, so `"foo" = "foo"^^xsd:string`
//! evaluates to `false` today: confirmed by a throwaway probe during the
//! #360 investigation before this file existed.
//!
//! This normalization is a prerequisite for #360's ingestion-side
//! investigation: whatever `turtle`/other producers do to `xsd:string`
//! literals, `=`/`!=`/`IN`/`NOT IN` must not regress on the plain-literal/
//! `xsd:string`-typed-literal value equivalence that RDF 1.1 guarantees.
//!
//! See <https://github.com/daghovland/rdf-datalog/issues/360>.

use dag_rdf::Datastore;
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult};
use std::collections::HashMap;

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
        base: None,
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

const XSD_STRING: &str = "<http://www.w3.org/2001/XMLSchema#string>";

// ── `=` : plain literal vs. explicit xsd:string, both directions ─────────────

#[test]
fn filter_eq_plain_matches_explicit_xsd_string() {
    let n = row_count(&format!(
        "SELECT ?x WHERE {{ BIND(1 AS ?x) FILTER(\"foo\" = \"foo\"^^{XSD_STRING}) }}"
    ));
    assert_eq!(
        n, 1,
        "\"foo\" = \"foo\"^^xsd:string must hold: RDF 1.1 value equivalence"
    );
}

#[test]
fn filter_eq_explicit_xsd_string_matches_plain() {
    let n = row_count(&format!(
        "SELECT ?x WHERE {{ BIND(1 AS ?x) FILTER(\"foo\"^^{XSD_STRING} = \"foo\") }}"
    ));
    assert_eq!(
        n, 1,
        "\"foo\"^^xsd:string = \"foo\" must hold (symmetric to the above)"
    );
}

// ── `!=` ───────────────────────────────────────────────────────────────────

#[test]
fn filter_ne_plain_vs_explicit_xsd_string_is_false() {
    let n = row_count(&format!(
        "SELECT ?x WHERE {{ BIND(1 AS ?x) FILTER(\"foo\" != \"foo\"^^{XSD_STRING}) }}"
    ));
    assert_eq!(
        n, 0,
        "\"foo\" != \"foo\"^^xsd:string must NOT hold: same value"
    );
}

// ── Negative case: different lexical values must still compare unequal ──────

#[test]
fn filter_eq_different_values_still_not_equal() {
    // Not ignored: this must already pass and must keep passing after the fix
    // — guards against a fix that makes `=` trivially true for any pair of
    // string-shaped literals.
    let n = row_count(&format!(
        "SELECT ?x WHERE {{ BIND(1 AS ?x) FILTER(\"foo\" = \"bar\"^^{XSD_STRING}) }}"
    ));
    assert_eq!(n, 0, "\"foo\" = \"bar\"^^xsd:string must not hold");
}

// ── Language-tagged literals must NOT be swept into the same normalization ──

#[test]
fn filter_eq_lang_literal_vs_xsd_string_not_equal() {
    // Not ignored: already correct today (language-tagged literals never
    // equal a differently-tagged/untagged literal under XPath value
    // equality) and must stay correct — a string/xsd:string normalization
    // must not accidentally widen to also match language-tagged literals.
    let n = row_count(&format!(
        "SELECT ?x WHERE {{ BIND(1 AS ?x) FILTER(\"foo\"@en = \"foo\"^^{XSD_STRING}) }}"
    ));
    assert_eq!(n, 0, "\"foo\"@en = \"foo\"^^xsd:string must not hold");
}

#[test]
fn filter_eq_lang_literal_vs_plain_not_equal() {
    // Baseline: language-tagged vs. plain, no xsd:string involved.
    let n = row_count("SELECT ?x WHERE { BIND(1 AS ?x) FILTER(\"foo\"@en = \"foo\") }");
    assert_eq!(n, 0, "\"foo\"@en = \"foo\" must not hold");
}

// ── IN / NOT IN: SPARQL defines these in terms of `=` (§17.4.1.9) ───────────

#[test]
fn filter_in_plain_matches_xsd_string_in_list() {
    let n = row_count(&format!(
        "SELECT ?x WHERE {{ BIND(1 AS ?x) FILTER(\"foo\" IN (\"foo\"^^{XSD_STRING}, \"zzz\")) }}"
    ));
    assert_eq!(
        n, 1,
        "\"foo\" IN (\"foo\"^^xsd:string, \"zzz\") should hold"
    );
}

#[test]
fn filter_not_in_excludes_xsd_string_match() {
    let n = row_count(&format!(
        "SELECT ?x WHERE {{ BIND(1 AS ?x) FILTER(\"foo\" NOT IN (\"foo\"^^{XSD_STRING}, \"zzz\")) }}"
    ));
    assert_eq!(
        n, 0,
        "\"foo\" NOT IN (\"foo\"^^xsd:string, \"zzz\") must not hold: \"foo\" is in the list"
    );
}

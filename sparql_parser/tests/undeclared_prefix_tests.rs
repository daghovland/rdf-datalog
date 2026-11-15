//! An undeclared prefix in a SPARQL query must be a parse error, not a silent
//! "matches nothing" fallback. Before this fix, `parse_prefixed_name` treated
//! any prefix not present in `ctx.prefixes` as a literal string
//! (`prefix.to_string() + ":"`), so e.g. `totallyundeclaredprefix:foo` parsed
//! "successfully" into a bizarre, never-matching IRI instead of failing —
//! `SELECT`/`ASK`/etc. would then return HTTP 200 with an empty result set
//! instead of an error, inconsistent with SPARQL Update (which already
//! rejects undeclared prefixes, via `turtle_parser::parse_turtle`).
//!
//! See issue [#389](https://github.com/daghovland/rdf-datalog/issues/389).

use sparql_parser::{parse_query, ParserContext};
use std::collections::HashMap;

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
        base: None,
    }
}

/// Asserts that an undeclared-prefix parse error is specifically a hard
/// `nom::Err::Failure`, not a soft `nom::Err::Error`.
///
/// This distinction matters for issue
/// [#747](https://github.com/daghovland/rdf-datalog/issues/747): a bare
/// `result.is_err()` check (as this file originally had for every case) is
/// satisfied even when `parse_prefixed_name`'s `Err::Failure` gets silently
/// swallowed and replaced by a different, generic `Err::Error` somewhere
/// higher up the call stack (which is exactly what was happening for
/// `SELECT`/`ASK`/`CONSTRUCT` before #747's fix to
/// `parse_group_graph_pattern_contents`'s triple-pattern loop) -- the query
/// still fails to parse either way, so `is_err()` can't tell the two apart,
/// but only the `Failure` case carries the remaining-input slice that lets
/// `sparql_parser::describe_parse_error` name the offending prefix.
fn assert_undeclared_prefix_failure(sparql: &str) {
    let mut c = ctx();
    let result = parse_query(sparql, &mut c);
    match result {
        Err(nom::Err::Failure(_)) => {}
        other => panic!(
            "expected a hard `nom::Err::Failure` for an undeclared prefix \
             (not just any error), got: {other:?}\nquery: {sparql}"
        ),
    }
}

/// Control / non-regression: the same query shape, but with the prefix
/// properly declared, must still parse. Guards against the fix being
/// over-eager and rejecting legitimate declared prefixes.
#[test]
fn test_declared_prefix_still_works() {
    let sparql = r#"
        PREFIX ex: <http://example.org/>
        SELECT * WHERE { ?s ex:foo ?o }
    "#;
    parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
}

/// Exact repro from #389.
#[test]
fn test_select_with_undeclared_prefix_is_parse_error() {
    assert_undeclared_prefix_failure("SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }");
}

#[test]
fn test_ask_with_undeclared_prefix_is_parse_error() {
    assert_undeclared_prefix_failure("ASK { ?s totallyundeclaredprefix:foo ?o }");
}

#[test]
fn test_construct_with_undeclared_prefix_is_parse_error() {
    assert_undeclared_prefix_failure(
        "CONSTRUCT { ?s totallyundeclaredprefix:foo ?o } WHERE { ?s ?p ?o }",
    );
}

#[test]
fn test_describe_with_undeclared_prefix_is_parse_error() {
    assert_undeclared_prefix_failure("DESCRIBE totallyundeclaredprefix:foo");
}

/// Undeclared prefix used as a `^^`-datatype IRI on a typed literal inside a
/// `FILTER` — proves the fix is centralized in the shared
/// `parse_prefixed_name`, not just the triple-pattern call site.
#[test]
fn test_undeclared_prefix_in_filter_datatype_is_parse_error() {
    assert_undeclared_prefix_failure(
        r#"SELECT * WHERE { ?s ?p ?o . FILTER(?o = "1"^^undeclaredprefix:integer) }"#,
    );
}

/// Undeclared prefix as a constant inside `VALUES`.
#[test]
fn test_undeclared_prefix_in_values_is_parse_error() {
    assert_undeclared_prefix_failure(
        r#"
        SELECT * WHERE {
            VALUES ?o { undeclaredprefix:foo }
            ?s ?p ?o .
        }
    "#,
    );
}

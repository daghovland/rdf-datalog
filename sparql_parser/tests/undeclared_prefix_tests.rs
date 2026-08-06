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
    let sparql = "SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }";
    let mut c = ctx();
    let result = parse_query(sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a parse error for an undeclared prefix, got: {result:?}"
    );
}

#[test]
fn test_ask_with_undeclared_prefix_is_parse_error() {
    let sparql = "ASK { ?s totallyundeclaredprefix:foo ?o }";
    let mut c = ctx();
    let result = parse_query(sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a parse error for an undeclared prefix, got: {result:?}"
    );
}

#[test]
fn test_construct_with_undeclared_prefix_is_parse_error() {
    let sparql = "CONSTRUCT { ?s totallyundeclaredprefix:foo ?o } WHERE { ?s ?p ?o }";
    let mut c = ctx();
    let result = parse_query(sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a parse error for an undeclared prefix, got: {result:?}"
    );
}

#[test]
fn test_describe_with_undeclared_prefix_is_parse_error() {
    let sparql = "DESCRIBE totallyundeclaredprefix:foo";
    let mut c = ctx();
    let result = parse_query(sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a parse error for an undeclared prefix, got: {result:?}"
    );
}

/// Undeclared prefix used as a `^^`-datatype IRI on a typed literal inside a
/// `FILTER` — proves the fix is centralized in the shared
/// `parse_prefixed_name`, not just the triple-pattern call site.
#[test]
fn test_undeclared_prefix_in_filter_datatype_is_parse_error() {
    let sparql = r#"SELECT * WHERE { ?s ?p ?o . FILTER(?o = "1"^^undeclaredprefix:integer) }"#;
    let mut c = ctx();
    let result = parse_query(sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a parse error for an undeclared prefix, got: {result:?}"
    );
}

/// Undeclared prefix as a constant inside `VALUES`.
#[test]
fn test_undeclared_prefix_in_values_is_parse_error() {
    let sparql = r#"
        SELECT * WHERE {
            VALUES ?o { undeclaredprefix:foo }
            ?s ?p ?o .
        }
    "#;
    let mut c = ctx();
    let result = parse_query(sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a parse error for an undeclared prefix, got: {result:?}"
    );
}

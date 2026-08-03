//! Regression tests for issue #364: unbounded recursion depth in nested
//! FILTER/group-graph-pattern parsing (stack-overflow DoS).
//!
//! `sparql_parser`'s recursive-descent parser had no depth guard on either of
//! its two genuinely-recursive entry points:
//! - `parse_primary_expression` recursing into `parse_expression` for
//!   parenthesized sub-expressions (`FILTER(((...)))`).
//! - `parse_group_graph_pattern` recursing into itself for nested
//!   `{ { { ... } } }` group graph patterns (also reached via `OPTIONAL`,
//!   `GRAPH`, `MINUS`, `SERVICE`, `EXISTS`/`NOT EXISTS`, and subqueries).
//!
//! A query crafted with tens of thousands of nesting levels drove the native
//! call stack arbitrarily deep and could abort the whole process — since
//! `sparql_endpoint` is a single axum/tokio process, a stack overflow there
//! takes down every other in-flight request, not just the malicious one.
//!
//! These tests assert that excessive nesting now returns a clean parse
//! `Err` (not a crash/hang), while nesting depths a real query would
//! plausibly use still parse successfully.

use sparql_parser::{parse_query, ParserContext};
use std::collections::HashMap;

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
        base: None,
    }
}

/// Build a query with `depth` levels of parenthesized nesting inside a FILTER,
/// e.g. depth=3 -> `FILTER(((1=1)))`.
fn nested_filter_query(depth: usize) -> String {
    let open: String = "(".repeat(depth);
    let close: String = ")".repeat(depth);
    format!("SELECT * WHERE {{ ?s ?p ?o . FILTER{open}1=1{close} }}")
}

/// Build a query with `depth` levels of nested group graph patterns,
/// e.g. depth=3 -> `{ { { ?s ?p ?o } } }`.
fn nested_group_query(depth: usize) -> String {
    let open: String = "{ ".repeat(depth);
    let close: String = "} ".repeat(depth);
    format!("SELECT * WHERE {{ {open}?s ?p ?o {close}}}")
}

#[test]
#[ignore = "TDD red phase (#364): no depth guard implemented yet — this would stack-overflow and abort the test binary, not just fail. Unignore once the depth guard lands."]
fn deeply_nested_filter_parens_returns_clean_error() {
    let sparql = nested_filter_query(2000);
    let mut c = ctx();
    let result = parse_query(&sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a clean parse error for 2000-deep FILTER nesting, got Ok"
    );
}

#[test]
#[ignore = "TDD red phase (#364): no depth guard implemented yet — this would stack-overflow and abort the test binary, not just fail. Unignore once the depth guard lands."]
fn deeply_nested_group_graph_patterns_returns_clean_error() {
    let sparql = nested_group_query(2000);
    let mut c = ctx();
    let result = parse_query(&sparql, &mut c);
    assert!(
        result.is_err(),
        "expected a clean parse error for 2000-deep group graph pattern nesting, got Ok"
    );
}

#[test]
fn reasonable_filter_nesting_still_parses() {
    // 15 levels: deeper than any real query is likely to need, but nowhere
    // near the DoS threshold.
    let sparql = nested_filter_query(15);
    let mut c = ctx();
    let result = parse_query(&sparql, &mut c);
    assert!(
        result.is_ok(),
        "15-level FILTER nesting should still parse: {:?}",
        result.err()
    );
}

#[test]
fn reasonable_group_graph_pattern_nesting_still_parses() {
    let sparql = nested_group_query(15);
    let mut c = ctx();
    let result = parse_query(&sparql, &mut c);
    assert!(
        result.is_ok(),
        "15-level group graph pattern nesting should still parse: {:?}",
        result.err()
    );
}

#[test]
fn depth_guard_does_not_leak_across_independent_parses() {
    // Regression guard for a depth-counter leak: if the recursion-depth
    // counter isn't correctly restored on backtrack/error (e.g. a
    // thread-local counter decremented only on success, or an `alt` branch
    // that errors out without unwinding the guard), repeated independent
    // parses on the same thread would eventually push the counter past the
    // limit even though each individual query is shallow.
    let shallow = nested_filter_query(5);
    let mut c = ctx();
    for _ in 0..500 {
        let result = parse_query(&shallow, &mut c);
        assert!(result.is_ok(), "shallow query should always parse cleanly");
    }
}

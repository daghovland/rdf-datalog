//! Tests for SPARQL 1.1 scalar builtin functions (#52).
//! See docs/plans/SPARQL_MISSING_FEATURES_PLAN.md and
//! https://github.com/daghovland/rdf-datalog/issues/52

use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource, Triple};
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult};
use std::collections::HashMap;

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
        base: None,
    }
}

/// Execute a SELECT query over an empty datastore and return the first row's
/// binding for `?result` as a GraphElement.  Queries are expected to use
/// `BIND(fn(...) AS ?result)` to expose the function output.
fn eval_function(sparql: &str) -> Option<GraphElement> {
    let ds = Datastore::new(100);
    let (_, query) = parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
    match execute(&query, &ds, NetworkPolicy::Deny).expect("execute should succeed") {
        QueryResult::Select(r) => r
            .rows
            .into_iter()
            .next()
            .and_then(|row| row.get("result").cloned()),
        _ => panic!("expected SELECT"),
    }
}

fn str_literal(s: &str) -> GraphElement {
    GraphElement::GraphLiteral(RdfLiteral::LiteralString(s.to_string()))
}

fn typed_literal(s: &str, type_iri: &str) -> GraphElement {
    GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(type_iri.to_string()),
        literal: s.to_string(),
    })
}

fn lang_literal(s: &str, lang: &str) -> GraphElement {
    GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
        lang: lang.to_string(),
        literal: s.to_string(),
    })
}

fn iri_node(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
#[allow(dead_code)]
const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";
const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
const XSD_FLOAT: &str = "http://www.w3.org/2001/XMLSchema#float";
const XSD_DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

// ── String functions ──────────────────────────────────────────────────────────

#[test]

fn test_ucase() {
    let result = eval_function(r#"SELECT (UCASE("hello") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("HELLO")));
}

#[test]

fn test_lcase() {
    let result = eval_function(r#"SELECT (LCASE("HELLO") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("hello")));
}

#[test]

fn test_concat() {
    let result = eval_function(r#"SELECT (CONCAT("foo", "bar") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("foobar")));
}

#[test]

fn test_substr_two_arg() {
    // SPARQL SUBSTR is 1-indexed; SUBSTR("hello", 2) → "ello"
    let result = eval_function(r#"SELECT (SUBSTR("hello", 2) AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("ello")));
}

#[test]

fn test_substr_three_arg() {
    let result = eval_function(r#"SELECT (SUBSTR("hello", 2, 3) AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("ell")));
}

#[test]

fn test_strstarts_true() {
    let result = eval_function(r#"SELECT (STRSTARTS("hello", "he") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(true)))
    );
}

#[test]

fn test_strstarts_false() {
    let result = eval_function(r#"SELECT (STRSTARTS("hello", "lo") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
            false
        )))
    );
}

#[test]

fn test_strends_true() {
    let result = eval_function(r#"SELECT (STRENDS("hello", "lo") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(true)))
    );
}

#[test]

fn test_contains_true() {
    let result = eval_function(r#"SELECT (CONTAINS("hello", "ell") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(true)))
    );
}

#[test]

fn test_strbefore() {
    let result = eval_function(r#"SELECT (STRBEFORE("hello world", " ") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("hello")));
}

#[test]

fn test_strafter() {
    let result = eval_function(r#"SELECT (STRAFTER("hello world", " ") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("world")));
}

// ── Type testing ──────────────────────────────────────────────────────────────

#[test]

fn test_isnumeric_integer() {
    let result = eval_function(r#"SELECT (ISNUMERIC(42) AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(true)))
    );
}

#[test]

fn test_isnumeric_string() {
    let result = eval_function(r#"SELECT (ISNUMERIC("hello") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
            false
        )))
    );
}

#[test]

fn test_sameterm_equal() {
    let result = eval_function(
        r#"PREFIX ex: <http://example.org/> SELECT (SAMETERM(ex:a, ex:a) AS ?result) WHERE {}"#,
    );
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(true)))
    );
}

#[test]

fn test_sameterm_different() {
    let result = eval_function(
        r#"PREFIX ex: <http://example.org/> SELECT (SAMETERM(ex:a, ex:b) AS ?result) WHERE {}"#,
    );
    assert_eq!(
        result,
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
            false
        )))
    );
}

// ── Term construction ─────────────────────────────────────────────────────────

#[test]

fn test_iri_from_string() {
    let result = eval_function(r#"SELECT (IRI("http://example.org/foo") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(iri_node("http://example.org/foo")));
}

#[test]

fn test_strdt() {
    let result = eval_function(&format!(
        r#"SELECT (STRDT("42", <{XSD_INTEGER}>) AS ?result) WHERE {{}}"#
    ));
    assert_eq!(result, Some(typed_literal("42", XSD_INTEGER)));
}

#[test]

fn test_strlang() {
    let result = eval_function(r#"SELECT (STRLANG("hello", "en") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(lang_literal("hello", "en")));
}

// ── Numeric functions ─────────────────────────────────────────────────────────

#[test]

fn test_abs_negative() {
    // #228: numeric builtins emit `TypedLiteral{xsd:integer, ..}` (matching
    // real parsed data), not the native `IntegerLiteral` variant, so a
    // computed result can join against already-interned data of the same
    // value.
    let result = eval_function(r#"SELECT (ABS(-5) AS ?result) WHERE {}"#);
    assert_eq!(result, Some(typed_literal("5", XSD_INTEGER)));
}

#[test]

fn test_round() {
    // SPARQL ROUND uses half-to-even; 2.5 rounds to 2 or 3 — accept either.
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(r#"SELECT (ROUND(2.5) AS ?result) WHERE {}"#);
    if let Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })) =
        &result
    {
        assert_eq!(type_iri.0, XSD_INTEGER);
        assert!(
            literal == "2" || literal == "3",
            "ROUND(2.5) must be 2 or 3, got {literal}"
        );
    } else {
        panic!("expected TypedLiteral{{xsd:integer, ..}} from ROUND, got {result:?}");
    }
}

#[test]

fn test_ceil() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(r#"SELECT (CEIL(1.2) AS ?result) WHERE {}"#);
    assert_eq!(result, Some(typed_literal("2", XSD_INTEGER)));
}

#[test]

fn test_floor() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(r#"SELECT (FLOOR(1.9) AS ?result) WHERE {}"#);
    assert_eq!(result, Some(typed_literal("1", XSD_INTEGER)));
}

// ── Logic / control ───────────────────────────────────────────────────────────

#[test]

fn test_coalesce_returns_first_bound() {
    // ?x is unbound; COALESCE(?x, "default") should return "default"
    let result = eval_function(r#"SELECT (COALESCE(?x, "default") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("default")));
}

#[test]

fn test_if_true_branch() {
    let result = eval_function(r#"SELECT (IF(true, "yes", "no") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("yes")));
}

#[test]

fn test_if_false_branch() {
    let result = eval_function(r#"SELECT (IF(false, "yes", "no") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("no")));
}

// ── Remaining missing builtins (issue #52) ────────────────────────────────────
// https://github.com/daghovland/rdf-datalog/issues/52

#[test]
fn test_bnode_no_arg_returns_blank_node() {
    let result = eval_function(r#"SELECT (BNODE() AS ?result) WHERE {}"#);
    assert!(
        matches!(
            result,
            Some(GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(_)))
        ),
        "BNODE() must return a blank node; got: {result:?}"
    );
}

#[test]
fn test_bnode_with_arg_returns_blank_node() {
    let result = eval_function(r#"SELECT (BNODE("x") AS ?result) WHERE {}"#);
    assert!(
        matches!(
            result,
            Some(GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(_)))
        ),
        "BNODE(str) must return a blank node; got: {result:?}"
    );
}

#[test]
fn test_encode_for_uri() {
    let result = eval_function(r#"SELECT (ENCODE_FOR_URI("Los Angeles") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("Los%20Angeles")));
}

#[test]
fn test_replace_basic() {
    let result = eval_function(r#"SELECT (REPLACE("ababab", "b", "Z") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("aZaZaZ")));
}

#[test]
fn test_replace_with_flag_i() {
    let result =
        eval_function(r#"SELECT (REPLACE("Hello World", "hello", "Hi", "i") AS ?result) WHERE {}"#);
    assert_eq!(result, Some(str_literal("Hi World")));
}

#[test]
fn test_rand_returns_double_in_unit_interval() {
    let result = eval_function(r#"SELECT (RAND() AS ?result) WHERE {}"#);
    match result {
        Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(v))) => {
            assert!(
                v.into_inner() >= 0.0 && v.into_inner() <= 1.0,
                "RAND() must be in [0,1]; got {v}"
            );
        }
        other => panic!("expected DoubleLiteral in [0,1]; got {other:?}"),
    }
}

#[test]
fn test_now_returns_datetime() {
    let result = eval_function(r#"SELECT (NOW() AS ?result) WHERE {}"#);
    assert!(
        matches!(
            result,
            Some(GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(_)))
        ),
        "NOW() must return an xsd:dateTime; got {result:?}"
    );
}

#[test]
fn test_year_from_datetime() {
    // #228 (extended beyond the issue's enumerated scope to the same
    // producer bug in the date/time component functions): TypedLiteral, not
    // the native IntegerLiteral variant.
    let result = eval_function(
        r#"SELECT (YEAR("2023-01-15T10:30:45Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("2023", XSD_INTEGER)));
}

#[test]
fn test_month_from_datetime() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"SELECT (MONTH("2023-01-15T10:30:45Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("1", XSD_INTEGER)));
}

#[test]
fn test_day_from_datetime() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"SELECT (DAY("2023-01-15T10:30:45Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("15", XSD_INTEGER)));
}

#[test]
fn test_hours_from_datetime() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"SELECT (HOURS("2023-01-15T10:30:45Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("10", XSD_INTEGER)));
}

#[test]
fn test_minutes_from_datetime() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"SELECT (MINUTES("2023-01-15T10:30:45Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("30", XSD_INTEGER)));
}

#[test]
fn test_seconds_from_datetime() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"SELECT (SECONDS("2023-01-15T10:30:45Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("45", XSD_DECIMAL)));
}

#[test]
fn test_tz_utc() {
    let result = eval_function(
        r#"SELECT (TZ("2023-01-15T10:30:45Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(str_literal("Z")));
}

#[test]
fn test_md5() {
    // MD5("abc") = 900150983cd24fb0d6963f7d28e17f72
    let result = eval_function(r#"SELECT (MD5("abc") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(str_literal("900150983cd24fb0d6963f7d28e17f72"))
    );
}

#[test]
fn test_sha1() {
    // SHA1("abc") = a9993e364706816aba3e25717850c26c9cd0d89d
    let result = eval_function(r#"SELECT (SHA1("abc") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(str_literal("a9993e364706816aba3e25717850c26c9cd0d89d"))
    );
}

#[test]
fn test_sha256() {
    // SHA256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
    let result = eval_function(r#"SELECT (SHA256("abc") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(str_literal(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ))
    );
}

#[test]
fn test_sha384() {
    // SHA384("abc") = cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7
    let result = eval_function(r#"SELECT (SHA384("abc") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(str_literal(
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        ))
    );
}

#[test]
fn test_sha512() {
    // SHA512("abc") = ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f
    let result = eval_function(r#"SELECT (SHA512("abc") AS ?result) WHERE {}"#);
    assert_eq!(
        result,
        Some(str_literal(
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        ))
    );
}

#[test]
fn test_uuid_returns_urn_iri() {
    let result = eval_function(r#"SELECT (UUID() AS ?result) WHERE {}"#);
    match result {
        Some(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri)))) => {
            assert!(
                iri.starts_with("urn:uuid:"),
                "UUID() must return a urn:uuid: IRI; got {iri}"
            );
        }
        other => panic!("expected IRI starting with urn:uuid:; got {other:?}"),
    }
}

#[test]
fn test_struuid_returns_uuid_string() {
    let result = eval_function(r#"SELECT (STRUUID() AS ?result) WHERE {}"#);
    match result {
        Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s))) => {
            // UUID format: 8-4-4-4-12 hex digits
            assert_eq!(
                s.len(),
                36,
                "STRUUID() string should be 36 chars; got {s:?}"
            );
            assert_eq!(&s[8..9], "-", "UUID part 1-2 separator missing");
            assert_eq!(&s[13..14], "-", "UUID part 2-3 separator missing");
        }
        other => panic!("expected plain string; got {other:?}"),
    }
}

// ── Prefixed-name function-call parsing (#186) ─────────────────────────────
//
// `parse_function_call` used to try a bare-alphanumeric-word match *before*
// `parse_prefixed_name` when parsing a function's name. For input like
// `xsd:integer(?o)`, the bare-word alternative greedily matches just `xsd`
// (stopping at `:`) and *succeeds*, so nom's `alt` commits to it and never
// backtracks into the prefixed-name alternative — the parser then expects
// `(` right after `xsd`, sees `:`, and the whole function-call parse fails.
// See https://github.com/daghovland/rdf-datalog/issues/186.

/// Minimal repro from #186: a prefixed-name function call (`xsd:integer(...)`)
/// must parse. This does not assert anything about the *value* `xsd:integer`
/// produces — casting to `xsd:integer` isn't implemented as a value function
/// yet (a separate concern from this parser bug) — only that the query
/// parses and executes without error.
#[test]
fn test_prefixed_name_function_call_parses() {
    let sparql = r#"
        PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
        PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
        SELECT (xsd:integer(?o) AS ?count) { ?s rdfs:label ?o . }
    "#;
    parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
}

/// Same query as above, but executed against a datastore with one matching
/// `rdfs:label` triple, to confirm the parsed query also *executes* cleanly
/// (returns exactly one row) rather than merely parsing.
#[test]
fn test_prefixed_name_function_call_executes() {
    let mut ds = Datastore::new(64);
    let s = ds.add_node_resource(RdfResource::Iri(IriReference(
        "http://example.org/s".to_string(),
    )));
    let p = ds.add_node_resource(RdfResource::Iri(IriReference(
        "http://www.w3.org/2000/01/rdf-schema#label".to_string(),
    )));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("42".to_string()));
    ds.add_triple(Triple {
        subject: s,
        predicate: p,
        obj: o,
    });

    let sparql = r#"
        PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
        PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
        SELECT (xsd:integer(?o) AS ?count) { ?s rdfs:label ?o . }
    "#;
    let (_, query) = parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
    let result = match execute(&query, &ds, NetworkPolicy::Deny).expect("execute should succeed") {
        QueryResult::Select(r) => r,
        _ => panic!("expected SELECT"),
    };
    assert_eq!(
        result.rows.len(),
        1,
        "expected exactly one row for the single matching triple"
    );
}

/// Other prefixed-name-as-function-name cast forms from the same family
/// (`xsd:string`, `xsd:double`) must also parse — the bug affects any
/// `prefix:localname(...)` call, not just `xsd:integer`.
#[test]
fn test_other_xsd_prefixed_cast_calls_parse() {
    for fname in ["xsd:string", "xsd:double", "xsd:dateTime"] {
        let sparql = format!(
            r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT ({fname}(?o) AS ?v) {{ ?s ?p ?o }}"#
        );
        parse_query(&sparql, &mut ctx())
            .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
    }
}

/// The exact query shape from the DBLP benchmark's `strstarts`/`strends`
/// diagnostics (`tests/testdata/dblp.benchmark.tsv`) that originally
/// surfaced #186: a builtin call (`STRSTARTS`) nested inside a prefixed-name
/// cast (`xsd:integer`) nested inside an aggregate (`SUM`).
#[test]
fn test_benchmark_shape_sum_xsd_integer_strstarts_parses() {
    let sparql = r#"
        PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
        PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
        SELECT (SUM(xsd:integer(STRSTARTS(?o, "a"))) AS ?count) { ?s rdfs:label ?o . }
    "#;
    parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
}

/// Regression: bare-word builtin function calls (which never contain `:`)
/// must keep parsing after the `alt` reorder in `parse_function_call`.
#[test]
fn test_bare_word_builtin_calls_still_parse_after_reorder() {
    for sparql in [
        r#"SELECT (SUM(?x) AS ?v) { ?s ?p ?x }"#,
        r#"SELECT (ABS(?x) AS ?v) { ?s ?p ?x }"#,
        r#"SELECT (STRSTARTS(?x, "a") AS ?v) { ?s ?p ?x }"#,
        r#"SELECT (COUNT(*) AS ?v) { ?s ?p ?x }"#,
    ] {
        parse_query(sparql, &mut ctx())
            .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
    }
}

// ── XSD cast/constructor functions (#190) ───────────────────────────────────
//
// SPARQL 1.1 §17.4.2 "Functional Forms" datatype constructor/cast semantics:
// `xsd:integer(v)`, `xsd:decimal(v)`, `xsd:double(v)`, `xsd:float(v)`,
// `xsd:string(v)`, `xsd:boolean(v)`. Follow-up from #186/PR #189, which fixed
// *parsing* of `prefix:localname(...)` function calls but deliberately left
// value-casting semantics unimplemented (the call evaluated to unbound).
// `xsd:dateTime(v)` casting is covered separately below (#194).
// See https://github.com/daghovland/rdf-datalog/issues/190

#[test]
fn test_xsd_cast_integer_from_string() {
    // #228: xsd casts emit `TypedLiteral{type_iri, ..}` (matching real
    // parsed data), not a native `RdfLiteral` variant, so a cast result can
    // join against already-interned data of the same value.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:integer("42") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("42", XSD_INTEGER)));
}

#[test]
fn test_xsd_cast_integer_from_decimal_truncates() {
    // xsd:integer(3.7) truncates toward zero (XPath fn:integer cast rules),
    // it does not floor/round. See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:integer(3.7) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("3", XSD_INTEGER)));
}

#[test]
fn test_xsd_cast_integer_from_negative_decimal_truncates_toward_zero() {
    // Discriminates truncation from floor: floor(-3.7) == -4, but XPath cast
    // truncation of -3.7 must be -3. See #228 for the TypedLiteral output
    // shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:integer(-3.7) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("-3", XSD_INTEGER)));
}

#[test]
fn test_xsd_cast_integer_from_boolean() {
    // See #228 for the TypedLiteral output shape.
    let result_true = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:integer(true) AS ?result) WHERE {}"#,
    );
    assert_eq!(result_true, Some(typed_literal("1", XSD_INTEGER)));
    let result_false = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:integer(false) AS ?result) WHERE {}"#,
    );
    assert_eq!(result_false, Some(typed_literal("0", XSD_INTEGER)));
}

#[test]
fn test_xsd_cast_integer_invalid_string_is_unbound() {
    // Per SPARQL error-handling rules, a failed cast makes the projected
    // variable unbound rather than erroring the whole query (matches the
    // established convention for other builtins in this file: `?result`
    // simply doesn't appear in the row).
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:integer("not a number") AS ?result) WHERE {}"#,
    );
    assert_eq!(
        result, None,
        "invalid xsd:integer cast should leave ?result unbound"
    );
}

#[test]
fn test_xsd_cast_string_from_integer() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:string(42) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(str_literal("42")));
}

#[test]
fn test_xsd_cast_string_from_boolean() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:string(true) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(str_literal("true")));
}

#[test]
fn test_xsd_cast_string_from_decimal() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:string(12.5) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(str_literal("12.5")));
}

#[test]
fn test_xsd_cast_boolean_from_string_true() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:boolean("true") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("true", XSD_BOOLEAN)));
}

#[test]
fn test_xsd_cast_boolean_from_string_zero_is_false() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:boolean("0") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("false", XSD_BOOLEAN)));
}

#[test]
fn test_xsd_cast_boolean_from_integer() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:boolean(5) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("true", XSD_BOOLEAN)));
}

#[test]
fn test_xsd_cast_boolean_invalid_string_is_unbound() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:boolean("maybe") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, None);
}

#[test]
fn test_xsd_cast_double_from_string() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:double("12.5") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("12.5", XSD_DOUBLE)));
}

#[test]
fn test_xsd_cast_double_from_integer() {
    // See #228 for the TypedLiteral output shape. Rust's `f64` Display
    // omits the trailing ".0" for whole values, so `42.0` renders as `"42"`.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:double(42) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("42", XSD_DOUBLE)));
}

#[test]
fn test_xsd_cast_double_invalid_string_is_unbound() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:double("abc") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, None);
}

#[test]
fn test_xsd_cast_float_from_string() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:float("2.5") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("2.5", XSD_FLOAT)));
}

#[test]
fn test_xsd_cast_decimal_from_string() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:decimal("3.25") AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("3.25", XSD_DECIMAL)));
}

#[test]
fn test_xsd_cast_decimal_from_integer() {
    // See #228 for the TypedLiteral output shape.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:decimal(7) AS ?result) WHERE {}"#,
    );
    assert_eq!(result, Some(typed_literal("7", XSD_DECIMAL)));
}

/// `xsd:boolean(?o)` used directly as a FILTER condition (not just BIND).
#[test]
fn test_xsd_cast_boolean_in_filter_context() {
    let mut ds = Datastore::new(64);
    let s = ds.add_node_resource(RdfResource::Iri(IriReference(
        "http://example.org/s".to_string(),
    )));
    let p = ds.add_node_resource(RdfResource::Iri(IriReference(
        "http://example.org/flag".to_string(),
    )));
    let o = ds.add_literal_resource(RdfLiteral::TypedLiteral {
        type_iri: IriReference(XSD_INTEGER.to_string()),
        literal: "1".to_string(),
    });
    ds.add_triple(Triple {
        subject: s,
        predicate: p,
        obj: o,
    });

    let sparql = r#"
        PREFIX ex: <http://example.org/>
        PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
        SELECT ?s WHERE { ?s ex:flag ?o . FILTER(xsd:boolean(?o)) }
    "#;
    let (_, query) = parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
    let result = match execute(&query, &ds, NetworkPolicy::Deny).expect("execute should succeed") {
        QueryResult::Select(r) => r,
        _ => panic!("expected SELECT"),
    };
    assert_eq!(
        result.rows.len(),
        1,
        "row with nonzero flag should pass FILTER(xsd:boolean(?o))"
    );
}

/// The DBLP benchmark shape (#190/#35): `SUM(xsd:integer(STRSTARTS(?o, "a")))`
/// must now produce a real, non-zero aggregate reflecting actual per-row cast
/// results, not just "executes without erroring" (which #186 already covered).
#[test]
fn test_sum_xsd_integer_strstarts_benchmark_shape() {
    let mut ds = Datastore::new(64);
    let p = ds.add_node_resource(RdfResource::Iri(IriReference(
        "http://www.w3.org/2000/01/rdf-schema#label".to_string(),
    )));
    let labels = ["apple", "banana", "avocado", "cherry"];
    for (i, label) in labels.iter().enumerate() {
        let s = ds.add_node_resource(RdfResource::Iri(IriReference(format!(
            "http://example.org/s{i}"
        ))));
        let o = ds.add_literal_resource(RdfLiteral::LiteralString(label.to_string()));
        ds.add_triple(Triple {
            subject: s,
            predicate: p,
            obj: o,
        });
    }

    let sparql = r#"
        PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
        PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
        SELECT (SUM(xsd:integer(STRSTARTS(?o, "a"))) AS ?count) { ?s rdfs:label ?o . }
    "#;
    let (_, query) = parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
    let result = match execute(&query, &ds, NetworkPolicy::Deny).expect("execute should succeed") {
        QueryResult::Select(r) => r,
        _ => panic!("expected SELECT"),
    };
    let count = result
        .rows
        .first()
        .and_then(|row| row.get("count").cloned());
    // "apple" and "avocado" start with "a" → sum should be 2, not 0.
    //
    // Each `xsd:integer(...)` cast now produces `TypedLiteral{xsd:integer,
    // ..}` rather than the native `IntegerLiteral` variant (#228), so this
    // also exercises `sum_values` recognizing `TypedLiteral` integers via
    // `classify_numeric` instead of silently falling back to an `xsd:double`
    // sum.
    assert_eq!(count, Some(typed_literal("2", XSD_INTEGER)));
}

// ── xsd:dateTime cast (#194) ─────────────────────────────────────────────
//
// SPARQL 1.1 §17.4.2 datatype constructor/cast semantics: `xsd:dateTime(v)`,
// deferred from #190 because it needs more involved lexical parsing (full
// ISO 8601 dateTime lexical space, plus normalizing `xsd:date` to midnight
// UTC) than the other cast targets. Reuses the lexical parsing that backs
// `parse_xsd_datetime` (the helper `YEAR`/`MONTH`/`DAY`/etc. already use),
// factored so the cast gets the strict dateTime/date lexical space (no
// bare-`xsd:gYear` fallback — that fallback stays specific to the
// YEAR/MONTH/DAY helpers, since a bare year is not a valid cast source per
// the XPath casting rules).
// See https://github.com/daghovland/rdf-datalog/issues/194

// #228: `xsd:dateTime(...)` now emits `TypedLiteral{xsd:dateTime,
// dt.to_rfc3339()}` rather than the native `DateTimeLiteral` variant — the
// Turtle parser always produces `TypedLiteral` for `xsd:dateTime` data (see
// `turtle::convert_literal`), so a native cast result could never
// structurally join against already-interned `xsd:dateTime` data. Note
// `chrono`'s `to_rfc3339()` always normalizes the UTC offset to `+00:00`
// (never `Z`), including for `Z`-suffixed input.

#[test]
fn test_xsd_cast_datetime_from_valid_lexical_form() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:dateTime("2023-01-15T10:30:45Z") AS ?result) WHERE {}"#,
    );
    let expected = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        result,
        Some(typed_literal(&expected.to_rfc3339(), XSD_DATE_TIME))
    );
}

#[test]
fn test_xsd_cast_datetime_from_lexical_form_without_timezone() {
    // `2004-04-12T13:20:00` (no timezone) is a valid xsd:dateTime lexical
    // form; `chrono::DateTime::parse_from_rfc3339` alone rejects it (RFC 3339
    // requires an offset), so the shared parser must fall back to a naive
    // datetime parse and assume UTC.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:dateTime("2004-04-12T13:20:00") AS ?result) WHERE {}"#,
    );
    let expected = chrono::NaiveDate::from_ymd_opt(2004, 4, 12)
        .unwrap()
        .and_hms_opt(13, 20, 0)
        .unwrap()
        .and_utc();
    assert_eq!(
        result,
        Some(typed_literal(&expected.to_rfc3339(), XSD_DATE_TIME))
    );
}

#[test]
fn test_xsd_cast_datetime_from_date_normalizes_to_midnight_utc() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:dateTime("2023-01-15") AS ?result) WHERE {}"#,
    );
    let expected = chrono::NaiveDate::from_ymd_opt(2023, 1, 15)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    assert_eq!(
        result,
        Some(typed_literal(&expected.to_rfc3339(), XSD_DATE_TIME))
    );
}

#[test]
fn test_xsd_cast_datetime_from_typed_datetime_literal() {
    let result = eval_function(&format!(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:dateTime("2023-01-15T10:30:45Z"^^<{XSD_DATE_TIME}>) AS ?result) WHERE {{}}"#
    ));
    let expected = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        result,
        Some(typed_literal(&expected.to_rfc3339(), XSD_DATE_TIME))
    );
}

#[test]
fn test_xsd_cast_datetime_from_datetime_literal_is_identity() {
    // `xsd:dateTime(NOW())` casts an already-native `DateTimeLiteral` input.
    // Per #228 this is no longer a true identity: the *value* passes through
    // unchanged, but the result is always re-emitted as `TypedLiteral{
    // xsd:dateTime, ..}` (not the native variant) so a cast result can join
    // against real interned `xsd:dateTime` data.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:dateTime(NOW()) AS ?result) WHERE {}"#,
    );
    assert!(
        matches!(
            &result,
            Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, .. }))
                if type_iri.0 == XSD_DATE_TIME
        ),
        "xsd:dateTime(NOW()) must be TypedLiteral{{xsd:dateTime, ..}}; got {result:?}"
    );
}

#[test]
fn test_xsd_cast_datetime_invalid_lexical_form_is_unbound() {
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:dateTime("not a date") AS ?result) WHERE {}"#,
    );
    assert_eq!(
        result, None,
        "invalid xsd:dateTime cast should leave ?result unbound"
    );
}

#[test]
fn test_xsd_cast_datetime_bare_year_is_unbound() {
    // A bare `xsd:gYear`-shaped string ("2020") is not a valid xsd:dateTime
    // or xsd:date lexical form, so it must NOT cast successfully — even
    // though `parse_xsd_datetime` (used by YEAR/MONTH/DAY) accepts bare
    // years as a gYear fallback. That fallback is intentionally NOT shared
    // with the cast, which needs the strict dateTime/date lexical space.
    let result = eval_function(
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> SELECT (xsd:dateTime("2020") AS ?result) WHERE {}"#,
    );
    assert_eq!(
        result, None,
        "a bare gYear-shaped string is not a valid xsd:dateTime cast source"
    );
}

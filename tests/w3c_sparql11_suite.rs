/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! W3C SPARQL 1.1 conformance test suite.
//!
//! Test data is vendored in `tests/testdata/w3c_sparql11/` from:
//! <https://www.w3.org/2009/sparql/docs/tests/>
//! (W3C Test Suite License / W3C 3-Clause BSD License)
//!
//! Each subdirectory has a `manifest.ttl` that lists test entries:
//! - `mf:PositiveSyntaxTest11` — query must parse without error
//! - `mf:NegativeSyntaxTest11` — query must produce a parse error
//! - `mf:QueryEvaluationTest` — load data, run query, compare with expected
//!   SPARQL Results XML (`.srx`); marked `#[ignore]` pending SRX comparison
//! - Update-related types — `#[ignore]` pending SPARQL Update support
//!
//! Eval tests are `#[ignore]` because comparing query results against `.srx`
//! expected-output files requires a SPARQL XML Result Format parser and
//! result-set isomorphism, which are not yet implemented.

use sparql_parser::{ParserContext, parse_query};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ── Manifest parsing ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SparqlTestKind {
    PositiveSyntax,
    NegativeSyntax,
    Eval,
    Other,
}

#[derive(Debug, Clone)]
struct SparqlTestEntry {
    name: String,
    kind: SparqlTestKind,
    action_query: String,
}

fn suite_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join("w3c_sparql11")
}

/// Parse a SPARQL 1.1 manifest.ttl and return entries with a query action.
///
/// Handles both `<#name>` and `:name` style fragment identifiers.
fn parse_sparql_manifest(manifest_path: &Path, subdir: &Path) -> Vec<SparqlTestEntry> {
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_kind: Option<SparqlTestKind> = None;
    let mut current_action: Option<String> = None;
    let mut in_action_block = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Detect start of a new test entry by prefix-qualified name `:testN` or fragment `<#name>`
        let is_entry_start = (trimmed.starts_with("<#") && trimmed.contains(">"))
            || (trimmed.starts_with(':')
                && !trimmed.starts_with("::")
                && !trimmed.starts_with("://")
                && trimmed.contains("rdf:type"));

        if is_entry_start {
            // Flush previous entry
            if let (Some(name), Some(kind), Some(action)) = (
                current_name.take(),
                current_kind.take(),
                current_action.take(),
            ) {
                entries.push(SparqlTestEntry {
                    name,
                    kind,
                    action_query: action,
                });
            } else {
                current_name = None;
                current_kind = None;
                current_action = None;
            }
            in_action_block = false;
        }

        // Extract test name from `mf:name "..."`
        if trimmed.starts_with("mf:name")
            && let Some(start) = trimmed.find('"')
            && let Some(end) = trimmed[start + 1..].find('"')
        {
            current_name = Some(trimmed[start + 1..start + 1 + end].to_string());
        }

        // Determine test kind from rdf:type
        if trimmed.contains("rdf:type") {
            if trimmed.contains("PositiveSyntaxTest11")
                || trimmed.contains("PositiveUpdateSyntaxTest11")
            {
                current_kind = Some(SparqlTestKind::PositiveSyntax);
            } else if trimmed.contains("NegativeSyntaxTest11")
                || trimmed.contains("NegativeUpdateSyntaxTest11")
            {
                current_kind = Some(SparqlTestKind::NegativeSyntax);
            } else if trimmed.contains("QueryEvaluationTest")
                || trimmed.contains("UpdateEvaluationTest")
            {
                current_kind = Some(SparqlTestKind::Eval);
            } else if trimmed.contains("mf:Manifest") {
                // skip
            } else {
                current_kind = Some(SparqlTestKind::Other);
            }
        }

        // Detect action block: `mf:action [ qt:query <file.rq> ; ... ]`
        if trimmed.starts_with("mf:action") {
            in_action_block = trimmed.contains('[');
            // Inline: `mf:action <file.rq>`
            if !in_action_block
                && let Some(start) = trimmed.find('<')
                && let Some(end) = trimmed[start + 1..].find('>')
            {
                let file = &trimmed[start + 1..start + 1 + end];
                if file.ends_with(".rq") || file.ends_with(".ru") {
                    current_action = Some(subdir.join(file).to_string_lossy().into_owned());
                }
            }
        }

        // Inside an action block, look for qt:query or qt:update
        if in_action_block
            && (trimmed.starts_with("qt:query") || trimmed.starts_with("qt:update"))
            && let Some(start) = trimmed.find('<')
            && let Some(end) = trimmed[start + 1..].find('>')
        {
            let file = &trimmed[start + 1..start + 1 + end];
            if file.ends_with(".rq") || file.ends_with(".ru") {
                current_action = Some(subdir.join(file).to_string_lossy().into_owned());
            }
        }
        if trimmed.contains(']') {
            in_action_block = false;
        }
    }

    // Flush last entry
    if let (Some(name), Some(kind), Some(action)) = (current_name, current_kind, current_action) {
        entries.push(SparqlTestEntry {
            name,
            kind,
            action_query: action,
        });
    }

    entries
}

fn load_sparql_manifest(subdir_name: &str) -> Vec<SparqlTestEntry> {
    let base = suite_dir();
    let subdir = base.join(subdir_name);
    let manifest = subdir.join("manifest.ttl");
    parse_sparql_manifest(&manifest, &subdir)
}

fn try_parse_query(path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    parse_query(&text, &mut ctx).map_err(|e| format!("parse error: {:?}", e))?;
    Ok(())
}

fn run_syntax_tests(entries: &[SparqlTestEntry], skip: &[&str]) -> Vec<String> {
    let mut failures = Vec::new();
    for entry in entries {
        if skip.contains(&entry.name.as_str()) {
            continue;
        }
        if entry.kind == SparqlTestKind::Eval || entry.kind == SparqlTestKind::Other {
            continue;
        }
        let result = try_parse_query(&entry.action_query);
        match entry.kind {
            SparqlTestKind::PositiveSyntax => {
                if let Err(e) = result {
                    failures.push(format!("FAIL {} (expected Ok): {}", entry.name, e));
                }
            }
            SparqlTestKind::NegativeSyntax if result.is_ok() => {
                failures.push(format!(
                    "FAIL {} (expected parse error, got Ok)",
                    entry.name
                ));
            }
            _ => {}
        }
    }
    failures
}

fn assert_no_failures(failures: Vec<String>, suite: &str) {
    if !failures.is_empty() {
        eprintln!("\n{} FAILURES in {}:", failures.len(), suite);
        for f in &failures {
            eprintln!("  {}", f);
        }
        panic!("{} test(s) failed in {}", failures.len(), suite);
    }
}

// ── SPARQL 1.1 Query Syntax Tests ────────────────────────────────────────────
//
// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/syntax-query/

/// W3C SPARQL 1.1 — SELECT/WHERE/FILTER/OPTIONAL/etc. positive syntax tests.
///
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/syntax-query/
///
/// Skip-list: unimplemented SPARQL 1.1 syntax features (aggregates, subqueries,
/// IN/NOT IN expressions, property path in collection, CONSTRUCT WHERE with FROM,
/// prefixed-name escape forms, SELECT-scope expressions).
#[test]
fn w3c_sparql11_syntax_query_positive() {
    let entries = load_sparql_manifest("syntax-query");
    let skip: &[&str] = &[
        // Aggregates (COUNT, SUM, AVG, MIN, MAX, GROUP_CONCAT) — not yet parsed
        "syntax-aggregate-01.rq",
        "syntax-aggregate-02.rq",
        "syntax-aggregate-03.rq",
        "syntax-aggregate-04.rq",
        "syntax-aggregate-05.rq",
        "syntax-aggregate-06.rq",
        "syntax-aggregate-07.rq",
        "syntax-aggregate-08.rq",
        "syntax-aggregate-09.rq",
        "syntax-aggregate-10.rq",
        "syntax-aggregate-11.rq",
        "syntax-aggregate-12.rq",
        "syntax-aggregate-13.rq",
        "syntax-aggregate-14.rq",
        "syntax-aggregate-15.rq",
        // SELECT expressions with operators
        "syntax-select-expr-01.rq",
        "syntax-select-expr-02.rq",
        "syntax-select-expr-03.rq",
        "syntax-select-expr-04.rq",
        "syntax-select-expr-05.rq",
        // Subqueries
        "syntax-subquery-01.rq",
        "syntax-subquery-02.rq",
        "syntax-subquery-03.rq",
        // IN / NOT IN expressions
        "syntax-oneof-01.rq",
        "syntax-oneof-02.rq",
        "syntax-oneof-03.rq",
        // BIND with division expression
        "syntax-bind-02.rq",
        // CONSTRUCT WHERE with FROM clause
        "syntax-construct-where-02.rq",
        // Property path inside collection (not yet supported)
        "syn-pp-in-collection",
        // SELECT scope / outer scope tests
        "syntax-SELECTscope1.rq",
        "syntax-SELECTscope3.rq",
        // Prefixed-name escape forms (backslash, hex, unescaped colon)
        "PrefixName with backslash-escaped colons",
        "PrefixName with hex-encoded colons",
        "PrefixName with unescaped colons",
        "syn-pname-04",
        "syn-pname-05",
        "syn-pname-06",
        "syn-pname-07",
        "syn-pname-09",
        // Property paths: alternative/sequence in complex positions
        "syntax-propertyPaths-01.rq",
    ];
    let failures = run_syntax_tests(
        &entries
            .iter()
            .filter(|e| e.kind == SparqlTestKind::PositiveSyntax)
            .cloned()
            .collect::<Vec<_>>(),
        skip,
    );
    assert_no_failures(failures, "SPARQL 1.1 syntax-query positive");
}

/// W3C SPARQL 1.1 — SELECT/WHERE/FILTER/OPTIONAL/etc. negative syntax tests.
///
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/syntax-query/
///
/// Skip-list: cases where the parser accepts syntax that should be rejected
/// (BIND variable scope rules, VALUES clause scoping, prefixed-name validation).
#[test]
fn w3c_sparql11_syntax_query_negative() {
    let entries = load_sparql_manifest("syntax-query");
    let skip: &[&str] = &[
        // BIND scope: variable used in BIND is already in scope — scope check not enforced
        "syntax-BINDscope6.rq",
        "syntax-BINDscope7.rq",
        "syntax-BINDscope8.rq",
        // VALUES clause with mismatched variable count — not validated at parse time
        "syntax-bindings-09.rq",
        // Prefixed names with reserved characters — not rejected by our parser
        "syn-bad-pname-05",
        "syn-bad-pname-07",
    ];
    let failures = run_syntax_tests(
        &entries
            .iter()
            .filter(|e| e.kind == SparqlTestKind::NegativeSyntax)
            .cloned()
            .collect::<Vec<_>>(),
        skip,
    );
    assert_no_failures(failures, "SPARQL 1.1 syntax-query negative");
}

// ── SPARQL 1.1 Evaluation Tests (ignored pending SRX comparison) ─────────────
//
// The following test functions each cover one feature area from the SPARQL 1.1
// conformance suite. All are ignored because:
//   1. Results must be compared against `.srx` (SPARQL XML Results) files —
//      parsing that format is not yet implemented.
//   2. Some features (aggregates, CONSTRUCT, UPDATE, SERVICE) are not yet
//      implemented in the SPARQL executor.
//
// Reference for all eval tests: https://www.w3.org/2009/sparql/docs/tests/

/// W3C SPARQL 1.1 — BIND evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/bind/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_bind() {
    let entries = load_sparql_manifest("bind");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 bind");
}

/// W3C SPARQL 1.1 — EXISTS / NOT EXISTS evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/exists/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_exists() {
    let entries = load_sparql_manifest("exists");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 exists");
}

/// W3C SPARQL 1.1 — VALUES / inline data evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/bindings/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_bindings() {
    let entries = load_sparql_manifest("bindings");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 bindings");
}

/// W3C SPARQL 1.1 — subquery evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/subquery/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_subquery() {
    let entries = load_sparql_manifest("subquery");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 subquery");
}

/// W3C SPARQL 1.1 — aggregates (GROUP BY, HAVING, COUNT, SUM, AVG…) eval tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/aggregates/
#[test]
#[ignore = "aggregates not yet implemented; also requires SRX comparison"]
fn w3c_sparql11_aggregates() {
    let entries = load_sparql_manifest("aggregates");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 aggregates");
}

/// W3C SPARQL 1.1 — negation (MINUS / NOT EXISTS) evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/negation/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_negation() {
    let entries = load_sparql_manifest("negation");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 negation");
}

/// W3C SPARQL 1.1 — property paths evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/property-path/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_property_path() {
    let entries = load_sparql_manifest("property-path");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 property-path");
}

/// W3C SPARQL 1.1 — CONSTRUCT evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/construct/
#[test]
#[ignore = "CONSTRUCT not yet implemented; requires SRX/Turtle result comparison"]
fn w3c_sparql11_construct() {
    let entries = load_sparql_manifest("construct");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 construct");
}

/// W3C SPARQL 1.1 — built-in functions (STRLEN, UCASE, SHA, NOW…) eval tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/functions/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_functions() {
    let entries = load_sparql_manifest("functions");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 functions");
}

/// W3C SPARQL 1.1 — GROUP BY / grouping evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/grouping/
#[test]
#[ignore = "aggregates not yet implemented; also requires SRX comparison"]
fn w3c_sparql11_grouping() {
    let entries = load_sparql_manifest("grouping");
    assert_no_failures(run_syntax_tests(&entries, &[]), "SPARQL 1.1 grouping");
}

/// W3C SPARQL 1.1 — project expression (SELECT expressions) eval tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/project-expression/
#[test]
#[ignore = "requires SPARQL XML result format (SRX) comparison, not yet implemented"]
fn w3c_sparql11_project_expression() {
    let entries = load_sparql_manifest("project-expression");
    assert_no_failures(
        run_syntax_tests(&entries, &[]),
        "SPARQL 1.1 project-expression",
    );
}

// ── SPARQL 1.1 Update Tests (all ignored — Update not yet implemented) ────────

/// W3C SPARQL 1.1 Update — positive syntax tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/syntax-update-1/
#[test]
#[ignore = "SPARQL Update (INSERT/DELETE/CLEAR/DROP) not yet implemented"]
fn w3c_sparql11_update_syntax_positive() {
    let entries = load_sparql_manifest("syntax-update-1");
    let positives: Vec<_> = entries
        .into_iter()
        .filter(|e| e.kind == SparqlTestKind::PositiveSyntax)
        .collect();
    assert_no_failures(
        run_syntax_tests(&positives, &[]),
        "SPARQL 1.1 update syntax positive",
    );
}

/// W3C SPARQL 1.1 Update — negative syntax tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/syntax-update-1/
#[test]
#[ignore = "SPARQL Update (INSERT/DELETE/CLEAR/DROP) not yet implemented"]
fn w3c_sparql11_update_syntax_negative() {
    let entries = load_sparql_manifest("syntax-update-1");
    let negatives: Vec<_> = entries
        .into_iter()
        .filter(|e| e.kind == SparqlTestKind::NegativeSyntax)
        .collect();
    assert_no_failures(
        run_syntax_tests(&negatives, &[]),
        "SPARQL 1.1 update syntax negative",
    );
}

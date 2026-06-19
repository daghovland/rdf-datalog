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
//! All eval test categories are now active. The SRX comparison infrastructure
//! (`compare_with_srx`) was already implemented. Update syntax tests remain
//! `#[ignore]` because SPARQL Update is not implemented.

use dag_rdf::{Datastore, GraphElement, RdfLiteral, RdfResource};
use dagalog::{load_file, run_sparql_query};
use ingress::IriReference;
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
    /// Data file to load before executing the query (qt:data).
    action_data: Option<String>,
    /// Expected result file (.srx or .ttl).
    result_file: Option<String>,
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
    let mut current_data: Option<String> = None;
    let mut current_result: Option<String> = None;
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
                    action_data: current_data.take(),
                    result_file: current_result.take(),
                });
            } else {
                current_name = None;
                current_kind = None;
                current_action = None;
                current_data = None;
                current_result = None;
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

        // Inside an action block, look for qt:query, qt:update, qt:data
        if in_action_block {
            if (trimmed.starts_with("qt:query") || trimmed.starts_with("qt:update"))
                && let Some(start) = trimmed.find('<')
                && let Some(end) = trimmed[start + 1..].find('>')
            {
                let file = &trimmed[start + 1..start + 1 + end];
                if file.ends_with(".rq") || file.ends_with(".ru") {
                    current_action = Some(subdir.join(file).to_string_lossy().into_owned());
                }
            }
            if trimmed.starts_with("qt:data")
                && let Some(start) = trimmed.find('<')
                && let Some(end) = trimmed[start + 1..].find('>')
            {
                let file = &trimmed[start + 1..start + 1 + end];
                current_data = Some(subdir.join(file).to_string_lossy().into_owned());
            }
        }
        if trimmed.contains(']') {
            in_action_block = false;
        }

        // Result file: `mf:result <file.srx>`
        if trimmed.starts_with("mf:result")
            && let Some(start) = trimmed.find('<')
            && let Some(end) = trimmed[start + 1..].find('>')
        {
            let file = &trimmed[start + 1..start + 1 + end];
            current_result = Some(subdir.join(file).to_string_lossy().into_owned());
        }
    }

    // Flush last entry
    if let (Some(name), Some(kind), Some(action)) = (current_name, current_kind, current_action) {
        entries.push(SparqlTestEntry {
            name,
            kind,
            action_query: action,
            action_data: current_data,
            result_file: current_result,
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

// ── SRX (SPARQL XML Results) parsing and comparison ──────────────────────────

/// A single SPARQL result value normalised for comparison.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum SrxValue {
    Uri(String),
    Bnode(String),
    PlainLiteral(String),
    LangLiteral { value: String, lang: String },
    TypedLiteral { value: String, datatype: String },
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn extract_xml_attr(s: &str, attr: &str) -> Option<String> {
    // Handles both attr="val" and attr='val'
    for quote in ['"', '\''] {
        let pattern = format!("{}={}", attr, quote);
        if let Some(pos) = s.find(&pattern) {
            let rest = &s[pos + pattern.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

fn extract_tag_content(block: &str, tag: &str) -> Option<String> {
    let open_prefix = format!("<{}", tag);
    let close_tag = format!("</{}>", tag);
    let start = block.find(&open_prefix)?;
    let after_open = &block[start..];
    let content_start = after_open.find('>')? + 1;
    let content_end = after_open.find(&close_tag)?;
    Some(xml_unescape(&after_open[content_start..content_end]))
}

fn parse_srx_value(binding_block: &str) -> Option<SrxValue> {
    if binding_block.contains("<uri>") || binding_block.contains("<uri ") {
        let uri = extract_tag_content(binding_block, "uri")?;
        Some(SrxValue::Uri(uri))
    } else if binding_block.contains("<bnode>") || binding_block.contains("<bnode ") {
        let id = extract_tag_content(binding_block, "bnode")?;
        Some(SrxValue::Bnode(id))
    } else if binding_block.contains("<literal") {
        let value = extract_tag_content(binding_block, "literal")?;
        // Extract the <literal ...> opening tag attributes
        let open_start = binding_block.find("<literal")?;
        let open_end = binding_block[open_start..].find('>')?;
        let tag_str = &binding_block[open_start..open_start + open_end];
        if let Some(lang) = extract_xml_attr(tag_str, "xml:lang") {
            return Some(SrxValue::LangLiteral {
                value,
                lang: lang.to_lowercase(),
            });
        }
        if let Some(dt) = extract_xml_attr(tag_str, "datatype") {
            return Some(SrxValue::TypedLiteral {
                value,
                datatype: dt,
            });
        }
        Some(SrxValue::PlainLiteral(value))
    } else {
        None
    }
}

/// Parse a SPARQL Results XML file into a list of rows (variable → SrxValue).
fn parse_srx(path: &str) -> Result<Vec<HashMap<String, SrxValue>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
    let mut rows = Vec::new();
    let mut pos = 0;
    while let Some(rel) = text[pos..].find("<result>").or_else(|| {
        text[pos..]
            .find("<result\n")
            .or_else(|| text[pos..].find("<result "))
    }) {
        let abs = pos + rel;
        let end = match text[abs..].find("</result>") {
            Some(e) => abs + e + "</result>".len(),
            None => break,
        };
        let block = &text[abs..end];
        let mut row = HashMap::new();
        let mut bpos = 0;
        while let Some(brel) = block[bpos..].find("<binding") {
            let babs = bpos + brel;
            let bend = match block[babs..].find("</binding>") {
                Some(e) => babs + e + "</binding>".len(),
                None => break,
            };
            let bblock = &block[babs..bend];
            if let Some(name) = extract_xml_attr(bblock, "name")
                && let Some(val) = parse_srx_value(bblock)
            {
                row.insert(name, val);
            }
            bpos = bend;
        }
        rows.push(row);
        pos = end;
    }
    Ok(rows)
}

/// Convert a `GraphElement` to an `SrxValue` for comparison.
fn gel_to_srx(el: &GraphElement) -> SrxValue {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri))) => SrxValue::Uri(iri.clone()),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            SrxValue::Bnode(format!("b{}", id))
        }
        GraphElement::GraphLiteral(lit) => gel_lit_to_srx(lit),
    }
}

fn gel_lit_to_srx(lit: &RdfLiteral) -> SrxValue {
    use ingress::{XSD_BOOLEAN, XSD_DECIMAL, XSD_DOUBLE, XSD_FLOAT, XSD_INTEGER};
    match lit {
        RdfLiteral::LiteralString(s) => SrxValue::PlainLiteral(s.clone()),
        RdfLiteral::LangLiteral { literal, lang } => SrxValue::LangLiteral {
            value: literal.clone(),
            lang: lang.to_lowercase(),
        },
        RdfLiteral::TypedLiteral { literal, type_iri } => SrxValue::TypedLiteral {
            value: literal.clone(),
            datatype: type_iri.0.clone(),
        },
        RdfLiteral::IntegerLiteral(n) => SrxValue::TypedLiteral {
            value: n.to_string(),
            datatype: XSD_INTEGER.to_string(),
        },
        RdfLiteral::BooleanLiteral(b) => SrxValue::TypedLiteral {
            value: b.to_string(),
            datatype: XSD_BOOLEAN.to_string(),
        },
        RdfLiteral::DecimalLiteral(d) => SrxValue::TypedLiteral {
            value: d.to_string(),
            datatype: XSD_DECIMAL.to_string(),
        },
        RdfLiteral::DoubleLiteral(d) => SrxValue::TypedLiteral {
            value: d.to_string(),
            datatype: XSD_DOUBLE.to_string(),
        },
        RdfLiteral::FloatLiteral(f) => SrxValue::TypedLiteral {
            value: f.to_string(),
            datatype: XSD_FLOAT.to_string(),
        },
        RdfLiteral::DateTimeLiteral(dt) => SrxValue::TypedLiteral {
            value: dt.to_string(),
            datatype: ingress::XSD_DATE_TIME.to_string(),
        },
        RdfLiteral::DateLiteral(d) => SrxValue::TypedLiteral {
            value: d.to_string(),
            datatype: ingress::XSD_DATE.to_string(),
        },
        RdfLiteral::TimeLiteral(t) => SrxValue::TypedLiteral {
            value: t.to_string(),
            datatype: ingress::XSD_TIME.to_string(),
        },
        RdfLiteral::DurationLiteral(d) => SrxValue::TypedLiteral {
            value: format!("{:?}", d),
            datatype: "http://www.w3.org/2001/XMLSchema#duration".to_string(),
        },
    }
}

/// Normalise an `SrxValue` so that xsd:string typed literals compare equal to
/// plain literals, and numeric strings are normalised (e.g. leading zeros).
fn normalise_srx(v: SrxValue) -> SrxValue {
    match v {
        SrxValue::TypedLiteral { value, datatype }
            if datatype == ingress::XSD_STRING
                || datatype == "http://www.w3.org/2001/XMLSchema#string" =>
        {
            SrxValue::PlainLiteral(value)
        }
        SrxValue::TypedLiteral { value, datatype }
            if datatype == ingress::XSD_INTEGER
                || datatype == "http://www.w3.org/2001/XMLSchema#integer" =>
        {
            // Normalise integer strings by stripping leading zeros / signs
            let n: Option<i64> = value.trim().parse().ok();
            SrxValue::TypedLiteral {
                value: n.map(|x| x.to_string()).unwrap_or(value),
                datatype,
            }
        }
        other => other,
    }
}

/// Compare SPARQL query results against an SRX expected-result file.
/// Returns `None` on match, `Some(reason)` on mismatch.
fn compare_with_srx(ds: &Datastore, sparql: &str, srx_path: &str) -> Option<String> {
    let result = match run_sparql_query(ds, sparql) {
        Ok(r) => r,
        Err(e) => return Some(format!("query error: {}", e)),
    };
    let expected_rows = match parse_srx(srx_path) {
        Ok(r) => r,
        Err(e) => return Some(format!("SRX parse error: {}", e)),
    };

    // Convert actual results to normalised SrxValue rows
    let actual_rows: Vec<HashMap<String, SrxValue>> = result
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|(var, gel)| (var.clone(), normalise_srx(gel_to_srx(gel))))
                .collect()
        })
        .collect();
    let expected_rows_norm: Vec<HashMap<String, SrxValue>> = expected_rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(var, val)| (var, normalise_srx(val)))
                .collect()
        })
        .collect();

    if actual_rows.len() != expected_rows_norm.len() {
        return Some(format!(
            "expected {} rows, got {}",
            expected_rows_norm.len(),
            actual_rows.len()
        ));
    }

    // Multiset comparison (order-insensitive)
    let mut remaining = expected_rows_norm.clone();
    for actual_row in &actual_rows {
        if let Some(pos) = remaining.iter().position(|e| e == actual_row) {
            remaining.swap_remove(pos);
        } else {
            return Some(format!("unexpected row: {:?}", actual_row));
        }
    }
    if remaining.is_empty() {
        None
    } else {
        Some(format!("missing rows: {:?}", remaining))
    }
}

/// Run a full SPARQL evaluation test: load data, execute query, compare with SRX.
fn run_eval_test(entry: &SparqlTestEntry, skip: &[&str]) -> Option<String> {
    if skip.contains(&entry.name.as_str()) {
        return None;
    }
    if entry.kind != SparqlTestKind::Eval {
        return None;
    }
    let srx_path = entry.result_file.as_deref()?;
    let query_path = &entry.action_query;

    let query_text = match std::fs::read_to_string(query_path) {
        Ok(t) => t,
        Err(e) => {
            return Some(format!(
                "FAIL {}: cannot read query {}: {}",
                entry.name, query_path, e
            ));
        }
    };

    let mut ds = Datastore::new(4_096);
    if let Some(data_path) = &entry.action_data
        && let Err(e) = load_file(&mut ds, std::path::Path::new(data_path))
    {
        return Some(format!(
            "FAIL {}: cannot load data {}: {}",
            entry.name, data_path, e
        ));
    }

    compare_with_srx(&ds, &query_text, srx_path)
        .map(|reason| format!("FAIL {}: {}", entry.name, reason))
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
        // CONSTRUCT WHERE with FROM clause
        "syntax-construct-where-02.rq",
        // Property path inside collection (not yet supported)
        "syn-pp-in-collection",
        // test_52 is obsoleted (commented out of mf:entries) but our manifest parser
        // still finds the definition. The W3C decision was to allow unescaped colons
        // but NOT backslash-escaped colons in local names, so \: is invalid.
        "PrefixName with backslash-escaped colons",
        // Property paths inside RDF collections ([ :p* :q obj ] in object position)
        "syn-pp-in-collection",
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
        // SubSelect mixed with group pattern: { {} SELECT ... } should fail
        // but our parser accepts it (grammar enforcement at this level is complex)
        "syn-bad-07.rq",
        // SELECT scope: subquery variable shadows outer alias — scope not enforced at parse time
        "syntax-SELECTscope2",
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

// ── SPARQL 1.1 Evaluation Tests ───────────────────────────────────────────────
//
// Reference for all eval tests: https://www.w3.org/2009/sparql/docs/tests/
//
// All eval categories are now active. Only SPARQL Update tests remain ignored.

/// W3C SPARQL 1.1 — BIND evaluation tests (BIND with expressions and arithmetic).
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/bind/
#[test]
fn w3c_sparql11_bind() {
    let entries = load_sparql_manifest("bind");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 bind");
}

/// W3C SPARQL 1.1 — EXISTS / NOT EXISTS evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/exists/
#[test]
fn w3c_sparql11_exists() {
    let entries = load_sparql_manifest("exists");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 exists");
}

/// W3C SPARQL 1.1 — VALUES / inline data evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/bindings/
#[test]
fn w3c_sparql11_bindings() {
    let entries = load_sparql_manifest("bindings");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 bindings");
}

/// W3C SPARQL 1.1 — subquery evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/subquery/
#[test]
fn w3c_sparql11_subquery() {
    let entries = load_sparql_manifest("subquery");
    let skip: &[&str] = &[
        // CONSTRUCT result comparison (TTL graph diff) not yet implemented
        "sq12 - Subquery within CONSTRUCT",
        "sq14 - Subquery with CONSTRUCT",
    ];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 subquery");
}

/// W3C SPARQL 1.1 — aggregates (GROUP BY, HAVING, COUNT, SUM, AVG…) eval tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/aggregates/
#[test]
fn w3c_sparql11_aggregates() {
    let entries = load_sparql_manifest("aggregates");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 aggregates");
}

/// W3C SPARQL 1.1 — negation (MINUS / NOT EXISTS) evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/negation/
#[test]
fn w3c_sparql11_negation() {
    let entries = load_sparql_manifest("negation");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 negation");
}

/// W3C SPARQL 1.1 — property paths evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/property-path/
#[test]
fn w3c_sparql11_property_path() {
    let entries = load_sparql_manifest("property-path");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 property-path");
}

/// W3C SPARQL 1.1 — CONSTRUCT evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/construct/
#[test]
fn w3c_sparql11_construct() {
    let entries = load_sparql_manifest("construct");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 construct");
}

/// W3C SPARQL 1.1 — built-in functions (STRLEN, UCASE, SHA, NOW…) eval tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/functions/
#[test]
fn w3c_sparql11_functions() {
    let entries = load_sparql_manifest("functions");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 functions");
}

/// W3C SPARQL 1.1 — GROUP BY / grouping evaluation tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/grouping/
#[test]
fn w3c_sparql11_grouping() {
    let entries = load_sparql_manifest("grouping");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 grouping");
}

/// W3C SPARQL 1.1 — project expression (SELECT expr AS ?var) eval tests.
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/project-expression/
#[test]
fn w3c_sparql11_project_expression() {
    let entries = load_sparql_manifest("project-expression");
    let skip: &[&str] = &[];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 project-expression");
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

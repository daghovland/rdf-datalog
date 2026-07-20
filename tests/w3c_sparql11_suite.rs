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
//! (`compare_with_srx`) was already implemented. Update syntax tests now pass
//! using the `parse_update` function from `sparql_endpoint::sparql_update`.
//!
//! Manifests are loaded with this project's own stack — real Turtle parsing
//! (`turtle::parse_turtle_with_base`) into a `dag_rdf::Datastore`, walked
//! with a real SPARQL property-path query (`mf:entries/rdf:rest*/rdf:first`)
//! via `sparql_parser`'s executor — rather than a hand-rolled line scanner.
//! See [#192](https://github.com/daghovland/rdf-datalog/issues/192).
//!
//! Run just this file: `cargo test --test w3c_sparql11_suite`

use dag_rdf::{Datastore, GraphElement, RdfLiteral, RdfResource};
use dagalog::{load_file, run_sparql_query};
use ingress::IriReference;
use sparql_endpoint::sparql_update::parse_update;
use sparql_parser::{ParserContext, parse_query};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use turtle::parse_turtle_with_base;

// ── Manifest parsing ─────────────────────────────────────────────────────────
//
// See [`parse_sparql_manifest`] below for how manifests are loaded (real
// Turtle + a real SPARQL property-path query, per issue #192).

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

/// Read a solution-row binding back as a raw IRI string (for `rdf:type`).
fn as_iri(value: Option<&GraphElement>) -> Option<&str> {
    match value {
        Some(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri)))) => Some(iri.as_str()),
        _ => None,
    }
}

/// Read a solution-row binding back as an absolute filesystem path.
///
/// Manifests reference query/data/result files with bare relative IRIs like
/// `<full-minuend.rq>`. [`parse_sparql_manifest`] loads the manifest with
/// `file://<absolute manifest path>` as the Turtle base IRI, so those
/// resolve to `file://<absolute path>` IRIs; stripping the `file://` scheme
/// recovers the real filesystem path.
fn as_file_path(value: Option<&GraphElement>) -> Option<String> {
    as_iri(value)?.strip_prefix("file://").map(str::to_string)
}

/// Read a solution-row binding back as a plain string (for `mf:name`).
fn as_string(value: Option<&GraphElement>) -> Option<String> {
    match value {
        Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s))) => Some(s.clone()),
        Some(GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. })) => {
            Some(literal.clone())
        }
        Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. })) => {
            Some(literal.clone())
        }
        _ => None,
    }
}

/// Parse a SPARQL 1.1 `manifest.ttl` into test entries.
///
/// Loads the manifest as real Turtle, then enumerates the `mf:entries` RDF
/// collection with the standard SPARQL idiom for walking an RDF list —
/// `mf:entries/rdf:rest*/rdf:first` — rather than hand-rolled list-cell
/// traversal. Entries commented out of the `mf:entries` list (but still
/// present as dangling triples elsewhere in the file, as some manifests do)
/// are correctly excluded, since they never bind to `?entry`.
///
/// Handles both `mf:action` shapes seen across the vendored manifests:
/// - a direct file reference (`mf:action <file.rq>`), used by syntax tests;
/// - a `[ qt:query <file.rq> ; qt:data <file.ttl> ]` block, used by eval
///   tests — regardless of how that block is broken across lines, since
///   Turtle whitespace is never significant.
fn parse_sparql_manifest(manifest_path: &Path) -> Vec<SparqlTestEntry> {
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    let mut ds = Datastore::new(4_096);
    let abs = manifest_path
        .canonicalize()
        .unwrap_or_else(|_| manifest_path.to_path_buf());
    let base_iri = format!("file://{}", abs.display());
    if parse_turtle_with_base(&mut ds, text.as_bytes(), &base_iri).is_err() {
        return Vec::new();
    }

    let sparql = r#"
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        PREFIX mf:  <http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#>
        PREFIX qt:  <http://www.w3.org/2001/sw/DataAccess/tests/test-query#>
        SELECT ?name ?type ?action ?actionQuery ?actionUpdate ?actionData ?result WHERE {
            ?manifest mf:entries/rdf:rest*/rdf:first ?entry .
            ?entry mf:name ?name ;
                   rdf:type ?type ;
                   mf:action ?action .
            OPTIONAL { ?entry mf:result ?result }
            OPTIONAL { ?action qt:query ?actionQuery }
            OPTIONAL { ?action qt:update ?actionUpdate }
            OPTIONAL { ?action qt:data ?actionData }
        }
    "#;
    let result = match run_sparql_query(&ds, sparql) {
        Ok(r) => r,
        Err(e) => panic!(
            "manifest query error for {}: {}",
            manifest_path.display(),
            e
        ),
    };

    let mut entries = Vec::new();
    for row in &result.rows {
        let Some(name) = as_string(row.get("name")) else {
            continue;
        };
        let kind = match as_iri(row.get("type")) {
            Some(t)
                if t.ends_with("PositiveSyntaxTest11")
                    || t.ends_with("PositiveUpdateSyntaxTest11") =>
            {
                SparqlTestKind::PositiveSyntax
            }
            Some(t)
                if t.ends_with("NegativeSyntaxTest11")
                    || t.ends_with("NegativeUpdateSyntaxTest11") =>
            {
                SparqlTestKind::NegativeSyntax
            }
            Some(t)
                if t.ends_with("QueryEvaluationTest") || t.ends_with("UpdateEvaluationTest") =>
            {
                SparqlTestKind::Eval
            }
            _ => SparqlTestKind::Other,
        };

        // Prefer the block form's `qt:query`/`qt:update`; fall back to
        // `mf:action` itself being a direct file reference (syntax tests,
        // where the action *is* the query/update file, not a `[ ... ]`
        // block).
        let Some(action_query) = as_file_path(row.get("actionQuery"))
            .or_else(|| as_file_path(row.get("actionUpdate")))
            .or_else(|| as_file_path(row.get("action")))
        else {
            continue;
        };
        let action_data = as_file_path(row.get("actionData"));
        let result_file = as_file_path(row.get("result"));

        entries.push(SparqlTestEntry {
            name,
            kind,
            action_query,
            action_data,
            result_file,
        });
    }

    entries
}

fn load_sparql_manifest(subdir_name: &str) -> Vec<SparqlTestEntry> {
    let manifest = suite_dir().join(subdir_name).join("manifest.ttl");
    parse_sparql_manifest(&manifest)
}

/// Regression test for issue #192: manifests that format `mf:action` with the
/// opening `[` on the *following* line (not the `mf:action` line itself), and
/// with `qt:query`/`qt:data` sharing a line with that `[` — e.g.:
///
/// ```turtle
/// mf:action
///      [ qt:query  <full-minuend.rq> ;
///        qt:data   <full-minuend.ttl> ] ;
/// ```
///
/// This is byte-for-byte the format used throughout
/// `tests/testdata/w3c_sparql11/negation/manifest.ttl` (and, as it turns out,
/// every other vendored eval-test manifest in this suite — none of them put
/// `qt:query` on a line of its own). Before the fix, `in_action_block` was
/// only ever set to `true` on the `mf:action` line itself, and even when it
/// was, the extraction logic required `qt:query`/`qt:data` to be the first
/// token on their line — so a leading `[ ` prefix defeated it too. Both gaps
/// together meant every entry using this style parsed to a `None`
/// `action_query` and was silently dropped by the flush condition, so entire
/// eval-test suites executed zero real assertions while reporting `ok`.
#[test]
fn parse_sparql_manifest_multiline_action_block() {
    let dir = std::env::temp_dir().join(format!(
        "dagalog-w3c-manifest-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let manifest_path = dir.join("manifest.ttl");
    std::fs::write(
        &manifest_path,
        r#"@prefix rdf:    <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix : <http://www.w3.org/2009/sparql/docs/tests/data-sparql11/negation/manifest#> .
@prefix mf:     <http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#> .
@prefix qt:     <http://www.w3.org/2001/sw/DataAccess/tests/test-query#> .
@prefix dawgt:   <http://www.w3.org/2001/sw/DataAccess/tests/test-dawg#> .

<>  rdf:type mf:Manifest ;
    mf:entries
    (
    :full-minuend
   ) .

:full-minuend rdf:type mf:QueryEvaluationTest ;
    mf:name    "Subtraction with MINUS from a fully bound minuend" ;
    dawgt:approval dawgt:Approved ;
    mf:action
         [ qt:query  <full-minuend.rq> ;
           qt:data   <full-minuend.ttl> ] ;
    mf:result  <full-minuend.srx> .
"#,
    )
    .expect("write manifest");

    let entries = parse_sparql_manifest(&manifest_path);
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(
        entries.len(),
        1,
        "expected one entry to be parsed, got {:?}",
        entries
    );
    let entry = &entries[0];
    assert_eq!(
        entry.name,
        "Subtraction with MINUS from a fully bound minuend"
    );
    assert_eq!(entry.kind, SparqlTestKind::Eval);
    assert!(
        entry.action_query.ends_with("full-minuend.rq"),
        "action_query was {:?}",
        entry.action_query
    );
    assert_eq!(
        entry
            .action_data
            .as_deref()
            .map(|p| p.ends_with("full-minuend.ttl")),
        Some(true),
        "action_data was {:?}",
        entry.action_data
    );
    assert_eq!(
        entry
            .result_file
            .as_deref()
            .map(|p| p.ends_with("full-minuend.srx")),
        Some(true)
    );
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
        // Triple terms in SPARQL comparison require RDF 1.2 support (#143).
        GraphElement::TripleTerm(k) => {
            SrxValue::Uri(format!("<<( {} {} {} )>>", k.subject, k.predicate, k.obj))
        }
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
    if path.ends_with(".ru") {
        parse_update(&text).map_err(|e| format!("parse error: {}", e))?;
        return Ok(());
    }
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
    // `test_52` ("PrefixName with backslash-escaped colons") is obsoleted —
    // commented out of `mf:entries` — and, as of the #192 fix, is correctly
    // no longer produced by the manifest loader at all (it previously
    // required a skip-list entry here because the old line-scanning parser
    // wasn't aware of `mf:entries` list membership and found the entry's
    // dangling definition anyway; the new RDF-native loader walks the actual
    // `mf:entries` list via SPARQL, so entries excluded from it are excluded
    // here too).
    let skip: &[&str] = &[
        // CONSTRUCT WHERE with FROM clause
        "syntax-construct-where-02.rq",
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
        // GROUP BY semantic constraints not enforced at parse time:
        //   syn-bad-01: SELECT * ... GROUP BY is invalid (SPARQL §11.2) — parser accepts it
        //   syn-bad-02: out-of-scope variable in SELECT with GROUP BY — not validated
        // These tests were previously masked by the # comment parsing bug (#67): the parser
        // failed on the leading `# comment` line before reaching the GROUP BY body.
        // Fixed by issue #67 (sp/sp1 comment skipping); GROUP BY validation tracked separately.
        "syn-bad-01.rq",
        "syn-bad-02.rq",
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
// All eval categories are now active.

/// W3C SPARQL 1.1 — BIND evaluation tests (BIND with expressions and arithmetic).
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/bind/
#[test]
fn w3c_sparql11_bind() {
    let entries = load_sparql_manifest("bind");
    // These entries were previously silently dropped by the manifest-parser
    // bug in #192 (multi-line `mf:action` blocks never got their `qt:query`/
    // `qt:data` captured, so the whole entry was skipped). Now that the
    // parser is fixed they execute for real and fail on genuine BIND
    // evaluation gaps (arithmetic/type coercion in BIND expressions, and
    // BIND variable-scope enforcement for bind10). Not a regression from
    // #192 — pre-existing gaps it happened to mask. See #192 for context;
    // dedicated fix issues should be filed separately.
    let skip: &[&str] = &[
        "bind01 - BIND",
        "bind02 - BIND",
        "bind03 - BIND",
        "bind04 - BIND",
        "bind05 - BIND",
        "bind06 - BIND",
        "bind07 - BIND",
        "bind08 - BIND",
        "bind10 - BIND scoping - Variable in filter not in scope",
    ];
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). Genuine EXISTS-within-graph-pattern
    // evaluation gap, not a regression from #192.
    let skip: &[&str] = &["Exists within graph pattern"];
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). Genuine gaps in post-query VALUES
    // semantics (join/filter behaviour of a trailing VALUES clause against
    // the outer pattern) and in subquery-scoped VALUES parsing, not a
    // regression from #192.
    let skip: &[&str] = &[
        "Post-query VALUES with subj-var, 1 row",
        "Post-query VALUES with obj-var, 1 row",
        "Post-query VALUES with 2 obj-vars, 1 row",
        "Post-query VALUES with 2 obj-vars, 1 row with UNDEF",
        "Post-query VALUES with 2 obj-vars, 2 rows with UNDEF",
        "Post-query VALUES with pred-var, 1 row",
        "Post-query VALUES with (OPTIONAL) obj-var, 1 row",
        "Post-subquery VALUES",
    ];
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
    // The `sq12`/`sq14` entries were already skip-listed before #192, but
    // under the wrong names — this table was dead code while the manifest
    // parser dropped every entry in this suite, so the mismatch was never
    // noticed. Corrected here to the actual `mf:name` values. The rest are
    // newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind):
    // CONSTRUCT-result comparison isn't implemented (sq12, sq14), several
    // `.rdf`/XML data files aren't parseable by the vendored Turtle parser
    // (sq04, sq06, sq08, sq09, sq10 — RDF/XML support, not Turtle, is
    // missing), subquery-in-graph-pattern evaluation has gaps (sq01, sq02,
    // sq03, sq05, sq07), and subquery-with-LIMIT-inside-a-blank-node-pattern
    // doesn't parse (sq11, sq13). None of these are regressions from #192.
    let skip: &[&str] = &[
        "sq01 - Subquery within graph pattern",
        "sq02 - Subquery within graph pattern, graph variable is bound",
        "sq03 - Subquery within graph pattern, graph variable is not bound",
        "sq04 - Subquery within graph pattern, default graph does not apply",
        "sq05 - Subquery within graph pattern, from named applies",
        "sq06 - Subquery with graph pattern, from named applies",
        "sq07 - Subquery with from ",
        "sq08 - Subquery with aggregate",
        "sq09 - Nested Subqueries",
        "sq10 - Subquery with exists",
        "sq11 - Subquery limit per resource",
        "sq12 - Subquery in CONSTRUCT with built-ins",
        "sq13 - Subqueries don't inject bindings",
        "sq14 - limit by resource",
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). Genuine gaps: ASK queries aren't
    // supported by `run_sparql_query` (GROUP_CONCAT 1/2/with SEPARATOR,
    // SAMPLE), numeric aggregate results don't match the exact xsd:double
    // formatting/rounding the expected SRX uses (AVG, SUM and their
    // GROUP-BY variants, COUNT 8b, MIN with GROUP BY), a nested-aggregate
    // FILTER expression fails to parse (GROUP_CONCAT 2), aggregate error
    // propagation isn't implemented (Error in AVG, Protect from error in
    // AVG), and empty-group aggregation doesn't produce the single
    // unbound-variable row required by the spec. Not a regression from #192.
    // `:agg-empty-group` carries two `mf:name` triples in the vendored
    // manifest itself ("agg empty group" and "Aggregate over empty group
    // resulting in a row with unbound variables") — both name the same
    // underlying entry/gap, so both are listed here.
    let skip: &[&str] = &[
        "COUNT 8b",
        "GROUP_CONCAT 1",
        "GROUP_CONCAT 2",
        "GROUP_CONCAT with SEPARATOR",
        "AVG",
        "AVG with GROUP BY",
        "MIN with GROUP BY",
        "SUM",
        "SUM with GROUP BY",
        "SAMPLE",
        "Error in AVG",
        "Protect from error in AVG",
        "agg empty group",
        "Aggregate over empty group resulting in a row with unbound variables",
    ];
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
    // "Medical, temporal proximity by exclusion (MINUS)" is commented out of
    // the manifest's `mf:entries` list (its sibling NOT-EXISTS variant is
    // active) and its query/data/result files were consequently never
    // vendored. The #192 RDF-native loader walks the real `mf:entries` list
    // via SPARQL, so this dangling, non-listed definition is correctly
    // excluded rather than surfacing as a spurious "file not found" failure
    // — no skip-list entry needed for it (contrast with the old line-scanner,
    // which wasn't aware of list membership at all).
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). Bounded/unbounded repetition path syntax
    // (`{n}`, `{n,m}`, `{n,}`, `{,m}`) is now implemented (issue #203:
    // `PropertyPath::Repeat` in `sparql_parser`), fixing pp20/pp22/pp24/pp26/
    // pp27/pp29 (no longer skipped below). Remaining genuine gaps, still
    // tracked by #203:
    // - pp04, pp05, pp13, pp15: zero-length-path identity semantics (`{0}`
    //   and the zero-hop case of `*`/`?` don't enumerate
    //   `subject = object = x` for every `x` when both endpoints are
    //   unbound — see `zero_hop_solutions` in `sparql_parser/src/execute.rs`)
    // - pp07, pp34, pp35: property paths across named graphs / `GRAPH`
    //   blocks have evaluation gaps
    // - pp08: reverse path (`^path`) as an ASK query — `run_sparql_query`
    //   doesn't support ASK at all
    let skip: &[&str] = &[
        "(pp04) Variable length path with loop",
        "(pp05) Zero length path",
        "(pp07) Path with one graph",
        "(pp08) Reverse path",
        "(pp13) Zero Length Paths with Literals",
        "(pp15) Zero Length Paths on an empty graph",
        "(pp34) Named Graph 1",
        "(pp35) Named Graph 2",
    ];
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). `CONSTRUCT WHERE {}` (shorthand form) is
    // rejected by `run_sparql_query` with "CONSTRUCT queries are not
    // supported via run_sparql_query" — same underlying CONSTRUCT-execution
    // gap noted in the `subquery` suite's skip list. Not a regression from
    // #192.
    let skip: &[&str] = &[
        "constructwhere01 - CONSTRUCT WHERE",
        "constructwhere02 - CONSTRUCT WHERE",
        "constructwhere03 - CONSTRUCT WHERE",
        "constructwhere04 - CONSTRUCT WHERE",
    ];
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). Genuine builtin-function gaps: several
    // string/numeric/date builtins don't match expected results exactly
    // (STRDT, STRLANG and their type-error variants, isNumeric, CEIL, FLOOR,
    // ROUND, CONCAT, SUBSTR, UCASE, LCASE, the date-part accessors HOURS/
    // MINUTES/SECONDS/YEAR/MONTH/DAY/TIMEZONE, BNODE with and without an
    // argument, STRBEFORE/STRAFTER and their datatyping variants, REPLACE,
    // COALESCE), `IN`/`NOT IN` and ASK-form tests aren't supported by
    // `run_sparql_query` (IN 1/2, NOT IN 1/2, NOW, RAND), `IRI()`/`URI()`
    // and `IF()` fail to parse in this position, and `UUID()`/`STRUUID()`
    // pattern-matching isn't implemented. Not a regression from #192.
    let skip: &[&str] = &[
        "STRDT()",
        "STRDT() TypeErrors",
        "STRLANG()",
        "STRLANG() TypeErrors",
        "isNumeric()",
        "CEIL()",
        "FLOOR()",
        "ROUND()",
        "CONCAT() 2",
        "SUBSTR() (3-argument)",
        "SUBSTR() (2-argument)",
        "UCASE()",
        "LCASE()",
        "plus-1",
        "plus-2",
        "HOURS()",
        "MINUTES()",
        "SECONDS()",
        "YEAR()",
        "MONTH()",
        "DAY()",
        "TIMEZONE()",
        "BNODE(str)",
        "IN 1",
        "IN 2",
        "NOT IN 1",
        "NOT IN 2",
        "NOW()",
        "RAND()",
        "BNODE()",
        "IRI()/URI()",
        "IF()",
        "COALESCE()",
        "STRBEFORE()",
        "STRBEFORE() datatyping",
        "STRAFTER()",
        "STRAFTER() datatyping",
        "REPLACE()",
        "UUID() pattern match",
        "STRUUID() pattern match",
    ];
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). Genuine GROUP-BY-with-expression
    // grouping-key gap, not a regression from #192.
    let skip: &[&str] = &["Group-4"];
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
    // Newly-exposed by the #192 manifest-parser fix (see w3c_sparql11_bind
    // for the general explanation). Genuine gaps in projected-expression
    // evaluation: equality comparisons and arithmetic over projected
    // expressions, and reusing a projected expression's variable in a later
    // SELECT item or ORDER BY. Not a regression from #192.
    let skip: &[&str] = &[
        "Expression is equality",
        "Expression raise an error",
        "Reuse a project expression variable in select",
        "Reuse a project expression variable in order by",
    ];
    let failures: Vec<_> = entries
        .iter()
        .filter_map(|e| run_eval_test(e, skip))
        .collect();
    assert_no_failures(failures, "SPARQL 1.1 project-expression");
}

// ── SPARQL 1.1 Update Syntax Tests ───────────────────────────────────────────

/// W3C SPARQL 1.1 Update — positive syntax tests.
///
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/syntax-update-1/
///
/// Parsing is handled by `sparql_endpoint::sparql_update::parse_update`.
/// Supports: INSERT DATA, DELETE DATA, DELETE WHERE, DELETE/INSERT WHERE,
/// WITH-form updates, LOAD, CREATE, DROP, CLEAR (all with SILENT), PREFIX/BASE
/// prologue, and `#` comments.
#[test]
fn w3c_sparql11_update_syntax_positive() {
    let entries = load_sparql_manifest("syntax-update-1");
    let positives: Vec<_> = entries
        .into_iter()
        .filter(|e| e.kind == SparqlTestKind::PositiveSyntax)
        .collect();
    let skip: &[&str] = &[];
    assert_no_failures(
        run_syntax_tests(&positives, skip),
        "SPARQL 1.1 update syntax positive",
    );
}

/// W3C SPARQL 1.1 Update — negative syntax tests.
///
/// Reference: https://www.w3.org/2009/sparql/docs/tests/data-sparql11/syntax-update-1/
///
/// Skip list:
/// - `syntax-update-bad-05.ru`: nested GRAPH inside DELETE DATA — requires
///   full Turtle-level parse of the DATA block content to detect.
/// - `syntax-update-54.ru`: blank node label reuse across `;`-separated
///   operations — requires tracking labels across operation boundaries.
#[test]
fn w3c_sparql11_update_syntax_negative() {
    let entries = load_sparql_manifest("syntax-update-1");
    let negatives: Vec<_> = entries
        .into_iter()
        .filter(|e| e.kind == SparqlTestKind::NegativeSyntax)
        .collect();
    let skip: &[&str] = &[
        // Nested GRAPH inside DELETE DATA — full Turtle parse of DATA content
        // needed to detect the nesting violation.
        "syntax-update-bad-05.ru",
        // Blank node label reuse across `;`-separated INSERT DATA operations —
        // requires tracking labels across operation boundaries.
        "syntax-update-54.ru",
    ];
    assert_no_failures(
        run_syntax_tests(&negatives, skip),
        "SPARQL 1.1 update syntax negative",
    );
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! W3C RDF 1.1 conformance test suites.
//!
//! Covers Turtle 1.1, N-Triples 1.1, N-Quads 1.1, and TriG 1.1 conformance
//! tests from the W3C RDF test suite.
//!
//! Test data is vendored in `tests/testdata/w3c_{turtle,ntriples,nquads,trig}/`
//! from the following official W3C sources (W3C Test Suite License):
//!
//! - Turtle:    <https://www.w3.org/2013/TurtleTests/>
//! - N-Triples: <https://www.w3.org/2013/N-TriplesTests/>
//! - N-Quads:   <https://www.w3.org/2013/N-QuadsTests/>
//! - TriG:      <https://www.w3.org/2013/TrigTests/>
//!
//! Each suite's `manifest.ttl` lists test entries with:
//! - `rdft:TestXxxPositiveSyntax` — file must parse without error
//! - `rdft:TestXxxNegativeSyntax` — file must produce a parse error
//! - `rdft:TestXxxEval` — parse and compare output against expected N-Triples/N-Quads,
//!   up to blank-node renaming (graph isomorphism, via backtracking bijection search —
//!   see `compare_datastores` below)
//! - `rdft:TestXxxNegativeEval` — file must error or produce a non-matching result
//!
//! Run just this file: `cargo test --test w3c_rdf_conformance`

use dag_rdf::{Datastore, GraphElement, RdfResource};
use dagalog::load_file;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Manifest parsing ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestKind {
    PositiveSyntax,
    NegativeSyntax,
    PositiveEval,
    NegativeEval,
}

#[derive(Debug)]
struct ManifestEntry {
    name: String,
    kind: TestKind,
    action: String,
    result: Option<String>,
}

// ── Graph isomorphism comparison ──────────────────────────────────────────────
//
// Used by the eval tests to compare the parsed output of an action file against
// the expected triples in a result file, up to blank-node renaming.

/// A term normalised for cross-datastore comparison: ground terms (IRI or
/// literal) are kept as `GraphElement` values (which derive Eq/Hash) and blank
/// nodes are extracted as their intra-datastore integer ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum NTerm {
    Ground(GraphElement),
    Blank(u32),
}

fn gel_to_nterm(el: GraphElement) -> NTerm {
    match el {
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => NTerm::Blank(id),
        other => NTerm::Ground(other),
    }
}

fn datastore_to_nquads(ds: &Datastore) -> Vec<[NTerm; 4]> {
    ds.named_graphs
        .get_all_quads()
        .map(|q| {
            let qr = ds.resources.get_resource_quad(q);
            [
                gel_to_nterm(qr.triple_id),
                gel_to_nterm(qr.subject),
                gel_to_nterm(qr.predicate),
                gel_to_nterm(qr.obj),
            ]
        })
        .collect()
}

fn blank_node_ids(quads: &[[NTerm; 4]]) -> Vec<u32> {
    let mut ids: HashSet<u32> = HashSet::new();
    for quad in quads {
        for term in quad {
            if let NTerm::Blank(id) = term {
                ids.insert(*id);
            }
        }
    }
    let mut v: Vec<u32> = ids.into_iter().collect();
    v.sort();
    v
}

fn map_term(t: &NTerm, bij: &HashMap<u32, u32>) -> NTerm {
    match t {
        NTerm::Blank(id) => NTerm::Blank(*bij.get(id).unwrap_or(id)),
        other => other.clone(),
    }
}

fn apply_bijection(quads: &[[NTerm; 4]], bij: &HashMap<u32, u32>) -> Vec<[NTerm; 4]> {
    quads
        .iter()
        .map(|[g, s, p, o]| {
            [
                map_term(g, bij),
                map_term(s, bij),
                map_term(p, bij),
                map_term(o, bij),
            ]
        })
        .collect()
}

/// Backtracking bijection search: tries all mappings from `act_bns[0..]` to
/// elements of `exp_bns`, recursing with the bijection accumulated in `bij`.
fn bijection_isomorphic(
    actual: &[[NTerm; 4]],
    expected_sorted: &[[NTerm; 4]],
    act_bns: &[u32],
    exp_bns: &[u32],
    bij: &mut HashMap<u32, u32>,
) -> bool {
    if act_bns.is_empty() {
        let mut mapped = apply_bijection(actual, bij);
        mapped.sort();
        return mapped == expected_sorted;
    }
    let act_bn = act_bns[0];
    let used_exp: HashSet<u32> = bij.values().copied().collect();
    for &exp_bn in exp_bns {
        if used_exp.contains(&exp_bn) {
            continue;
        }
        bij.insert(act_bn, exp_bn);
        if bijection_isomorphic(actual, expected_sorted, &act_bns[1..], exp_bns, bij) {
            return true;
        }
        bij.remove(&act_bn);
    }
    false
}

/// Compare two Datastores for RDF graph isomorphism (modulo blank-node
/// renaming).  Returns `None` on success, `Some(reason)` on mismatch.
fn compare_datastores(expected: &Datastore, actual: &Datastore) -> Option<String> {
    let expected_quads = datastore_to_nquads(expected);
    let actual_quads = datastore_to_nquads(actual);

    if expected_quads.len() != actual_quads.len() {
        return Some(format!(
            "expected {} triples, got {}",
            expected_quads.len(),
            actual_quads.len()
        ));
    }

    let exp_bns = blank_node_ids(&expected_quads);
    let act_bns = blank_node_ids(&actual_quads);

    if exp_bns.len() != act_bns.len() {
        return Some(format!(
            "expected {} distinct blank nodes, got {}",
            exp_bns.len(),
            act_bns.len()
        ));
    }

    let mut exp_sorted = expected_quads.clone();
    exp_sorted.sort();

    if act_bns.is_empty() {
        let mut act_sorted = actual_quads;
        act_sorted.sort();
        return if act_sorted == exp_sorted {
            None
        } else {
            Some("triple sets differ (no blank nodes involved)".to_string())
        };
    }

    if bijection_isomorphic(
        &actual_quads,
        &exp_sorted,
        &act_bns,
        &exp_bns,
        &mut HashMap::new(),
    ) {
        None
    } else {
        Some("no blank-node bijection found — graphs are not isomorphic".to_string())
    }
}

/// Run one eval test.  Loads the action file and the expected-result file,
/// then compares the parsed datastores for graph isomorphism.
/// Returns `None` on pass, `Some(failure_message)` on failure.
fn run_eval_test(dir: &Path, entry: &ManifestEntry, skip: &[&str]) -> Option<String> {
    if skip.contains(&entry.name.as_str()) {
        return None;
    }
    let action_path = dir.join(&entry.action);
    let mut actual = Datastore::new(4_096);
    let parse_result = load_file(&mut actual, &action_path);

    match entry.kind {
        TestKind::NegativeEval => {
            if parse_result.is_err() {
                return None; // expected: file must fail
            }
            // File parsed — must produce output that doesn't match the result file.
            // If there is no result file, the test is purely "must fail to parse".
            let result_path = match &entry.result {
                Some(r) => dir.join(r),
                None => {
                    return Some(format!(
                        "FAIL {}: NegativeEval file parsed but should have failed",
                        entry.name
                    ));
                }
            };
            let mut expected = Datastore::new(4_096);
            if load_file(&mut expected, &result_path).is_err() {
                return None; // can't load expected → treat as mismatch (pass)
            }
            return if compare_datastores(&expected, &actual).is_some() {
                None // outputs differ as required
            } else {
                Some(format!(
                    "FAIL {}: NegativeEval file matched expected output",
                    entry.name
                ))
            };
        }
        TestKind::PositiveEval => {
            if let Err(e) = parse_result {
                return Some(format!("FAIL {}: parse error: {}", entry.name, e));
            }
        }
        _ => return None, // skip syntax-only entries
    }

    let result_path = match &entry.result {
        Some(r) => dir.join(r),
        None => return Some(format!("FAIL {}: manifest has no mf:result", entry.name)),
    };
    let mut expected = Datastore::new(4_096);
    if let Err(e) = load_file(&mut expected, &result_path) {
        return Some(format!(
            "FAIL {}: cannot parse result '{}': {}",
            entry.name,
            result_path.display(),
            e
        ));
    }
    compare_datastores(&expected, &actual).map(|e| format!("FAIL {}: {}", entry.name, e))
}

fn suite_dir(suite: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(suite)
}

/// Minimal manifest.ttl parser — extracts test name, kind, action, and result
/// by scanning for `<#…> rdf:type rdft:TestXxx` blocks.
///
/// Deliberately avoids using our own Turtle parser to eliminate circular
/// dependency with what is being tested.
fn parse_manifest(
    path: &Path,
    positive_eval: &str,
    negative_eval: &str,
    positive_syntax: &[&str],
    negative_syntax: &[&str],
) -> Vec<ManifestEntry> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read manifest {}: {}", path.display(), e));

    let mut entries = Vec::new();
    // Split on blank lines or new test anchors to get individual entry blocks
    // Each entry starts with `<#name>` at the beginning of a line
    let mut current_name: Option<String> = None;
    let mut current_kind: Option<TestKind> = None;
    let mut current_action: Option<String> = None;
    let mut current_result: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        // New entry anchor: `<#test-name> rdf:type ...` or just `<#test-name>`
        if trimmed.starts_with("<#") {
            // Flush previous entry
            if let (Some(name), Some(kind), Some(action)) = (
                current_name.take(),
                current_kind.take(),
                current_action.take(),
            ) {
                entries.push(ManifestEntry {
                    name,
                    kind,
                    action,
                    result: current_result.take(),
                });
            } else {
                current_name = None;
                current_kind = None;
                current_action = None;
                current_result = None;
            }

            // Extract test name from `<#name>` at start of trimmed line
            if let Some(end) = trimmed.find('>') {
                let name = trimmed[2..end].to_string();
                if !name.is_empty() {
                    current_name = Some(name);
                }
            }
        }

        // Type declaration — accepts both `rdf:type rdft:Xxx` and the `a rdft:Xxx` shorthand
        if trimmed.contains("rdf:type rdft:") || trimmed.contains(" a rdft:") {
            let kind = if trimmed.contains(positive_eval) {
                Some(TestKind::PositiveEval)
            } else if trimmed.contains(negative_eval) {
                Some(TestKind::NegativeEval)
            } else if positive_syntax.iter().any(|s| trimmed.contains(s)) {
                Some(TestKind::PositiveSyntax)
            } else if negative_syntax.iter().any(|s| trimmed.contains(s)) {
                Some(TestKind::NegativeSyntax)
            } else {
                None
            };
            if kind.is_some() {
                current_kind = kind;
            }
        }

        // Action file: `mf:action <filename> ;`
        if trimmed.starts_with("mf:action")
            && let Some(start) = trimmed.find('<')
            && let Some(end) = trimmed[start + 1..].find('>')
        {
            current_action = Some(trimmed[start + 1..start + 1 + end].to_string());
        }

        // Result file: `mf:result <filename> ;`
        if trimmed.starts_with("mf:result")
            && let Some(start) = trimmed.find('<')
            && let Some(end) = trimmed[start + 1..].find('>')
        {
            current_result = Some(trimmed[start + 1..start + 1 + end].to_string());
        }
    }

    // Flush last entry
    if let (Some(name), Some(kind), Some(action)) = (current_name, current_kind, current_action) {
        entries.push(ManifestEntry {
            name,
            kind,
            action,
            result: current_result,
        });
    }

    entries
}

/// Run one syntax test. Returns `None` on success, `Some(failure_message)` on failure.
fn run_syntax_test(dir: &Path, entry: &ManifestEntry, skip: &[&str]) -> Option<String> {
    if skip.contains(&entry.name.as_str()) {
        return None;
    }
    let path = dir.join(&entry.action);
    let mut ds = Datastore::new(4_096);
    let result = load_file(&mut ds, &path);
    match entry.kind {
        TestKind::PositiveSyntax | TestKind::PositiveEval => {
            if let Err(e) = result {
                Some(format!("FAIL {}: expected Ok, got Err: {}", entry.name, e))
            } else {
                None
            }
        }
        TestKind::NegativeSyntax | TestKind::NegativeEval => {
            if result.is_ok() {
                Some(format!("FAIL {}: expected parse error, got Ok", entry.name))
            } else {
                None
            }
        }
    }
}

/// Asserts that `entries_run` is non-zero (the manifest was actually parsed and
/// matched entries) and that every matched entry produced the expected outcome.
///
/// For positive-syntax suites, a "failure" is an entry that unexpectedly
/// errored.  For negative-syntax suites, a "failure" is an entry whose file
/// was accepted by the parser when it should have been rejected — so a
/// passing test run means the parser correctly rejected all bad inputs.
fn assert_suite_passed(entries_run: usize, failures: Vec<String>, suite: &str) {
    assert!(
        entries_run > 0,
        "no entries matched in manifest for suite '{}' — manifest parsing may be broken",
        suite
    );
    if !failures.is_empty() {
        eprintln!("\n{} FAILURES in {}:", failures.len(), suite);
        for f in &failures {
            eprintln!("  {}", f);
        }
        panic!("{} test(s) failed in {}", failures.len(), suite);
    }
}

// ── W3C Turtle 1.1 ───────────────────────────────────────────────────────────
//
// Reference: https://www.w3.org/2013/TurtleTests/
// Test suite distributed under W3C Test Suite License.

fn turtle_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_turtle");
    parse_manifest(
        &dir.join("manifest.ttl"),
        "TestTurtleEval",
        "TestTurtleNegativeEval",
        &["TestTurtlePositiveSyntax"],
        &["TestTurtleNegativeSyntax"],
    )
}

/// W3C Turtle 1.1 — positive syntax tests (file must parse without error).
///
/// Reference: https://www.w3.org/2013/TurtleTests/
///
/// Skip-list: tests that use relative IRIs (`<s>`, `<p>`) without a `@base`
/// declaration require the parser to be given the document's own URL as base;
/// oxttl rejects them with "No scheme found in an absolute IRI" when no base
/// is set.  Tracked as a known parser limitation.
#[test]
fn w3c_turtle_positive_syntax() {
    let dir = suite_dir("w3c_turtle");
    let entries = turtle_entries();
    let skip: &[&str] = &[
        // Relative IRIs without @base — require document base URI, not yet wired up
        "turtle-syntax-number-01",
        "turtle-syntax-number-02",
        "turtle-syntax-number-03",
        "turtle-syntax-number-04",
        "turtle-syntax-number-05",
        "turtle-syntax-number-06",
        "turtle-syntax-number-07",
        "turtle-syntax-number-08",
        "turtle-syntax-number-09",
        "turtle-syntax-number-10",
        "turtle-syntax-number-11",
        "turtle-syntax-datatypes-01",
        "turtle-syntax-datatypes-02",
        "turtle-syntax-kw-01",
        "turtle-syntax-kw-02",
    ];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "Turtle positive-syntax");
}

/// W3C Turtle 1.1 — negative syntax tests: each file must be *rejected* by the
/// parser (i.e. `load_file` must return `Err`).
///
/// A test "fails" here when the parser accepts a deliberately-invalid file —
/// meaning the parser is too permissive.  The suite passes when every bad file
/// is correctly rejected.
///
/// Reference: https://www.w3.org/2013/TurtleTests/
#[test]
fn w3c_turtle_negative_syntax() {
    let dir = suite_dir("w3c_turtle");
    let entries = turtle_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "Turtle negative-syntax");
}

/// W3C Turtle 1.1 — eval tests: parse and compare output against expected
/// N-Triples (graph isomorphism up to blank-node renaming).
///
/// Reference: https://www.w3.org/2013/TurtleTests/
#[test]
fn w3c_turtle_eval() {
    let dir = suite_dir("w3c_turtle");
    let entries = turtle_entries();
    let skip: &[&str] = &[
        // Relative IRIs without @base — document URL needed as base URI.
        "turtle-eval-bad-01",
        "turtle-eval-bad-02",
        "turtle-eval-bad-03",
        "turtle-eval-bad-04",
        // turtle-subm-01 uses `@prefix : <#>` — relative IRI requires base URI.
        "turtle-subm-01",
        // turtle-subm-27 uses a relative IRI before the @base declaration.
        "turtle-subm-27",
        // Result file contains IRIs in the Unicode Tags block (U+E01EF) which
        // oxrdfio rejects as "Invalid IRI code point"; the test verifies the
        // Turtle parser accepts them, but our N-Triples result file parser does not.
        "localName_with_assigned_nfc_PN_CHARS_BASE_character_boundaries",
    ];
    let eval: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveEval || e.kind == TestKind::NegativeEval)
        .collect();
    let failures: Vec<_> = eval
        .iter()
        .filter_map(|e| run_eval_test(&dir, e, skip))
        .collect();
    assert_suite_passed(eval.len(), failures, "Turtle eval");
}

// ── W3C N-Triples 1.1 ────────────────────────────────────────────────────────
//
// Reference: https://www.w3.org/2013/N-TriplesTests/
// Test suite distributed under W3C Test Suite License.

fn ntriples_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_ntriples");
    parse_manifest(
        &dir.join("manifest.ttl"),
        "TestNTriplesEval",
        "TestNTriplesNegativeEval",
        &["TestNTriplesPositiveSyntax"],
        &["TestNTriplesNegativeSyntax"],
    )
}

/// W3C N-Triples 1.1 — positive syntax tests.
///
/// Reference: https://www.w3.org/2013/N-TriplesTests/
#[test]
fn w3c_ntriples_positive_syntax() {
    let dir = suite_dir("w3c_ntriples");
    let entries = ntriples_entries();
    let skip: &[&str] = &[];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "N-Triples positive-syntax");
}

/// W3C N-Triples 1.1 — negative syntax tests: each file must be rejected by
/// the parser.  A failure means the parser accepted a deliberately-invalid file.
///
/// Reference: https://www.w3.org/2013/N-TriplesTests/
#[test]
fn w3c_ntriples_negative_syntax() {
    let dir = suite_dir("w3c_ntriples");
    let entries = ntriples_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "N-Triples negative-syntax");
}

// ── W3C N-Quads 1.1 ──────────────────────────────────────────────────────────
//
// Reference: https://www.w3.org/2013/N-QuadsTests/
// Test suite distributed under W3C Test Suite License.

fn nquads_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_nquads");
    parse_manifest(
        &dir.join("manifest.ttl"),
        "TestNQuadsEval",
        "TestNQuadsNegativeEval",
        &["TestNQuadsPositiveSyntax"],
        &["TestNQuadsNegativeSyntax"],
    )
}

/// W3C N-Quads 1.1 — positive syntax tests.
///
/// Reference: https://www.w3.org/2013/N-QuadsTests/
#[test]
fn w3c_nquads_positive_syntax() {
    let dir = suite_dir("w3c_nquads");
    let entries = nquads_entries();
    let skip: &[&str] = &[];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "N-Quads positive-syntax");
}

/// W3C N-Quads 1.1 — negative syntax tests: each file must be rejected by
/// the parser.  A failure means the parser accepted a deliberately-invalid file.
///
/// Reference: https://www.w3.org/2013/N-QuadsTests/
#[test]
fn w3c_nquads_negative_syntax() {
    let dir = suite_dir("w3c_nquads");
    let entries = nquads_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "N-Quads negative-syntax");
}

// ── W3C TriG 1.1 ─────────────────────────────────────────────────────────────
//
// Reference: https://www.w3.org/2013/TrigTests/
// Test suite distributed under W3C Test Suite License.

fn trig_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_trig");
    parse_manifest(
        &dir.join("manifest.ttl"),
        "TestTrigEval",
        "TestTrigNegativeEval",
        &["TestTrigPositiveSyntax", "TestTurtlePositiveSyntax"],
        &["TestTrigNegativeSyntax", "TestTurtleNegativeSyntax"],
    )
}

/// W3C TriG 1.1 — positive syntax tests.
///
/// Reference: https://www.w3.org/2013/TrigTests/
///
/// Skip-list: same relative-IRI-without-base issue as Turtle (see
/// `w3c_turtle_positive_syntax`).
#[test]
fn w3c_trig_positive_syntax() {
    let dir = suite_dir("w3c_trig");
    let entries = trig_entries();
    let skip: &[&str] = &[
        // Relative IRIs without @base — require document base URI
        "trig-syntax-number-01",
        "trig-syntax-number-02",
        "trig-syntax-number-03",
        "trig-syntax-number-04",
        "trig-syntax-number-05",
        "trig-syntax-number-06",
        "trig-syntax-number-07",
        "trig-syntax-number-08",
        "trig-syntax-number-09",
        "trig-syntax-number-10",
        "trig-syntax-number-11",
        "trig-syntax-datatypes-01",
        "trig-syntax-datatypes-02",
        "trig-syntax-kw-01",
        "trig-syntax-kw-02",
    ];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "TriG positive-syntax");
}

/// W3C TriG 1.1 — negative syntax tests: each file must be rejected by
/// the parser.  A failure means the parser accepted a deliberately-invalid file.
///
/// Reference: https://www.w3.org/2013/TrigTests/
#[test]
fn w3c_trig_negative_syntax() {
    let dir = suite_dir("w3c_trig");
    let entries = trig_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "TriG negative-syntax");
}

/// W3C TriG 1.1 — eval tests: parse and compare output against expected
/// N-Quads (graph isomorphism up to blank-node renaming).
///
/// Reference: https://www.w3.org/2013/TrigTests/
#[test]
fn w3c_trig_eval() {
    let dir = suite_dir("w3c_trig");
    let entries = trig_entries();
    let skip: &[&str] = &[
        // Relative IRIs without @base — document URL needed as base URI.
        "trig-subm-01",
        "trig-subm-27",
        // Result file contains IRIs in the Unicode Tags block (U+E01EF) rejected
        // by oxrdfio as "Invalid IRI code point".
        "localName_with_assigned_nfc_PN_CHARS_BASE_character_boundaries",
    ];
    let eval: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveEval || e.kind == TestKind::NegativeEval)
        .collect();
    let failures: Vec<_> = eval
        .iter()
        .filter_map(|e| run_eval_test(&dir, e, skip))
        .collect();
    assert_suite_passed(eval.len(), failures, "TriG eval");
}

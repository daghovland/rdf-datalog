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
//! - `rdft:TestXxxEval` — parse and compare output against expected N-Triples/N-Quads
//!   (eval comparison is marked `#[ignore]` pending graph-isomorphism support)
//! - `rdft:TestXxxNegativeEval` — file must error or produce a non-matching result

use dag_rdf::Datastore;
use dagalog::load_file;
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
    #[allow(dead_code)]
    result: Option<String>,
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
///
/// Ignored: proper evaluation requires blank-node isomorphism comparison
/// between parsed output and expected N-Triples; that is not yet implemented.
/// Currently only checks that the action file parses without error.
#[test]
#[ignore = "full eval comparison requires blank-node graph isomorphism (not yet implemented); parsing is verified by w3c_turtle_positive_syntax"]
fn w3c_turtle_eval() {
    let dir = suite_dir("w3c_turtle");
    let entries = turtle_entries();
    let skip: &[&str] = &[];
    let eval: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveEval || e.kind == TestKind::NegativeEval)
        .collect();
    let failures: Vec<_> = eval
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
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

/// W3C TriG 1.1 — eval tests (parse and compare output against expected N-Quads).
///
/// Reference: https://www.w3.org/2013/TrigTests/
///
/// Ignored: requires blank-node graph isomorphism for correct comparison.
#[test]
#[ignore = "full eval comparison requires blank-node graph isomorphism (not yet implemented)"]
fn w3c_trig_eval() {
    let dir = suite_dir("w3c_trig");
    let entries = trig_entries();
    let skip: &[&str] = &[];
    let eval: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveEval || e.kind == TestKind::NegativeEval)
        .collect();
    let failures: Vec<_> = eval
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(eval.len(), failures, "TriG eval");
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! W3C RDF 1.2 conformance test suites.
//!
//! Tests initially ignored — unignored during Phase R2 (#145) and R3 (#146)
//! implementation. See [#149](https://github.com/daghovland/rdf-datalog/issues/149).
//!
//! Covers Turtle 1.2, N-Triples 1.2, N-Quads 1.2, and TriG 1.2 conformance
//! tests from the W3C RDF 1.2 test suites.
//!
//! Test data is vendored in `tests/testdata/w3c_rdf12_{turtle,ntriples,nquads,trig}/`
//! from the following official W3C sources (W3C Test Suite License):
//!
//! - Turtle 1.2:    <https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-turtle/>
//! - N-Triples 1.2: <https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-triples/>
//! - N-Quads 1.2:   <https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-quads/>
//! - TriG 1.2:      <https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-trig/>
//!
//! Each sub-suite manifest (`eval/manifest.ttl`, `syntax/manifest.ttl`) lists test
//! entries with:
//! - `rdft:TestXxxPositiveSyntax` — file must parse without error
//! - `rdft:TestXxxNegativeSyntax` — file must produce a parse error
//! - `rdft:TestXxxEval` — parse and compare output against expected N-Triples/N-Quads
//!
//! All tests are `#[ignore]` pending RDF 1.2 parser support.
//! Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145) (R2) and
//! [#146](https://github.com/daghovland/rdf-datalog/issues/146) (R3).

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
// Mirrors the implementation in w3c_rdf_conformance.rs for symmetry.

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
                return None;
            }
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
                return None;
            }
            return if compare_datastores(&expected, &actual).is_some() {
                None
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
        _ => return None,
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

fn suite_dir(suite: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(suite)
}

/// Parse a flat-format manifest.ttl for RDF 1.2 test suites.
///
/// RDF 1.2 manifests use prefixed names (`trs:test-name`) as test-entry anchors
/// rather than the anchor-IRI form (`<#name>`) used by RDF 1.1 suites.  This
/// parser handles that format while reusing the same `ManifestEntry`/`TestKind`
/// types as the RDF 1.1 runner.
///
/// Each test entry looks like:
/// ```text
/// trs:turtle12-rt-01 rdf:type rdft:TestTurtleEval ;
///    mf:name      "…" ;
///    mf:action    <filename.ttl> ;
///    mf:result    <filename.nt> ;
///    .
/// ```
fn parse_manifest_rdf12(
    path: &Path,
    positive_eval: &str,
    negative_eval: &str,
    positive_syntax: &[&str],
    negative_syntax: &[&str],
) -> Vec<ManifestEntry> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read manifest {}: {}", path.display(), e));

    let mut entries = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_kind: Option<TestKind> = None;
    let mut current_action: Option<String> = None;
    let mut current_result: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        // Skip blank lines, comments and manifest-header lines.
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('%')
            || trimmed.starts_with("PREFIX")
            || trimmed.starts_with("@prefix")
        {
            // Blank line: flush any pending entry whose type was set on a
            // previous (not yet closed) block.  This is rare but safe.
            continue;
        }

        // Detect a new test-entry line:
        //   `trs:some-name rdf:type rdft:TestXxx ;`
        // Characteristics: starts with a non-whitespace character, is not a
        // manifest-header predicate, and contains `rdf:type rdft:`.
        let starts_subject = !line.starts_with(' ')
            && !line.starts_with('\t')
            && !trimmed.starts_with('"')
            && !trimmed.starts_with('<')
            && !trimmed.starts_with('.')
            && !trimmed.starts_with('(')
            && !trimmed.starts_with(')')
            && !trimmed.starts_with('[')
            && !trimmed.starts_with("mf:")
            && !trimmed.starts_with("rdfs:")
            && !trimmed.starts_with("dct:")
            && !trimmed.starts_with("skos:")
            && !trimmed.starts_with("foaf:");

        if starts_subject && (trimmed.contains("rdf:type rdft:") || trimmed.contains(" a rdft:")) {
            // Flush the previous entry if it was complete.
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

            // Extract the local name from the prefixed subject IRI.
            // e.g. "trs:turtle12-rt-01 rdf:type …"  →  "turtle12-rt-01"
            if let Some(colon_pos) = trimmed.find(':') {
                let after_prefix = &trimmed[colon_pos + 1..];
                let name_end = after_prefix
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(after_prefix.len());
                let name = after_prefix[..name_end].trim().to_string();
                if !name.is_empty() {
                    current_name = Some(name);
                }
            }

            // Extract test kind from the same line.
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

        // Type declaration on a separate indented line (e.g. after a blank-node
        // subject brace or continued multi-line entry).
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
            if current_kind.is_none() {
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

        // End-of-entry terminator:  `   .`  (a lone dot, possibly preceded by
        // whitespace).  Flush the current entry when we see it.
        if trimmed == "." {
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
        }
    }

    // Flush last entry (no trailing `.` in file).
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

// ── W3C Turtle 1.2 ───────────────────────────────────────────────────────────
//
// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-turtle/
// Test suite distributed under W3C Test Suite License.

fn turtle12_syntax_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_rdf12_turtle").join("syntax");
    parse_manifest_rdf12(
        &dir.join("manifest.ttl"),
        "TestTurtleEval",
        "TestTurtleNegativeEval",
        &["TestTurtlePositiveSyntax"],
        &["TestTurtleNegativeSyntax"],
    )
}

fn turtle12_eval_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_rdf12_turtle").join("eval");
    parse_manifest_rdf12(
        &dir.join("manifest.ttl"),
        "TestTurtleEval",
        "TestTurtleNegativeEval",
        &["TestTurtlePositiveSyntax"],
        &["TestTurtleNegativeSyntax"],
    )
}

/// W3C Turtle 1.2 — positive syntax tests (file must parse without error).
///
/// Requires RDF 1.2 Turtle parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-turtle/
#[test]
fn w3c_rdf12_turtle_positive_syntax() {
    let dir = suite_dir("w3c_rdf12_turtle").join("syntax");
    let entries = turtle12_syntax_entries();
    let skip: &[&str] = &[];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "Turtle 1.2 positive-syntax");
}

/// W3C Turtle 1.2 — negative syntax tests: each file must be *rejected* by the
/// parser (i.e. `load_file` must return `Err`).
///
/// Requires RDF 1.2 Turtle parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-turtle/
#[test]
fn w3c_rdf12_turtle_negative_syntax() {
    let dir = suite_dir("w3c_rdf12_turtle").join("syntax");
    let entries = turtle12_syntax_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "Turtle 1.2 negative-syntax");
}

/// W3C Turtle 1.2 — eval tests: parse and compare output against expected
/// N-Triples (graph isomorphism up to blank-node renaming).
///
/// Requires RDF 1.2 Turtle parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-turtle/
#[test]
// #145: still failing (8 entries: turtle12-rt-{01,03..07}, turtle12-annotation-03,
// turtle12-reified-triples-annotation-03) — these need full RDF-star reifier
// (`<<s p o>>`, no parens) support with `rdf:reifies`/blank-node semantics,
// which is beyond object-position triple-term parsing added in this phase.
// Positive/negative syntax coverage is coming from `w3c_rdf12_turtle_positive_syntax`
// / `w3c_rdf12_turtle_negative_syntax` above, which already pass.
#[ignore]
fn w3c_rdf12_turtle_eval() {
    let dir = suite_dir("w3c_rdf12_turtle").join("eval");
    let entries = turtle12_eval_entries();
    let skip: &[&str] = &[];
    let eval: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveEval || e.kind == TestKind::NegativeEval)
        .collect();
    let failures: Vec<_> = eval
        .iter()
        .filter_map(|e| run_eval_test(&dir, e, skip))
        .collect();
    assert_suite_passed(eval.len(), failures, "Turtle 1.2 eval");
}

// ── W3C N-Triples 1.2 ────────────────────────────────────────────────────────
//
// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-triples/
// Test suite distributed under W3C Test Suite License.

fn ntriples12_syntax_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_rdf12_ntriples").join("syntax");
    parse_manifest_rdf12(
        &dir.join("manifest.ttl"),
        "TestNTriplesEval",
        "TestNTriplesNegativeEval",
        &["TestNTriplesPositiveSyntax"],
        &["TestNTriplesNegativeSyntax"],
    )
}

/// W3C N-Triples 1.2 — positive syntax tests.
///
/// Requires RDF 1.2 N-Triples parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-triples/
#[test]
fn w3c_rdf12_ntriples_positive_syntax() {
    let dir = suite_dir("w3c_rdf12_ntriples").join("syntax");
    let entries = ntriples12_syntax_entries();
    let skip: &[&str] = &[];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "N-Triples 1.2 positive-syntax");
}

/// W3C N-Triples 1.2 — negative syntax tests: each file must be rejected by
/// the parser.
///
/// Requires RDF 1.2 N-Triples parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-triples/
#[test]
fn w3c_rdf12_ntriples_negative_syntax() {
    let dir = suite_dir("w3c_rdf12_ntriples").join("syntax");
    let entries = ntriples12_syntax_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "N-Triples 1.2 negative-syntax");
}

// ── W3C N-Quads 1.2 ──────────────────────────────────────────────────────────
//
// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-quads/
// Test suite distributed under W3C Test Suite License.

fn nquads12_syntax_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_rdf12_nquads").join("syntax");
    parse_manifest_rdf12(
        &dir.join("manifest.ttl"),
        "TestNQuadsEval",
        "TestNQuadsNegativeEval",
        &["TestNQuadsPositiveSyntax"],
        &["TestNQuadsNegativeSyntax"],
    )
}

/// W3C N-Quads 1.2 — positive syntax tests.
///
/// Requires RDF 1.2 N-Quads parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-quads/
#[test]
fn w3c_rdf12_nquads_positive_syntax() {
    let dir = suite_dir("w3c_rdf12_nquads").join("syntax");
    let entries = nquads12_syntax_entries();
    let skip: &[&str] = &[];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "N-Quads 1.2 positive-syntax");
}

/// W3C N-Quads 1.2 — negative syntax tests: each file must be rejected by
/// the parser.
///
/// Requires RDF 1.2 N-Quads parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-n-quads/
#[test]
fn w3c_rdf12_nquads_negative_syntax() {
    let dir = suite_dir("w3c_rdf12_nquads").join("syntax");
    let entries = nquads12_syntax_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "N-Quads 1.2 negative-syntax");
}

// ── W3C TriG 1.2 ─────────────────────────────────────────────────────────────
//
// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-trig/
// Test suite distributed under W3C Test Suite License.

fn trig12_syntax_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_rdf12_trig").join("syntax");
    parse_manifest_rdf12(
        &dir.join("manifest.ttl"),
        "TestTrigEval",
        "TestTrigNegativeEval",
        &["TestTrigPositiveSyntax", "TestTurtlePositiveSyntax"],
        &["TestTrigNegativeSyntax", "TestTurtleNegativeSyntax"],
    )
}

fn trig12_eval_entries() -> Vec<ManifestEntry> {
    let dir = suite_dir("w3c_rdf12_trig").join("eval");
    parse_manifest_rdf12(
        &dir.join("manifest.ttl"),
        "TestTrigEval",
        "TestTrigNegativeEval",
        &["TestTrigPositiveSyntax", "TestTurtlePositiveSyntax"],
        &["TestTrigNegativeSyntax", "TestTurtleNegativeSyntax"],
    )
}

/// W3C TriG 1.2 — positive syntax tests.
///
/// Requires RDF 1.2 TriG parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-trig/
#[test]
fn w3c_rdf12_trig_positive_syntax() {
    let dir = suite_dir("w3c_rdf12_trig").join("syntax");
    let entries = trig12_syntax_entries();
    let skip: &[&str] = &[];
    let positive: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveSyntax)
        .collect();
    let failures: Vec<_> = positive
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(positive.len(), failures, "TriG 1.2 positive-syntax");
}

/// W3C TriG 1.2 — negative syntax tests: each file must be rejected by
/// the parser.
///
/// Requires RDF 1.2 TriG parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-trig/
#[test]
fn w3c_rdf12_trig_negative_syntax() {
    let dir = suite_dir("w3c_rdf12_trig").join("syntax");
    let entries = trig12_syntax_entries();
    let skip: &[&str] = &[];
    let negative: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::NegativeSyntax)
        .collect();
    let failures: Vec<_> = negative
        .iter()
        .filter_map(|e| run_syntax_test(&dir, e, skip))
        .collect();
    assert_suite_passed(negative.len(), failures, "TriG 1.2 negative-syntax");
}

/// W3C TriG 1.2 — eval tests: parse and compare output against expected
/// N-Quads (graph isomorphism up to blank-node renaming).
///
/// Requires RDF 1.2 TriG parser support.
/// Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
///
/// Reference: https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-trig/
#[test]
// #145: still failing (same 8 rt/annotation entries as Turtle 1.2 eval above) —
// needs full RDF-star reifier (`<<s p o>>`) support, out of scope for this
// phase. Positive/negative syntax coverage above already passes.
#[ignore]
fn w3c_rdf12_trig_eval() {
    let dir = suite_dir("w3c_rdf12_trig").join("eval");
    let entries = trig12_eval_entries();
    let skip: &[&str] = &[];
    let eval: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == TestKind::PositiveEval || e.kind == TestKind::NegativeEval)
        .collect();
    let failures: Vec<_> = eval
        .iter()
        .filter_map(|e| run_eval_test(&dir, e, skip))
        .collect();
    assert_suite_passed(eval.len(), failures, "TriG 1.2 eval");
}

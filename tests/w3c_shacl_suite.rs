/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! W3C SHACL 1.0 conformance test suite.
//!
//! Test data is vendored in `tests/testdata/w3c_shacl/core/` from:
//! <https://github.com/w3c/data-shapes> (`data-shapes-test-suite/tests/core/`)
//! (W3C Software and Document License — see `tests/testdata/w3c_shacl/LICENSE`).
//!
//! `core/sparql/` (SHACL-SPARQL constraints, §5-6) is intentionally **not**
//! vendored — already out of scope per
//! [#54](https://github.com/daghovland/rdf-datalog/issues/54).
//!
//! See [`docs/plans/W3C_SHACL_SUITE_PLAN.md`](../docs/plans/W3C_SHACL_SUITE_PLAN.md)
//! for the full design rationale. Report comparison canonicalizes both the
//! expected and actual report graphs (via `rdf_canon::canonicalize_graph`,
//! [RDFC-1.0](https://www.w3.org/TR/rdf-canon/)) and compares the resulting
//! canonical N-Quads strings for equality, the same approach this repo's
//! RDF/SPARQL W3C conformance suites already use (see
//! `compare_construct_with_ttl` in `tests/w3c_sparql11_suite.rs`) — rather
//! than the hand-written field-by-field comparator with explicit
//! blank-node-skip flags this replaced. See
//! [#313](https://github.com/daghovland/rdf-datalog/issues/313).
//!
//! Manifests are loaded with this project's own stack — real Turtle parsing
//! (`turtle::parse_turtle_with_base`) into a `dag_rdf::Datastore`, walked
//! with real SPARQL queries (`mf:include`, `mf:entries/rdf:rest*/rdf:first`)
//! via `sparql_parser`'s executor — per the convention established by #192.
//!
//! Run just this file: `cargo test --test w3c_shacl_suite`

use dag_rdf::{Datastore, GraphElement, RdfLiteral, RdfResource};
use dagalog::run_sparql_query;
use ingress::IriReference;
use rdf_canon::canonicalize_graph;
use shacl::graph::element_display;
use shacl::{Severity, ValidationReport, ValidationResult, report_to_datastore, validate};
use std::path::{Path, PathBuf};
use turtle::parse_turtle_with_base;

fn suite_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join("w3c_shacl")
        .join("core")
}

// ── Small RDF term helpers (mirrors `tests/w3c_sparql11_suite.rs`) ──────────

fn as_iri(value: Option<&GraphElement>) -> Option<&str> {
    match value {
        Some(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri)))) => Some(iri.as_str()),
        _ => None,
    }
}

fn as_file_path(value: Option<&GraphElement>) -> Option<String> {
    as_iri(value)?.strip_prefix("file://").map(str::to_string)
}

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

fn as_bool(value: Option<&GraphElement>) -> Option<bool> {
    match value {
        Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b))) => Some(*b),
        Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. })) => {
            match literal.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

fn abs_base_iri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", abs.display())
}

fn load_turtle(path: &Path) -> Option<Datastore> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut ds = Datastore::new(4_096);
    parse_turtle_with_base(&mut ds, text.as_bytes(), &abs_base_iri(path)).ok()?;
    Some(ds)
}

// ── mf:include discovery ─────────────────────────────────────────────────────

/// Resolve a manifest's `mf:include` targets to absolute file paths.
fn list_includes(manifest_path: &Path) -> Vec<PathBuf> {
    let Some(ds) = load_turtle(manifest_path) else {
        return Vec::new();
    };
    let sparql = r#"
        PREFIX mf: <http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#>
        SELECT ?inc WHERE { ?m mf:include ?inc }
    "#;
    let Ok(result) = run_sparql_query(&ds, sparql) else {
        return Vec::new();
    };
    result
        .rows
        .iter()
        .filter_map(|row| as_file_path(row.get("inc")))
        .map(PathBuf::from)
        .collect()
}

// ── Per-test manifest entries ────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ShaclTestEntry {
    label: String,
    data_graph: PathBuf,
    shapes_graph: PathBuf,
    /// The manifest's inline expected `sh:ValidationReport`, rebuilt as this
    /// crate's own `ValidationReport` type so it can be fed through
    /// `report_to_datastore` symmetrically with the actual report — see
    /// `compare_report`.
    expected_report: ValidationReport,
}

/// Parse one vendored SHACL test file (self-contained: shapes/data graph(s)
/// plus its own one-entry `mf:Manifest` and inline expected-report graph).
fn parse_shacl_test_file(path: &Path) -> Vec<ShaclTestEntry> {
    let Some(ds) = load_turtle(path) else {
        return Vec::new();
    };

    let entry_sparql = r#"
        PREFIX rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
        PREFIX mf:   <http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#>
        PREFIX sht:  <http://www.w3.org/ns/shacl-test#>
        SELECT ?entry ?label ?dataGraph ?shapesGraph ?result WHERE {
            ?manifest mf:entries/rdf:rest*/rdf:first ?entry .
            ?entry rdf:type sht:Validate ;
                   rdfs:label ?label ;
                   mf:action ?action .
            OPTIONAL { ?entry mf:result ?result }
            OPTIONAL { ?action sht:dataGraph ?dataGraph }
            OPTIONAL { ?action sht:shapesGraph ?shapesGraph }
        }
    "#;
    let Ok(entry_result) = run_sparql_query(&ds, entry_sparql) else {
        return Vec::new();
    };

    let report_sparql = r#"
        PREFIX sh: <http://www.w3.org/ns/shacl#>
        SELECT ?report ?conforms ?result ?focus ?severity ?component ?shape ?path ?value WHERE {
            ?report sh:conforms ?conforms .
            OPTIONAL {
                ?report sh:result ?result .
                OPTIONAL { ?result sh:focusNode ?focus }
                OPTIONAL { ?result sh:resultSeverity ?severity }
                OPTIONAL { ?result sh:sourceConstraintComponent ?component }
                OPTIONAL { ?result sh:sourceShape ?shape }
                OPTIONAL { ?result sh:resultPath ?path }
                OPTIONAL { ?result sh:value ?value }
            }
        }
    "#;
    let Ok(report_result) = run_sparql_query(&ds, report_sparql) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for row in &entry_result.rows {
        let (Some(label), Some(data_graph), Some(shapes_graph)) = (
            as_string(row.get("label")),
            as_file_path(row.get("dataGraph")),
            as_file_path(row.get("shapesGraph")),
        ) else {
            continue;
        };
        let Some(report_node) = row.get("result").cloned() else {
            continue;
        };

        let mut expected_conforms = true;
        let mut expected_results = Vec::new();
        for rrow in &report_result.rows {
            if rrow.get("report") != Some(&report_node) {
                continue;
            }
            if let Some(c) = as_bool(rrow.get("conforms")) {
                expected_conforms = c;
            }
            if rrow.get("result").is_none() {
                continue;
            }
            let focus = rrow.get("focus");
            let shape = rrow.get("shape");
            let path = rrow.get("path");
            let value = rrow.get("value");
            expected_results.push(ValidationResult {
                focus_node: focus.map(|e| element_display(&ds, ds_id(&ds, e))),
                severity: as_iri(rrow.get("severity"))
                    .and_then(Severity::from_iri)
                    .unwrap_or_default(),
                // `sh:resultMessage` is intentionally not extracted here — see
                // the note on `compare_report` below.
                message: None,
                // Parse the expected `sh:resultPath` object out of this same
                // `ds` (the fixture's own inline expected-report graph) into
                // the structured `ShPath` AST — same type `validate()`
                // itself produces, and `compare_report` feeds both sides
                // through the same `report_to_datastore`, so this fixture's
                // literal RDF shape (however it happens to write the path)
                // and the actual side's freshly-serialized shape only need
                // to be isomorphic, not textually identical. See
                // https://github.com/daghovland/rdf-datalog/issues/335.
                result_path: path.and_then(|e| shacl::path::parse_path(&ds, ds_id(&ds, e))),
                source_shape: shape
                    .map(|e| element_display(&ds, ds_id(&ds, e)))
                    .unwrap_or_default(),
                source_constraint: as_iri(rrow.get("component")).map(str::to_string),
                value: value.map(|e| element_display(&ds, ds_id(&ds, e))),
            });
        }

        entries.push(ShaclTestEntry {
            label,
            data_graph: PathBuf::from(data_graph),
            shapes_graph: PathBuf::from(shapes_graph),
            expected_report: ValidationReport {
                conforms: expected_conforms,
                results: expected_results,
            },
        });
    }
    entries
}

/// Re-resolve a `GraphElement` back to its `GraphElementId` within `ds`.
/// SPARQL solution rows carry the resolved `GraphElement` value, not the
/// interned id, but `element_display`/`is_blank_node` need the id — this is
/// a cheap re-lookup through the same interning table, always a hit since
/// the element came from `ds` in the first place.
fn ds_id(ds: &Datastore, el: &GraphElement) -> dag_rdf::GraphElementId {
    ds.resources
        .resource_map
        .get(el)
        .copied()
        .unwrap_or_else(|| panic!("element {el:?} not interned in its own datastore"))
}

/// Load all self-contained test files reachable (two `mf:include` hops) from
/// `core/manifest.ttl` under directory `subdir`, e.g. `"node"`.
fn load_shacl_manifest(subdir: &str) -> Vec<ShaclTestEntry> {
    let dir_manifest = suite_dir().join(subdir).join("manifest.ttl");
    let mut entries = Vec::new();
    for test_file in list_includes(&dir_manifest) {
        entries.extend(parse_shacl_test_file(&test_file));
    }
    entries
}

// ── Comparison ────────────────────────────────────────────────────────────────

/// Compare one entry's expected report against the report actually produced
/// by `shacl::validate`, by canonicalizing both as RDF graphs (RDFC-1.0, via
/// `rdf_canon::canonicalize_graph`) and comparing the resulting canonical
/// N-Quads strings — see the module doc comment. Returns `None` on match,
/// `Some(reason)` on mismatch.
///
/// Both sides are built via `report_to_datastore`, not just the actual side:
/// routing the expected report through the very same function is what makes
/// this an apples-to-apples graph comparison. `report_to_datastore`
/// re-derives an RDF term's kind (IRI / blank node / literal, including
/// datatype/language) from the `ValidationResult` string fields'
/// `element_display` text form; since [#337](https://github.com/daghovland/rdf-datalog/issues/337),
/// `element_display` renders literals as genuine Turtle syntax and
/// `intern_result_term` parses that back into a proper typed/lang-tagged
/// `RdfLiteral` (not a plain string), so this round trip is faithful — an
/// integer `sh:value` compares as `xsd:integer` on both sides, not as an
/// opaque string that happens to cancel out.
///
/// `sh:resultMessage` is deliberately zeroed out on both sides before
/// canonicalizing (expected: never extracted by the SPARQL query above;
/// actual: cleared here) — same "never compared" behavior as the
/// field-by-field comparator this replaced. A lang-tagged `sh:message` (e.g.
/// `core/misc/message-001.ttl`'s `"Test message"@en`) is still emitted by
/// `report_to_datastore` as a plain string literal — `message` is interned
/// directly from `ViolMeta`/`shape.message`, not routed through
/// `element_display`/`intern_result_term`, so #337's fix does not reach it;
/// see [#332](https://github.com/daghovland/rdf-datalog/issues/332) for
/// extending the comparison to cover `sh:resultMessage` properly.
fn compare_report(entry: &ShaclTestEntry) -> Option<String> {
    let data = load_turtle(&entry.data_graph)?;
    let shapes = load_turtle(&entry.shapes_graph)?;
    let mut report = match validate(&data, &shapes) {
        Ok(r) => r,
        Err(e) => return Some(format!("validate() returned Err: {e}")),
    };
    for result in &mut report.results {
        result.message = None;
    }

    // Cheap checks first: a graph-isomorphism diff is a much worse error
    // message than "conforms mismatch" / "result count mismatch" for the
    // common failure shapes.
    if report.conforms != entry.expected_report.conforms {
        return Some(format!(
            "conforms mismatch: expected {}, got {}",
            entry.expected_report.conforms, report.conforms
        ));
    }
    if report.results.len() != entry.expected_report.results.len() {
        return Some(format!(
            "result count mismatch: expected {}, got {}",
            entry.expected_report.results.len(),
            report.results.len()
        ));
    }

    let actual_ds = report_to_datastore(&report);
    let expected_ds = report_to_datastore(&entry.expected_report);

    let actual_canon = match canonicalize_graph(&actual_ds, dag_rdf::DEFAULT_GRAPH_ELEMENT_ID) {
        Ok(c) => c,
        Err(e) => return Some(format!("canonicalization error (actual): {e}")),
    };
    let expected_canon = match canonicalize_graph(&expected_ds, dag_rdf::DEFAULT_GRAPH_ELEMENT_ID) {
        Ok(c) => c,
        Err(e) => return Some(format!("canonicalization error (expected): {e}")),
    };

    if actual_canon == expected_canon {
        None
    } else {
        Some(format!(
            "report graph mismatch:\n--- actual ---\n{actual_canon}--- expected ---\n{expected_canon}"
        ))
    }
}

fn run_entries(entries: &[ShaclTestEntry], skip: &[&str]) -> Vec<String> {
    let mut failures = Vec::new();
    for entry in entries {
        if skip.contains(&entry.label.as_str()) {
            continue;
        }
        if let Some(reason) = compare_report(entry) {
            failures.push(format!("FAIL {}: {}", entry.label, reason));
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

// ── Suite tests, one per vendored sub-directory ──────────────────────────────
//
// Reference: https://github.com/w3c/data-shapes/tree/main/data-shapes-test-suite/tests/core

#[test]
fn w3c_shacl_core_node() {
    let entries = load_shacl_manifest("node");
    assert!(
        entries.len() >= 30,
        "expected at least 30 core/node entries, got {} — manifest discovery may be broken",
        entries.len()
    );
    let skip: &[&str] = &[
        // sh:closed violations don't populate sh:resultPath. See
        // https://github.com/daghovland/rdf-datalog/issues/308.
        "Test of sh:closed at node shape 001",
        "Test of sh:closed at node shape 002",
    ];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/node");
}

#[test]
fn w3c_shacl_core_property() {
    let entries = load_shacl_manifest("property");
    assert!(
        entries.len() >= 35,
        "expected at least 35 core/property entries, got {}",
        entries.len()
    );
    // Property-shape-scoped false negatives and mis-counted violations. See
    // https://github.com/daghovland/rdf-datalog/issues/311.
    let skip: &[&str] = &[
        // Nested `sh:property` (a property shape containing another
        // `sh:property` block, applying the inner shape to each outer
        // path-traversed value as a fresh focus node) is not implemented at
        // all — `ParsedPropShape` (shacl/src/shapes.rs) has no field for it,
        // so this fixture currently reports `conforms=true` (zero results),
        // not even one. Additionally, even with nesting implemented, this
        // fixture's expected report has two *content-identical* results
        // (reached via `ex:InvalidPerson1`/`ex:InvalidPerson2`, both pointing
        // at the same shared `ex:InvalidAddress`) — the same violation-
        // multiplicity collapse `sh:lessThan`/`sh:lessThanOrEquals` had
        // (#341), fixed there via a per-derivation discriminated violation
        // predicate (`shacl::vocab::viol_discriminated`, called from those
        // two constraint arms in shacl/src/evaluate.rs). That mechanism is
        // NOT yet applied anywhere else — nested `sh:property`'s eventual
        // `add_viol` call site will need the same discriminator treatment,
        // it does not come for free from #341's fix. See
        // https://github.com/daghovland/rdf-datalog/issues/341.
        "Test of sh:property at property shape 001",
        // "Test of sh:nodeKind at property shape 001" was previously skipped
        // here too, attributed to a genuine violation-generation undercount.
        // Investigating that claim while switching this suite to
        // canonicalization-based comparison (#313) found otherwise: `report.
        // results.len()` and `entry.expected_report.results.len()` are both
        // 27 for this fixture, and the two report graphs canonicalize
        // identically. The old field-by-field comparator's greedy multiset
        // matcher ("try each pairing, remove on match", no backtracking)
        // is what actually failed — this fixture has many violations sharing
        // focus/path/value across six sibling `sh:nodeKind` shapes, exactly
        // the shape a greedy matcher can fail to pair even when a valid
        // assignment exists (classic bipartite-matching-without-backtracking
        // failure), which a real graph-isomorphism check (RDFC-1.0
        // canonicalization) does not have. No skip needed any more.
    ];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/property");
}

#[test]
fn w3c_shacl_core_misc() {
    let entries = load_shacl_manifest("misc");
    assert!(
        entries.len() >= 5,
        "expected at least 5 core/misc entries, got {}",
        entries.len()
    );
    // All core/misc entries (sh:deactivated 002, sh:severity 001/002) were
    // fixed under https://github.com/daghovland/rdf-datalog/issues/312:
    // literal-valued sh:targetNode was silently dropped by target
    // resolution, and a shape recognised only via a target-declaring
    // predicate (no explicit rdf:type sh:NodeShape/sh:PropertyShape) was
    // never picked up by shape discovery. See shacl/src/lib.rs and
    // shacl/src/shapes.rs. No skips remain.
    let skip: &[&str] = &[];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/misc");
}

#[test]
fn w3c_shacl_core_targets() {
    let entries = load_shacl_manifest("targets");
    assert!(
        entries.len() >= 5,
        "expected at least 5 core/targets entries, got {}",
        entries.len()
    );
    // "Test of implicit sh:targetClass 001" was fixed under
    // https://github.com/daghovland/rdf-datalog/issues/312: sh:targetClass /
    // implicit class-as-shape target resolution now applies the
    // rdfs:subClassOf* closure required by SHACL spec §2.1.3.1
    // (`?this rdf:type/rdfs:subClassOf* $class`) — see
    // shacl::class_target_instances in shacl/src/lib.rs. No skips remain.
    let skip: &[&str] = &[];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/targets");
}

#[test]
fn w3c_shacl_core_validation_reports() {
    let entries = load_shacl_manifest("validation-reports");
    assert!(
        !entries.is_empty(),
        "expected at least 1 core/validation-reports entries, got {}",
        entries.len()
    );
    // See https://github.com/daghovland/rdf-datalog/issues/312.
    // "Test of validation report for shape shared by property constraints" was
    // triaged under #312 and found to be a property-shape-scoped duplicate-report
    // gap (a shape reached twice via two sh:property constraints), not a misc/
    // deactivated/severity/targetClass issue. Tracked under
    // https://github.com/daghovland/rdf-datalog/issues/311 instead.
    let skip: &[&str] = &["Test of validation report for shape shared by property constraints"];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/validation-reports");
}

#[test]
fn w3c_shacl_core_complex() {
    let entries = load_shacl_manifest("complex");
    assert!(
        entries.len() >= 2,
        "expected at least 2 core/complex entries, got {}",
        entries.len()
    );
    let skip: &[&str] = &[
        // shacl-shacl validates SHACL's own shapes-of-shapes ontology, which
        // relies on SHACL-SPARQL-ish meta-shape machinery — out of scope,
        // same as core/sparql/ (#54).
        "frozen eat your own ( eat your own frozen dogfood )",
    ];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/complex");
}

#[test]
fn w3c_shacl_core_path() {
    let entries = load_shacl_manifest("path");
    assert!(
        entries.len() >= 10,
        "expected at least 10 core/path entries, got {}",
        entries.len()
    );
    // Complex `sh:path` expressions (sequence/alternative/inverse/
    // zeroOrMore/oneOrMore/zeroOrOne) are fully supported for *validation*
    // (https://github.com/daghovland/rdf-datalog/issues/328), and
    // `sh:resultPath` for a compound path is now serialized back into its
    // full SHACL-spec RDF encoding (`sh:alternativePath`/`sh:inversePath`/
    // RDF-list structure) rather than reported as an opaque, triple-less
    // blank node — see `ValidationResult::result_path` (a `path::ShPath`
    // AST), `path::to_datastore`, and this file's `parse_shacl_test_file`
    // (which parses the expected side's `sh:resultPath` object into the same
    // `ShPath` AST via `path::parse_path`, so both sides of `compare_report`
    // only need to be isomorphic, not textually identical). See
    // https://github.com/daghovland/rdf-datalog/issues/335. No skips remain.
    let skip: &[&str] = &[];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/path");
}

// ── Unit tests for the canonicalization-based comparator itself ─────────────
//
// These exercise `report_to_datastore` + `canonicalize_graph` directly,
// independent of the full W3C suite, to prove the comparator is correct on
// its own terms rather than only via the suite's aggregate pass/fail.

/// Build a minimal one-result `ValidationReport`.
fn mk_report(focus_node: &str, value: &str) -> ValidationReport {
    ValidationReport {
        conforms: false,
        results: vec![ValidationResult {
            focus_node: Some(focus_node.to_string()),
            severity: Severity::Violation,
            message: None,
            result_path: Some(shacl::path::ShPath::Predicate(
                "http://example.org/p".to_string(),
            )),
            source_shape: "http://example.org/Shape".to_string(),
            source_constraint: Some("http://www.w3.org/ns/shacl#ClassConstraintComponent".into()),
            value: Some(value.to_string()),
        }],
    }
}

fn canon(report: &ValidationReport) -> String {
    let ds = report_to_datastore(report);
    canonicalize_graph(&ds, dag_rdf::DEFAULT_GRAPH_ELEMENT_ID)
        .expect("canonicalization of a freshly built report graph cannot fail")
}

#[test]
fn canonical_comparator_matches_identical_reports() {
    let a = mk_report("http://example.org/x", "http://example.org/y");
    let b = mk_report("http://example.org/x", "http://example.org/y");
    assert_eq!(canon(&a), canon(&b));
}

#[test]
fn canonical_comparator_detects_real_mismatch() {
    let a = mk_report("http://example.org/x", "http://example.org/y");
    let b = mk_report("http://example.org/x", "http://example.org/DIFFERENT");
    assert_ne!(canon(&a), canon(&b));
}

/// The case the removed `*_is_blank` flags existed to paper over: the same
/// blank node used as *both* `sh:focusNode` and `sh:value` of one result
/// (e.g. `core/node/class-002.ttl`'s `_:b9751`), but spelled with different
/// labels across two independently-built reports (as expected-vs-actual
/// always are, coming from separate parses/`Datastore`s). Graph
/// canonicalization must recognize these as the same shape regardless of the
/// arbitrary label, since the label carries no meaning on its own — only the
/// graph structure (here: one blank node reachable via both `sh:focusNode`
/// and `sh:value` from the same result) does.
#[test]
fn canonical_comparator_ignores_blank_node_label_spelling() {
    let a = mk_report("_:b9751", "_:b9751");
    let b = mk_report("_:zzz", "_:zzz");
    assert_eq!(canon(&a), canon(&b));

    // Sanity check: if the two occurrences *didn't* refer to the same blank
    // node, that's a structurally different (and detectably different) graph.
    let c = ValidationReport {
        conforms: false,
        results: vec![ValidationResult {
            focus_node: Some("_:b1".to_string()),
            severity: Severity::Violation,
            message: None,
            result_path: Some(shacl::path::ShPath::Predicate(
                "http://example.org/p".to_string(),
            )),
            source_shape: "http://example.org/Shape".to_string(),
            source_constraint: Some("http://www.w3.org/ns/shacl#ClassConstraintComponent".into()),
            value: Some("_:b2".to_string()),
        }],
    };
    assert_ne!(canon(&a), canon(&c));
}

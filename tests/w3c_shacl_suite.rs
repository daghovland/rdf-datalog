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
//! for the full design rationale, in particular why report comparison here
//! is a tiered field-by-field comparator rather than full graph isomorphism
//! (the SHACL suite's manifest structure and this crate's `ValidationResult`
//! type differ enough from the SPARQL/RDF suites' that neither of those
//! suites' comparison approach applies directly).
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
use shacl::graph::{element_display, is_blank_node};
use shacl::validate;
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
struct ExpectedResult {
    focus_node: Option<String>,
    severity: Option<String>,
    component: Option<String>,
    source_shape: Option<String>,
    result_path: Option<String>,
    value: Option<String>,
    /// `true` if the corresponding raw term is a blank node (unstable across
    /// the two independent parses involved — see the plan doc). Fields on a
    /// blank-node term are recorded but never used for comparison.
    focus_is_blank: bool,
    source_shape_is_blank: bool,
    result_path_is_blank: bool,
    value_is_blank: bool,
}

#[derive(Debug, Clone)]
struct ShaclTestEntry {
    label: String,
    data_graph: PathBuf,
    shapes_graph: PathBuf,
    expected_conforms: bool,
    expected_results: Vec<ExpectedResult>,
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
            expected_results.push(ExpectedResult {
                focus_node: focus.map(|e| element_display(&ds, ds_id(&ds, e))),
                severity: as_iri(rrow.get("severity")).map(str::to_string),
                component: as_iri(rrow.get("component")).map(str::to_string),
                source_shape: shape.map(|e| element_display(&ds, ds_id(&ds, e))),
                result_path: path.map(|e| element_display(&ds, ds_id(&ds, e))),
                value: value.map(|e| element_display(&ds, ds_id(&ds, e))),
                focus_is_blank: focus.is_some_and(|e| is_blank_node(&ds, ds_id(&ds, e))),
                source_shape_is_blank: shape.is_some_and(|e| is_blank_node(&ds, ds_id(&ds, e))),
                result_path_is_blank: path.is_some_and(|e| is_blank_node(&ds, ds_id(&ds, e))),
                value_is_blank: value.is_some_and(|e| is_blank_node(&ds, ds_id(&ds, e))),
            });
        }

        entries.push(ShaclTestEntry {
            label,
            data_graph: PathBuf::from(data_graph),
            shapes_graph: PathBuf::from(shapes_graph),
            expected_conforms,
            expected_results,
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
/// by `shacl::validate`. See the plan doc for the tiered-comparison
/// rationale. Returns `None` on match, `Some(reason)` on mismatch.
fn compare_report(entry: &ShaclTestEntry) -> Option<String> {
    let data = load_turtle(&entry.data_graph)?;
    let shapes = load_turtle(&entry.shapes_graph)?;
    let report = match validate(&data, &shapes) {
        Ok(r) => r,
        Err(e) => return Some(format!("validate() returned Err: {e}")),
    };

    if report.conforms != entry.expected_conforms {
        return Some(format!(
            "conforms mismatch: expected {}, got {}",
            entry.expected_conforms, report.conforms
        ));
    }
    if report.results.len() != entry.expected_results.len() {
        return Some(format!(
            "result count mismatch: expected {}, got {}",
            entry.expected_results.len(),
            report.results.len()
        ));
    }

    let mut unused: Vec<&shacl::ValidationResult> = report.results.iter().collect();
    for expected in &entry.expected_results {
        let pos = unused
            .iter()
            .position(|actual| results_match(expected, actual));
        match pos {
            Some(i) => {
                unused.remove(i);
            }
            None => {
                return Some(format!(
                    "no actual result matched expected {expected:?}; remaining actual: {unused:?}"
                ));
            }
        }
    }
    None
}

fn results_match(expected: &ExpectedResult, actual: &shacl::ValidationResult) -> bool {
    if let Some(sev) = &expected.severity
        && sev.as_str() != actual.severity.iri()
    {
        return false;
    }
    if let Some(comp) = &expected.component
        && Some(comp.as_str()) != actual.source_constraint.as_deref()
    {
        return false;
    }
    if !expected.focus_is_blank
        && let Some(f) = &expected.focus_node
        && Some(f.as_str()) != actual.focus_node.as_deref()
    {
        return false;
    }
    if !expected.value_is_blank
        && let Some(v) = &expected.value
        && Some(v.as_str()) != actual.value.as_deref()
    {
        return false;
    }
    if !expected.source_shape_is_blank
        && let Some(s) = &expected.source_shape
        && s.as_str() != actual.source_shape
    {
        return false;
    }
    if !expected.result_path_is_blank
        && let Some(p) = &expected.result_path
        && Some(p.as_str()) != actual.result_path.as_deref()
    {
        return false;
    }
    true
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
        // path-traversed value as a fresh focus node). Not implemented at
        // all yet — `ParsedPropShape` has no field for nested property
        // shapes. Additionally, this fixture's expected report has two
        // *content-identical* results (same focus/path/value, reached via
        // two different outer paths to the same shared value node) — the
        // current violation representation (RDF quads in a `Datastore`,
        // which is set-semantics) cannot represent that multiplicity
        // without a larger change to how violations are recorded.
        "Test of sh:property at property shape 001",
        // Same quad-set-dedup multiplicity limitation as above: this
        // fixture expects 4 results but 2 pairs are content-identical
        // (same focus/path/value, different failing comparison partner) —
        // fixed the "silently skip a value entirely" undercounting bug,
        // but not this deeper multiplicity limitation.
        "Test of sh:lessThan at property shape 002",
        // Confirmed NOT the "greedy comparator" issue originally suspected —
        // undercounting is real: violations for ex:InstanceWithBlankNode and
        // ex:InstanceWithBlankNodeAndIRI (the two instances whose only
        // sh:myProperty values are blank nodes) are never generated at all,
        // across all 6 sibling nodeKind shapes. Root cause not yet
        // identified; needs further investigation independent of the fixes
        // in this PR.
        "Test of sh:nodeKind at property shape 001",
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
    // This crate's `sh:path` parsing (shacl/src/shapes.rs) only supports a
    // single predicate IRI, not sequence/inverse/alternative/zeroOrMore/
    // oneOrMore/zeroOrOne SHACL property-path expressions. Tracked by
    // https://github.com/daghovland/rdf-datalog/issues/307 — every skipped
    // label below uses a complex (list- or predicate-valued-blank-node)
    // `sh:path`. `path-unused-001` is not skipped: it only *declares*
    // dangling complex-path blank nodes that the shape itself never
    // references (the shape's own constraint is a plain `sh:class`).
    let skip: &[&str] = &[
        "Test of path sh:alternativePath 001",
        "Test of path complex (rdf:type/rdfs:subClassOf*) 001",
        "Test of complex path validation results",
        "Test of path sh:inversePath 001",
        "Test of path sh:oneOrMorePath 001",
        "Test of path sequence 001",
        "Test of path sequence 002",
        "Test of path sequence with duplicate 001",
        "Test of strange path 001 two valid paths together",
        "Test of strange path 002 valid and invalid paths together",
        "Test of path sh:zeroOrMorePath 001",
        "Test of path sh:zeroOrOnePath 001",
    ];
    let failures = run_entries(&entries, skip);
    assert_no_failures(failures, "SHACL core/path");
}

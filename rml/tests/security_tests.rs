//! Security regression tests for the RML source pipeline.
//!
//! Issues covered:
//! - [#84](https://github.com/daghovland/rdf-datalog/issues/84) Arbitrary file read via absolute `rml:source`
//! - [#85](https://github.com/daghovland/rdf-datalog/issues/85) Path traversal via cell magic
//! - [#86](https://github.com/daghovland/rdf-datalog/issues/86) Unbounded memory allocation
//! - [#88](https://github.com/daghovland/rdf-datalog/issues/88) XPath/JSONPath expression DoS

use dag_rdf::Datastore;
use rml::sandbox::confine_path;
use rml::sources::csv::CsvSource;
use rml::sources::json::JsonSource;
use rml::sources::xml::XmlSource;
use rml::{MAX_SOURCE_ROWS, RmlError, apply_rml_mapping};
use std::path::Path;

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rml_security_{label}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ── #84 / #85: confine_path unit tests ───────────────────────────────────────

#[test]
#[ignore] // #84 https://github.com/daghovland/rdf-datalog/issues/84
fn confine_path_allows_direct_child() {
    let base = temp_dir("confine_child");
    let file = base.join("data.csv");
    std::fs::write(&file, "name\nAlice\n").unwrap();

    let result = confine_path(&base, Path::new("data.csv"));
    assert!(
        result.is_ok(),
        "direct child path must be allowed: {result:?}"
    );
}

#[test]
#[ignore] // #84 https://github.com/daghovland/rdf-datalog/issues/84
fn confine_path_allows_nested_child() {
    let base = temp_dir("confine_nested");
    std::fs::create_dir_all(base.join("subdir")).unwrap();
    let file = base.join("subdir/data.csv");
    std::fs::write(&file, "name\nAlice\n").unwrap();

    let result = confine_path(&base, Path::new("subdir/data.csv"));
    assert!(
        result.is_ok(),
        "nested child path must be allowed: {result:?}"
    );
}

#[test]
#[ignore] // #84 https://github.com/daghovland/rdf-datalog/issues/84
fn confine_path_rejects_absolute_path() {
    let base = temp_dir("confine_absolute");

    let err =
        confine_path(&base, Path::new("/etc/passwd")).expect_err("absolute path must be rejected");
    assert!(
        matches!(err, RmlError::PathTraversal { .. }),
        "expected PathTraversal, got: {err}"
    );
}

#[test]
#[ignore] // #84 https://github.com/daghovland/rdf-datalog/issues/84
fn confine_path_rejects_dotdot_escape() {
    let base = temp_dir("confine_dotdot");

    let err = confine_path(&base, Path::new("../../etc/passwd"))
        .expect_err("../ escape must be rejected");
    assert!(
        matches!(err, RmlError::PathTraversal { .. }),
        "expected PathTraversal, got: {err}"
    );
}

#[test]
#[ignore] // #84 https://github.com/daghovland/rdf-datalog/issues/84
fn confine_path_rejects_dotdot_via_subdir() {
    let base = temp_dir("confine_via_subdir");
    std::fs::create_dir_all(base.join("sub")).unwrap();

    // sub/../../../ escapes the base directory
    let err = confine_path(&base, Path::new("sub/../../../etc/passwd"))
        .expect_err("dotdot-via-subdir must be rejected");
    assert!(
        matches!(err, RmlError::PathTraversal { .. }),
        "expected PathTraversal, got: {err}"
    );
}

// ── #84: apply_rml_mapping rejects absolute rml:source ───────────────────────

#[test]
#[ignore] // #84 https://github.com/daghovland/rdf-datalog/issues/84
fn rml_source_absolute_path_is_rejected() {
    let base = temp_dir("rml_abs_source");

    // Mapping that references an absolute path as its data source
    let mapping = r#"@prefix rml: <http://w3id.org/rml/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
<#tm> a rml:TriplesMap ;
  rml:logicalSource [
    rml:source "/etc/passwd" ;
    rml:referenceFormulation rml:CSV
  ] ;
  rml:subjectMap [ rml:template "http://ex.org/{name}" ] .
"#;
    let mapping_path = base.join("mapping.ttl");
    std::fs::write(&mapping_path, mapping).unwrap();

    let mut ds = Datastore::new(100);
    let err = apply_rml_mapping(&mapping_path, &base, &mut ds)
        .expect_err("absolute rml:source must be rejected");
    assert!(
        matches!(err, RmlError::PathTraversal { .. }),
        "expected PathTraversal, got: {err}"
    );
}

#[test]
#[ignore] // #84 https://github.com/daghovland/rdf-datalog/issues/84
fn rml_source_dotdot_path_is_rejected() {
    let base = temp_dir("rml_dotdot_source");

    let mapping = r#"@prefix rml: <http://w3id.org/rml/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
<#tm> a rml:TriplesMap ;
  rml:logicalSource [
    rml:source "../../etc/shadow" ;
    rml:referenceFormulation rml:CSV
  ] ;
  rml:subjectMap [ rml:template "http://ex.org/{name}" ] .
"#;
    let mapping_path = base.join("mapping.ttl");
    std::fs::write(&mapping_path, mapping).unwrap();

    let mut ds = Datastore::new(100);
    let err = apply_rml_mapping(&mapping_path, &base, &mut ds)
        .expect_err("dotdot rml:source must be rejected");
    assert!(
        matches!(err, RmlError::PathTraversal { .. }),
        "expected PathTraversal, got: {err}"
    );
}

// ── #86: file size limits ─────────────────────────────────────────────────────

#[test]
#[ignore] // #86 https://github.com/daghovland/rdf-datalog/issues/86
fn csv_source_oversized_is_rejected() {
    let dir = temp_dir("csv_size_limit");
    let path = dir.join("big.csv");
    // 200 bytes of content, limit set to 100 bytes
    std::fs::write(&path, "name\n".repeat(40)).unwrap();

    let source = CsvSource::new(path).with_size_limit(100);
    let first = source
        .rows()
        .next()
        .expect("should yield at least one result");
    let err = first.expect_err("oversized CSV must yield an error");
    assert!(
        matches!(err, RmlError::SourceTooLarge { .. }),
        "expected SourceTooLarge, got: {err}"
    );
}

#[test]
#[ignore] // #86 https://github.com/daghovland/rdf-datalog/issues/86
fn csv_source_within_limit_succeeds() {
    let dir = temp_dir("csv_size_ok");
    let path = dir.join("small.csv");
    std::fs::write(&path, "name\nAlice\nBob\n").unwrap();

    // 1 KB limit — file is far below that
    let source = CsvSource::new(path).with_size_limit(1024);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
#[ignore] // #86 https://github.com/daghovland/rdf-datalog/issues/86
fn csv_source_row_limit_is_enforced() {
    let dir = temp_dir("csv_row_limit");
    let path = dir.join("many.csv");
    // 20 rows, limit of 5
    let mut content = "name\n".to_string();
    for i in 0..20 {
        content.push_str(&format!("row{i}\n"));
    }
    std::fs::write(&path, &content).unwrap();

    let source = CsvSource::new(path).with_row_limit(5);
    let results: Vec<_> = source.rows().collect();
    let has_error = results
        .iter()
        .any(|r| matches!(r, Err(RmlError::SourceTooLarge { .. })));
    assert!(has_error, "row limit must produce SourceTooLarge");
}

#[test]
#[ignore] // #86 https://github.com/daghovland/rdf-datalog/issues/86
fn json_source_oversized_is_rejected() {
    let dir = temp_dir("json_size_limit");
    let path = dir.join("big.json");
    // Build a 200-byte JSON array, limit to 100 bytes
    std::fs::write(
        &path,
        r#"[{"name":"Alice"},{"name":"Bob"},{"name":"Carol"}]"#,
    )
    .unwrap();

    let source = JsonSource::new(path).with_size_limit(20);
    let first = source
        .rows()
        .next()
        .expect("should yield at least one result");
    let err = first.expect_err("oversized JSON must yield an error");
    assert!(
        matches!(err, RmlError::SourceTooLarge { .. }),
        "expected SourceTooLarge, got: {err}"
    );
}

#[test]
#[ignore] // #86 https://github.com/daghovland/rdf-datalog/issues/86
fn xml_source_oversized_is_rejected() {
    let dir = temp_dir("xml_size_limit");
    let path = dir.join("big.xml");
    std::fs::write(&path, "<root><item><name>Alice</name></item></root>").unwrap();

    let source = XmlSource::new(path).with_size_limit(10);
    let first = source
        .rows()
        .next()
        .expect("should yield at least one result");
    let err = first.expect_err("oversized XML must yield an error");
    assert!(
        matches!(err, RmlError::SourceTooLarge { .. }),
        "expected SourceTooLarge, got: {err}"
    );
}

// ── #86: row count limit via MAX_SOURCE_ROWS constant ─────────────────────────

#[test]
#[ignore] // #86 https://github.com/daghovland/rdf-datalog/issues/86
#[allow(clippy::assertions_on_constants)]
fn max_source_rows_constant_is_sane() {
    // MAX_SOURCE_ROWS must be positive and ≤ 10 million
    assert!(MAX_SOURCE_ROWS > 0);
    assert!(MAX_SOURCE_ROWS <= 10_000_000);
}

// ── #88: XPath expression complexity validation ───────────────────────────────

#[test]
#[ignore] // #88 https://github.com/daghovland/rdf-datalog/issues/88
fn xml_iterator_with_nested_predicate_is_rejected() {
    let dir = temp_dir("xpath_nested_predicate");
    let path = dir.join("data.xml");
    std::fs::write(&path, "<root><a/><a/></root>").unwrap();

    // Nested predicates cause exponential node-set evaluation
    let bad_xpath = "//a[//a[//a[//a]]]";
    let source = XmlSource::new(path).with_iterator(bad_xpath.to_string());
    let first = source.rows().next().expect("should yield a result");
    let err = first.expect_err("nested-predicate XPath must be rejected");
    assert!(
        matches!(err, RmlError::UnsafeExpression(_)),
        "expected UnsafeExpression, got: {err}"
    );
}

#[test]
#[ignore] // #88 https://github.com/daghovland/rdf-datalog/issues/88
fn xml_iterator_simple_path_is_accepted() {
    let dir = temp_dir("xpath_simple_ok");
    let path = dir.join("data.xml");
    std::fs::write(&path, "<root><item><name>Alice</name></item></root>").unwrap();

    let source = XmlSource::new(path).with_iterator("/root/item".to_string());
    let rows: Vec<_> = source.rows().collect();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_ok());
}

#[test]
#[ignore] // #88 https://github.com/daghovland/rdf-datalog/issues/88
fn xml_iterator_descendant_without_nested_predicate_is_accepted() {
    let dir = temp_dir("xpath_descendant_ok");
    let path = dir.join("data.xml");
    std::fs::write(&path, "<root><item><name>Alice</name></item></root>").unwrap();

    // Descendant axis alone is fine; danger is only when combined with nested predicates
    let source = XmlSource::new(path).with_iterator("//item".to_string());
    let rows: Vec<_> = source.rows().collect();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_ok());
}

#[test]
#[ignore] // #88 https://github.com/daghovland/rdf-datalog/issues/88
fn json_iterator_recursive_descent_is_rejected() {
    let dir = temp_dir("jsonpath_recursive");
    let path = dir.join("data.json");
    std::fs::write(&path, r#"[{"a":{"a":{"a":{"a":{}}}}}]"#).unwrap();

    // $..a is recursive descent — can be O(n²) on deeply nested input
    let source = JsonSource::new(path).with_iterator("$..a".to_string());
    let first = source.rows().next().expect("should yield a result");
    let err = first.expect_err("recursive-descent JSONPath iterator must be rejected");
    assert!(
        matches!(err, RmlError::UnsafeExpression(_)),
        "expected UnsafeExpression, got: {err}"
    );
}

#[test]
#[ignore] // #88 https://github.com/daghovland/rdf-datalog/issues/88
fn json_iterator_simple_path_is_accepted() {
    let dir = temp_dir("jsonpath_simple_ok");
    let path = dir.join("data.json");
    std::fs::write(&path, r#"{"items":[{"name":"Alice"},{"name":"Bob"}]}"#).unwrap();

    let source = JsonSource::new(path).with_iterator("$.items".to_string());
    let rows: Vec<_> = source.rows().collect();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_ok());
}

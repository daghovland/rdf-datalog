use rml::sources::SourceRow;
use rml::sources::json::JsonSource;
use std::path::PathBuf;

fn write_temp(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("rml_json_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

// ── JSON (array document) ─────────────────────────────────────────────────────

#[test]
fn json_source_reads_single_object() {
    let path = write_temp("single.json", r#"[{"id": 1, "name": "Alice"}]"#);
    let source = JsonSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("$.id"), Some("1".to_string()));
    assert_eq!(rows[0].get_str("$.name"), Some("Alice".to_string()));
}

#[test]
fn json_source_reads_multiple_objects() {
    let path = write_temp(
        "multi.json",
        r#"[{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]"#,
    );
    let source = JsonSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get_str("$.name"), Some("Alice".to_string()));
    assert_eq!(rows[1].get_str("$.name"), Some("Bob".to_string()));
}

#[test]
fn json_source_iterator_selects_array() {
    // iterator JSONPath drills into a nested array
    let path = write_temp(
        "nested.json",
        r#"{"students": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]}"#,
    );
    let source = JsonSource::new(path).with_iterator("$.students[*]".to_string());
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get_str("$.name"), Some("Alice".to_string()));
    assert_eq!(rows[1].get_str("$.name"), Some("Bob".to_string()));
}

#[test]
fn json_source_no_iterator_treats_root_object_as_single_row() {
    // Root is a JSON object with no iterator: the whole document is one row.
    let path = write_temp("root_obj.json", r#"{"id": 1, "name": "Alice"}"#);
    let source = JsonSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("$.name"), Some("Alice".to_string()));
}

#[test]
fn json_source_empty_array_yields_no_rows() {
    let path = write_temp("empty.json", r#"[]"#);
    let source = JsonSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 0);
}

#[test]
fn json_source_missing_file_yields_error() {
    let path = PathBuf::from("/tmp/rml_json_tests/does_not_exist_xyz.json");
    let source = JsonSource::new(path);
    let result: Result<Vec<_>, _> = source.rows().collect();
    assert!(result.is_err());
}

#[test]
fn json_source_nested_field_via_jsonpath() {
    let path = write_temp(
        "nested_field.json",
        r#"[{"address": {"city": "Paris", "country": "France"}}]"#,
    );
    let source = JsonSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("$.address.city"), Some("Paris".to_string()));
}

// ── JSONL (newline-delimited JSON) ────────────────────────────────────────────

#[test]
fn jsonl_source_reads_lines() {
    let content = "{\"id\": 1, \"name\": \"Alice\"}\n{\"id\": 2, \"name\": \"Bob\"}\n{\"id\": 3, \"name\": \"Carol\"}\n";
    let path = write_temp("three.jsonl", content);
    let source = JsonSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get_str("$.name"), Some("Alice".to_string()));
    assert_eq!(rows[2].get_str("$.name"), Some("Carol".to_string()));
}

#[test]
fn jsonl_source_skips_blank_lines() {
    let content = "{\"name\": \"Alice\"}\n\n{\"name\": \"Bob\"}\n";
    let path = write_temp("blank_lines.jsonl", content);
    let source = JsonSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn jsonl_source_stops_on_parse_error() {
    let content = "{\"name\": \"Alice\"}\nnot valid json\n{\"name\": \"Bob\"}\n";
    let path = write_temp("bad_line.jsonl", content);
    let source = JsonSource::new(path);
    let result: Result<Vec<_>, _> = source.rows().collect();
    assert!(result.is_err());
}

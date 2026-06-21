/// Tests for the `SourceRow` trait and its two implementations:
/// `CsvRow` (column-name lookup) and `JsonRow` (JSONPath evaluation).
use rml::sources::json::JsonRow;
use rml::sources::{CsvRow, SourceRow};
use serde_json::json;

fn csv(pairs: &[(&str, &str)]) -> CsvRow {
    CsvRow(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

// ── CsvRow ────────────────────────────────────────────────────────────────────

#[test]
fn csv_row_get_str_returns_value() {
    let row = csv(&[("name", "Alice"), ("age", "30")]);
    assert_eq!(row.get_str("name"), Some("Alice".to_string()));
    assert_eq!(row.get_str("age"), Some("30".to_string()));
}

#[test]
fn csv_row_empty_value_returns_none() {
    // Empty cell must be treated as absent — no triple should be generated.
    let row = csv(&[("name", "")]);
    assert_eq!(row.get_str("name"), None);
}

#[test]
fn csv_row_missing_key_returns_none() {
    let row = csv(&[("name", "Alice")]);
    assert_eq!(row.get_str("age"), None);
}

#[test]
fn csv_row_implements_source_row_trait_object() {
    // SourceRow must be object-safe so the engine can hold Box<dyn SourceRow>.
    let row = csv(&[("name", "Alice")]);
    let row_ref: &dyn SourceRow = &row;
    assert_eq!(row_ref.get_str("name"), Some("Alice".to_string()));
}

// ── JsonRow: simple field access ──────────────────────────────────────────────

#[test]
fn json_row_get_str_simple_field() {
    let row = JsonRow(json!({"name": "Alice"}));
    assert_eq!(row.get_str("$.name"), Some("Alice".to_string()));
}

#[test]
fn json_row_get_str_nested_field() {
    let row = JsonRow(json!({"address": {"city": "Paris"}}));
    assert_eq!(row.get_str("$.address.city"), Some("Paris".to_string()));
}

#[test]
fn json_row_number_coerced_to_string() {
    let row = JsonRow(json!({"age": 30}));
    assert_eq!(row.get_str("$.age"), Some("30".to_string()));
}

#[test]
fn json_row_float_coerced_to_string() {
    let row = JsonRow(json!({"score": 9.5}));
    assert_eq!(row.get_str("$.score"), Some("9.5".to_string()));
}

#[test]
fn json_row_bool_true_coerced_to_string() {
    let row = JsonRow(json!({"active": true}));
    assert_eq!(row.get_str("$.active"), Some("true".to_string()));
}

#[test]
fn json_row_bool_false_coerced_to_string() {
    let row = JsonRow(json!({"active": false}));
    assert_eq!(row.get_str("$.active"), Some("false".to_string()));
}

// ── JsonRow: None cases ───────────────────────────────────────────────────────

#[test]
fn json_row_null_returns_none() {
    let row = JsonRow(json!({"name": null}));
    assert_eq!(row.get_str("$.name"), None);
}

#[test]
fn json_row_empty_string_returns_none() {
    // An empty string value should be treated the same as CSV empty cell: skip.
    let row = JsonRow(json!({"name": ""}));
    assert_eq!(row.get_str("$.name"), None);
}

#[test]
fn json_row_missing_field_returns_none() {
    let row = JsonRow(json!({"name": "Alice"}));
    assert_eq!(row.get_str("$.age"), None);
}

#[test]
fn json_row_nested_object_value_returns_none() {
    // Object values cannot be coerced to a string scalar — skip the triple.
    let row = JsonRow(json!({"address": {"city": "Paris"}}));
    assert_eq!(row.get_str("$.address"), None);
}

// ── JsonRow: array handling ───────────────────────────────────────────────────

#[test]
fn json_row_array_first_element_returned() {
    // When a field is an array, the first element is used (if it is a scalar).
    let row = JsonRow(json!({"tags": ["rdf", "owl"]}));
    assert_eq!(row.get_str("$.tags[0]"), Some("rdf".to_string()));
}

#[test]
fn json_row_empty_array_returns_none() {
    let row = JsonRow(json!({"tags": []}));
    assert_eq!(row.get_str("$.tags[0]"), None);
}

// ── JsonRow: bare field name without `$` prefix ───────────────────────────────

#[test]
fn json_row_bare_field_name_auto_prefixed() {
    // Many existing RML mappings write  rml:reference "name"  (no `$.`).
    // JsonRow must auto-prefix with `$.` before running the JSONPath query.
    let row = JsonRow(json!({"name": "Alice"}));
    assert_eq!(row.get_str("name"), Some("Alice".to_string()));
}

#[test]
fn json_row_bare_nested_path_auto_prefixed() {
    // "address.city"  →  "$.address.city"
    let row = JsonRow(json!({"address": {"city": "Paris"}}));
    assert_eq!(row.get_str("address.city"), Some("Paris".to_string()));
}

// ── JsonRow: object-safety ────────────────────────────────────────────────────

#[test]
fn json_row_implements_source_row_trait_object() {
    let row = JsonRow(json!({"name": "Bob"}));
    let row_ref: &dyn SourceRow = &row;
    assert_eq!(row_ref.get_str("$.name"), Some("Bob".to_string()));
}

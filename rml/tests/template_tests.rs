use rml::template::{expand_template, percent_encode};
use std::collections::HashMap;

fn row(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

// ── IRI mode (encode = true) ──────────────────────────────────────────────────

#[test]
#[ignore]
fn iri_mode_simple_substitution() {
    let r = row(&[("id", "42")]);
    let result = expand_template("http://example.com/Student/{id}", &r, true);
    assert_eq!(result, Some("http://example.com/Student/42".to_string()));
}

#[test]
#[ignore]
fn iri_mode_space_is_percent_encoded() {
    let r = row(&[("name", "Venus Williams")]);
    let result = expand_template("http://example.com/{name}", &r, true);
    assert_eq!(result, Some("http://example.com/Venus%20Williams".to_string()));
}

#[test]
#[ignore]
fn iri_mode_slash_in_value_is_encoded() {
    let r = row(&[("path", "a/b")]);
    let result = expand_template("http://example.com/{path}", &r, true);
    assert_eq!(result, Some("http://example.com/a%2Fb".to_string()));
}

#[test]
#[ignore]
fn iri_mode_multiple_placeholders() {
    let r = row(&[("first", "John"), ("last", "Doe")]);
    let result = expand_template("http://example.com/{first}/{last}", &r, true);
    assert_eq!(result, Some("http://example.com/John/Doe".to_string()));
}

#[test]
#[ignore]
fn iri_mode_unreserved_chars_not_encoded() {
    // A-Z a-z 0-9 - . _ ~ are unreserved and must not be encoded
    let r = row(&[("id", "abc-123.test_value~")]);
    let result = expand_template("http://example.com/{id}", &r, true);
    assert_eq!(result, Some("http://example.com/abc-123.test_value~".to_string()));
}

// ── Literal mode (encode = false) ─────────────────────────────────────────────

#[test]
#[ignore]
fn literal_mode_no_encoding_applied() {
    // Same name value with a space: must NOT be encoded in literal mode
    let r = row(&[("name", "Venus Williams")]);
    let result = expand_template("{name}", &r, false);
    assert_eq!(result, Some("Venus Williams".to_string()));
}

#[test]
#[ignore]
fn literal_mode_float_value_unchanged() {
    let r = row(&[("score", "3.14")]);
    let result = expand_template("{score}", &r, false);
    assert_eq!(result, Some("3.14".to_string()));
}

#[test]
#[ignore]
fn literal_mode_comma_in_value_unchanged() {
    let r = row(&[("note", "hello, world")]);
    let result = expand_template("{note}", &r, false);
    assert_eq!(result, Some("hello, world".to_string()));
}

// ── None / skip semantics ─────────────────────────────────────────────────────

#[test]
#[ignore]
fn empty_column_value_returns_none() {
    let r = row(&[("id", "")]);
    let result = expand_template("http://example.com/{id}", &r, true);
    assert_eq!(result, None);
}

#[test]
#[ignore]
fn missing_column_returns_none() {
    let r = row(&[("other", "value")]);
    let result = expand_template("http://example.com/{id}", &r, true);
    assert_eq!(result, None);
}

#[test]
#[ignore]
fn none_if_any_placeholder_missing() {
    // Two placeholders — if either column is absent, whole result is None
    let r = row(&[("first", "John")]);
    let result = expand_template("http://example.com/{first}/{last}", &r, true);
    assert_eq!(result, None);
}

// ── Template with no placeholders ────────────────────────────────────────────

#[test]
#[ignore]
fn no_placeholder_returns_template_verbatim() {
    // A template with no {…} is a constant — row is irrelevant
    let r = row(&[]);
    let result = expand_template("http://example.com/ConstantValue", &r, true);
    assert_eq!(
        result,
        Some("http://example.com/ConstantValue".to_string())
    );
}

// ── percent_encode unit tests ─────────────────────────────────────────────────

#[test]
#[ignore]
fn percent_encode_space() {
    assert_eq!(percent_encode("hello world"), "hello%20world");
}

#[test]
#[ignore]
fn percent_encode_slash() {
    assert_eq!(percent_encode("a/b"), "a%2Fb");
}

#[test]
#[ignore]
fn percent_encode_unreserved_unchanged() {
    let input = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    assert_eq!(percent_encode(input), input);
}

#[test]
#[ignore]
fn percent_encode_at_sign() {
    assert_eq!(percent_encode("user@host"), "user%40host");
}

#[test]
#[ignore]
fn percent_encode_empty_string() {
    assert_eq!(percent_encode(""), "");
}

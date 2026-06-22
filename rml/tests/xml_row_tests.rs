/// Unit tests for `XmlRow` — the `SourceRow` implementation for XML sources.
///
/// Each test constructs an `XmlRow` containing a small XML fragment
/// (as if it had been serialized from a selected iterator node) and
/// verifies that `get_str` correctly evaluates XPath references against it.
use rml::sources::SourceRow;
use rml::sources::xml::XmlRow;

fn row(xml: &str) -> XmlRow {
    XmlRow(xml.to_string())
}

// ── Element text content ──────────────────────────────────────────────────────

#[test]
fn xml_row_get_str_simple_element() {
    let r = row("<student><name>Alice</name></student>");
    assert_eq!(r.get_str("name"), Some("Alice".to_string()));
}

#[test]
fn xml_row_get_str_nested_element() {
    let r = row("<student><address><city>Paris</city></address></student>");
    assert_eq!(r.get_str("address/city"), Some("Paris".to_string()));
}

#[test]
fn xml_row_get_str_text_node_explicit() {
    // name/text() should return the same string as just "name"
    let r = row("<student><name>Alice</name></student>");
    assert_eq!(r.get_str("name/text()"), Some("Alice".to_string()));
}

#[test]
fn xml_row_number_as_text() {
    let r = row("<student><id>42</id></student>");
    assert_eq!(r.get_str("id"), Some("42".to_string()));
}

// ── Attributes ────────────────────────────────────────────────────────────────

#[test]
fn xml_row_get_str_attribute() {
    let r = row(r#"<student id="10"><name>Venus Williams</name></student>"#);
    assert_eq!(r.get_str("@id"), Some("10".to_string()));
}

// ── None / skip semantics ─────────────────────────────────────────────────────

#[test]
fn xml_row_get_str_missing_element_returns_none() {
    let r = row("<student><name>Alice</name></student>");
    assert_eq!(r.get_str("age"), None);
}

#[test]
fn xml_row_get_str_empty_element_returns_none() {
    let r = row("<student><name></name></student>");
    assert_eq!(r.get_str("name"), None);
}

#[test]
fn xml_row_get_str_missing_attribute_returns_none() {
    let r = row("<student><name>Alice</name></student>");
    assert_eq!(r.get_str("@id"), None);
}

// ── SourceRow trait-object dispatch ──────────────────────────────────────────

#[test]
fn xml_row_implements_source_row_trait_object() {
    let r = row("<student><name>Alice</name></student>");
    let row_ref: &dyn SourceRow = &r;
    assert_eq!(row_ref.get_str("name"), Some("Alice".to_string()));
}

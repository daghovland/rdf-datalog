/// Tests for `XmlSource::rows()` — reading XML files and producing `XmlRow`s.
use rml::sources::SourceRow;
use rml::sources::xml::XmlSource;
use std::path::PathBuf;

fn write_temp(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("rml_xml_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn rows_names_and_ids(
    xml: &str,
    file: &str,
    iterator: Option<&str>,
) -> Vec<(Option<String>, Option<String>)> {
    let path = write_temp(file, xml);
    let mut src = XmlSource::new(path);
    if let Some(it) = iterator {
        src = src.with_iterator(it.to_string());
    }
    src.rows()
        .map(|r| {
            let row = r.unwrap();
            (row.get_str("name"), row.get_str("@id"))
        })
        .collect()
}

// ── Basic reading ─────────────────────────────────────────────────────────────

#[test]
fn xml_source_reads_single_element() {
    let xml = "<students><student><name>Alice</name></student></students>";
    let rows = rows_names_and_ids(xml, "src_single.xml", Some("/students/student"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, Some("Alice".to_string()));
}

#[test]
fn xml_source_reads_multiple_elements() {
    let xml = "<students>\
        <student><name>Alice</name></student>\
        <student><name>Bob</name></student>\
    </students>";
    let rows = rows_names_and_ids(xml, "src_multi.xml", Some("/students/student"));
    assert_eq!(rows.len(), 2);
    let names: Vec<_> = rows.iter().map(|r| r.0.as_deref()).collect();
    assert!(names.contains(&Some("Alice")));
    assert!(names.contains(&Some("Bob")));
}

#[test]
fn xml_source_default_iterator_yields_root_element() {
    // No iterator: defaults to /*, selecting the document root element as one row.
    let xml = "<student><name>Alice</name></student>";
    let rows = rows_names_and_ids(xml, "src_root.xml", None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, Some("Alice".to_string()));
}

#[test]
fn xml_source_empty_nodeset_yields_no_rows() {
    let xml = "<students></students>";
    let rows = rows_names_and_ids(xml, "src_empty.xml", Some("/students/student"));
    assert_eq!(rows.len(), 0);
}

// ── Attribute access after selection ─────────────────────────────────────────

#[test]
fn xml_source_attribute_accessible_in_row() {
    let xml = r#"<students><student id="10"><name>Venus Williams</name></student></students>"#;
    let rows = rows_names_and_ids(xml, "src_attr.xml", Some("/students/student"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, Some("10".to_string())); // @id
    assert_eq!(rows[0].0, Some("Venus Williams".to_string())); // name
}

// ── Nested iterator ───────────────────────────────────────────────────────────

#[test]
fn xml_source_nested_iterator() {
    let xml = "<root><items><item><name>X</name></item><item><name>Y</name></item></items></root>";
    let path = write_temp("src_nested.xml", xml);
    let src = XmlSource::new(path).with_iterator("/root/items/item".to_string());
    let count = src.rows().count();
    assert_eq!(count, 2);
}

// ── Element child as id ───────────────────────────────────────────────────────

#[test]
fn xml_source_id_element_accessible_in_row() {
    let xml = "<students><student><id>10</id><name>Venus Williams</name></student></students>";
    let path = write_temp("src_id_elem.xml", xml);
    let src = XmlSource::new(path).with_iterator("/students/student".to_string());
    let rows: Vec<_> = src.rows().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("id"), Some("10".to_string()));
    assert_eq!(rows[0].get_str("name"), Some("Venus Williams".to_string()));
}

// ── Error cases ───────────────────────────────────────────────────────────────

#[test]
fn xml_source_missing_file_yields_error() {
    let src = XmlSource::new(PathBuf::from("/nonexistent/path/file.xml"));
    let result: Vec<_> = src.rows().collect();
    assert_eq!(result.len(), 1);
    assert!(result[0].is_err());
}

#[test]
fn xml_source_malformed_xml_yields_error() {
    let path = write_temp("src_bad.xml", "<unclosed>");
    let src = XmlSource::new(path);
    let result: Vec<_> = src.rows().collect();
    assert_eq!(result.len(), 1);
    assert!(result[0].is_err());
}

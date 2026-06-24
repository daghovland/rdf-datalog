use rml::sources::csv::CsvSource;
use std::path::PathBuf;

fn write_temp_csv(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("rml_csv_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
//#[ignore]
fn csv_source_reads_header_and_rows() {
    let path = write_temp_csv("basic.csv", "id,name\n1,Alice\n2,Bob\n");
    let source = CsvSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], "1");
    assert_eq!(rows[0]["name"], "Alice");
    assert_eq!(rows[1]["id"], "2");
    assert_eq!(rows[1]["name"], "Bob");
}

#[test]
//#[ignore]
fn csv_source_empty_file_yields_no_rows() {
    let path = write_temp_csv("empty.csv", "id,name\n");
    let source = CsvSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 0);
}

#[test]
//#[ignore]
fn csv_source_empty_cell_is_empty_string_not_absent() {
    let path = write_temp_csv("empty_cell.csv", "id,name\n1,\n");
    let source = CsvSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 1);
    // Empty cell must be present as "" not absent from the map
    assert!(rows[0].contains_key("name"));
    assert_eq!(rows[0]["name"], "");
}

#[test]
//#[ignore]
fn csv_source_missing_file_yields_error() {
    let path = PathBuf::from("/tmp/rml_csv_tests/does_not_exist_xyz.csv");
    let source = CsvSource::new(path);
    let result: Result<Vec<_>, _> = source.rows().collect();
    assert!(result.is_err());
}

#[test]
//#[ignore]
fn csv_source_semicolon_delimiter() {
    let path = write_temp_csv("semicolon.csv", "id;name\n1;Alice\n");
    let source = CsvSource::new(path).with_delimiter(b';');
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "1");
    assert_eq!(rows[0]["name"], "Alice");
}

#[test]
//#[ignore]
fn csv_source_quoted_field_with_comma() {
    let path = write_temp_csv("quoted.csv", "id,name\n1,\"Smith, Alice\"\n");
    let source = CsvSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows[0]["name"], "Smith, Alice");
}

#[test]
//#[ignore]
fn csv_source_single_column() {
    let path = write_temp_csv("single.csv", "name\nAlice\nBob\n");
    let source = CsvSource::new(path);
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "Alice");
    assert_eq!(rows[1]["name"], "Bob");
}

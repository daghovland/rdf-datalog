/// `ottr::load_ottr_file` / `ottr::parse_ottr_str`: dispatch between stOTTR
/// text syntax and wOTTR (RDF/Turtle) so CLI/kernel/HTTP call sites don't
/// need format-specific logic. https://github.com/daghovland/rdf-datalog/issues/246
use dag_rdf::Datastore;
use ottr::{expand_documents, load_ottr_file, parse_ottr_str};
use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn load_ottr_file_dispatches_ttl_extension_to_wottr() {
    let doc = load_ottr_file(&fixtures_dir().join("wottr").join("no_params.ttl")).unwrap();
    assert_eq!(doc.templates.len(), 1);
    assert_eq!(doc.instances.len(), 1);
}

#[test]
fn load_ottr_file_dispatches_stottr_extension_to_stottr() {
    let doc = load_ottr_file(&fixtures_dir().join("combined.stottr")).unwrap();
    assert!(!doc.templates.is_empty());
}

#[test]
fn parse_ottr_str_detects_stottr_text() {
    let text = r#"
@prefix ex: <http://example.com/> .

ex:Person(<http://example.com/Alice>) .
"#;
    let doc = parse_ottr_str(text).unwrap();
    assert_eq!(doc.instances.len(), 1);
}

#[test]
fn parse_ottr_str_falls_back_to_wottr_turtle() {
    let text =
        std::fs::read_to_string(fixtures_dir().join("wottr").join("untyped_params.ttl")).unwrap();
    let doc = parse_ottr_str(&text).unwrap();
    assert_eq!(doc.templates.len(), 1);

    let mut ds = Datastore::new(100);
    expand_documents(&[doc], &mut ds).unwrap();
}

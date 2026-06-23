pub mod ast;
pub mod engine;
pub mod loader;
pub mod optimizer;
pub mod plan;
pub mod sources;
pub mod template;
pub mod translate;

use std::path::Path;

use dag_rdf::Datastore;

#[derive(Debug, thiserror::Error)]
pub enum RmlError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Mapping parse error: {0}")]
    MappingParse(String),
    #[error("CSV error in {file}: {source}")]
    Csv {
        file: std::path::PathBuf,
        source: csv::Error,
    },
    #[error("Missing required property {property} on {subject}")]
    MissingProperty { subject: String, property: String },
    #[error("JSON parse error in {file}: {source}")]
    Json {
        file: std::path::PathBuf,
        source: serde_json::Error,
    },
    #[error("XML parse error in {file}: {source}")]
    Xml {
        file: std::path::PathBuf,
        source: sxd_document::parser::Error,
    },
}

pub fn apply_rml_mapping(
    mapping_path: &Path,
    base_dir: &Path,
    datastore: &mut Datastore,
) -> Result<(), RmlError> {
    eprintln!("apply_rml_mapping: about to load_mapping({:?})", mapping_path);
    let mapping = loader::load_mapping(mapping_path).inspect_err(|e| eprintln!("load_mapping failed: {e}"))?;
    eprintln!("apply_rml_mapping: loaded {} triples maps", mapping.triples_maps.len());
    let plans = translate::translate(&mapping);
    eprintln!("apply_rml_mapping: translated {} plans", plans.len());
    let plans = optimizer::constant_fold(plans);
    eprintln!("apply_rml_mapping: about to engine::execute with base_dir={:?}", base_dir);
    engine::execute(&plans, base_dir, datastore).inspect_err(|e| eprintln!("engine::execute failed: {e}"))
}

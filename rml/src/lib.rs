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
    MissingProperty {
        subject: String,
        property: String,
    },
}

pub fn apply_rml_mapping(
    _mapping_path: &Path,
    _base_dir: &Path,
    _datastore: &mut Datastore,
) -> Result<(), RmlError> {
    todo!()
}

pub mod ast;
pub mod engine;
pub mod loader;
pub mod optimizer;
pub mod plan;
pub mod sandbox;
pub mod sources;
pub mod template;
pub mod translate;

use std::path::Path;

use dag_rdf::Datastore;

/// Maximum bytes read from any single RML source file. See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
pub const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

/// Maximum rows yielded from any single RML source. See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
pub const MAX_SOURCE_ROWS: usize = 1_000_000;

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
    /// Returned when rml:source resolves to a path outside the mapping's base directory.
    #[error("Path traversal rejected: {path} escapes base directory {base}")]
    PathTraversal {
        path: std::path::PathBuf,
        base: std::path::PathBuf,
    },
    /// Source file or row count exceeds the configured limit.
    /// See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
    #[error("source too large: limit {limit} bytes/rows, got {actual}")]
    SourceTooLarge { limit: u64, actual: u64 },
    /// Iterator or reference expression is structurally unsafe (e.g. exponential XPath).
    /// See [#88](https://github.com/daghovland/rdf-datalog/issues/88).
    #[error("unsafe expression rejected: {0}")]
    UnsafeExpression(String),
}

pub fn apply_rml_mapping(
    mapping_path: &Path,
    base_dir: &Path,
    datastore: &mut Datastore,
) -> Result<(), RmlError> {
    let mapping = loader::load_mapping(mapping_path)?;
    let plans = translate::translate(&mapping);
    let plans = optimizer::constant_fold(plans);
    engine::execute(&plans, base_dir, datastore)
}

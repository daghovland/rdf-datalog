pub mod ast;
pub mod engine;
pub mod functions;
pub mod loader;
pub mod optimizer;
pub mod plan;
pub mod sandbox;
pub mod sources;
pub mod template;
pub mod translate;

use std::fmt;
use std::path::Path;

use dag_rdf::Datastore;

/// Maximum bytes read from any single RML source file. See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
pub const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

/// Maximum rows yielded from any single RML source. See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
pub const MAX_SOURCE_ROWS: usize = 1_000_000;

/// Returns only the file name component of a path for use in user-facing messages.
/// See [#90](https://github.com/daghovland/rdf-datalog/issues/90).
fn path_file_name(p: &Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("<file>")
}

#[derive(Debug)]
pub enum RmlError {
    Io(std::io::Error),
    MappingParse(String),
    Csv {
        file: std::path::PathBuf,
        source: csv::Error,
    },
    MissingProperty {
        subject: String,
        property: String,
    },
    Json {
        file: std::path::PathBuf,
        source: serde_json::Error,
    },
    Xml {
        file: std::path::PathBuf,
        source: sxd_document::parser::Error,
    },
    /// Returned when rml:source resolves to a path outside the mapping's base directory.
    PathTraversal {
        path: std::path::PathBuf,
        base: std::path::PathBuf,
    },
    /// Source file or row count exceeds the configured limit.
    /// See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
    SourceTooLarge {
        limit: u64,
        actual: u64,
    },
    /// Iterator or reference expression is structurally unsafe (e.g. exponential XPath).
    /// See [#88](https://github.com/daghovland/rdf-datalog/issues/88).
    UnsafeExpression(String),
    /// An `fno:executes` function IRI that isn't in the built-in FNML
    /// registry. See `docs/plans/RML_FNML_PLAN.md` and
    /// [#27](https://github.com/daghovland/rdf-datalog/issues/27).
    UnknownFunction(String),
}

impl fmt::Display for RmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RmlError::Io(e) => write!(f, "IO error: {e}"),
            RmlError::MappingParse(s) => write!(f, "Mapping parse error: {s}"),
            // Show only the filename component — full path is logged server-side.
            // See [#90](https://github.com/daghovland/rdf-datalog/issues/90).
            RmlError::Csv { file, source } => {
                write!(f, "CSV error in '{}': {source}", path_file_name(file))
            }
            RmlError::MissingProperty { subject, property } => {
                write!(f, "Missing required property {property} on {subject}")
            }
            // Show only the filename component — full path is logged server-side.
            // See [#90](https://github.com/daghovland/rdf-datalog/issues/90).
            RmlError::Json { file, source } => {
                write!(
                    f,
                    "JSON parse error in '{}': {source}",
                    path_file_name(file)
                )
            }
            // Show only the filename component — full path is logged server-side.
            // See [#90](https://github.com/daghovland/rdf-datalog/issues/90).
            RmlError::Xml { file, source } => {
                write!(f, "XML parse error in '{}': {source}", path_file_name(file))
            }
            RmlError::PathTraversal { path, base } => write!(
                f,
                "Path traversal rejected: {} escapes base directory {}",
                path.display(),
                base.display()
            ),
            RmlError::SourceTooLarge { limit, actual } => {
                write!(
                    f,
                    "source too large: limit {limit} bytes/rows, got {actual}"
                )
            }
            RmlError::UnsafeExpression(s) => write!(f, "unsafe expression rejected: {s}"),
            RmlError::UnknownFunction(iri) => {
                write!(f, "unknown FNML function: {iri}")
            }
        }
    }
}

impl std::error::Error for RmlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RmlError::Io(e) => Some(e),
            RmlError::Csv { source, .. } => Some(source),
            RmlError::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for RmlError {
    fn from(e: std::io::Error) -> Self {
        RmlError::Io(e)
    }
}

pub fn apply_rml_mapping(
    mapping_path: &Path,
    base_dir: &Path,
    datastore: &mut Datastore,
) -> Result<(), RmlError> {
    let mapping = loader::load_mapping(mapping_path)?;
    // Validate all logical source paths upfront — even mappings with no
    // predicate-object maps (which generate no execution plans) must have
    // their sources confined to base_dir.
    // See [#84](https://github.com/daghovland/rdf-datalog/issues/84).
    validate_mapping_sources(&mapping, base_dir)?;
    let plans = translate::translate(&mapping)?;
    let plans = optimizer::constant_fold(plans);
    engine::execute(&plans, base_dir, datastore)
}

/// Validate that every logical source path in `mapping` is confined to `base_dir`.
fn validate_mapping_sources(
    mapping: &ast::MappingDocument,
    base_dir: &Path,
) -> Result<(), RmlError> {
    use crate::ast::LogicalSourceRef;
    use crate::sandbox::confine_path;

    for tm in &mapping.triples_maps {
        let LogicalSourceRef::File(rel_path) = &tm.logical_source.source;
        confine_path(base_dir, rel_path)?;
    }
    Ok(())
}

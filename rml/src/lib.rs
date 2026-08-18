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

#[derive(Debug, thiserror::Error)]
pub enum RmlError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Mapping parse error: {0}")]
    MappingParse(String),
    // Show only the filename component — full path is logged server-side.
    // See [#90](https://github.com/daghovland/rdf-datalog/issues/90).
    #[error("CSV error in '{}': {source}", path_file_name(.file))]
    Csv {
        file: std::path::PathBuf,
        source: csv::Error,
    },
    #[error("Missing required property {property} on {subject}")]
    MissingProperty { subject: String, property: String },
    // Show only the filename component — full path is logged server-side.
    // See [#90](https://github.com/daghovland/rdf-datalog/issues/90).
    #[error("JSON parse error in '{}': {source}", path_file_name(.file))]
    Json {
        file: std::path::PathBuf,
        source: serde_json::Error,
    },
    // Show only the filename component — full path is logged server-side.
    // See [#90](https://github.com/daghovland/rdf-datalog/issues/90).
    //
    // Field is deliberately not named `source`: thiserror auto-detects a
    // field literally named `source` as the `Error::source()` value, but
    // this variant has never returned `Some` from `source()` (only
    // Io/Csv/Json/Sql/Postgres do) — see the `impl Error` note below.
    #[error("XML parse error in '{}': {xml_error}", path_file_name(.file))]
    Xml {
        file: std::path::PathBuf,
        xml_error: sxd_document::parser::Error,
    },
    /// Returned when rml:source resolves to a path outside the mapping's base directory.
    #[error(
        "Path traversal rejected: {} escapes base directory {}",
        .path.display(),
        .base.display()
    )]
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
    /// An `fno:executes` function IRI that isn't in the built-in FNML
    /// registry. See `docs/plans/RML_FNML_PLAN.md` and
    /// [#27](https://github.com/daghovland/rdf-datalog/issues/27).
    #[error("unknown FNML function: {0}")]
    UnknownFunction(String),
    /// A SQL `LogicalSource` error: connection failure, malformed SQL, or a
    /// missing table. See `docs/plans/RML_SQL_PLAN.md` and
    /// [#26](https://github.com/daghovland/rdf-datalog/issues/26).
    #[error("SQL error ({context}): {source}")]
    Sql {
        /// The table name or SQL query text that failed, for diagnostics
        /// (not necessarily a filename — held as a plain string, unlike the
        /// `Csv`/`Json`/`Xml` variants' `path_file_name`-truncated paths).
        context: String,
        source: rusqlite::Error,
    },
    /// A PostgreSQL `LogicalSource` error: connection failure or malformed
    /// SQL. Kept as an additive variant alongside `Sql` (rather than boxing
    /// `Sql`'s `source` to a generic error) so existing callers matching on
    /// `RmlError::Sql { source: rusqlite::Error, .. }` are unaffected. See
    /// `docs/plans/RML_SQL_PLAN.md`'s phase 5 and
    /// [#354](https://github.com/daghovland/rdf-datalog/issues/354).
    #[error("PostgreSQL error ({context}): {source}")]
    Postgres {
        context: String,
        source: postgres::Error,
    },
    /// A SQL `LogicalSource`'s `rml:source` was `"${VAR}"` but `VAR` isn't
    /// set in the process environment. Named for the variable, never the
    /// (absent) value — see `docs/plans/RML_SQL_PLAN.md`'s "Credentials".
    #[error("environment variable '{0}' is not set (required by rml:source)")]
    MissingEnvVar(String),
    /// A SQL `LogicalSource`'s `rml:source` looked like a literal database
    /// connection string/DSN (containing a URI scheme like `postgres://`, or
    /// libpq `key=value` credential fields) rather than either a plain
    /// SQLite file path or a `"${VAR}"` environment-variable reference.
    /// Rejected outright — no literal credential is ever accepted from a
    /// mapping file. The property name is reported, never the value, so the
    /// credential itself never reaches an error message or log line. See
    /// `docs/plans/RML_SQL_PLAN.md`'s "Credentials" section and
    /// [#354](https://github.com/daghovland/rdf-datalog/issues/354).
    #[error(
        "rejected {property}: literal database connection strings/credentials are not accepted — use \"${{VAR}}\" to reference an environment variable instead"
    )]
    InsecureSqlSource { property: String },
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
    use crate::ast::{LogicalSourceRef, SqlConnection};
    use crate::sandbox::confine_path;

    for tm in &mapping.triples_maps {
        match &tm.logical_source.source {
            LogicalSourceRef::File(rel_path) => {
                confine_path(base_dir, rel_path)?;
            }
            LogicalSourceRef::Sql(sql_ref) => match &sql_ref.connection {
                SqlConnection::Sqlite(rel_path) => {
                    confine_path(base_dir, rel_path)?;
                }
                // No filesystem path to sandbox-confine — the loader has
                // already validated the env var reference at load time
                // (`loader::resolve_sql_connection`).
                SqlConnection::Postgres(_) => {}
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod rml_error_tests {
    //! Regression tests for `RmlError`'s `Display`/`Error::source()` text and
    //! semantics, preserved exact when converting from a hand-rolled
    //! `impl fmt::Display`/`impl std::error::Error` to `#[derive(thiserror::Error)]`.
    //! See [#495](https://github.com/daghovland/rdf-datalog/issues/495).
    use super::*;
    use std::error::Error as _;

    fn csv_error() -> csv::Error {
        csv::Error::from(std::io::Error::other("boom"))
    }

    fn json_error() -> serde_json::Error {
        serde_json::from_str::<serde_json::Value>("not json").unwrap_err()
    }

    #[test]
    fn io_display_and_source() {
        let io_err = std::io::Error::other("disk full");
        let io_text = io_err.to_string();
        let err = RmlError::from(io_err);
        assert_eq!(err.to_string(), format!("IO error: {io_text}"));
        assert!(err.source().is_some());
    }

    #[test]
    fn csv_display_truncates_filename_and_has_source() {
        let err = RmlError::Csv {
            file: std::path::PathBuf::from("/some/deep/dir/data.csv"),
            source: csv_error(),
        };
        assert!(err.to_string().starts_with("CSV error in 'data.csv': "));
        assert!(err.source().is_some());
    }

    #[test]
    fn json_display_truncates_filename_and_has_source() {
        let err = RmlError::Json {
            file: std::path::PathBuf::from("/some/deep/dir/data.json"),
            source: json_error(),
        };
        assert!(
            err.to_string()
                .starts_with("JSON parse error in 'data.json': ")
        );
        assert!(err.source().is_some());
    }

    #[test]
    fn xml_display_truncates_filename_but_has_no_source() {
        let xml_error = sxd_document::parser::parse("not xml").unwrap_err();
        let err = RmlError::Xml {
            file: std::path::PathBuf::from("/some/deep/dir/data.xml"),
            xml_error,
        };
        assert!(
            err.to_string()
                .starts_with("XML parse error in 'data.xml': ")
        );
        assert!(
            err.source().is_none(),
            "Xml variant has never returned Some from source() — only \
             Io/Csv/Json/Sql/Postgres do"
        );
    }

    #[test]
    fn missing_property_display() {
        let err = RmlError::MissingProperty {
            subject: "ex:TM".to_string(),
            property: "rml:logicalSource".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Missing required property rml:logicalSource on ex:TM"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn path_traversal_display() {
        let err = RmlError::PathTraversal {
            path: std::path::PathBuf::from("/base/../etc/passwd"),
            base: std::path::PathBuf::from("/base"),
        };
        assert_eq!(
            err.to_string(),
            "Path traversal rejected: /base/../etc/passwd escapes base directory /base"
        );
    }

    #[test]
    fn source_too_large_display() {
        let err = RmlError::SourceTooLarge {
            limit: 100,
            actual: 200,
        };
        assert_eq!(
            err.to_string(),
            "source too large: limit 100 bytes/rows, got 200"
        );
    }

    #[test]
    fn unsafe_expression_display() {
        let err = RmlError::UnsafeExpression("//a[//a]".to_string());
        assert_eq!(err.to_string(), "unsafe expression rejected: //a[//a]");
    }

    #[test]
    fn unknown_function_display() {
        let err = RmlError::UnknownFunction("http://example.org/fn".to_string());
        assert_eq!(
            err.to_string(),
            "unknown FNML function: http://example.org/fn"
        );
    }

    #[test]
    fn missing_env_var_display() {
        let err = RmlError::MissingEnvVar("PG_DSN".to_string());
        assert_eq!(
            err.to_string(),
            "environment variable 'PG_DSN' is not set (required by rml:source)"
        );
    }

    #[test]
    fn insecure_sql_source_display_never_echoes_a_value_and_keeps_literal_braces() {
        let err = RmlError::InsecureSqlSource {
            property: "rml:source".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "rejected rml:source: literal database connection strings/credentials are not \
             accepted — use \"${VAR}\" to reference an environment variable instead"
        );
        assert!(err.source().is_none());
    }
}

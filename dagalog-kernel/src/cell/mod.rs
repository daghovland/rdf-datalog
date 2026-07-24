use std::path::{Path, PathBuf};

/// Verify that `untrusted` is a safe, confined relative path.
///
/// Rejects:
/// - absolute paths (e.g. `/etc/passwd`)
/// - paths containing `..` components (e.g. `../../.ssh/id_rsa`)
///
/// A path that passes this check is still relative and may or may not exist on
/// disk — the caller is responsible for opening it. The check is purely
/// structural and does **not** require the file to exist.
///
/// The error message intentionally omits both the input path and the process
/// working directory so that callers cannot use it as an information oracle.
/// See [#85](https://github.com/daghovland/rdf-datalog/issues/85) and
/// [#90](https://github.com/daghovland/rdf-datalog/issues/90).
pub fn check_path_safe(untrusted: &Path) -> Result<(), String> {
    if untrusted.is_absolute() {
        return Err(
            "absolute paths are not allowed in cell magic arguments; use a relative path"
                .to_string(),
        );
    }
    for component in untrusted.components() {
        if component == std::path::Component::ParentDir {
            return Err(
                "path traversal sequences ('..') are not allowed in cell magic arguments"
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// The kind of operation a notebook cell requests.
#[derive(Debug, Clone, PartialEq)]
pub enum CellType {
    /// A cell with no (or only whitespace) source — a no-op, matching how a
    /// real Jupyter kernel treats running a blank cell. Without this, an
    /// empty cell falls through to `Sparql("")`, which fails to parse as a
    /// query and reports a confusing "SPARQL parse error" for doing nothing.
    Empty,
    /// Default: treat the whole cell as a SPARQL query/update.
    Sparql(String),
    /// `%%rml <path>` — apply an RML mapping file.
    Rml(PathBuf),
    /// `%%manchester <path>` — load an OWL 2 Manchester Syntax (`.omn`) file:
    /// materialise its ABox as quads and its TBox as immediately-evaluated
    /// Datalog rules. See [`crate::cell::manchester::execute_manchester_file`].
    Manchester(PathBuf),
    /// `%%load <path>` — load a Turtle/TriG/N-Triples file.
    Load(PathBuf),
    /// `%%reason` — run OWL-RL reasoning on the current datastore.
    Reason,
    /// `%%validate <path>` — run SHACL validation with the given shapes file.
    Validate(PathBuf),
    /// `%%datalog\n<rules>` — parse and assert Datalog rules.
    Datalog(String),
    /// `%%turtle\n<triples>` — parse inline Turtle and add to datastore.
    Turtle(String),
    /// `%%ottr <path>` — load a stOTTR file from disk and expand its instances.
    OttrFile(PathBuf),
    /// `%%ottr\n<stottr>` — parse inline stOTTR and expand its instances.
    OttrInline(String),
}

/// Parse a cell string into a `CellType` by inspecting the first line for `%%` magics.
pub fn detect_cell_type(cell: &str) -> CellType {
    if cell.trim().is_empty() {
        return CellType::Empty;
    }
    let trimmed = cell.trim_start();
    if let Some(rest) = trimmed.strip_prefix("%%") {
        let (first_line, remainder) = rest.split_once('\n').unwrap_or((rest, ""));
        let mut parts = first_line.trim().splitn(2, ' ');
        match parts.next().unwrap_or("") {
            "rml" => {
                let path = parts.next().unwrap_or("").trim();
                CellType::Rml(PathBuf::from(path))
            }
            "manchester" => {
                let path = parts.next().unwrap_or("").trim();
                CellType::Manchester(PathBuf::from(path))
            }
            "load" => {
                let path = parts.next().unwrap_or("").trim();
                CellType::Load(PathBuf::from(path))
            }
            "reason" => CellType::Reason,
            "validate" => {
                let path = parts.next().unwrap_or("").trim();
                CellType::Validate(PathBuf::from(path))
            }
            "datalog" => CellType::Datalog(remainder.to_string()),
            "turtle" => CellType::Turtle(remainder.to_string()),
            "ottr" => {
                let path = parts.next().unwrap_or("").trim();
                if path.is_empty() {
                    CellType::OttrInline(remainder.to_string())
                } else {
                    CellType::OttrFile(PathBuf::from(path))
                }
            }
            _ => CellType::Sparql(cell.to_string()),
        }
    } else {
        CellType::Sparql(trimmed.to_string())
    }
}

pub mod datalog;
pub mod manchester;
pub mod ottr;
pub mod rml;
pub mod shacl;
pub mod sparql;
pub mod turtle;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_plain_sparql_cell() {
        let cell = "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10";
        assert_eq!(detect_cell_type(cell), CellType::Sparql(cell.to_string()));
    }

    #[test]
    fn test_empty_cell_is_noop_not_sparql() {
        assert_eq!(detect_cell_type(""), CellType::Empty);
    }

    #[test]
    fn test_whitespace_only_cell_is_noop() {
        assert_eq!(detect_cell_type("   \n  \n"), CellType::Empty);
    }

    #[test]
    fn test_sparql_cell_leading_whitespace() {
        let cell = "  \nSELECT * WHERE { ?s ?p ?o }";
        assert!(matches!(detect_cell_type(cell), CellType::Sparql(_)));
    }

    #[test]
    fn test_manchester_magic() {
        let cell = "%%manchester ontologies/animals.omn";
        assert_eq!(
            detect_cell_type(cell),
            CellType::Manchester(PathBuf::from("ontologies/animals.omn"))
        );
    }

    #[test]
    fn test_manchester_magic_trailing_newline() {
        let cell = "%%manchester animals.omn\n";
        assert_eq!(
            detect_cell_type(cell),
            CellType::Manchester(PathBuf::from("animals.omn"))
        );
    }

    #[test]
    fn test_rml_magic() {
        let cell = "%%rml path/to/mapping.ttl\n";
        assert_eq!(
            detect_cell_type(cell),
            CellType::Rml(PathBuf::from("path/to/mapping.ttl"))
        );
    }

    #[test]
    fn test_load_magic() {
        let cell = "%%load data/people.ttl";
        assert_eq!(
            detect_cell_type(cell),
            CellType::Load(PathBuf::from("data/people.ttl"))
        );
    }

    #[test]
    fn test_reason_magic_no_args() {
        let cell = "%%reason";
        assert_eq!(detect_cell_type(cell), CellType::Reason);
    }

    #[test]
    fn test_reason_magic_trailing_newline() {
        let cell = "%%reason\n";
        assert_eq!(detect_cell_type(cell), CellType::Reason);
    }

    #[test]
    fn test_validate_magic() {
        let cell = "%%validate shapes/person.ttl";
        assert_eq!(
            detect_cell_type(cell),
            CellType::Validate(PathBuf::from("shapes/person.ttl"))
        );
    }

    #[test]
    fn test_datalog_magic_with_body() {
        let cell = "%%datalog\n?x a owl:Thing :- ?x a ex:Person .\n";
        match detect_cell_type(cell) {
            CellType::Datalog(body) => {
                assert!(body.contains("owl:Thing"));
            }
            other => panic!("expected Datalog, got {:?}", other),
        }
    }

    #[test]
    fn test_turtle_magic_with_body() {
        let cell = "%%turtle\n<http://example.com/Alice> a <http://example.com/Person> .\n";
        match detect_cell_type(cell) {
            CellType::Turtle(body) => {
                assert!(body.contains("Alice"));
            }
            other => panic!("expected Turtle, got {:?}", other),
        }
    }

    #[test]
    fn test_unknown_magic_falls_back_to_sparql() {
        let cell = "%%unknown some args\nstuff";
        assert!(matches!(detect_cell_type(cell), CellType::Sparql(_)));
    }

    #[test]
    fn test_ottr_inline_magic() {
        let cell = "%%ottr\n@prefix ex: <http://example.com/> .\n";
        assert!(matches!(detect_cell_type(cell), CellType::OttrInline(_)));
    }

    #[test]
    fn test_ottr_file_magic() {
        let cell = "%%ottr path/to/templates.stottr";
        assert_eq!(
            detect_cell_type(cell),
            CellType::OttrFile(PathBuf::from("path/to/templates.stottr"))
        );
    }

    #[test]
    fn test_ottr_file_magic_with_trailing_newline() {
        let cell = "%%ottr templates/person.stottr\n";
        assert_eq!(
            detect_cell_type(cell),
            CellType::OttrFile(PathBuf::from("templates/person.stottr"))
        );
    }
}

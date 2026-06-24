use std::path::PathBuf;

/// The kind of operation a notebook cell requests.
#[derive(Debug, Clone, PartialEq)]
pub enum CellType {
    /// Default: treat the whole cell as a SPARQL query/update.
    Sparql(String),
    /// `%%rml <path>` — apply an RML mapping file.
    Rml(PathBuf),
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
}

/// Parse a cell string into a `CellType` by inspecting the first line for `%%` magics.
pub fn detect_cell_type(cell: &str) -> CellType {
    let trimmed = cell.trim_start();
    if let Some(rest) = trimmed.strip_prefix("%%") {
        let (first_line, remainder) = rest.split_once('\n').unwrap_or((rest, ""));
        let mut parts = first_line.trim().splitn(2, ' ');
        match parts.next().unwrap_or("") {
            "rml" => {
                let path = parts.next().unwrap_or("").trim();
                CellType::Rml(PathBuf::from(path))
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
            _ => CellType::Sparql(cell.to_string()),
        }
    } else {
        CellType::Sparql(trimmed.to_string())
    }
}

pub mod datalog;
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
    fn test_sparql_cell_leading_whitespace() {
        let cell = "  \nSELECT * WHERE { ?s ?p ?o }";
        assert!(matches!(detect_cell_type(cell), CellType::Sparql(_)));
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
}

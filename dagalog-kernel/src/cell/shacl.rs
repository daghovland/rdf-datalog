use dag_rdf::Datastore;
use std::path::Path;

/// Validate the session datastore against a SHACL shapes file.
/// Returns a human-readable status string.
pub fn execute_validate(ds: &Datastore, shapes_path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(shapes_path)
        .map_err(|e| format!("cannot open {}: {}", shapes_path.display(), e))?;
    let mut shapes_store = Datastore::new(4096);
    turtle::parse_turtle(&mut shapes_store, std::io::BufReader::new(file))
        .map_err(|e| format!("Turtle parse error in shapes file: {}", e))?;
    let report =
        shacl::validate(ds, &shapes_store).map_err(|e| format!("SHACL validation error: {}", e))?;
    if report.conforms {
        Ok("Conforms. 0 violation(s).".to_string())
    } else {
        Ok(format!("{} violation(s).", report.results.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_turtle_file(ds: &mut Datastore, path: &str) {
        let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("open {path}: {e}"));
        turtle::parse_turtle(ds, std::io::BufReader::new(file))
            .unwrap_or_else(|e| panic!("parse {path}: {e}"));
    }

    #[test]
    fn test_validate_conforms() {
        let mut ds = Datastore::new(1_000);
        load_turtle_file(
            &mut ds,
            "../tests/testdata/shacl_s2_target_subjects_data.ttl",
        );
        let msg = execute_validate(
            &ds,
            Path::new("../tests/testdata/shacl_s2_target_subjects_shapes.ttl"),
        )
        .unwrap();
        assert_eq!(msg, "Conforms. 0 violation(s).");
    }

    #[test]
    fn test_validate_reports_violations() {
        let mut ds = Datastore::new(1_000);
        load_turtle_file(&mut ds, "../tests/testdata/shacl_s1_intro_data.ttl");
        let msg = execute_validate(
            &ds,
            Path::new("../tests/testdata/shacl_s1_intro_shapes.ttl"),
        )
        .unwrap();
        assert_eq!(msg, "4 violation(s).");
    }

    #[test]
    fn test_validate_missing_shapes_file_returns_error() {
        let ds = Datastore::new(1_000);
        let result = execute_validate(&ds, Path::new("does/not/exist.ttl"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_turtle_shapes_returns_error() {
        let ds = Datastore::new(1_000);
        let tmp = std::env::temp_dir().join("dagalog-kernel-test-invalid-shapes.ttl");
        std::fs::write(&tmp, "this is not valid turtle @@@@").unwrap();
        let result = execute_validate(&ds, &tmp);
        let _ = std::fs::remove_file(&tmp);
        assert!(result.is_err());
    }
}

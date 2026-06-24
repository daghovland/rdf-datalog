use dag_rdf::Datastore;

/// Parse inline Turtle text and add resulting triples to the session datastore.
/// Returns a human-readable status line.
pub fn execute_turtle(ds: &mut Datastore, turtle_src: &str) -> Result<String, String> {
    let before = ds.named_graphs.quad_count;
    turtle::parse_turtle(ds, turtle_src.as_bytes())
        .map_err(|e| format!("Turtle parse error: {}", e))?;
    let added = ds.named_graphs.quad_count - before;
    Ok(format!(
        "Loaded {} triple{}.",
        added,
        if added == 1 { "" } else { "s" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turtle_adds_triples() {
        let mut ds = Datastore::new(1_000);
        let before = ds.named_graphs.quad_count;
        let src = r#"@prefix ex: <http://example.org/> .
ex:Alice a ex:Person ; ex:name "Alice" ."#;
        let msg = execute_turtle(&mut ds, src).unwrap();
        assert!(
            ds.named_graphs.quad_count > before,
            "triples should be added"
        );
        assert!(
            msg.contains("triple"),
            "status should mention triples: {msg}"
        );
    }

    #[test]
    fn test_turtle_invalid_syntax_returns_error() {
        let mut ds = Datastore::new(1_000);
        let result = execute_turtle(&mut ds, "this is not valid turtle @@@@");
        assert!(result.is_err(), "invalid Turtle should return Err");
    }

    #[test]
    fn test_turtle_singular_triple_label() {
        let mut ds = Datastore::new(1_000);
        let src = "@prefix ex: <http://example.org/> . ex:Alice a ex:Person .";
        let msg = execute_turtle(&mut ds, src).unwrap();
        assert!(
            msg.contains("1 triple."),
            "one triple should say '1 triple.': {msg}"
        );
    }
}

use dag_rdf::Datastore;
use sparql_parser::{NetworkPolicy, ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;

/// Execute a SPARQL cell against the session datastore.
/// Returns (mime_type, content) pairs for display_data.
/// SELECT → HTML table + plain count; ASK → plain bool; CONSTRUCT → plain N-Triple-like text.
pub fn execute_sparql(ds: &mut Datastore, code: &str) -> Result<Vec<(String, String)>, String> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) =
        parse_query(code, &mut ctx).map_err(|e| format!("SPARQL parse error: {:?}", e))?;

    match execute(&query, ds, NetworkPolicy::Deny)? {
        QueryResult::Select(result) => {
            let cols: Vec<&str> = result.variables.iter().map(String::as_str).collect();
            let rows: Vec<Vec<String>> = result
                .rows
                .iter()
                .map(|row| {
                    result
                        .variables
                        .iter()
                        .map(|var| row.get(var).map(|el| el.to_string()).unwrap_or_default())
                        .collect()
                })
                .collect();

            let html = crate::output::table::select_results_to_html(&cols, &rows);
            let plain = format!("{} result(s).", result.rows.len());
            Ok(vec![
                ("text/html".to_string(), html),
                ("text/plain".to_string(), plain),
            ])
        }
        QueryResult::Ask(b) => Ok(vec![("text/plain".to_string(), b.to_string())]),
        QueryResult::Construct(triples) => {
            let text: String = triples
                .iter()
                .map(|t| format!("{} {} {} .\n", t.subject, t.predicate, t.object))
                .collect();
            Ok(vec![("text/plain".to_string(), text)])
        }
        QueryResult::Describe(triples) => {
            let text: String = triples
                .iter()
                .map(|t| format!("{} {} {} .\n", t.subject, t.predicate, t.object))
                .collect();
            Ok(vec![("text/plain".to_string(), text)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ds_with_alice() -> Datastore {
        let mut ds = Datastore::new(1_000);
        let ttl = r#"@prefix ex: <http://example.org/> .
ex:Alice a ex:Person ; ex:name "Alice" .
ex:Bob   a ex:Person ; ex:name "Bob" ."#;
        turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();
        ds
    }

    #[test]
    fn test_sparql_select_returns_html() {
        let mut ds = ds_with_alice();
        let results = execute_sparql(
            &mut ds,
            "PREFIX ex: <http://example.org/> SELECT ?s WHERE { ?s a ex:Person }",
        )
        .unwrap();
        let html = results
            .iter()
            .find(|(m, _)| m == "text/html")
            .map(|(_, c)| c.as_str())
            .unwrap_or("");
        assert!(html.contains("<table"), "should contain <table");
        assert!(html.contains("Alice") || html.contains("example.org/Alice"));
    }

    #[test]
    fn test_sparql_ask_returns_bool() {
        let mut ds = ds_with_alice();
        let results = execute_sparql(
            &mut ds,
            "PREFIX ex: <http://example.org/> ASK { ex:Alice a ex:Person }",
        )
        .unwrap();
        let text = results
            .iter()
            .find(|(m, _)| m == "text/plain")
            .map(|(_, c)| c.as_str())
            .unwrap_or("");
        assert_eq!(text, "true");
    }

    #[test]
    fn test_sparql_parse_error() {
        let mut ds = ds_with_alice();
        let result = execute_sparql(&mut ds, "not valid sparql @@@@");
        assert!(result.is_err());
    }

    #[test]
    fn test_sparql_empty_result() {
        let mut ds = ds_with_alice();
        let results = execute_sparql(
            &mut ds,
            "PREFIX ex: <http://example.org/> SELECT ?s WHERE { ?s a ex:NonExistent }",
        )
        .unwrap();
        let plain = results
            .iter()
            .find(|(m, _)| m == "text/plain")
            .map(|(_, c)| c.as_str())
            .unwrap_or("");
        assert_eq!(plain, "0 result(s).");
    }
}

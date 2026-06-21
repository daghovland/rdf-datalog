/// Render SPARQL SELECT results as an HTML `<table>`.
///
/// `columns` — variable names in order.
/// `rows` — each row is a parallel slice of string values (empty = unbound).
pub fn select_results_to_html(columns: &[&str], rows: &[Vec<String>]) -> String {
    todo!("select_results_to_html: build <table> with header + rows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_html_table_header() {
        let cols = ["s", "p", "o"];
        let html = select_results_to_html(&cols, &[]);
        assert!(html.contains("<table"), "output must contain <table");
        for col in &cols {
            assert!(html.contains(col), "header must contain variable name '{}'", col);
        }
    }

    #[test]
    #[ignore]
    fn test_html_table_rows() {
        let cols = ["name", "age"];
        let rows = vec![
            vec!["Alice".to_string(), "30".to_string()],
            vec!["Bob".to_string(), "25".to_string()],
        ];
        let html = select_results_to_html(&cols, &rows);
        assert!(html.contains("Alice"));
        assert!(html.contains("Bob"));
        assert!(html.contains("30"));
        assert!(html.contains("25"));
    }

    #[test]
    #[ignore]
    fn test_html_table_empty_results() {
        let cols = ["s", "p", "o"];
        let html = select_results_to_html(&cols, &[]);
        // Should still produce a valid table with header but no data rows
        assert!(html.contains("<table"));
        assert!(html.contains("</table>"));
    }

    #[test]
    #[ignore]
    fn test_html_table_escapes_special_chars() {
        let cols = ["value"];
        let rows = vec![vec!["<script>alert('xss')</script>".to_string()]];
        let html = select_results_to_html(&cols, &rows);
        // Must not contain raw unescaped script tag
        assert!(
            !html.contains("<script>"),
            "HTML must escape < and > in cell values"
        );
    }

    #[test]
    #[ignore]
    fn test_html_table_unbound_value() {
        let cols = ["s", "label"];
        let rows = vec![
            vec!["http://example.com/x".to_string(), String::new()],
        ];
        let html = select_results_to_html(&cols, &rows);
        // Empty string for unbound — just check it doesn't panic and produces valid structure
        assert!(html.contains("<table"));
    }
}

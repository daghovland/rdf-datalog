pub mod csv;
pub mod json;

use std::collections::HashMap;

/// A single row from a non-RDF source: column name → cell value (always String).
/// Empty CSV cells are represented as empty strings, not omitted.
pub type RawRow = HashMap<String, String>;

/// Abstraction over how a single row resolves a reference expression to a string.
///
/// CSV rows look up a column name; JSON rows evaluate a JSONPath expression.
/// Returning `None` signals that a triple should be skipped (absent, null, or empty).
pub trait SourceRow {
    fn get_str(&self, reference: &str) -> Option<String>;
}

/// CSV row — wraps a column-name → cell-value map.
pub struct CsvRow(pub HashMap<String, String>);

impl SourceRow for CsvRow {
    fn get_str(&self, reference: &str) -> Option<String> {
        let v = self.0.get(reference)?;
        if v.is_empty() { None } else { Some(v.clone()) }
    }
}

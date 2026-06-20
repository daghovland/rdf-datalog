pub mod csv;

use std::collections::HashMap;

/// A single row from a non-RDF source: column name → cell value (always String).
/// Empty CSV cells are represented as empty strings, not omitted.
pub type RawRow = HashMap<String, String>;

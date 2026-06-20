use std::path::Path;

use crate::ast::MappingDocument;
use crate::RmlError;

/// Load an RML mapping from a Turtle file on disk.
pub fn load_mapping(_path: &Path) -> Result<MappingDocument, RmlError> {
    todo!()
}

/// Load an RML mapping from a Turtle string (convenience for tests).
pub fn load_mapping_from_str(_turtle: &str) -> Result<MappingDocument, RmlError> {
    todo!()
}

use std::path::Path;

use dag_rdf::Datastore;

use crate::plan::LogicalPlan;
use crate::RmlError;

/// Execute a list of optimised logical plans, inserting generated quads into
/// the datastore. Source file paths are resolved relative to `base_dir`.
/// Each plan is a Volcano-style iterator pipeline: Scan → Projection → Serialize.
pub fn execute(
    _plans: &[LogicalPlan],
    _base_dir: &Path,
    _datastore: &mut Datastore,
) -> Result<(), RmlError> {
    todo!()
}

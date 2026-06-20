use crate::ast::MappingDocument;
use crate::plan::LogicalPlan;

/// Translate a MappingDocument into a flat list of LogicalPlans.
/// Each (TriplesMap × predicate-object pair) and each rml:class shorthand
/// becomes one Projection(Scan(...)) plan. Plans sharing the same source
/// each open the source independently (partitioning optimisation is deferred).
pub fn translate(_mapping: &MappingDocument) -> Vec<LogicalPlan> {
    todo!()
}

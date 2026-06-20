use crate::plan::LogicalPlan;

/// Walk every LogicalProjection's spec list and replace Dynamic(FormatFunction)
/// entries that have no `{...}` column placeholders with Constant(GraphElement).
/// Also converts any Dynamic entry that was translated from TermMap::Constant.
/// Constant predicates (the common case) cost only a clone per row after folding.
pub fn constant_fold(_plans: Vec<LogicalPlan>) -> Vec<LogicalPlan> {
    todo!()
}

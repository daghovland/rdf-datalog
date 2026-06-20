use ingress::{GraphElement, IriReference};

use crate::ast::{LogicalSourceRef, ReferenceFormulation, TermType};

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalPlan {
    Scan(LogicalScan),
    Projection(LogicalProjection),
    Join(LogicalJoin),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalScan {
    pub source: LogicalSourceRef,
    pub reference_formulation: ReferenceFormulation,
    pub iterator: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalProjection {
    pub input: Box<LogicalPlan>,
    /// Spec list A from the paper: [(output attribute, generation logic)]
    /// Always contains Subject, Predicate, Object, and Graph entries (in that order).
    pub attrs: Vec<(OutputAttr, GenerationLogic)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalJoin {
    pub left: Box<LogicalPlan>,
    pub right: Box<LogicalPlan>,
    pub condition: JoinCondition,
    pub algorithm: JoinAlgorithm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinCondition {
    pub left_column: String,
    pub right_column: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinAlgorithm {
    HashJoin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputAttr {
    Subject,
    Predicate,
    Object,
    Graph,
}

/// How to produce a single RDF term, either for every row (Constant)
/// or by evaluating a format function against the current row (Dynamic).
#[derive(Debug, Clone, PartialEq)]
pub enum GenerationLogic {
    /// Pre-evaluated: same GraphElement for every row.
    /// After constant_fold(), all TermMap::Constant and no-placeholder templates
    /// become this variant.
    Constant(GraphElement),
    /// Evaluated per row using the given format function.
    Dynamic(FormatFunction),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormatFunction {
    pub pattern: TermPattern,
    pub term_type: TermType,
    /// rml:language — present only on Literal object maps with a language tag.
    pub language: Option<String>,
    /// rml:datatype — present only on Literal object maps with an explicit datatype.
    pub datatype: Option<IriReference>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TermPattern {
    /// "{column}" placeholder template
    Template(String),
    /// Direct column reference: the whole column value becomes the term lexical form
    Reference(String),
}

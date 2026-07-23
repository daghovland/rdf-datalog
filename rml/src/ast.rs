use std::path::PathBuf;

use ingress::{GraphElement, IriReference};

#[derive(Debug, Clone)]
pub struct MappingDocument {
    pub triples_maps: Vec<TriplesMap>,
}

#[derive(Debug, Clone)]
pub struct TriplesMap {
    pub id: IriReference,
    pub logical_source: LogicalSource,
    pub subject_map: SubjectMap,
    pub predicate_object_maps: Vec<PredicateObjectMap>,
}

#[derive(Debug, Clone)]
pub struct LogicalSource {
    pub source: LogicalSourceRef,
    pub reference_formulation: ReferenceFormulation,
    pub iterator: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalSourceRef {
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceFormulation {
    Csv,
    JsonPath,
    XPath,
}

#[derive(Debug, Clone)]
pub struct SubjectMap {
    pub term_map: TermMap,
    pub term_type: TermType,
    pub classes: Vec<IriReference>,
    pub graph_maps: Vec<GraphMap>,
}

#[derive(Debug, Clone)]
pub struct PredicateObjectMap {
    pub predicate_maps: Vec<(TermMap, TermType)>,
    pub object_maps: Vec<ObjectMap>,
    pub graph_maps: Vec<GraphMap>,
}

#[derive(Debug, Clone)]
pub struct ObjectMap {
    pub term_map: TermMap,
    pub term_type: TermType,
    pub language: Option<String>,
    pub datatype: Option<IriReference>,
    pub parent_triples_map: Option<IriReference>,
    pub join_conditions: Vec<JoinConditionRef>,
}

/// A single `rml:joinCondition` (`rml:child`/`rml:parent`) pair on an `ObjectMap`
/// that has a `rml:parentTriplesMap`. Multiple conditions on the same object map
/// are combined with AND semantics.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinConditionRef {
    pub child: String,
    pub parent: String,
}

#[derive(Debug, Clone)]
pub struct GraphMap {
    pub term_map: TermMap,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TermMap {
    Template(String),
    Constant(GraphElement),
    Reference(String),
    /// `fnml:functionValue [ ... ]` — invoke a named function against
    /// parameter values sourced from ordinary term maps. See
    /// [`docs/plans/RML_FNML_PLAN.md`](../../docs/plans/RML_FNML_PLAN.md) and
    /// [#27](https://github.com/daghovland/rdf-datalog/issues/27).
    FunctionCall(FunctionCall),
}

/// The parsed shape of an `fnml:functionValue [ ... ]` node: which function
/// to invoke (`fno:executes`) and its parameter bindings (every other
/// `rml:predicateObjectMap` inside the function-map node).
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCall {
    /// The `fno:executes` object — the function IRI, e.g. `grel:toUpperCase`.
    pub function_iri: IriReference,
    /// One entry per non-`fno:executes` `rml:predicateObjectMap`.
    pub parameters: Vec<FunctionParameter>,
}

/// A single named-parameter binding on a `FunctionCall`. `value_map` is
/// restricted to `Template`/`Reference`/`Constant` in this pass — nested
/// function composition is deferred, see the plan doc.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionParameter {
    pub param_iri: IriReference,
    pub value_map: TermMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermType {
    Iri,
    BlankNode,
    Literal,
}

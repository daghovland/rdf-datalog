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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermType {
    Iri,
    BlankNode,
    Literal,
}

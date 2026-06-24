use ingress::{IriReference, RdfLiteral};

#[derive(Debug, Clone, PartialEq)]
pub struct StottrDocument {
    pub templates: Vec<TemplateDef>,
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateDef {
    pub id: IriReference,
    pub parameters: Vec<Parameter>,
    pub body: Vec<Instance>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub variable: String,
    pub ottr_type: crate::types::OttrType,
    pub optional: bool,
    pub default: Option<Argument>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub template: IriReference,
    pub arguments: Vec<Argument>,
    pub expander: Option<Expander>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expander {
    Cross,
    ZipMin,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Argument {
    Term(Term),
    List(Vec<Argument>),
    None,
    ListExpand(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Iri(IriReference),
    Variable(String),
    Literal(RdfLiteral),
    BlankNode(String),
}

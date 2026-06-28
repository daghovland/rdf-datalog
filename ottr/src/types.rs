use ingress::IriReference;

#[derive(Debug, Clone, PartialEq)]
pub enum OttrType {
    Iri,
    BlankNode,
    Literal(Option<IriReference>),
    List(Box<OttrType>),
    NEList(Box<OttrType>),
}

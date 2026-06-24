use crate::ast::{Instance, TemplateDef};
use crate::error::OttrError;
use dag_rdf::Datastore;
use ingress::IriReference;
use std::collections::HashMap;

/// Expand a list of top-level instance calls into quads in `datastore`,
/// using `templates` to resolve user-defined templates.
pub fn expand(
    templates: &HashMap<IriReference, TemplateDef>,
    instances: &[Instance],
    datastore: &mut Datastore,
) -> Result<(), OttrError> {
    let _ = (templates, instances, datastore);
    todo!()
}

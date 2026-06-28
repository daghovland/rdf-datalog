pub mod ast;
pub mod base_templates;
pub mod error;
pub mod expander;
pub mod parser;
pub mod types;

use ast::StottrDocument;
use dag_rdf::Datastore;

pub use error::OttrError;
pub use expander::expand;
pub use parser::parse_stottr;

/// Read a stOTTR file from disk and parse it.
pub fn load_stottr_file(path: &std::path::Path) -> Result<ast::StottrDocument, OttrError> {
    let text = std::fs::read_to_string(path)?;
    parse_stottr(&text)
}

/// Merge multiple parsed documents (e.g. a templates file + an instances
/// file), then expand all instances into `datastore`.
pub fn expand_documents(
    docs: &[StottrDocument],
    datastore: &mut Datastore,
) -> Result<(), OttrError> {
    let mut templates = std::collections::HashMap::new();
    let mut instances = Vec::new();
    for doc in docs {
        for template in &doc.templates {
            templates.insert(template.id.clone(), template.clone());
        }
        instances.extend(doc.instances.iter().cloned());
    }
    expand(&templates, &instances, datastore)
}

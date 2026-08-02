pub mod ast;
pub mod base_templates;
pub mod error;
pub mod expander;
pub mod parser;
pub mod types;
pub mod wottr;

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

/// Load an OTTR file from disk, dispatching between stOTTR text syntax and
/// wOTTR (RDF/Turtle) by file extension: `.ttl`/`.turtle`/`.trig` are parsed
/// as wOTTR via [`wottr::parse_wottr_str`]; everything else (including the
/// conventional `.stottr`) is parsed as stOTTR text via [`parse_stottr`].
/// Used by the CLI's `--ottr` flag and the Jupyter kernel's `%%ottr <file>`
/// form. See [#246](https://github.com/daghovland/rdf-datalog/issues/246).
pub fn load_ottr_file(path: &std::path::Path) -> Result<ast::StottrDocument, OttrError> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ttl") | Some("turtle") | Some("trig") => {
            let text = std::fs::read_to_string(path)?;
            wottr::parse_wottr_str(&text)
        }
        _ => load_stottr_file(path),
    }
}

/// Parse OTTR content of unknown format (no filename or `Content-Type` to
/// dispatch on, e.g. an inline `%%ottr` Jupyter cell): try stOTTR text syntax
/// first, and fall back to wOTTR (Turtle) if that fails. stOTTR's `[...] ::
/// {...}` template syntax and bare `name(args)` instance calls are not valid
/// Turtle, so a real wOTTR document reliably fails the stOTTR parse (only
/// its leading `@prefix` declarations are shared syntax) and falls through
/// to the wOTTR attempt.
pub fn parse_ottr_str(text: &str) -> Result<ast::StottrDocument, OttrError> {
    match parse_stottr(text) {
        Ok(doc) => Ok(doc),
        Err(stottr_err) => wottr::parse_wottr_str(text).map_err(|wottr_err| {
            OttrError::Parse(format!(
                "not valid stOTTR ({stottr_err}) or wOTTR turtle ({wottr_err})"
            ))
        }),
    }
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

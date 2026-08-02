//! wOTTR: the RDF/Turtle serialisation of OTTR templates and instances.
//!
//! Spec: <https://spec.ottr.xyz/wOTTR/0.4.5/>. This module is a second front
//! end alongside [`crate::parser::parse_stottr`]: it reads `ottr:Template`/
//! `ottr:Instance`-shaped triples out of a [`Datastore`] and builds the same
//! [`StottrDocument`] that the stOTTR text parser builds, so
//! [`crate::expander::expand`]/[`crate::expand_documents`] need no changes.
//!
//! See `docs/plans/WOTTR_PLAN.md` for the full vocabulary-to-AST mapping.
//! Tracked by [#246](https://github.com/daghovland/rdf-datalog/issues/246).

use crate::OttrError;
use crate::ast::StottrDocument;
use dag_rdf::Datastore;

/// Read templates and instances out of `datastore`, per the wOTTR vocabulary,
/// and build a [`StottrDocument`] equivalent to what
/// [`crate::parser::parse_stottr`] would build from the corresponding stOTTR
/// text.
pub fn parse_wottr(datastore: &Datastore) -> Result<StottrDocument, OttrError> {
    let _ = datastore;
    todo!("implemented incrementally per docs/plans/WOTTR_PLAN.md, issue #246")
}

/// Convenience wrapper: parse `text` as Turtle into a fresh [`Datastore`],
/// then run [`parse_wottr`] over it. Mainly used by tests and as the shape
/// later CLI/HTTP/Jupyter wiring (content-type dispatch) would use.
pub fn parse_wottr_str(text: &str) -> Result<StottrDocument, OttrError> {
    let mut datastore = Datastore::new(100);
    turtle::parse_turtle(&mut datastore, text.as_bytes())
        .map_err(|e| OttrError::Parse(format!("wOTTR turtle parse error: {e}")))?;
    parse_wottr(&datastore)
}

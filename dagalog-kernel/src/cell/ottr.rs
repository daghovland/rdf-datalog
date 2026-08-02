use dag_rdf::Datastore;
use std::path::Path;

/// Expand an inline `%%ottr` cell. The content's format (stOTTR text vs.
/// wOTTR Turtle) is auto-detected via [`ottr::parse_ottr_str`], since an
/// inline cell has no filename or `Content-Type` to dispatch on. See
/// [#246](https://github.com/daghovland/rdf-datalog/issues/246).
pub fn execute_ottr_inline(ds: &mut Datastore, src: &str) -> Result<String, String> {
    let doc = ottr::parse_ottr_str(src).map_err(|e| format!("OTTR parse error: {e}"))?;
    let before = ds.named_graphs.quad_count;
    ottr::expand_documents(&[doc], ds).map_err(|e| format!("OTTR expansion error: {e}"))?;
    let added = ds.named_graphs.quad_count - before;
    Ok(format!(
        "Expanded {} triple{}.",
        added,
        if added == 1 { "" } else { "s" }
    ))
}

/// Expand `%%ottr <path>`. The file's format (stOTTR vs. wOTTR) is dispatched
/// by extension via [`ottr::load_ottr_file`].
pub fn execute_ottr_file(ds: &mut Datastore, path: &Path) -> Result<String, String> {
    let doc =
        ottr::load_ottr_file(path).map_err(|e| format!("cannot load {}: {e}", path.display()))?;
    let before = ds.named_graphs.quad_count;
    ottr::expand_documents(&[doc], ds).map_err(|e| format!("OTTR expansion error: {e}"))?;
    let added = ds.named_graphs.quad_count - before;
    Ok(format!(
        "Expanded {} triple{}.",
        added,
        if added == 1 { "" } else { "s" }
    ))
}

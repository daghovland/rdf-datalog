use dag_rdf::Datastore;
use std::path::Path;

pub fn execute_ottr_inline(ds: &mut Datastore, src: &str) -> Result<String, String> {
    let doc = ottr::parse_stottr(src).map_err(|e| format!("stOTTR parse error: {e}"))?;
    let before = ds.named_graphs.quad_count;
    ottr::expand_documents(&[doc], ds).map_err(|e| format!("OTTR expansion error: {e}"))?;
    let added = ds.named_graphs.quad_count - before;
    Ok(format!(
        "Expanded {} triple{}.",
        added,
        if added == 1 { "" } else { "s" }
    ))
}

pub fn execute_ottr_file(ds: &mut Datastore, path: &Path) -> Result<String, String> {
    let doc =
        ottr::load_stottr_file(path).map_err(|e| format!("cannot load {}: {e}", path.display()))?;
    let before = ds.named_graphs.quad_count;
    ottr::expand_documents(&[doc], ds).map_err(|e| format!("OTTR expansion error: {e}"))?;
    let added = ds.named_graphs.quad_count - before;
    Ok(format!(
        "Expanded {} triple{}.",
        added,
        if added == 1 { "" } else { "s" }
    ))
}

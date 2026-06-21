use dag_rdf::Datastore;
use std::path::Path;

/// Apply an RML mapping file to the session datastore.
/// Returns a human-readable status string.
pub fn execute_rml(ds: &mut Datastore, mapping_path: &Path) -> Result<String, String> {
    let base_dir = mapping_path.parent().unwrap_or(Path::new("."));
    let before = ds.named_graphs.quad_count;
    rml::apply_rml_mapping(mapping_path, base_dir, ds).map_err(|e| format!("RML error: {}", e))?;
    let added = ds.named_graphs.quad_count - before;
    Ok(format!(
        "Loaded {} triple{}.",
        added,
        if added == 1 { "" } else { "s" }
    ))
}

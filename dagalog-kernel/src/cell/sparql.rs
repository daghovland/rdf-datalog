use dag_rdf::Datastore;

/// Execute a SPARQL cell against the session datastore.
/// Returns (mime_type, content) pairs for display_data.
pub fn execute_sparql(_ds: &mut Datastore, _code: &str) -> Result<Vec<(String, String)>, String> {
    todo!("execute_sparql: parse and run SPARQL SELECT/CONSTRUCT/UPDATE/ASK")
}

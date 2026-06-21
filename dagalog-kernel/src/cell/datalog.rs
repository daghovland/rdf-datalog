use dag_rdf::Datastore;

/// Parse and materialise Datalog rules written inline in a cell.
pub fn execute_datalog(_ds: &mut Datastore, _rules_src: &str) -> Result<String, String> {
    todo!("execute_datalog: parse rules and call datalog::evaluate_rules")
}

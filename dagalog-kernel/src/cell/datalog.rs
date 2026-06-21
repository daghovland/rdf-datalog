use dag_rdf::Datastore;

/// Parse and materialise Datalog rules written inline in a cell.
pub fn execute_datalog(ds: &mut Datastore, rules_src: &str) -> Result<String, String> {
    let rules =
        datalog_parser::parse(rules_src, ds).map_err(|e| format!("Datalog parse error: {}", e))?;
    let count = rules.len();
    datalog::evaluate_rules(rules, ds);
    Ok(format!(
        "Applied {} rule{}.",
        count,
        if count == 1 { "" } else { "s" }
    ))
}

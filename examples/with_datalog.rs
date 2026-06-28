// Example: define custom Datalog rules and query derived facts.
//
// Run with:  cargo run --example with_datalog
//
// Demonstrates that after applying Datalog rules, new facts are derived:
// if A worksFor Org and B worksFor Org, then A and B are colleagues.

use dag_rdf::Datastore;
use dagalog::{load_file, run_sparql_query};
use std::path::Path;

fn main() {
    let data_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/people.ttl");

    let mut ds = Datastore::new(1024);
    load_file(&mut ds, &data_path).expect("failed to load people.ttl");

    // Define a Datalog rule: if X and Y both work for the same organisation,
    // they are colleagues.
    let rules_src = "
        prefix ex: <http://example.org/>
        ex:colleague[?x, ?y] :-
            ex:worksFor[?x, ?org],
            ex:worksFor[?y, ?org] .
    ";
    let rules = datalog_parser::parse(rules_src, &mut ds).expect("failed to parse rules");
    datalog::evaluate_rules(rules, &mut ds);

    let sparql = "
        PREFIX ex: <http://example.org/>
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?nameA ?nameB WHERE {
            ?a ex:colleague ?b .
            ?a foaf:name ?nameA .
            ?b foaf:name ?nameB .
            FILTER (?a != ?b)
        }
        ORDER BY ?nameA ?nameB
    ";
    let result = run_sparql_query(&ds, sparql).expect("query failed");
    println!("Colleague pairs (derived by Datalog):");
    for row in &result.rows {
        let a = row
            .get("nameA")
            .map(dagalog::graph_element_display)
            .unwrap_or_default();
        let b = row
            .get("nameB")
            .map(dagalog::graph_element_display)
            .unwrap_or_default();
        println!("  {a} — {b}");
    }
    println!("{} pair(s)", result.rows.len());
}

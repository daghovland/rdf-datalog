// Example: load a Turtle file and run a SPARQL SELECT query.
//
// Run with:  cargo run --example load_and_query
//
// This is the simplest entry point: point dagalog at a .ttl file and query it.

use dag_rdf::Datastore;
use dagalog::{load_file, run_sparql_query};
use std::path::Path;

fn main() {
    let data_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/people.ttl");

    let mut ds = Datastore::new(1024);
    load_file(&mut ds, &data_path).expect("failed to load people.ttl");

    let sparql = "
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        PREFIX ex:   <http://example.org/>
        SELECT ?person ?name WHERE {
            ?person a foaf:Person ;
                    foaf:name ?name .
        }
        ORDER BY ?name
    ";

    let result = run_sparql_query(&ds, sparql).expect("query failed");
    println!("People in the dataset:");
    for row in &result.rows {
        let name = row
            .get("name")
            .map(dagalog::graph_element_display)
            .unwrap_or_default();
        let iri = row
            .get("person")
            .map(dagalog::graph_element_display)
            .unwrap_or_default();
        println!("  {name} — {iri}");
    }
    println!("{} result(s)", result.rows.len());
}

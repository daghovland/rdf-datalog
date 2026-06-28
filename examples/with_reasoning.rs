// Example: load an OWL ontology, run OWL-RL reasoning, query inferred facts.
//
// Run with:  cargo run --example with_reasoning
//
// Demonstrates that after OWL-RL materialisation, a resource asserted only as
// ex:Employee is also inferred to be a foaf:Person (via rdfs:subClassOf).

use dag_rdf::Datastore;
use dagalog::{load_file, run_owlrl_reasoning, run_sparql_query};
use std::path::Path;

fn main() {
    let data_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/employees.ttl");

    let mut ds = Datastore::new(1024);
    load_file(&mut ds, &data_path).expect("failed to load employees.ttl");

    // Before reasoning: Carol is only an ex:Employee, not yet a foaf:Person
    let before = count_persons(&ds);
    println!("Before OWL-RL reasoning: {before} foaf:Person instance(s)");

    // Run OWL-RL materialisation (propagates rdfs:subClassOf, owl:equivalentClass, etc.)
    run_owlrl_reasoning(&mut ds);

    // After reasoning: Carol is also inferred to be a foaf:Person
    let after = count_persons(&ds);
    println!("After  OWL-RL reasoning: {after} foaf:Person instance(s)");

    let result = run_sparql_query(
        &ds,
        "PREFIX foaf: <http://xmlns.com/foaf/0.1/>
         SELECT ?person ?name WHERE {
             ?person a foaf:Person ; foaf:name ?name .
         } ORDER BY ?name",
    )
    .unwrap();
    for row in &result.rows {
        let name = row
            .get("name")
            .map(dagalog::graph_element_display)
            .unwrap_or_default();
        println!("  {name} (inferred foaf:Person)");
    }
}

fn count_persons(ds: &Datastore) -> usize {
    run_sparql_query(
        ds,
        "PREFIX foaf: <http://xmlns.com/foaf/0.1/> SELECT ?p WHERE { ?p a foaf:Person }",
    )
    .map(|r| r.rows.len())
    .unwrap_or(0)
}

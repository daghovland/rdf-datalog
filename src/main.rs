

use dag_rdf::{Datastore};
use owl_ontology::Ontology;
use ingress;
use owl2rl2datalog::owl2datalog;
use turtle_parser::parse_turtle;
use std::env;
use std::fs::File;
use std::io::BufReader;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <turtle_file>", args[0]);
        return;
    }

    let filename = &args[1];
    let file = File::open(filename).expect("Failed to open file");
    let reader = BufReader::new(file);

    let mut datastore = Datastore::new(100_000);
    if let Err(e) = parse_turtle(&mut datastore, reader) {
        eprintln!("Error parsing Turtle: {:?}", e);
        return;
    }

    println!("Parsed {} triples.", datastore.named_graphs.quad_count);

    // Now we need an Ontology object. 
    // Usually, we'd have an RDF-to-Ontology translator.
    // In DagSemTools, there's a translator that builds the Ontology from the Datastore.
    // For now, let's just create an empty ontology to see if it links up.
    let ontology = Ontology::new(vec![], ingress::OntologyVersion::UnNamedOntology, vec![], vec![]);
    let rules = owl2datalog(&mut datastore.resources, &ontology);

    println!("Generated {} Datalog rules.", rules.len());
    for rule in rules {
        println!("{}", rule);
    }
}

use dag_rdf::Datastore;
use owl2rl2datalog::owl2datalog;
use rdf_owl_translator::rdf2owl;
use std::env;
use std::fs::File;
use std::io::BufReader;
use turtle_parser::parse_turtle;

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

    // Translate RDF triples into an OWL Ontology
    let ontology_doc = rdf2owl(&mut datastore);
    let ontology = &ontology_doc.ontology;
    println!("Extracted {} OWL axioms.", ontology.axioms.len());

    // Generate Datalog rules from the OWL ontology
    let rules = owl2datalog(&mut datastore.resources, ontology);
    println!("Generated {} Datalog rules.", rules.len());
    for rule in &rules {
        println!("{}", rule);
    }
}

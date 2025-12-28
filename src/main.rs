

use ingress::{IriReference, RdfResource};

fn main() {
    let res = RdfResource::Iri(IriReference("http://example.org".to_string()));
    println!("Resource: {}", res);
    println!("Hello, from datalog!");
}

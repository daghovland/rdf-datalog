

use ingress::{IriReference, RdfResource};



fn main() {
    let res = RdfResource::Iri(IriReference("http://example.org".to_string()));
    println!("Resource: {}", res);
    println!("Hello, from datalog!");
    let mut ds = dag_rdf::GraphElementManager::new(1000000);
    let el_id = ds.add_node_resource(RdfResource::Iri(IriReference("http://example.org/test".to_string())));
    let el = ds.get_resource(el_id).unwrap();
    println!("{:?}", el);
}

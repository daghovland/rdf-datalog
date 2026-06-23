fn main() {
    let cwd = std::env::current_dir().unwrap();
    println!("cwd = {:?}", cwd);
    let mapping_path = std::path::Path::new("tests/testdata/rml_persons_mapping.ttl");
    println!("exists: {}", mapping_path.exists());
    let mut ds = dag_rdf::Datastore::new(1000);
    let base_dir = mapping_path.parent().unwrap();
    match rml::apply_rml_mapping(mapping_path, base_dir, &mut ds) {
        Ok(()) => println!("OK, quads = {}", ds.named_graphs.quad_count),
        Err(e) => println!("ERR: {}", e),
    }
}

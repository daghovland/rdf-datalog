use dag_rdf::Datastore;

/// Serialize all triples in the default graph of `ds` as a Turtle string.
#[allow(dead_code)]
pub fn datastore_to_turtle(_ds: &Datastore) -> String {
    todo!("datastore_to_turtle: iterate quads and emit Turtle syntax")
}

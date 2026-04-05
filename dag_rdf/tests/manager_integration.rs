use dag_rdf::{GraphElementManager, RdfResource, IriReference};

#[test]
fn test_blank_node_lifecycle() {
    let mut manager = GraphElementManager::new(100);
    
    // Create named blank nodes
    let id1 = manager.get_or_create_named_anon_resource("b1".to_string());
    let id2 = manager.get_or_create_named_anon_resource("b1".to_string());
    
    assert_eq!(id1, id2, "Named blank nodes with same name should have same ID");
    
    // Reset the map
    manager.reset_blank_nodes_map();
    
    let id3 = manager.get_or_create_named_anon_resource("b1".to_string());
    assert_ne!(id1, id3, "After reset, same name should get a new ID");
}

#[test]
fn test_iri_filtering() {
    let mut manager = GraphElementManager::new(100);
    
    manager.add_node_resource(RdfResource::Iri(IriReference("http://a.com".to_string())));
    manager.create_unnamed_anon_resource();
    manager.add_node_resource(RdfResource::Iri(IriReference("http://b.com".to_string())));
    
    let iris = manager.get_iri_resource_ids();
    // 2 explicitly added IRIs + 1 for the default graph pre-populated at ID 0
    assert_eq!(iris.len(), 3, "Should find 2 user IRI resources plus the default graph IRI");
}
use rml::ast::{ReferenceFormulation, TermMap, TermType};
use rml::loader::load_mapping_from_str;

const SIMPLE_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TriplesMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;

#[test]
//#[ignore]
fn loader_finds_one_triples_map() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    assert_eq!(doc.triples_maps.len(), 1);
}

#[test]
//#[ignore]
fn loader_sets_csv_reference_formulation() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.reference_formulation,
        ReferenceFormulation::Csv
    );
}

#[test]
//#[ignore]
fn loader_parses_template_subject_map() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let tm = &doc.triples_maps[0];
    assert_eq!(
        tm.subject_map.term_map,
        TermMap::Template("http://example.com/Student/{id}".to_string())
    );
    assert_eq!(tm.subject_map.term_type, TermType::Iri);
}

#[test]
//#[ignore]
fn loader_parses_constant_predicate_via_shorthand() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let pom = &doc.triples_maps[0].predicate_object_maps[0];
    // rml:predicate is shorthand for a constant-IRI predicateMap
    let (pred_map, pred_type) = &pom.predicate_maps[0];
    assert!(matches!(pred_map, TermMap::Constant(_)));
    assert_eq!(*pred_type, TermType::Iri);
}

#[test]
//#[ignore]
fn loader_parses_reference_object_map() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let pom = &doc.triples_maps[0].predicate_object_maps[0];
    let obj = &pom.object_maps[0];
    assert_eq!(obj.term_map, TermMap::Reference("name".to_string()));
    // rml:reference in an objectMap defaults to Literal (CSV values are plain strings)
    assert_eq!(obj.term_type, TermType::Literal);
}

#[test]
//#[ignore]
fn loader_parses_class_shorthand() {
    let mapping = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Person/{id}" ;
        rml:class ex:Person
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;
    let doc = load_mapping_from_str(mapping).unwrap();
    let sm = &doc.triples_maps[0].subject_map;
    assert_eq!(sm.classes.len(), 1);
    assert_eq!(sm.classes[0].0, "http://example.com/Person");
}

#[test]
//#[ignore]
fn loader_parses_language_on_object_map() {
    let mapping = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [ rml:constant <http://example.com/S> ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [
            rml:reference "name" ;
            rml:language "en"
        ]
    ] .
"#;
    let doc = load_mapping_from_str(mapping).unwrap();
    let obj = &doc.triples_maps[0].predicate_object_maps[0].object_maps[0];
    assert_eq!(obj.language.as_deref(), Some("en"));
    // rml:language implies Literal term type
    assert_eq!(obj.term_type, TermType::Literal);
}

#[test]
//#[ignore]
fn loader_parses_datatype_on_object_map() {
    let mapping = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [ rml:constant <http://example.com/S> ] ;
    rml:predicateObjectMap [
        rml:predicate ex:age ;
        rml:objectMap [
            rml:reference "age" ;
            rml:datatype xsd:integer
        ]
    ] .
"#;
    let doc = load_mapping_from_str(mapping).unwrap();
    let obj = &doc.triples_maps[0].predicate_object_maps[0].object_maps[0];
    assert_eq!(
        obj.datatype.as_ref().map(|i| i.0.as_str()),
        Some("http://www.w3.org/2001/XMLSchema#integer")
    );
    assert_eq!(obj.term_type, TermType::Literal);
}

#[test]
//#[ignore]
fn loader_parses_graph_map() {
    let mapping = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/S/{id}" ;
        rml:graphMap [ rml:constant <http://example.com/MyGraph> ]
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;
    let doc = load_mapping_from_str(mapping).unwrap();
    let sm = &doc.triples_maps[0].subject_map;
    assert_eq!(sm.graph_maps.len(), 1);
}

#[test]
//#[ignore]
fn loader_parses_blank_node_term_type() {
    let mapping = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "{id}" ;
        rml:termType rml:BlankNode
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;
    let doc = load_mapping_from_str(mapping).unwrap();
    assert_eq!(
        doc.triples_maps[0].subject_map.term_type,
        TermType::BlankNode
    );
}

#[test]
//#[ignore]
fn loader_resolves_source_path_string() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let source = &doc.triples_maps[0].logical_source.source;
    // rml:source "data.csv" → File(PathBuf::from("data.csv"))
    use rml::ast::LogicalSourceRef;
    assert!(matches!(source, LogicalSourceRef::File(p) if p.to_str() == Some("data.csv")));
}

#[test]
//#[ignore]
fn loader_returns_error_on_invalid_turtle() {
    let result = load_mapping_from_str("this is not valid turtle @@@");
    assert!(result.is_err());
}

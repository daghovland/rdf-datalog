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
fn loader_finds_one_triples_map() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    assert_eq!(doc.triples_maps.len(), 1);
}

#[test]
fn loader_sets_csv_reference_formulation() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.reference_formulation,
        ReferenceFormulation::Csv
    );
}

#[test]
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
fn loader_parses_constant_predicate_via_shorthand() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let pom = &doc.triples_maps[0].predicate_object_maps[0];
    // rml:predicate is shorthand for a constant-IRI predicateMap
    let (pred_map, pred_type) = &pom.predicate_maps[0];
    assert!(matches!(pred_map, TermMap::Constant(_)));
    assert_eq!(*pred_type, TermType::Iri);
}

#[test]
fn loader_parses_reference_object_map() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let pom = &doc.triples_maps[0].predicate_object_maps[0];
    let obj = &pom.object_maps[0];
    assert_eq!(obj.term_map, TermMap::Reference("name".to_string()));
    // rml:reference in an objectMap defaults to Literal (CSV values are plain strings)
    assert_eq!(obj.term_type, TermType::Literal);
}

#[test]
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
fn loader_resolves_source_path_string() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let source = &doc.triples_maps[0].logical_source.source;
    // rml:source "data.csv" → File(PathBuf::from("data.csv"))
    use rml::ast::LogicalSourceRef;
    assert!(matches!(source, LogicalSourceRef::File(p) if p.to_str() == Some("data.csv")));
}

#[test]
fn loader_returns_error_on_invalid_turtle() {
    let result = load_mapping_from_str("this is not valid turtle @@@");
    assert!(result.is_err());
}

// ── JSON reference formulation ────────────────────────────────────────────────

const JSON_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.json" ;
        rml:referenceFormulation rml:JSONPath ;
        rml:iterator "$.students[*]"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{$.id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "$.name" ]
    ] .
"#;

const QL_JSON_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ql:  <http://semweb.mmlab.be/ns/ql#> .
@prefix ex:  <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.json" ;
        rml:referenceFormulation ql:JSONPath
    ] ;
    rml:subjectMap [ rml:constant <http://example.com/S> ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "$.name" ]
    ] .
"#;

#[test]
fn loader_parses_json_reference_formulation() {
    let doc = load_mapping_from_str(JSON_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.reference_formulation,
        ReferenceFormulation::JsonPath
    );
}

#[test]
fn loader_parses_ql_jsonpath_alias() {
    // Dimou-lab ql:JSONPath is treated as an alias for rml:JSONPath.
    let doc = load_mapping_from_str(QL_JSON_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.reference_formulation,
        ReferenceFormulation::JsonPath
    );
}

#[test]
fn loader_parses_iterator_string() {
    let doc = load_mapping_from_str(JSON_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.iterator.as_deref(),
        Some("$.students[*]")
    );
}

#[test]
fn loader_parses_jsonpath_reference() {
    // rml:reference "$.name" in a JSON mapping → TermMap::Reference("$.name")
    let doc = load_mapping_from_str(JSON_MAPPING).unwrap();
    let obj = &doc.triples_maps[0].predicate_object_maps[0].object_maps[0];
    assert_eq!(obj.term_map, TermMap::Reference("$.name".to_string()));
}

// ── XPath reference formulation ───────────────────────────────────────────────

const XML_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.xml" ;
        rml:referenceFormulation rml:XPath ;
        rml:iterator "/students/student"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{@id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;

const QL_XML_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ql:  <http://semweb.mmlab.be/ns/ql#> .
@prefix ex:  <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.xml" ;
        rml:referenceFormulation ql:XPath
    ] ;
    rml:subjectMap [ rml:constant <http://example.com/S> ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;

#[test]
fn loader_parses_xpath_reference_formulation() {
    let doc = load_mapping_from_str(XML_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.reference_formulation,
        ReferenceFormulation::XPath
    );
}

#[test]
fn loader_parses_ql_xpath_alias() {
    // Dimou-lab ql:XPath is treated as an alias for rml:XPath.
    let doc = load_mapping_from_str(QL_XML_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.reference_formulation,
        ReferenceFormulation::XPath
    );
}

#[test]
fn loader_parses_xml_iterator() {
    let doc = load_mapping_from_str(XML_MAPPING).unwrap();
    assert_eq!(
        doc.triples_maps[0].logical_source.iterator.as_deref(),
        Some("/students/student")
    );
}

#[test]
fn loader_parses_xpath_reference() {
    // rml:reference "name" in an XML mapping → TermMap::Reference("name")
    let doc = load_mapping_from_str(XML_MAPPING).unwrap();
    let obj = &doc.triples_maps[0].predicate_object_maps[0].object_maps[0];
    assert_eq!(obj.term_map, TermMap::Reference("name".to_string()));
}

// ── rml:joinCondition (red phase; see docs/plans/RML_JOIN_PLAN.md) ───────────

const JOIN_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/SportMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "sport.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/sport/{ID}"
    ] .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "student.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/student/{ID}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:practises ;
        rml:objectMap [
            rml:parentTriplesMap <http://example.com/SportMap> ;
            rml:joinCondition [
                rml:child "Sport" ;
                rml:parent "ID"
            ]
        ]
    ] .
"#;

const MULTI_JOIN_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/SportMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "sport.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/sport/{ID}"
    ] .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "student.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/student/{ID}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:practises ;
        rml:objectMap [
            rml:parentTriplesMap <http://example.com/SportMap> ;
            rml:joinCondition [ rml:child "Sport" ; rml:parent "ID" ] ;
            rml:joinCondition [ rml:child "Year" ; rml:parent "Year" ]
        ]
    ] .
"#;

#[test]
fn loader_parses_join_condition_child_and_parent() {
    let doc = load_mapping_from_str(JOIN_MAPPING).unwrap();
    let student_map = doc
        .triples_maps
        .iter()
        .find(|tm| tm.id.0 == "http://example.com/StudentMap")
        .unwrap();
    let obj = &student_map.predicate_object_maps[0].object_maps[0];
    assert_eq!(
        obj.parent_triples_map.as_ref().map(|i| i.0.as_str()),
        Some("http://example.com/SportMap")
    );
    assert_eq!(obj.join_conditions.len(), 1);
    assert_eq!(obj.join_conditions[0].child, "Sport");
    assert_eq!(obj.join_conditions[0].parent, "ID");
}

#[test]
fn loader_parses_multiple_join_conditions_as_and_semantics() {
    let doc = load_mapping_from_str(MULTI_JOIN_MAPPING).unwrap();
    let student_map = doc
        .triples_maps
        .iter()
        .find(|tm| tm.id.0 == "http://example.com/StudentMap")
        .unwrap();
    let obj = &student_map.predicate_object_maps[0].object_maps[0];
    assert_eq!(obj.join_conditions.len(), 2);
}

#[test]
fn loader_object_map_without_join_condition_has_empty_vec() {
    let doc = load_mapping_from_str(SIMPLE_MAPPING).unwrap();
    let obj = &doc.triples_maps[0].predicate_object_maps[0].object_maps[0];
    assert!(obj.join_conditions.is_empty());
}

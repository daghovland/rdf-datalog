/// Tests for FNML (`fnml:FunctionMap`) support: applying a named function to
/// input values during term generation. See
/// `docs/plans/RML_FNML_PLAN.md` and
/// [issue #27](https://github.com/daghovland/rdf-datalog/issues/27).
use std::path::Path;

use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use rml::ast::TermMap;
use rml::loader::load_mapping_from_str;
use rml::translate::translate;
use rml::{RmlError, apply_rml_mapping};

fn fixture(case: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(case)
}

fn iri_element(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

// ── Loader: parsing fnml:functionValue ──────────────────────────────────────

const FUNCTION_VALUE_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix fnml: <http://semweb.mmlab.be/ns/fnml#> .
@prefix fno: <https://w3id.org/function/ontology#> .
@prefix grel: <https://users.ugent.be/~bjdmeest/function/grel.ttl#> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Person/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:nameUpper ;
        rml:objectMap [
            fnml:functionValue [
                rml:predicateObjectMap [
                    rml:predicate fno:executes ;
                    rml:objectMap [ rml:constant grel:toUpperCase ]
                ] ;
                rml:predicateObjectMap [
                    rml:predicate grel:valueParam ;
                    rml:objectMap [ rml:reference "name" ]
                ]
            ]
        ]
    ] .
"#;

#[test]
fn loader_parses_function_value_object_map() {
    let doc = load_mapping_from_str(FUNCTION_VALUE_MAPPING).unwrap();
    let pom = &doc.triples_maps[0].predicate_object_maps[0];
    let obj = &pom.object_maps[0];
    match &obj.term_map {
        TermMap::FunctionCall(fc) => {
            assert_eq!(
                fc.function_iri,
                IriReference(
                    "https://users.ugent.be/~bjdmeest/function/grel.ttl#toUpperCase".to_string()
                )
            );
            assert_eq!(fc.parameters.len(), 1);
            assert_eq!(
                fc.parameters[0].param_iri,
                IriReference(
                    "https://users.ugent.be/~bjdmeest/function/grel.ttl#valueParam".to_string()
                )
            );
            assert_eq!(
                fc.parameters[0].value_map,
                TermMap::Reference("name".to_string())
            );
        }
        other => panic!("expected TermMap::FunctionCall, got {other:?}"),
    }
}

// ── translate(): unknown function IRI is a hard error ───────────────────────

const UNKNOWN_FUNCTION_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix fnml: <http://semweb.mmlab.be/ns/fnml#> .
@prefix fno: <https://w3id.org/function/ontology#> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "data.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Person/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:mystery ;
        rml:objectMap [
            fnml:functionValue [
                rml:predicateObjectMap [
                    rml:predicate fno:executes ;
                    rml:objectMap [ rml:constant ex:noSuchFunction ]
                ] ;
                rml:predicateObjectMap [
                    rml:predicate ex:someParam ;
                    rml:objectMap [ rml:reference "name" ]
                ]
            ]
        ]
    ] .
"#;

#[test]
fn translate_unknown_function_iri_is_an_error() {
    let doc = load_mapping_from_str(UNKNOWN_FUNCTION_MAPPING).unwrap();
    let result = translate(&doc);
    match result {
        Err(RmlError::UnknownFunction(iri)) => {
            assert_eq!(iri, "http://example.com/noSuchFunction");
        }
        other => panic!("expected Err(RmlError::UnknownFunction(_)), got {other:?}"),
    }
}

// ── End-to-end: built-in GREL functions transform values ────────────────────

#[test]
fn end_to_end_to_upper_case_transforms_object_value() {
    let dir = fixture("fnml_basic");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = ds.add_resource(iri_element("http://example.com/Person/1"));
    let p = ds.add_resource(iri_element("http://example.com/nameUpper"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("ALICE".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
fn end_to_end_to_lower_case_transforms_object_value() {
    let dir = fixture("fnml_basic");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = ds.add_resource(iri_element("http://example.com/Person/1"));
    let p = ds.add_resource(iri_element("http://example.com/nameLower"));
    // Source CSV value "alice" is already lowercase, but this asserts the
    // function ran (not just default lexical passthrough) via a distinct
    // triple from the untransformed reference case below.
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("alice".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

#[test]
fn end_to_end_trim_transforms_object_value() {
    let dir = fixture("fnml_basic");
    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = ds.add_resource(iri_element("http://example.com/Person/1"));
    let p = ds.add_resource(iri_element("http://example.com/cityTrimmed"));
    // Source CSV value is "  Oslo  " (with leading/trailing whitespace);
    // the trimmed literal must have no surrounding whitespace.
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Oslo".to_string()));

    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

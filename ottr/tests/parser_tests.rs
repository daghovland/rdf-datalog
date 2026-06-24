use ingress::{IriReference, RDF_TYPE, XSD_STRING};
use ottr::ast::{Argument, Term};
use ottr::parser::parse_stottr;
use ottr::types::OttrType;

const SINGLE_PARAM: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person ] :: {
  ottr:Triple (?person, rdf:type, foaf:Person)
} .
"#;

#[test]
#[ignore]
fn parses_signature_with_single_typed_parameter() {
    let doc = parse_stottr(SINGLE_PARAM).unwrap();
    assert_eq!(doc.templates.len(), 1);
    let tmpl = &doc.templates[0];
    assert_eq!(
        tmpl.id,
        IriReference("http://example.com/Person".to_string())
    );
    assert_eq!(tmpl.parameters.len(), 1);
    assert_eq!(tmpl.parameters[0].variable, "person");
    assert_eq!(tmpl.parameters[0].ottr_type, OttrType::Iri);
}

const MIXED_PARAMS: &str = r#"
@prefix ex: <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Person [ ottr:IRI ?person, xsd:string ?name ] :: {
  ottr:Triple (?person, rdf:type, foaf:Person),
  ottr:Triple (?person, foaf:name, ?name)
} .
"#;

#[test]
#[ignore]
fn parses_signature_with_multiple_mixed_type_parameters() {
    let doc = parse_stottr(MIXED_PARAMS).unwrap();
    let tmpl = &doc.templates[0];
    assert_eq!(tmpl.parameters.len(), 2);
    assert_eq!(tmpl.parameters[0].variable, "person");
    assert_eq!(tmpl.parameters[0].ottr_type, OttrType::Iri);
    assert_eq!(tmpl.parameters[1].variable, "name");
    assert_eq!(
        tmpl.parameters[1].ottr_type,
        OttrType::Literal(Some(IriReference(XSD_STRING.to_string())))
    );
}

#[test]
#[ignore]
fn parses_body_with_single_ottr_triple_instance() {
    let doc = parse_stottr(SINGLE_PARAM).unwrap();
    let tmpl = &doc.templates[0];
    assert_eq!(tmpl.body.len(), 1);
    let instance = &tmpl.body[0];
    assert_eq!(
        instance.template,
        IriReference("http://ns.ottr.xyz/0.4/Triple".to_string())
    );
    assert_eq!(
        instance.arguments,
        vec![
            Argument::Term(Term::Variable("person".to_string())),
            Argument::Term(Term::Iri(IriReference(RDF_TYPE.to_string()))),
            Argument::Term(Term::Iri(IriReference(
                "http://xmlns.com/foaf/0.1/Person".to_string()
            ))),
        ]
    );
}

#[test]
#[ignore]
fn parses_body_with_multiple_instances() {
    let doc = parse_stottr(MIXED_PARAMS).unwrap();
    let tmpl = &doc.templates[0];
    assert_eq!(tmpl.body.len(), 2);
    assert_eq!(
        tmpl.body[1].arguments,
        vec![
            Argument::Term(Term::Variable("person".to_string())),
            Argument::Term(Term::Iri(IriReference(
                "http://xmlns.com/foaf/0.1/name".to_string()
            ))),
            Argument::Term(Term::Variable("name".to_string())),
        ]
    );
}

#[test]
#[ignore]
fn resolves_prefixed_names_into_full_iris() {
    let doc = parse_stottr(SINGLE_PARAM).unwrap();
    let tmpl = &doc.templates[0];
    assert_eq!(
        tmpl.id,
        IriReference("http://example.com/Person".to_string())
    );
    assert_eq!(
        tmpl.body[0].arguments[2],
        Argument::Term(Term::Iri(IriReference(
            "http://xmlns.com/foaf/0.1/Person".to_string()
        )))
    );
}

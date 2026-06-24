use ingress::{IriReference, RdfLiteral, XSD_INTEGER};
use ottr::ast::{Argument, Term};
use ottr::parser::parse_stottr;

const IRI_ARGS: &str = r#"
@prefix ex: <http://example.com/> .

ex:Person(<http://example.com/Alice>, <http://example.com/Acme>) .
"#;

#[test]
#[ignore]
fn parses_single_instance_with_iri_arguments() {
    let doc = parse_stottr(IRI_ARGS).unwrap();
    assert_eq!(doc.instances.len(), 1);
    let instance = &doc.instances[0];
    assert_eq!(
        instance.template,
        IriReference("http://example.com/Person".to_string())
    );
    assert_eq!(
        instance.arguments,
        vec![
            Argument::Term(Term::Iri(IriReference(
                "http://example.com/Alice".to_string()
            ))),
            Argument::Term(Term::Iri(IriReference(
                "http://example.com/Acme".to_string()
            ))),
        ]
    );
}

const LITERAL_ARGS: &str = r#"
@prefix ex: <http://example.com/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

ex:Person(<http://example.com/Alice>, "Alice", "42"^^xsd:integer, "hello"@en) .
"#;

#[test]
#[ignore]
fn parses_literal_arguments_plain_typed_and_lang_tagged() {
    let doc = parse_stottr(LITERAL_ARGS).unwrap();
    let instance = &doc.instances[0];
    assert_eq!(
        instance.arguments[1],
        Argument::Term(Term::Literal(RdfLiteral::LiteralString(
            "Alice".to_string()
        )))
    );
    assert_eq!(
        instance.arguments[2],
        Argument::Term(Term::Literal(RdfLiteral::TypedLiteral {
            type_iri: IriReference(XSD_INTEGER.to_string()),
            literal: "42".to_string(),
        }))
    );
    assert_eq!(
        instance.arguments[3],
        Argument::Term(Term::Literal(RdfLiteral::LangLiteral {
            lang: "en".to_string(),
            literal: "hello".to_string(),
        }))
    );
}

const BLANK_NODE_ARG: &str = r#"
@prefix ex: <http://example.com/> .

ex:Person(_:b1, "Alice") .
"#;

#[test]
#[ignore]
fn parses_blank_node_argument() {
    let doc = parse_stottr(BLANK_NODE_ARG).unwrap();
    let instance = &doc.instances[0];
    assert_eq!(
        instance.arguments[0],
        Argument::Term(Term::BlankNode("b1".to_string()))
    );
}

const MULTIPLE_INSTANCES: &str = r#"
@prefix ex: <http://example.com/> .

ex:Person(<http://example.com/Alice>, "Alice") .
ex:Person(<http://example.com/Bob>, "Bob") .
"#;

#[test]
#[ignore]
fn parses_multiple_instances_in_one_file() {
    let doc = parse_stottr(MULTIPLE_INSTANCES).unwrap();
    assert_eq!(doc.instances.len(), 2);
    assert_eq!(
        doc.instances[1].arguments[1],
        Argument::Term(Term::Literal(RdfLiteral::LiteralString("Bob".to_string())))
    );
}

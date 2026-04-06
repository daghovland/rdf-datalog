use dag_rdf::{Datastore, IriReference, RdfLiteral, RdfResource, Triple};
use rio_api::model::{Literal, Subject, Term as RioTerm};
use rio_api::parser::TriplesParser;
use rio_turtle::{TurtleError, TurtleParser};
use std::io::BufRead;

pub fn parse_turtle<R: BufRead>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleError> {
    let mut parser = TurtleParser::new(reader, None);
    parser.parse_all(&mut |rio_triple| {
        let subject = match rio_triple.subject {
            Subject::NamedNode(node) => {
                datastore.add_node_resource(RdfResource::Iri(IriReference(node.iri.to_owned())))
            }
            Subject::BlankNode(node) => datastore
                .resources
                .get_or_create_named_anon_resource(node.id.to_owned()),
            _ => return Ok(()), // Rio might have other types in future?
        };

        let predicate = datastore.add_node_resource(RdfResource::Iri(IriReference(
            rio_triple.predicate.iri.to_owned(),
        )));

        let object = match rio_triple.object {
            RioTerm::NamedNode(node) => {
                datastore.add_node_resource(RdfResource::Iri(IriReference(node.iri.to_owned())))
            }
            RioTerm::BlankNode(node) => datastore
                .resources
                .get_or_create_named_anon_resource(node.id.to_owned()),
            RioTerm::Literal(lit) => {
                let rdf_lit = match lit {
                    Literal::Simple { value } => RdfLiteral::LiteralString(value.to_owned()),
                    Literal::LanguageTaggedString { value, language } => RdfLiteral::LangLiteral {
                        literal: value.to_owned(),
                        lang: language.to_owned(),
                    },
                    Literal::Typed { value, datatype } => RdfLiteral::TypedLiteral {
                        literal: value.to_owned(),
                        type_iri: IriReference(datatype.iri.to_owned()),
                    },
                };
                datastore.add_literal_resource(rdf_lit)
            }
            _ => return Ok(()),
        };

        datastore.add_triple(Triple {
            subject,
            predicate,
            obj: object,
        });
        Ok(())
    })
}

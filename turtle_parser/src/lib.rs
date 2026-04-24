use dag_rdf::{Datastore, IriReference, RdfLiteral, RdfResource, Triple};
use rio_api::model::{GraphName, Literal, Subject, Term as RioTerm};
use rio_api::parser::{QuadsParser, TriplesParser};
use rio_turtle::{TriGParser, TurtleError, TurtleParser};
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
            _ => return Ok(()),
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
                let rdf_lit = convert_literal(lit);
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

/// Parse a TriG file, loading triples from named graphs and the default graph.
///
/// Triples without a graph name go to the default graph. Triples with a named
/// graph IRI are stored as named-graph quads via `add_named_graph_triple`.
pub fn parse_trig<R: BufRead>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleError> {
    let mut parser = TriGParser::new(reader, None);
    parser.parse_all(&mut |rio_quad| {
        let subject = match rio_quad.subject {
            Subject::NamedNode(node) => {
                datastore.add_node_resource(RdfResource::Iri(IriReference(node.iri.to_owned())))
            }
            Subject::BlankNode(node) => datastore
                .resources
                .get_or_create_named_anon_resource(node.id.to_owned()),
            _ => return Ok(()),
        };

        let predicate = datastore.add_node_resource(RdfResource::Iri(IriReference(
            rio_quad.predicate.iri.to_owned(),
        )));

        let object = match rio_quad.object {
            RioTerm::NamedNode(node) => {
                datastore.add_node_resource(RdfResource::Iri(IriReference(node.iri.to_owned())))
            }
            RioTerm::BlankNode(node) => datastore
                .resources
                .get_or_create_named_anon_resource(node.id.to_owned()),
            RioTerm::Literal(lit) => {
                let rdf_lit = convert_literal(lit);
                datastore.add_literal_resource(rdf_lit)
            }
            _ => return Ok(()),
        };

        let triple = Triple {
            subject,
            predicate,
            obj: object,
        };

        match rio_quad.graph_name {
            None => {
                datastore.add_triple(triple);
            }
            Some(GraphName::NamedNode(node)) => {
                let graph_id = datastore
                    .add_node_resource(RdfResource::Iri(IriReference(node.iri.to_owned())));
                datastore.add_named_graph_triple(graph_id, triple);
            }
            Some(GraphName::BlankNode(node)) => {
                let graph_id = datastore
                    .resources
                    .get_or_create_named_anon_resource(node.id.to_owned());
                datastore.add_named_graph_triple(graph_id, triple);
            }
        }

        Ok(())
    })
}

fn convert_literal(lit: Literal<'_>) -> RdfLiteral {
    match lit {
        Literal::Simple { value } => RdfLiteral::LiteralString(value.to_owned()),
        Literal::LanguageTaggedString { value, language } => RdfLiteral::LangLiteral {
            literal: value.to_owned(),
            lang: language.to_owned(),
        },
        Literal::Typed { value, datatype } => RdfLiteral::TypedLiteral {
            literal: value.to_owned(),
            type_iri: IriReference(datatype.iri.to_owned()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;

    #[test]
    fn parse_simple_turtle() {
        let ttl = r#"
            @prefix ex: <http://example.org/> .
            ex:Alice a ex:Person .
            ex:Alice ex:name "Alice" .
        "#;
        let mut ds = Datastore::new(1000);
        parse_turtle(&mut ds, ttl.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    #[test]
    fn parse_trig_default_graph() {
        let trig = r#"
            @prefix ex: <http://example.org/> .
            ex:Alice a ex:Person .
        "#;
        let mut ds = Datastore::new(1000);
        parse_trig(&mut ds, trig.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    #[test]
    fn parse_trig_named_graph() {
        let trig = r#"
            @prefix ex: <http://example.org/> .
            <http://example.org/graph1> {
                ex:Alice a ex:Person .
                ex:Bob a ex:Person .
            }
        "#;
        let mut ds = Datastore::new(1000);
        parse_trig(&mut ds, trig.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    #[test]
    fn parse_trig_mixed_graphs() {
        let trig = r#"
            @prefix ex: <http://example.org/> .
            ex:Alice a ex:Person .
            <http://example.org/graph1> {
                ex:Bob a ex:Employee .
            }
        "#;
        let mut ds = Datastore::new(1000);
        parse_trig(&mut ds, trig.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
    }
}

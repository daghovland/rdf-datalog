/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

pub mod serialize;
pub use serialize::{
    serialize_graph, serialize_nquads, serialize_nquads_graph, serialize_trig, serialize_trig_graph,
};

use dag_rdf::{Datastore, GraphElementId, IriReference, RdfLiteral, RdfResource, Triple};
use oxrdf::{GraphName, Literal, NamedOrBlankNode, Term};
use oxttl::{NQuadsParser, NTriplesParser, TriGParser, TurtleParseError, TurtleParser};
use std::io::Read;

pub fn parse_turtle<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleParseError> {
    for result in TurtleParser::new().for_reader(reader) {
        let triple = result?;
        let subject = intern_subject(datastore, triple.subject);
        let predicate = intern_named_node(datastore, triple.predicate.into_string());
        if let Some(obj) = intern_term(datastore, triple.object) {
            datastore.add_triple(Triple {
                subject,
                predicate,
                obj,
            });
        }
    }
    Ok(())
}

pub fn parse_trig<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleParseError> {
    for result in TriGParser::new().for_reader(reader) {
        let quad = result?;
        let subject = intern_subject(datastore, quad.subject);
        let predicate = intern_named_node(datastore, quad.predicate.into_string());
        let Some(obj) = intern_term(datastore, quad.object) else {
            continue;
        };
        let triple = Triple {
            subject,
            predicate,
            obj,
        };
        match quad.graph_name {
            GraphName::DefaultGraph => datastore.add_triple(triple),
            GraphName::NamedNode(node) => {
                let graph_id = intern_named_node(datastore, node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
            GraphName::BlankNode(node) => {
                let graph_id = datastore
                    .resources
                    .get_or_create_named_anon_resource(node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
        }
    }
    Ok(())
}

pub fn parse_ntriples<R: Read>(
    datastore: &mut Datastore,
    reader: R,
) -> Result<(), TurtleParseError> {
    for result in NTriplesParser::new().for_reader(reader) {
        let triple = result?;
        let subject = intern_subject(datastore, triple.subject);
        let predicate = intern_named_node(datastore, triple.predicate.into_string());
        if let Some(obj) = intern_term(datastore, triple.object) {
            datastore.add_triple(Triple {
                subject,
                predicate,
                obj,
            });
        }
    }
    Ok(())
}

pub fn parse_nquads<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleParseError> {
    for result in NQuadsParser::new().for_reader(reader) {
        let quad = result?;
        let subject = intern_subject(datastore, quad.subject);
        let predicate = intern_named_node(datastore, quad.predicate.into_string());
        let Some(obj) = intern_term(datastore, quad.object) else {
            continue;
        };
        let triple = Triple {
            subject,
            predicate,
            obj,
        };
        match quad.graph_name {
            GraphName::DefaultGraph => datastore.add_triple(triple),
            GraphName::NamedNode(node) => {
                let graph_id = intern_named_node(datastore, node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
            GraphName::BlankNode(node) => {
                let graph_id = datastore
                    .resources
                    .get_or_create_named_anon_resource(node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
        }
    }
    Ok(())
}

fn intern_named_node(datastore: &mut Datastore, iri: String) -> GraphElementId {
    datastore.add_node_resource(RdfResource::Iri(IriReference(iri)))
}

fn intern_subject(datastore: &mut Datastore, subject: NamedOrBlankNode) -> GraphElementId {
    match subject {
        NamedOrBlankNode::NamedNode(node) => intern_named_node(datastore, node.into_string()),
        NamedOrBlankNode::BlankNode(node) => datastore
            .resources
            .get_or_create_named_anon_resource(node.into_string()),
    }
}

fn intern_term(datastore: &mut Datastore, term: Term) -> Option<GraphElementId> {
    match term {
        Term::NamedNode(node) => Some(intern_named_node(datastore, node.into_string())),
        Term::BlankNode(node) => Some(
            datastore
                .resources
                .get_or_create_named_anon_resource(node.into_string()),
        ),
        Term::Literal(lit) => Some(datastore.add_literal_resource(convert_literal(lit))),
    }
}

fn convert_literal(lit: Literal) -> RdfLiteral {
    if let Some(lang) = lit.language() {
        return RdfLiteral::LangLiteral {
            literal: lit.value().to_owned(),
            lang: lang.to_owned(),
        };
    }
    let datatype = lit.datatype().into_owned().into_string();
    if datatype == "http://www.w3.org/2001/XMLSchema#string" {
        RdfLiteral::LiteralString(lit.value().to_owned())
    } else {
        RdfLiteral::TypedLiteral {
            literal: lit.value().to_owned(),
            type_iri: IriReference(datatype),
        }
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

    #[test]
    fn parse_ntriples_basic() {
        let nt = "<http://example.org/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/Person> .\n";
        let mut ds = Datastore::new(1000);
        parse_ntriples(&mut ds, nt.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    #[test]
    fn parse_nquads_default_graph() {
        let nq = "<http://example.org/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/Person> .\n";
        let mut ds = Datastore::new(1000);
        parse_nquads(&mut ds, nq.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    #[test]
    fn parse_nquads_named_graph() {
        let nq = "<http://example.org/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/Person> <http://example.org/g> .\n";
        let mut ds = Datastore::new(1000);
        parse_nquads(&mut ds, nq.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }
}

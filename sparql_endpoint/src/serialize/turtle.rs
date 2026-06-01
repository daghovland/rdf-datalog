/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serialize an RDF graph to the N-Triples line format, served as `text/turtle`.
//!
//! N-Triples is a strict subset of Turtle (one triple per line, all terms
//! fully expanded), so the body is valid Turtle while staying simple to generate.

use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, RdfResource};

/// Serialize all quads with `triple_id == graph_id` to a Turtle string.
pub fn serialize_graph(store: &Datastore, graph_id: GraphElementId) -> String {
    let mut out = String::new();
    for quad in store.named_graphs.get_graph(graph_id) {
        let s = subject_term(store.resources.get_graph_element(quad.subject));
        let p = predicate_term(store.resources.get_graph_element(quad.predicate));
        let o = object_term(store.resources.get_graph_element(quad.obj));
        if let (Some(s), Some(p), Some(o)) = (s, p, o) {
            out.push_str(&s);
            out.push(' ');
            out.push_str(&p);
            out.push(' ');
            out.push_str(&o);
            out.push_str(" .\n");
        }
    }
    out
}

fn subject_term(elem: &GraphElement) -> Option<String> {
    match elem {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            Some(format!("<{}>", escape_iri(&iri.0)))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => Some(format!("_:b{id}")),
        GraphElement::GraphLiteral(_) => None,
    }
}

fn predicate_term(elem: &GraphElement) -> Option<String> {
    match elem {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            Some(format!("<{}>", escape_iri(&iri.0)))
        }
        _ => None,
    }
}

fn object_term(elem: &GraphElement) -> Option<String> {
    match elem {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            Some(format!("<{}>", escape_iri(&iri.0)))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => Some(format!("_:b{id}")),
        GraphElement::GraphLiteral(lit) => Some(format_literal(lit)),
    }
}

fn format_literal(lit: &RdfLiteral) -> String {
    match lit {
        RdfLiteral::LiteralString(s) => format!("\"{}\"", escape_str(s)),
        RdfLiteral::LangLiteral { lang, literal } => {
            format!("\"{}\"@{}", escape_str(literal), lang)
        }
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            format!("\"{}\"^^<{}>", escape_str(literal), escape_iri(&type_iri.0))
        }
        RdfLiteral::BooleanLiteral(b) => {
            format!("\"{}\"^^<http://www.w3.org/2001/XMLSchema#boolean>", b)
        }
        RdfLiteral::IntegerLiteral(i) => {
            format!("\"{}\"^^<http://www.w3.org/2001/XMLSchema#integer>", i)
        }
        RdfLiteral::DecimalLiteral(d) => {
            format!("\"{}\"^^<http://www.w3.org/2001/XMLSchema#decimal>", d)
        }
        RdfLiteral::FloatLiteral(f) => {
            format!("\"{}\"^^<http://www.w3.org/2001/XMLSchema#float>", f)
        }
        RdfLiteral::DoubleLiteral(d) => {
            format!("\"{}\"^^<http://www.w3.org/2001/XMLSchema#double>", d)
        }
        RdfLiteral::DateTimeLiteral(dt) => {
            format!(
                "\"{}\"^^<http://www.w3.org/2001/XMLSchema#dateTime>",
                dt.to_rfc3339()
            )
        }
        RdfLiteral::DateLiteral(d) => {
            format!(
                "\"{}\"^^<http://www.w3.org/2001/XMLSchema#date>",
                d.format("%Y-%m-%d")
            )
        }
        RdfLiteral::TimeLiteral(t) => {
            format!(
                "\"{}\"^^<http://www.w3.org/2001/XMLSchema#time>",
                t.format("%H:%M:%S")
            )
        }
        RdfLiteral::DurationLiteral(dur) => {
            format!("\"{}\"^^<http://www.w3.org/2001/XMLSchema#duration>", dur)
        }
    }
}

/// Escape characters that are not valid unescaped inside a Turtle IRI `<...>`.
///
/// Per the Turtle grammar (IRIREF production), the following code points must
/// be escaped: `\x00–\x20`, `<`, `>`, `"`, `{`, `}`, `|`, `^`, `` ` ``, `\`.
fn escape_iri(iri: &str) -> String {
    let mut out = String::with_capacity(iri.len());
    for c in iri.chars() {
        match c {
            '\x00'..='\x20' | '<' | '>' | '"' | '{' | '}' | '|' | '^' | '`' | '\\' => {
                // Use \uXXXX for BMP code points, \UXXXXXXXX for supplementary.
                if (c as u32) <= 0xFFFF {
                    out.push_str(&format!("\\u{:04X}", c as u32));
                } else {
                    out.push_str(&format!("\\U{:08X}", c as u32));
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Escape characters inside a double-quoted Turtle string literal.
fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::{Datastore, IriReference, RdfResource, Triple};

    #[test]
    fn serialize_empty_default_graph() {
        let ds = Datastore::new(64);
        let out = serialize_graph(&ds, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn serialize_iri_triple() {
        let mut ds = Datastore::new(64);
        let s = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/alice".to_owned(),
        )));
        let p = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://xmlns.com/foaf/0.1/name".to_owned(),
        )));
        let o = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/bob".to_owned(),
        )));
        ds.add_triple(Triple {
            subject: s,
            predicate: p,
            obj: o,
        });
        let out = serialize_graph(&ds, 0);
        assert!(out.contains("<http://example.org/alice>"));
        assert!(out.contains("<http://example.org/bob>"));
        assert!(out.ends_with(".\n"));
    }

    #[test]
    fn serialize_literal_triple() {
        let mut ds = Datastore::new(64);
        let s = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/alice".to_owned(),
        )));
        let p = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://xmlns.com/foaf/0.1/name".to_owned(),
        )));
        let o = ds.add_literal_resource(RdfLiteral::LiteralString("Alice".to_owned()));
        ds.add_triple(Triple {
            subject: s,
            predicate: p,
            obj: o,
        });
        let out = serialize_graph(&ds, 0);
        assert!(out.contains("\"Alice\""), "got: {out}");
    }
}

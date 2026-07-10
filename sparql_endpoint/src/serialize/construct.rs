/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serialize CONSTRUCT query results as N-Triples (a valid subset of Turtle).

use dag_rdf::{GraphElement, RdfLiteral, RdfResource};
use sparql_parser::ResolvedTriple;

/// Serialize a list of CONSTRUCT triples as N-Triples / Turtle.
pub fn serialize_construct_ntriples(triples: &[ResolvedTriple]) -> String {
    let mut out = String::new();
    for triple in triples {
        let s = subject_term(&triple.subject);
        let p = predicate_term(&triple.predicate);
        let o = object_term(&triple.object);
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
        // Literals cannot be subjects; triple terms require RDF 1.2 support (#143).
        GraphElement::GraphLiteral(_) | GraphElement::TripleTerm(_) => None,
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
        // Triple terms as objects require RDF 1.2 Turtle serialisation (#143).
        GraphElement::TripleTerm(_) => None,
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

fn escape_iri(iri: &str) -> String {
    let mut out = String::with_capacity(iri.len());
    for c in iri.chars() {
        match c {
            '\x00'..='\x20' | '<' | '>' | '"' | '{' | '}' | '|' | '^' | '`' | '\\' => {
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

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serializer for `application/sparql-results+xml`
//!
//! Spec: <https://www.w3.org/TR/rdf-sparql-XMLres/>

use dag_rdf::{GraphElement, RdfLiteral, RdfResource};
use sparql_parser::SelectResult;

/// Serialize a `SelectResult` as a SPARQL XML result document.
pub fn to_sparql_xml(result: &SelectResult) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <sparql xmlns=\"http://www.w3.org/2005/sparql-results#\">\n  <head>\n",
    );
    for var in &result.variables {
        out.push_str("    <variable name=\"");
        out.push_str(&xml_escape(var));
        out.push_str("\"/>\n");
    }
    out.push_str("  </head>\n  <results>\n");
    for row in &result.rows {
        out.push_str("    <result>\n");
        for var in &result.variables {
            if let Some(el) = row.get(var) {
                out.push_str("      <binding name=\"");
                out.push_str(&xml_escape(var));
                out.push_str("\">");
                out.push_str(&graph_element_to_xml(el));
                out.push_str("</binding>\n");
            }
        }
        out.push_str("    </result>\n");
    }
    out.push_str("  </results>\n</sparql>\n");
    out
}

/// Serialize an ASK result as a SPARQL XML result document.
pub fn ask_to_sparql_xml(boolean: bool) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <sparql xmlns=\"http://www.w3.org/2005/sparql-results#\">\n  \
         <head/>\n  <boolean>{}</boolean>\n</sparql>\n",
        boolean
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn graph_element_to_xml(el: &GraphElement) -> String {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            format!("<uri>{}</uri>", xml_escape(&iri.0))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            format!("<bnode>b{}</bnode>", id)
        }
        GraphElement::GraphLiteral(lit) => literal_to_xml(lit),
        // Triple terms in SPARQL XML output require RDF 1.2 support (#143).
        GraphElement::TripleTerm(k) => {
            format!(
                "<triple><s>{}</s><p>{}</p><o>{}</o></triple>",
                k.subject, k.predicate, k.obj
            )
        }
    }
}

fn literal_to_xml(lit: &RdfLiteral) -> String {
    match lit {
        RdfLiteral::LiteralString(s) => {
            format!("<literal>{}</literal>", xml_escape(s))
        }
        RdfLiteral::LangLiteral { lang, literal } => {
            format!(
                "<literal xml:lang=\"{}\">{}</literal>",
                xml_escape(lang),
                xml_escape(literal)
            )
        }
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            format!(
                "<literal datatype=\"{}\">{}</literal>",
                xml_escape(&type_iri.0),
                xml_escape(literal)
            )
        }
        RdfLiteral::BooleanLiteral(b) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#boolean\">{}</literal>",
            b
        ),
        RdfLiteral::IntegerLiteral(i) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#integer\">{}</literal>",
            i
        ),
        RdfLiteral::DecimalLiteral(d) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#decimal\">{}</literal>",
            d
        ),
        RdfLiteral::FloatLiteral(f) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#float\">{}</literal>",
            f
        ),
        RdfLiteral::DoubleLiteral(d) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#double\">{}</literal>",
            d
        ),
        RdfLiteral::DateTimeLiteral(dt) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#dateTime\">{}</literal>",
            dt.to_rfc3339()
        ),
        RdfLiteral::DateLiteral(d) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#date\">{}</literal>",
            d
        ),
        RdfLiteral::TimeLiteral(t) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#time\">{}</literal>",
            t
        ),
        RdfLiteral::DurationLiteral(dur) => format!(
            "<literal datatype=\"http://www.w3.org/2001/XMLSchema#duration\">{:?}</literal>",
            dur
        ),
    }
}

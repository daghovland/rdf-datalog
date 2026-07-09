/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serializer for `application/sparql-results+json`
//!
//! Spec: <https://www.w3.org/TR/sparql11-results-json/>

use dag_rdf::{GraphElement, RdfLiteral, RdfResource};
use serde_json::{Value, json};
use sparql_parser::SelectResult;

/// Serialize an ASK result as a SPARQL JSON result document.
///
/// Spec: <https://www.w3.org/TR/sparql11-results-json/#select-encode-terms-variables>
pub fn ask_to_sparql_json(result: bool) -> String {
    serde_json::json!({ "head": {}, "boolean": result }).to_string()
}

/// Serialize a `SelectResult` as a SPARQL JSON result document.
pub fn to_sparql_json(result: &SelectResult) -> String {
    let vars: Vec<Value> = result.variables.iter().map(|v| json!(v)).collect();

    let bindings: Vec<Value> = result
        .rows
        .iter()
        .map(|row| {
            let mut binding = serde_json::Map::new();
            for var in &result.variables {
                if let Some(element) = row.get(var) {
                    binding.insert(var.clone(), graph_element_to_json(element));
                }
            }
            Value::Object(binding)
        })
        .collect();

    let doc = json!({
        "head": { "vars": vars },
        "results": { "bindings": bindings }
    });

    doc.to_string()
}

pub(crate) fn graph_element_to_json(el: &GraphElement) -> Value {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            json!({ "type": "uri", "value": iri.0 })
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            json!({ "type": "bnode", "value": format!("b{}", id) })
        }
        GraphElement::GraphLiteral(lit) => literal_to_json(lit),
        // Triple terms in SPARQL JSON output require RDF 1.2 support (#143).
        GraphElement::TripleTerm(k) => {
            json!({ "type": "triple", "value": format!("<<( {} {} {} )>>", k.subject, k.predicate, k.obj) })
        }
    }
}

fn literal_to_json(lit: &RdfLiteral) -> Value {
    match lit {
        RdfLiteral::LiteralString(s) => {
            json!({ "type": "literal", "value": s })
        }
        RdfLiteral::BooleanLiteral(b) => {
            json!({ "type": "literal", "value": b.to_string(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#boolean" })
        }
        RdfLiteral::IntegerLiteral(i) => {
            json!({ "type": "literal", "value": i.to_string(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#integer" })
        }
        RdfLiteral::DecimalLiteral(d) => {
            json!({ "type": "literal", "value": d.to_string(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#decimal" })
        }
        RdfLiteral::FloatLiteral(f) => {
            json!({ "type": "literal", "value": f.to_string(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#float" })
        }
        RdfLiteral::DoubleLiteral(d) => {
            json!({ "type": "literal", "value": d.to_string(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#double" })
        }
        RdfLiteral::DateTimeLiteral(dt) => {
            json!({ "type": "literal", "value": dt.to_rfc3339(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#dateTime" })
        }
        RdfLiteral::DateLiteral(d) => {
            json!({ "type": "literal", "value": d.to_string(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#date" })
        }
        RdfLiteral::TimeLiteral(t) => {
            json!({ "type": "literal", "value": t.to_string(),
                    "datatype": "http://www.w3.org/2001/XMLSchema#time" })
        }
        RdfLiteral::DurationLiteral(dur) => {
            json!({ "type": "literal", "value": format!("{:?}", dur),
                    "datatype": "http://www.w3.org/2001/XMLSchema#duration" })
        }
        RdfLiteral::LangLiteral { lang, literal } => {
            json!({ "type": "literal", "value": literal, "xml:lang": lang })
        }
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            json!({ "type": "literal", "value": literal, "datatype": type_iri.0 })
        }
    }
}

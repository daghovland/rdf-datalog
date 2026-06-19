/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serializer for `text/csv` SPARQL SELECT results.
//!
//! Spec: <https://www.w3.org/TR/sparql11-results-csv-tsv/>

use dag_rdf::{GraphElement, RdfLiteral, RdfResource};
use sparql_parser::SelectResult;

/// Serialize a `SelectResult` as a SPARQL CSV result document.
///
/// Header row: variable names separated by commas.
/// Data rows: one binding per line, values quoted as per RFC 4180.
pub fn to_sparql_csv(result: &SelectResult) -> String {
    let mut out = String::new();

    // Header row
    out.push_str(&result.variables.join(","));
    out.push_str("\r\n");

    // Data rows
    for row in &result.rows {
        let cells: Vec<String> = result
            .variables
            .iter()
            .map(|var| match row.get(var) {
                None => String::new(),
                Some(el) => graph_element_to_csv(el),
            })
            .collect();
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    }

    out
}

fn graph_element_to_csv(el: &GraphElement) -> String {
    match el {
        // IRIs enclosed in angle brackets per spec §2
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            format!("<{}>", iri.0)
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            format!("_:b{}", id)
        }
        GraphElement::GraphLiteral(lit) => literal_to_csv(lit),
    }
}

fn literal_to_csv(lit: &RdfLiteral) -> String {
    let value = match lit {
        RdfLiteral::LiteralString(s) => s.clone(),
        RdfLiteral::LangLiteral { literal, .. } => literal.clone(),
        RdfLiteral::TypedLiteral { literal, .. } => literal.clone(),
        RdfLiteral::BooleanLiteral(b) => b.to_string(),
        RdfLiteral::IntegerLiteral(i) => i.to_string(),
        RdfLiteral::DecimalLiteral(d) => d.to_string(),
        RdfLiteral::FloatLiteral(f) => f.to_string(),
        RdfLiteral::DoubleLiteral(d) => d.to_string(),
        RdfLiteral::DateTimeLiteral(dt) => dt.to_rfc3339(),
        RdfLiteral::DateLiteral(d) => d.to_string(),
        RdfLiteral::TimeLiteral(t) => t.to_string(),
        RdfLiteral::DurationLiteral(dur) => format!("{:?}", dur),
    };
    csv_quote(&value)
}

/// RFC 4180 quoting: wrap in double-quotes and escape internal double-quotes by doubling.
fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

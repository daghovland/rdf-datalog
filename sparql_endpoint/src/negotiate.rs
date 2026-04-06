/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

/// Supported result formats for SELECT/ASK queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectFormat {
    SparqlJson,
    SparqlXml,
    Csv,
}

/// Negotiate the response format for a SELECT/ASK result based on the `Accept` header value.
///
/// Returns the best matching format, defaulting to SPARQL JSON.
pub fn negotiate_select_format(accept: Option<&str>) -> SelectFormat {
    let accept = match accept {
        Some(a) => a,
        None => return SelectFormat::SparqlJson,
    };

    // Walk the comma-separated media types in order (ignoring q= weights for now).
    for part in accept.split(',') {
        let mime = part.split(';').next().unwrap_or("").trim();
        match mime {
            "application/sparql-results+json" | "application/json" => {
                return SelectFormat::SparqlJson;
            }
            "application/sparql-results+xml" | "application/xml" => {
                return SelectFormat::SparqlXml;
            }
            "text/csv" => return SelectFormat::Csv,
            _ => {}
        }
    }

    SelectFormat::SparqlJson
}

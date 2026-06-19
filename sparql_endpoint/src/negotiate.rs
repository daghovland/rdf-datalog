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
/// Returns `Some(format)` when a supported type is found (or the header is absent),
/// or `None` when the client sent an explicit `Accept` header that contains no
/// supported media type — the caller should respond with `406 Not Acceptable`.
///
/// `*/*` and `application/*` wildcard tokens are treated as JSON.
pub fn negotiate_select_format(accept: Option<&str>) -> Option<SelectFormat> {
    let accept = match accept {
        None | Some("") => return Some(SelectFormat::SparqlJson),
        Some(a) => a,
    };

    for part in accept.split(',') {
        let mime = part.split(';').next().unwrap_or("").trim();
        match mime {
            "*/*" | "application/*" => return Some(SelectFormat::SparqlJson),
            "application/sparql-results+json" | "application/json" => {
                return Some(SelectFormat::SparqlJson);
            }
            "application/sparql-results+xml" | "application/xml" => {
                return Some(SelectFormat::SparqlXml);
            }
            "text/csv" => return Some(SelectFormat::Csv),
            _ => {}
        }
    }

    None
}

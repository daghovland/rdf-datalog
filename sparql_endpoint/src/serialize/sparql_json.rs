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

use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource, TripleTermKey};
use serde_json::{Value, json};
use sparql_parser::SelectResult;
use std::collections::HashMap;
use std::fmt;

/// Serialize an ASK result as a SPARQL JSON result document.
///
/// Spec: <https://www.w3.org/TR/sparql11-results-json/#select-encode-terms-variables>
pub fn ask_to_sparql_json(result: bool) -> String {
    serde_json::json!({ "head": {}, "boolean": result }).to_string()
}

/// Serialize a `SelectResult` as a SPARQL JSON result document.
///
/// `store` is needed to resolve RDF 1.2 triple-term bindings recursively
/// (see `graph_element_to_json` below).
pub fn to_sparql_json(result: &SelectResult, store: &Datastore) -> String {
    let vars: Vec<Value> = result.variables.iter().map(|v| json!(v)).collect();

    let bindings: Vec<Value> = result
        .rows
        .iter()
        .map(|row| {
            let mut binding = serde_json::Map::new();
            for var in &result.variables {
                if let Some(element) = row.get(var) {
                    binding.insert(var.clone(), graph_element_to_json(store, element));
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

/// Encode a single `GraphElement` as a SPARQL JSON result term.
///
/// RDF 1.2 triple terms (`GraphElement::TripleTerm`) are encoded per the
/// SPARQL 1.2 Query Results JSON Format draft's triple-term binding:
/// `{"type": "triple", "value": {"subject": S, "predicate": P, "object": O}}`,
/// where `S`/`P`/`O` are themselves full term encodings (recursive — a
/// triple term's own subject/object may be another triple term). Spec:
/// <https://www.w3.org/TR/sparql12-results-json/#select-encode-terms>
/// (Working Draft, not yet a Recommendation). Epic: #143.
pub(crate) fn graph_element_to_json(store: &Datastore, el: &GraphElement) -> Value {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            json!({ "type": "uri", "value": iri.0 })
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            json!({ "type": "bnode", "value": format!("b{}", id) })
        }
        GraphElement::GraphLiteral(lit) => literal_to_json(lit),
        GraphElement::TripleTerm(k) => {
            let subject =
                graph_element_to_json(store, store.resources.get_graph_element(k.subject));
            let predicate =
                graph_element_to_json(store, store.resources.get_graph_element(k.predicate));
            let object = graph_element_to_json(store, store.resources.get_graph_element(k.obj));
            json!({
                "type": "triple",
                "value": { "subject": subject, "predicate": predicate, "object": object }
            })
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
            json!({ "type": "literal", "value": format!("{}", dur),
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

// ─────────────────────────────────────────────────────────────────────────
// Deserializer: `application/sparql-results+json` -> solution bindings
// ─────────────────────────────────────────────────────────────────────────
//
// Federation (SPARQL `SERVICE`, epic #741) needs to parse a remote
// endpoint's JSON results response back into `SolutionRow`s before joining
// them into the local query's solution set. See issue #742.

const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";
const XSD_FLOAT: &str = "http://www.w3.org/2001/XMLSchema#float";
const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
const XSD_DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
const XSD_DATE: &str = "http://www.w3.org/2001/XMLSchema#date";
const XSD_TIME: &str = "http://www.w3.org/2001/XMLSchema#time";
const XSD_DURATION: &str = "http://www.w3.org/2001/XMLSchema#duration";

/// Errors parsing an `application/sparql-results+json` document.
///
/// Deliberately never panics on malformed/empty input: this parses data
/// received over the network from a remote SPARQL endpoint (federation,
/// epic #741), so malformed input must produce a clear `Err`.
#[derive(Debug, Clone, PartialEq)]
pub enum SparqlJsonError {
    /// The input was not valid JSON at all.
    InvalidJson(String),
    /// The document was a well-formed `boolean` (ASK) result, not a
    /// `SELECT` results document with `head`/`results.bindings`.
    AskResult,
    /// The document is missing a required field, or a field has the wrong
    /// shape (e.g. `results.bindings` is present but not an array).
    Malformed(String),
    /// A binding term's `"type"` was not one of `uri`/`literal`/
    /// `typed-literal`/`bnode`/`triple`.
    UnknownTermType(String),
}

impl fmt::Display for SparqlJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SparqlJsonError::InvalidJson(msg) => write!(f, "invalid JSON: {msg}"),
            SparqlJsonError::AskResult => {
                write!(f, "document is a boolean (ASK) result, not a SELECT result")
            }
            SparqlJsonError::Malformed(msg) => write!(f, "malformed SPARQL JSON results: {msg}"),
            SparqlJsonError::UnknownTermType(ty) => {
                write!(f, "unknown binding term type: {ty:?}")
            }
        }
    }
}

impl std::error::Error for SparqlJsonError {}

/// Parse an `application/sparql-results+json` SELECT results document into
/// a [`SelectResult`].
///
/// `store` is needed because RDF 1.2 triple-term bindings (`"type":
/// "triple"`) must be interned (`TripleTermKey` holds interned
/// `GraphElementId`s, not inline `GraphElement`s) — see
/// `Datastore::add_triple_term`. Plain terms (`uri`/`literal`/`bnode`) build
/// fresh `GraphElement`s directly and don't need the store, but blank nodes
/// still allocate a fresh anonymous node ID per distinct label via
/// `store.new_anonymous_blank_node()` so they don't collide with
/// already-interned local blank nodes.
///
/// Blank node labels (`"bnode"` values, e.g. `"b0"`) are scoped to this one
/// document per the spec: a per-call label -> `GraphElement` map keeps
/// repeated labels resolving to the same node *within this call*, but never
/// merges them with blank nodes from an unrelated call/document, and never
/// round-trips a label back into `RdfResource::AnonymousBlankNode`'s
/// internal numeric ID (that ID is purely local and a remote endpoint's
/// label has no relation to it).
pub fn from_sparql_json(
    json_str: &str,
    store: &mut Datastore,
) -> Result<SelectResult, SparqlJsonError> {
    let doc: Value =
        serde_json::from_str(json_str).map_err(|e| SparqlJsonError::InvalidJson(e.to_string()))?;
    let obj = doc.as_object().ok_or_else(|| {
        SparqlJsonError::Malformed("top-level JSON value is not an object".into())
    })?;

    if obj.contains_key("boolean") {
        return Err(SparqlJsonError::AskResult);
    }

    let vars: Vec<String> = obj
        .get("head")
        .and_then(Value::as_object)
        .and_then(|h| h.get("vars"))
        .and_then(Value::as_array)
        .ok_or_else(|| SparqlJsonError::Malformed("missing \"head\".\"vars\"".into()))?
        .iter()
        .map(|v| {
            v.as_str().map(str::to_string).ok_or_else(|| {
                SparqlJsonError::Malformed("\"head\".\"vars\" entry is not a string".into())
            })
        })
        .collect::<Result<_, _>>()?;

    let bindings = obj
        .get("results")
        .and_then(Value::as_object)
        .and_then(|r| r.get("bindings"))
        .and_then(Value::as_array)
        .ok_or_else(|| SparqlJsonError::Malformed("missing \"results\".\"bindings\"".into()))?;

    let mut bnodes: HashMap<String, GraphElement> = HashMap::new();
    let mut rows = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let binding_obj = binding.as_object().ok_or_else(|| {
            SparqlJsonError::Malformed("a \"bindings\" entry is not a JSON object".into())
        })?;
        let mut row = HashMap::with_capacity(binding_obj.len());
        for var in &vars {
            if let Some(term) = binding_obj.get(var) {
                let elem = json_to_graph_element(term, store, &mut bnodes)?;
                row.insert(var.clone(), elem);
            }
        }
        rows.push(row);
    }

    Ok(SelectResult {
        variables: vars,
        rows,
    })
}

fn json_obj<'a>(
    value: &'a Value,
    context: &str,
) -> Result<&'a serde_json::Map<String, Value>, SparqlJsonError> {
    value
        .as_object()
        .ok_or_else(|| SparqlJsonError::Malformed(format!("{context} is not a JSON object")))
}

fn json_str_field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, SparqlJsonError> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| SparqlJsonError::Malformed(format!("missing or non-string {key:?}")))
}

/// Parse a single SPARQL JSON binding term into a `GraphElement`.
fn json_to_graph_element(
    term: &Value,
    store: &mut Datastore,
    bnodes: &mut HashMap<String, GraphElement>,
) -> Result<GraphElement, SparqlJsonError> {
    let obj = json_obj(term, "a binding term")?;
    let ty = json_str_field(obj, "type")?;
    match ty {
        "uri" => {
            let value = json_str_field(obj, "value")?;
            Ok(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                value.to_string(),
            ))))
        }
        "bnode" => {
            let label = json_str_field(obj, "value")?;
            if let Some(existing) = bnodes.get(label) {
                Ok(existing.clone())
            } else {
                let id = store.new_anonymous_blank_node();
                let elem = store.resources.get_graph_element(id).clone();
                bnodes.insert(label.to_string(), elem.clone());
                Ok(elem)
            }
        }
        // `typed-literal` is not in the SPARQL 1.1 Query Results JSON Format
        // spec, but some endpoints (e.g. Virtuoso) emit it as a synonym for
        // `literal` with a `datatype`; accept it for interop.
        "literal" | "typed-literal" => {
            let value = json_str_field(obj, "value")?;
            if let Some(lang) = obj.get("xml:lang").and_then(Value::as_str) {
                return Ok(GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
                    lang: lang.to_string(),
                    literal: value.to_string(),
                }));
            }
            if let Some(datatype) = obj.get("datatype").and_then(Value::as_str) {
                return Ok(GraphElement::GraphLiteral(literal_from_lexical(
                    datatype, value,
                )));
            }
            Ok(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                value.to_string(),
            )))
        }
        "triple" => {
            let value_obj = obj.get("value").ok_or_else(|| {
                SparqlJsonError::Malformed("triple term missing \"value\"".into())
            })?;
            let parts = json_obj(value_obj, "a triple term's \"value\"")?;
            let subject = parts.get("subject").ok_or_else(|| {
                SparqlJsonError::Malformed("triple term missing \"subject\"".into())
            })?;
            let predicate = parts.get("predicate").ok_or_else(|| {
                SparqlJsonError::Malformed("triple term missing \"predicate\"".into())
            })?;
            let object = parts.get("object").ok_or_else(|| {
                SparqlJsonError::Malformed("triple term missing \"object\"".into())
            })?;

            let subject_elem = json_to_graph_element(subject, store, bnodes)?;
            let predicate_elem = json_to_graph_element(predicate, store, bnodes)?;
            let object_elem = json_to_graph_element(object, store, bnodes)?;

            let subject_id = store.add_resource(subject_elem);
            let predicate_id = store.add_resource(predicate_elem);
            let object_id = store.add_resource(object_elem);
            store.add_triple_term(subject_id, predicate_id, object_id);

            Ok(GraphElement::TripleTerm(TripleTermKey {
                subject: subject_id,
                predicate: predicate_id,
                obj: object_id,
            }))
        }
        other => Err(SparqlJsonError::UnknownTermType(other.to_string())),
    }
}

/// Parse a literal's lexical form back into the matching built-in `RdfLiteral`
/// variant for the fixed set of XSD datatypes `literal_to_json` encodes with
/// dedicated handling (see that function's doc comment). Any other
/// `datatype` IRI — including a recognized one whose lexical form fails to
/// parse (ill-typed RDF literals are legal) — falls back to
/// `RdfLiteral::TypedLiteral`, the spec-correct generic representation.
fn literal_from_lexical(datatype: &str, lexical: &str) -> RdfLiteral {
    let fallback = || RdfLiteral::TypedLiteral {
        type_iri: IriReference(datatype.to_string()),
        literal: lexical.to_string(),
    };
    match datatype {
        XSD_STRING => RdfLiteral::LiteralString(lexical.to_string()),
        XSD_BOOLEAN => match lexical {
            "true" | "1" => RdfLiteral::BooleanLiteral(true),
            "false" | "0" => RdfLiteral::BooleanLiteral(false),
            _ => fallback(),
        },
        XSD_INTEGER => lexical
            .parse::<num_bigint::BigInt>()
            .map(RdfLiteral::IntegerLiteral)
            .unwrap_or_else(|_| fallback()),
        XSD_DECIMAL => lexical
            .parse::<rust_decimal::Decimal>()
            .map(RdfLiteral::DecimalLiteral)
            .unwrap_or_else(|_| fallback()),
        XSD_FLOAT => lexical
            .parse::<f64>()
            .map(|f| RdfLiteral::FloatLiteral(ordered_float::OrderedFloat(f)))
            .unwrap_or_else(|_| fallback()),
        XSD_DOUBLE => lexical
            .parse::<f64>()
            .map(|d| RdfLiteral::DoubleLiteral(ordered_float::OrderedFloat(d)))
            .unwrap_or_else(|_| fallback()),
        XSD_DATE_TIME => chrono::DateTime::parse_from_rfc3339(lexical)
            .map(|dt| RdfLiteral::DateTimeLiteral(dt.with_timezone(&chrono::Utc)))
            .unwrap_or_else(|_| fallback()),
        XSD_DATE => chrono::NaiveDate::parse_from_str(lexical, "%Y-%m-%d")
            .map(RdfLiteral::DateLiteral)
            .unwrap_or_else(|_| fallback()),
        XSD_TIME => chrono::NaiveTime::parse_from_str(lexical, "%H:%M:%S%.f")
            .or_else(|_| chrono::NaiveTime::parse_from_str(lexical, "%H:%M:%S"))
            .map(RdfLiteral::TimeLiteral)
            .unwrap_or_else(|_| fallback()),
        XSD_DURATION => parse_xsd_duration(lexical)
            .map(RdfLiteral::DurationLiteral)
            .unwrap_or_else(fallback),
        _ => fallback(),
    }
}

/// Best-effort parser for the subset of ISO 8601 / xsd:duration lexical
/// forms expressible as a fixed `chrono::Duration` (day/hour/minute/second
/// components; `W` weeks). Calendar-dependent `Y`(ear)/`M`(onth) components
/// have no fixed-length representation and are rejected (`None`), which
/// callers fall back to `RdfLiteral::TypedLiteral` for — a safe, lossless
/// choice since `TypedLiteral` preserves the exact lexical form.
///
/// `literal_to_json`'s own encoding of `DurationLiteral` now uses `{}`
/// (Display), the valid ISO 8601 form (fixed by #769), so this parser's
/// target format round-trips through it correctly (see
/// `roundtrip_duration_literal` below).
fn parse_xsd_duration(s: &str) -> Option<chrono::Duration> {
    let mut chars = s.chars().peekable();
    let negative = if chars.peek() == Some(&'-') {
        chars.next();
        true
    } else {
        false
    };
    if chars.next() != Some('P') {
        return None;
    }
    let mut total = chrono::Duration::seconds(0);
    let mut in_time = false;
    let mut num = String::new();
    let mut saw_any = false;
    for c in chars {
        if c == 'T' {
            in_time = true;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let value: f64 = num.parse().ok()?;
        num.clear();
        let delta = match (c, in_time) {
            ('D', false) => chrono::Duration::seconds((value * 86_400.0).round() as i64),
            ('W', false) => chrono::Duration::seconds((value * 604_800.0).round() as i64),
            ('H', true) => chrono::Duration::seconds((value * 3_600.0).round() as i64),
            ('M', true) => chrono::Duration::seconds((value * 60.0).round() as i64),
            ('S', true) => {
                let secs = value.trunc() as i64;
                let nanos = ((value - value.trunc()) * 1e9).round() as i64;
                chrono::Duration::seconds(secs) + chrono::Duration::nanoseconds(nanos)
            }
            // `Y`/`M` (year/month, date part) have no fixed-length
            // representation as a `chrono::Duration`.
            _ => return None,
        };
        total += delta;
        saw_any = true;
    }
    if !num.is_empty() || !saw_any {
        return None;
    }
    Some(if negative { -total } else { total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;
    use num_bigint::BigInt;

    fn row(pairs: &[(&str, GraphElement)]) -> sparql_parser::execute::SolutionRow {
        pairs
            .iter()
            .cloned()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    // ── Round-trip through the existing serializer ──────────────────────────

    #[test]
    fn roundtrip_uri_and_plain_literal() {
        let mut store = Datastore::new(100);
        let result = SelectResult {
            variables: vec!["s".to_string(), "name".to_string()],
            rows: vec![row(&[
                (
                    "s",
                    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                        "http://example.org/alice".to_string(),
                    ))),
                ),
                (
                    "name",
                    GraphElement::GraphLiteral(RdfLiteral::LiteralString("Alice".to_string())),
                ),
            ])],
        };
        let json_str = to_sparql_json(&result, &store);
        let parsed = from_sparql_json(&json_str, &mut store).expect("should parse");
        assert_eq!(parsed.variables, result.variables);
        assert_eq!(parsed.rows, result.rows);
    }

    #[test]
    fn roundtrip_lang_literal() {
        let mut store = Datastore::new(100);
        let result = SelectResult {
            variables: vec!["label".to_string()],
            rows: vec![row(&[(
                "label",
                GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
                    lang: "en".to_string(),
                    literal: "hello".to_string(),
                }),
            )])],
        };
        let json_str = to_sparql_json(&result, &store);
        let parsed = from_sparql_json(&json_str, &mut store).expect("should parse");
        assert_eq!(parsed.rows, result.rows);
    }

    #[test]
    fn roundtrip_fixed_xsd_literals() {
        let mut store = Datastore::new(100);
        let result = SelectResult {
            variables: vec![
                "i".to_string(),
                "dec".to_string(),
                "fl".to_string(),
                "dbl".to_string(),
                "b".to_string(),
                "dt".to_string(),
                "d".to_string(),
                "t".to_string(),
            ],
            rows: vec![row(&[
                (
                    "i",
                    GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(42.into())),
                ),
                (
                    "dec",
                    GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral("3.14".parse().unwrap())),
                ),
                (
                    "fl",
                    GraphElement::GraphLiteral(RdfLiteral::FloatLiteral(1.5.into())),
                ),
                (
                    "dbl",
                    GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(2.5.into())),
                ),
                (
                    "b",
                    GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(true)),
                ),
                (
                    "dt",
                    GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(
                        chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z")
                            .unwrap()
                            .with_timezone(&chrono::Utc),
                    )),
                ),
                (
                    "d",
                    GraphElement::GraphLiteral(RdfLiteral::DateLiteral(
                        chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                    )),
                ),
                (
                    "t",
                    GraphElement::GraphLiteral(RdfLiteral::TimeLiteral(
                        chrono::NaiveTime::from_hms_opt(3, 4, 5).unwrap(),
                    )),
                ),
            ])],
        };
        let json_str = to_sparql_json(&result, &store);
        let parsed = from_sparql_json(&json_str, &mut store).expect("should parse");
        assert_eq!(parsed.rows, result.rows);
    }

    // Regression test for #769: `literal_to_json` used to encode
    // `DurationLiteral` with `{:?}` (chrono::Duration's Debug impl, e.g.
    // `TimeDelta { secs: 5, nanos: 0 }`) instead of its `Display` impl,
    // which is the valid ISO 8601 / xsd:duration lexical form. That made
    // the JSON value non-conformant and unable to round-trip through
    // `from_sparql_json`'s `parse_xsd_duration` (added by #742), which
    // targets the correct ISO 8601 form.
    #[test]
    fn duration_literal_value_is_iso8601() {
        let dur = chrono::Duration::seconds(5);
        let json = literal_to_json(&RdfLiteral::DurationLiteral(dur));
        let value = json["value"].as_str().expect("value should be a string");
        assert!(
            value.starts_with('P') || value.starts_with("-P"),
            "expected ISO 8601 duration lexical form (starting with P or -P), got {value:?}"
        );
        assert_eq!(
            json["datatype"],
            "http://www.w3.org/2001/XMLSchema#duration"
        );
    }

    #[test]
    fn roundtrip_duration_literal() {
        let mut store = Datastore::new(100);
        let result = SelectResult {
            variables: vec!["dur".to_string()],
            rows: vec![row(&[(
                "dur",
                GraphElement::GraphLiteral(RdfLiteral::DurationLiteral(
                    chrono::Duration::seconds(90) + chrono::Duration::nanoseconds(500_000_000),
                )),
            )])],
        };
        let json_str = to_sparql_json(&result, &store);
        let parsed = from_sparql_json(&json_str, &mut store).expect("should parse");
        assert_eq!(parsed.rows, result.rows);
    }

    #[test]
    fn roundtrip_typed_literal_generic() {
        let mut store = Datastore::new(100);
        let result = SelectResult {
            variables: vec!["x".to_string()],
            rows: vec![row(&[(
                "x",
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                    type_iri: IriReference("http://example.org/myType".to_string()),
                    literal: "custom-value".to_string(),
                }),
            )])],
        };
        let json_str = to_sparql_json(&result, &store);
        let parsed = from_sparql_json(&json_str, &mut store).expect("should parse");
        assert_eq!(parsed.rows, result.rows);
    }

    #[test]
    fn roundtrip_triple_term() {
        let mut store = Datastore::new(100);
        let s = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/s".to_string(),
        )));
        let p = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/p".to_string(),
        )));
        let o = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/o".to_string(),
        )));
        store.add_triple_term(s, p, o);
        let triple_elem = GraphElement::TripleTerm(TripleTermKey {
            subject: s,
            predicate: p,
            obj: o,
        });
        let result = SelectResult {
            variables: vec!["t".to_string()],
            rows: vec![row(&[("t", triple_elem.clone())])],
        };
        let json_str = to_sparql_json(&result, &store);
        let parsed = from_sparql_json(&json_str, &mut store).expect("should parse");
        assert_eq!(parsed.rows, vec![row(&[("t", triple_elem)])]);
    }

    #[test]
    fn roundtrip_nested_triple_term() {
        let mut store = Datastore::new(100);
        let s = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/s".to_string(),
        )));
        let p = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/p".to_string(),
        )));
        let o = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/o".to_string(),
        )));
        let inner_id = store.add_triple_term(s, p, o);
        let inner = GraphElement::TripleTerm(TripleTermKey {
            subject: s,
            predicate: p,
            obj: o,
        });
        let p2 = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/p2".to_string(),
        )));
        let o2 = store.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/o2".to_string(),
        )));
        let outer = GraphElement::TripleTerm(TripleTermKey {
            subject: inner_id,
            predicate: p2,
            obj: o2,
        });
        let _ = &inner; // keep for clarity; outer embeds inner's id
        let result = SelectResult {
            variables: vec!["t".to_string()],
            rows: vec![row(&[("t", outer.clone())])],
        };
        let json_str = to_sparql_json(&result, &store);
        let parsed = from_sparql_json(&json_str, &mut store).expect("should parse");
        assert_eq!(parsed.rows, vec![row(&[("t", outer)])]);
    }

    // ── Hand-written JSON fixtures ───────────────────────────────────────────

    #[test]
    fn parses_uri_fixture() {
        let doc = r#"{
            "head": { "vars": ["s"] },
            "results": { "bindings": [ { "s": { "type": "uri", "value": "http://example.org/x" } } ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        assert_eq!(parsed.variables, vec!["s".to_string()]);
        assert_eq!(
            parsed.rows[0].get("s"),
            Some(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                "http://example.org/x".to_string()
            ))))
        );
    }

    #[test]
    fn parses_bnode_fixture_same_label_same_node() {
        let doc = r#"{
            "head": { "vars": ["s", "o"] },
            "results": { "bindings": [
                { "s": { "type": "bnode", "value": "b0" }, "o": { "type": "bnode", "value": "b0" } }
            ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        let s = parsed.rows[0].get("s").unwrap();
        let o = parsed.rows[0].get("o").unwrap();
        assert_eq!(
            s, o,
            "same bnode label within a document must resolve to the same node"
        );
    }

    #[test]
    fn parses_bnode_fixture_different_labels_different_nodes() {
        let doc = r#"{
            "head": { "vars": ["s", "o"] },
            "results": { "bindings": [
                { "s": { "type": "bnode", "value": "b0" }, "o": { "type": "bnode", "value": "b1" } }
            ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        let s = parsed.rows[0].get("s").unwrap();
        let o = parsed.rows[0].get("o").unwrap();
        assert_ne!(s, o);
    }

    #[test]
    fn parses_typed_literal_integer_fixture() {
        let doc = r#"{
            "head": { "vars": ["n"] },
            "results": { "bindings": [
                { "n": { "type": "literal", "value": "42", "datatype": "http://www.w3.org/2001/XMLSchema#integer" } }
            ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        assert_eq!(
            parsed.rows[0].get("n"),
            Some(&GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(42)
            )))
        );
    }

    #[test]
    fn parses_typed_literal_boolean_fixture() {
        let doc = r#"{
            "head": { "vars": ["b"] },
            "results": { "bindings": [
                { "b": { "type": "literal", "value": "true", "datatype": "http://www.w3.org/2001/XMLSchema#boolean" } }
            ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        assert_eq!(
            parsed.rows[0].get("b"),
            Some(&GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
                true
            )))
        );
    }

    #[test]
    fn parses_typed_literal_xsd_string_as_plain_string() {
        let doc = r#"{
            "head": { "vars": ["s"] },
            "results": { "bindings": [
                { "s": { "type": "literal", "value": "hi", "datatype": "http://www.w3.org/2001/XMLSchema#string" } }
            ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        assert_eq!(
            parsed.rows[0].get("s"),
            Some(&GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                "hi".to_string()
            )))
        );
    }

    #[test]
    fn parses_typed_literal_alias_fixture() {
        let doc = r#"{
            "head": { "vars": ["n"] },
            "results": { "bindings": [
                { "n": { "type": "typed-literal", "value": "7", "datatype": "http://www.w3.org/2001/XMLSchema#integer" } }
            ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        assert_eq!(
            parsed.rows[0].get("n"),
            Some(&GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(7)
            )))
        );
    }

    #[test]
    fn ill_typed_literal_falls_back_to_typed_literal() {
        let doc = r#"{
            "head": { "vars": ["n"] },
            "results": { "bindings": [
                { "n": { "type": "literal", "value": "not-a-number", "datatype": "http://www.w3.org/2001/XMLSchema#integer" } }
            ] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        assert_eq!(
            parsed.rows[0].get("n"),
            Some(&GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                type_iri: IriReference("http://www.w3.org/2001/XMLSchema#integer".to_string()),
                literal: "not-a-number".to_string(),
            }))
        );
    }

    #[test]
    fn missing_bindings_is_an_error_not_a_panic() {
        let doc = r#"{ "head": { "vars": ["x"] } }"#;
        let mut store = Datastore::new(10);
        let err = match from_sparql_json(doc, &mut store) {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        };
        assert!(matches!(err, SparqlJsonError::Malformed(_)));
    }

    #[test]
    fn empty_results_bindings_is_ok_with_no_rows() {
        let doc = r#"{
            "head": { "vars": ["x"] },
            "results": { "bindings": [] }
        }"#;
        let mut store = Datastore::new(10);
        let parsed = from_sparql_json(doc, &mut store).unwrap();
        assert_eq!(parsed.rows.len(), 0);
    }

    #[test]
    fn invalid_json_is_an_error_not_a_panic() {
        let mut store = Datastore::new(10);
        let err = match from_sparql_json("not json at all {{{", &mut store) {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        };
        assert!(matches!(err, SparqlJsonError::InvalidJson(_)));
    }

    #[test]
    fn ask_boolean_document_is_an_error_not_silently_empty() {
        let doc = r#"{ "head": {}, "boolean": true }"#;
        let mut store = Datastore::new(10);
        let err = match from_sparql_json(doc, &mut store) {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        };
        assert_eq!(err, SparqlJsonError::AskResult);
    }

    #[test]
    fn unknown_term_type_is_an_error_not_a_panic() {
        let doc = r#"{
            "head": { "vars": ["x"] },
            "results": { "bindings": [ { "x": { "type": "mystery", "value": "?" } } ] }
        }"#;
        let mut store = Datastore::new(10);
        let err = match from_sparql_json(doc, &mut store) {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        };
        assert!(matches!(err, SparqlJsonError::UnknownTermType(_)));
    }
}

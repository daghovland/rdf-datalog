/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! dagalog library — pipeline functions for loading RDF data, applying OWL-RL
//! reasoning, and executing SPARQL queries.
//!
//! The CLI binary (`main.rs`) is a thin wrapper around this library.

use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use owl2rl2datalog::owl2datalog;
use rdf_owl_translator::rdf2owl;
use sparql_parser::{
    NetworkPolicy, ParserContext, QueryResult, SelectResult, execute, parse_query,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

// ── User-guide doctests ───────────────────────────────────────────────────────
//
// The Rust code fences in `docs/user/*.md` are wired up as real rustdoc
// doctests by re-exposing each file's contents as extra doc comments on this
// module, gated to `cfg(doctest)` so they never appear in normal `cargo doc`
// output and never affect ordinary builds. `dagalog` is the include point
// because it depends (directly or via `[dev-dependencies]`) on every crate
// these guides call out to — `jsonld_parser`, `ottr`, `rml`, `sparql_endpoint`,
// etc. — so the doctests can resolve every symbol they use.
//
// This module holds no code; it exists purely to carry the `doc` attributes.
// See [#167](https://github.com/daghovland/rdf-datalog/issues/167).
#[cfg_attr(doctest, doc = include_str!("../docs/user/deployment.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/formats.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/reasoning.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/rml-mapping.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/ottr-templates.md"))]
mod user_guide_doctests {}

// ── Output format ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "csv" => Ok(OutputFormat::Csv),
            "json" => Ok(OutputFormat::Json),
            other => Err(format!(
                "unknown format '{}': expected table, csv, or json",
                other
            )),
        }
    }
}

// ── Stats returned by apply_ontologies ────────────────────────────────────────

pub struct ReasoningStats {
    pub axiom_count: usize,
    pub rule_count: usize,
    pub triples_before: usize,
    pub triples_after: usize,
}

// ── Data loading ──────────────────────────────────────────────────────────────

/// Load one RDF file into `datastore`.
///
/// Format is inferred from the file extension:
/// - `.trig` → TriG
/// - `.nt` → N-Triples
/// - `.nq` → N-Quads
/// - everything else → Turtle
pub fn load_file(datastore: &mut Datastore, path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {}", path.display(), e))?;
    let reader = BufReader::new(file);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "trig" => turtle::parse_trig(datastore, reader)
            .map_err(|e| format!("TriG parse error in {}: {}", path.display(), e)),
        "nt" => turtle::parse_ntriples(datastore, reader)
            .map_err(|e| format!("N-Triples parse error in {}: {}", path.display(), e)),
        "nq" => turtle::parse_nquads(datastore, reader)
            .map_err(|e| format!("N-Quads parse error in {}: {}", path.display(), e)),
        _ => turtle::parse_turtle(datastore, reader)
            .map_err(|e| format!("Turtle parse error in {}: {}", path.display(), e)),
    }
}

// ── OWL reasoning ─────────────────────────────────────────────────────────────

/// Load OWL ontology files and apply OWL-RL materialisation to `datastore`.
///
/// Ontology triples are loaded into the same datastore as the data, then the
/// full RDF→OWL→Datalog→materialise pipeline is executed.
///
/// Returns reasoning statistics (axiom count, rule count, triple delta).
pub fn apply_ontologies(
    datastore: &mut Datastore,
    paths: &[std::path::PathBuf],
) -> Result<ReasoningStats, String> {
    let triples_before = datastore.named_graphs.quad_count;

    for path in paths {
        load_file(datastore, path)?;
    }

    let ontology_doc = rdf2owl(datastore);
    let ontology = &ontology_doc.ontology;
    let axiom_count = ontology.axioms.len();

    let rules = owl2datalog(&mut datastore.resources, ontology);
    let rule_count = rules.len();

    datalog::evaluate_rules(rules, datastore);

    let triples_after = datastore.named_graphs.quad_count;

    Ok(ReasoningStats {
        axiom_count,
        rule_count,
        triples_before,
        triples_after,
    })
}

/// Run OWL-RL materialisation over the triples already in `datastore`.
///
/// Extracts OWL axioms from the current triple set, converts them to Datalog
/// rules, and runs naive forward-chaining to closure.  Returns the number of
/// triples added by the reasoning step.
pub fn run_owlrl_reasoning(datastore: &mut Datastore) -> usize {
    let before = datastore.named_graphs.quad_count;
    let ontology_doc = rdf2owl(datastore);
    let rules = owl2datalog(&mut datastore.resources, &ontology_doc.ontology);
    datalog::evaluate_rules(rules, datastore);
    datastore.named_graphs.quad_count - before
}

// ── RML mapping ───────────────────────────────────────────────────────────────

/// Apply one or more RML mapping files to `datastore`.
///
/// For each mapping file, the source files referenced inside it are resolved
/// relative to that mapping file's parent directory. Mappings are applied in
/// order; triples from all mappings accumulate in the same datastore.
pub fn apply_rml_mappings(datastore: &mut Datastore, paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let base_dir = path
            .parent()
            .ok_or_else(|| format!("cannot determine parent directory of {}", path.display()))?;
        rml::apply_rml_mapping(path, base_dir, datastore)
            .map_err(|e| format!("RML mapping error in {}: {}", path.display(), e))?;
    }
    Ok(())
}

// ── Datalog rules ─────────────────────────────────────────────────────────────

/// Parse and apply Datalog rules from one or more `.datalog` files.
///
/// IRIs are interned into `datastore`; rules are then evaluated by naive
/// forward-chaining materialisation.  Returns the number of rules applied.
pub fn apply_rules(datastore: &mut Datastore, paths: &[PathBuf]) -> Result<usize, String> {
    let mut all_rules = Vec::new();
    for path in paths {
        let mut rules = datalog_parser::parse_file(path, datastore)?;
        all_rules.append(&mut rules);
    }
    let rule_count = all_rules.len();
    datalog::evaluate_rules(all_rules, datastore);
    Ok(rule_count)
}

/// Parse Datalog rules from one or more `.datalog` files WITHOUT applying them.
///
/// IRIs are interned into `datastore` so resource IDs in the returned rules are
/// valid in that store.  The caller is responsible for materialisation (e.g.
/// by passing the rules to [`sparql_endpoint::Config::initial_rules`] for
/// incremental reasoning via the HTTP endpoint).
///
/// For one-shot (non-serve) use, prefer [`apply_rules`] which also evaluates.
pub fn parse_rules(
    datastore: &mut Datastore,
    paths: &[PathBuf],
) -> Result<Vec<datalog::Rule>, String> {
    let mut all_rules = Vec::new();
    for path in paths {
        let mut rules = datalog_parser::parse_file(path, datastore)?;
        all_rules.append(&mut rules);
    }
    Ok(all_rules)
}

// ── SPARQL ────────────────────────────────────────────────────────────────────

/// Execute a SPARQL SELECT query string against `datastore`.
pub fn run_sparql_query(datastore: &Datastore, sparql: &str) -> Result<SelectResult, String> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) =
        parse_query(sparql, &mut ctx).map_err(|e| format!("SPARQL parse error: {:?}", e))?;
    match execute(&query, datastore, NetworkPolicy::Deny)? {
        QueryResult::Select(r) => Ok(r),
        QueryResult::Ask(_) => {
            Err("ASK queries are not supported via run_sparql_query".to_string())
        }
        QueryResult::Construct(_) => {
            Err("CONSTRUCT queries are not supported via run_sparql_query".to_string())
        }
        QueryResult::Describe(_) => {
            Err("DESCRIBE queries are not supported via run_sparql_query".to_string())
        }
    }
}

// ── Output formatting ─────────────────────────────────────────────────────────

/// Format SPARQL results as a string in the requested output format.
pub fn format_results(result: &SelectResult, format: &OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_table(result),
        OutputFormat::Csv => format_csv(result),
        OutputFormat::Json => format_json(result),
    }
}

/// Render a `GraphElement` as a human-readable string.
pub fn graph_element_display(el: &GraphElement) -> String {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => format!("<{}>", iri.0),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => format!("_:b{}", id),
        GraphElement::GraphLiteral(lit) => rdf_literal_display(lit),
        // Triple terms: display using interned IDs; full RDF 1.2 display tracked in #143.
        GraphElement::TripleTerm(k) => format!("<<( {} {} {} )>>", k.subject, k.predicate, k.obj),
    }
}

fn rdf_literal_display(lit: &RdfLiteral) -> String {
    match lit {
        RdfLiteral::LiteralString(s) => format!("\"{}\"", s),
        RdfLiteral::LangLiteral { literal, lang } => format!("\"{}\"@{}", literal, lang),
        RdfLiteral::TypedLiteral { literal, type_iri } => {
            format!("\"{}\"^^<{}>", literal, type_iri.0)
        }
        RdfLiteral::BooleanLiteral(b) => b.to_string(),
        RdfLiteral::IntegerLiteral(n) => n.to_string(),
        RdfLiteral::DoubleLiteral(d) => d.to_string(),
        RdfLiteral::DecimalLiteral(d) => d.to_string(),
        RdfLiteral::FloatLiteral(f) => f.to_string(),
        RdfLiteral::DurationLiteral(d) => format!("{:?}", d),
        RdfLiteral::DateTimeLiteral(dt) => dt.to_string(),
        RdfLiteral::TimeLiteral(t) => t.to_string(),
        RdfLiteral::DateLiteral(d) => d.to_string(),
    }
}

fn format_table(result: &SelectResult) -> String {
    if result.variables.is_empty() {
        return "(no variables)\n".to_string();
    }

    let mut widths: Vec<usize> = result.variables.iter().map(|v| v.len() + 1).collect();
    for row in &result.rows {
        for (i, var) in result.variables.iter().enumerate() {
            let val = row
                .get(var)
                .map(graph_element_display)
                .unwrap_or_else(|| "(unbound)".to_string());
            widths[i] = widths[i].max(val.len());
        }
    }

    let mut out = String::new();

    for (i, var) in result.variables.iter().enumerate() {
        out.push_str(&format!(
            "{:<width$}  ",
            format!("?{}", var),
            width = widths[i]
        ));
    }
    out.push('\n');

    for w in &widths {
        out.push_str(&"-".repeat(w + 2));
    }
    out.push('\n');

    if result.rows.is_empty() {
        out.push_str("(no results)\n");
    } else {
        for row in &result.rows {
            for (i, var) in result.variables.iter().enumerate() {
                let val = row
                    .get(var)
                    .map(graph_element_display)
                    .unwrap_or_else(|| "(unbound)".to_string());
                out.push_str(&format!("{:<width$}  ", val, width = widths[i]));
            }
            out.push('\n');
        }
    }

    out
}

/// Return the raw lexical value of an element for plain-text contexts (CSV).
///
/// IRIs are returned without angle brackets, literals without RDF quoting.
/// This follows the SPARQL 1.1 CSV/TSV results format convention.
fn graph_element_raw_value(el: &GraphElement) -> String {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => iri.0.clone(),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => format!("_:b{}", id),
        GraphElement::GraphLiteral(lit) => match lit {
            RdfLiteral::LiteralString(s) => s.clone(),
            RdfLiteral::LangLiteral { literal, .. } => literal.clone(),
            RdfLiteral::TypedLiteral { literal, .. } => literal.clone(),
            RdfLiteral::BooleanLiteral(b) => b.to_string(),
            RdfLiteral::IntegerLiteral(n) => n.to_string(),
            RdfLiteral::DoubleLiteral(d) => d.to_string(),
            RdfLiteral::DecimalLiteral(d) => d.to_string(),
            RdfLiteral::FloatLiteral(f) => f.to_string(),
            RdfLiteral::DurationLiteral(d) => format!("{:?}", d),
            RdfLiteral::DateTimeLiteral(dt) => dt.to_string(),
            RdfLiteral::TimeLiteral(t) => t.to_string(),
            RdfLiteral::DateLiteral(d) => d.to_string(),
        },
        // Triple terms: raw value is the display form (#143).
        GraphElement::TripleTerm(k) => format!("<<( {} {} {} )>>", k.subject, k.predicate, k.obj),
    }
}

fn format_csv(result: &SelectResult) -> String {
    let mut out = String::new();

    out.push_str(&result.variables.join(","));
    out.push('\n');

    for row in &result.rows {
        let values: Vec<String> = result
            .variables
            .iter()
            .map(|var| {
                let val = row
                    .get(var)
                    .map(graph_element_raw_value)
                    .unwrap_or_default();
                csv_escape(&val)
            })
            .collect();
        out.push_str(&values.join(","));
        out.push('\n');
    }

    out
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn format_json(result: &SelectResult) -> String {
    let vars: Vec<String> = result
        .variables
        .iter()
        .map(|v| format!("\"{}\"", json_escape(v)))
        .collect();

    let bindings: Vec<String> = result
        .rows
        .iter()
        .map(|row| {
            let pairs: Vec<String> = result
                .variables
                .iter()
                .filter_map(|var| {
                    let el = row.get(var)?;
                    Some(format!("\"{}\":{}", json_escape(var), element_to_json(el)))
                })
                .collect();
            format!("{{{}}}", pairs.join(","))
        })
        .collect();

    format!(
        "{{\"head\":{{\"vars\":[{}]}},\"results\":{{\"bindings\":[{}]}}}}",
        vars.join(","),
        bindings.join(",")
    )
}

fn element_to_json(el: &GraphElement) -> String {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri))) => {
            format!("{{\"type\":\"uri\",\"value\":\"{}\"}}", json_escape(iri))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            format!("{{\"type\":\"bnode\",\"value\":\"b{}\"}}", id)
        }
        GraphElement::GraphLiteral(lit) => literal_to_json(lit),
        // Triple terms: JSON representation tracked in #143.
        GraphElement::TripleTerm(k) => {
            format!(
                "{{\"type\":\"triple\",\"value\":\"<<( {} {} {} )>>\"}}",
                k.subject, k.predicate, k.obj
            )
        }
    }
}

fn literal_to_json(lit: &RdfLiteral) -> String {
    match lit {
        RdfLiteral::LiteralString(s) => {
            format!("{{\"type\":\"literal\",\"value\":\"{}\"}}", json_escape(s))
        }
        RdfLiteral::LangLiteral { literal, lang } => format!(
            "{{\"type\":\"literal\",\"xml:lang\":\"{}\",\"value\":\"{}\"}}",
            json_escape(lang),
            json_escape(literal)
        ),
        RdfLiteral::TypedLiteral { literal, type_iri } => format!(
            "{{\"type\":\"literal\",\"datatype\":\"{}\",\"value\":\"{}\"}}",
            json_escape(&type_iri.0),
            json_escape(literal)
        ),
        other => {
            let s = rdf_literal_display(other);
            format!("{{\"type\":\"literal\",\"value\":\"{}\"}}", json_escape(&s))
        }
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;

    const FAMILY_TTL: &str = r#"
@prefix ex: <http://example.org/family#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

<http://example.org/family> a owl:Ontology .

ex:Person a owl:Class .
ex:Employee a owl:Class ;
    rdfs:subClassOf ex:Person .

ex:Alice a ex:Person ;
    ex:name "Alice" .

ex:Bob a ex:Employee ;
    ex:name "Bob" .
"#;

    fn load_family() -> Datastore {
        let mut ds = Datastore::new(10_000);
        turtle::parse_turtle(&mut ds, FAMILY_TTL.as_bytes()).expect("parse should succeed");
        ds
    }

    #[test]
    fn sparql_basic_query() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person WHERE { ?person a ex:Person . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let persons: Vec<_> = result
            .rows
            .iter()
            .filter_map(|r| r.get("person"))
            .map(graph_element_display)
            .collect();
        assert!(persons.contains(&"<http://example.org/family#Alice>".to_string()));
        // Without reasoning Bob (an Employee) should NOT appear as a Person
        assert!(!persons.contains(&"<http://example.org/family#Bob>".to_string()));
    }

    #[test]
    fn sparql_with_reasoning() {
        let mut ds = Datastore::new(10_000);
        turtle::parse_turtle(&mut ds, FAMILY_TTL.as_bytes()).expect("parse should succeed");
        // Ontology IS the data file here; re-load for reasoning
        let ontology_doc = rdf2owl(&mut ds);
        let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
        datalog::evaluate_rules(rules, &mut ds);

        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person WHERE { ?person a ex:Person . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let persons: Vec<_> = result
            .rows
            .iter()
            .filter_map(|r| r.get("person"))
            .map(graph_element_display)
            .collect();
        assert!(persons.contains(&"<http://example.org/family#Alice>".to_string()));
        // After reasoning: Bob is an Employee which is a subClassOf Person → Bob is a Person
        assert!(
            persons.contains(&"<http://example.org/family#Bob>".to_string()),
            "expected Bob to be inferred as a Person after OWL-RL reasoning; got: {:?}",
            persons
        );
    }

    #[test]
    fn format_table_output() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person ?name WHERE {
    ?person a ex:Person .
    ?person ex:name ?name .
}
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let output = format_results(&result, &OutputFormat::Table);
        assert!(output.contains("?person"));
        assert!(output.contains("?name"));
        assert!(output.contains("Alice"));
    }

    #[test]
    fn format_csv_output() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person ?name WHERE {
    ?person a ex:Person .
    ?person ex:name ?name .
}
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let output = format_results(&result, &OutputFormat::Csv);
        let mut lines = output.lines();
        assert_eq!(
            lines.next(),
            Some("person,name"),
            "first line should be header"
        );
        // Values should be raw (no RDF quoting): IRI without <>, literal without ""
        assert!(
            output.contains("Alice"),
            "should contain raw literal value Alice"
        );
        assert!(
            !output.contains(r#""""Alice""""#),
            "should not double-escape the literal"
        );
    }

    #[test]
    fn format_json_output() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person WHERE { ?person a ex:Person . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let output = format_results(&result, &OutputFormat::Json);
        assert!(
            output.starts_with("{\"head\":{\"vars\":"),
            "should be SPARQL JSON"
        );
        assert!(output.contains("\"person\""));
        assert!(output.contains("http://example.org/family#Alice"));
    }

    #[test]
    fn empty_result_table() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?x WHERE { ?x a ex:NonExistentClass . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        assert!(result.rows.is_empty());
        let output = format_results(&result, &OutputFormat::Table);
        assert!(output.contains("(no results)"));
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("hello, world"), "\"hello, world\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn json_escape_special_chars() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(json_escape("line\nnewline"), "line\\nnewline");
    }
}

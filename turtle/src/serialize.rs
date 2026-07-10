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

use dag_rdf::{
    DEFAULT_GRAPH_ELEMENT_ID, DEFAULT_GRAPH_IRI, Datastore, GraphElement, GraphElementId,
    RdfLiteral, RdfResource,
};

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
        // Literals cannot be subjects; triple terms as subjects require RDF 1.2 support (#143).
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

// ── N-Quads serializer ────────────────────────────────────────────────────────

/// Serialize all quads in `store` as N-Quads.
///
/// Triples in the default graph (graph-id 0) are written as N-Triples lines
/// (no fourth field). Triples in named graphs include the graph IRI as the
/// fourth field. Output order follows insertion order of the quad list.
///
/// Spec: <https://www.w3.org/TR/n-quads/>
pub fn serialize_nquads(store: &Datastore) -> String {
    let mut out = String::new();
    for quad in store.named_graphs.get_all_quads() {
        let s = subject_term(store.resources.get_graph_element(quad.subject));
        let p = predicate_term(store.resources.get_graph_element(quad.predicate));
        let o = object_term(store.resources.get_graph_element(quad.obj));
        let (Some(s), Some(p), Some(o)) = (s, p, o) else {
            continue;
        };
        if quad.triple_id == DEFAULT_GRAPH_ELEMENT_ID {
            out.push_str(&format!("{s} {p} {o} .\n"));
        } else {
            if let Some(g) = graph_term(store.resources.get_graph_element(quad.triple_id)) {
                out.push_str(&format!("{s} {p} {o} {g} .\n"));
            }
        }
    }
    out
}

// ── TriG serializer ───────────────────────────────────────────────────────────

/// Serialize all quads in `store` as TriG.
///
/// Default-graph triples are written as bare `subject predicate object .` lines.
/// Named graphs are wrapped in `GRAPH <iri> { ... }` blocks.
///
/// Spec: <https://www.w3.org/TR/trig/>
pub fn serialize_trig(store: &Datastore) -> String {
    let mut out = String::new();

    // Default graph: bare triples
    for quad in store.named_graphs.get_graph(DEFAULT_GRAPH_ELEMENT_ID) {
        let s = subject_term(store.resources.get_graph_element(quad.subject));
        let p = predicate_term(store.resources.get_graph_element(quad.predicate));
        let o = object_term(store.resources.get_graph_element(quad.obj));
        if let (Some(s), Some(p), Some(o)) = (s, p, o) {
            out.push_str(&format!("{s} {p} {o} .\n"));
        }
    }

    // Named graphs
    let mut graph_ids: Vec<u32> = store
        .named_graphs
        .triple_id_index
        .keys()
        .copied()
        .filter(|&id| id != DEFAULT_GRAPH_ELEMENT_ID)
        .collect();
    graph_ids.sort_unstable(); // deterministic output order
    for graph_id in graph_ids {
        let Some(g) = graph_term(store.resources.get_graph_element(graph_id)) else {
            continue;
        };
        out.push_str(&format!("\nGRAPH {g} {{\n"));
        for quad in store.named_graphs.get_graph(graph_id) {
            let s = subject_term(store.resources.get_graph_element(quad.subject));
            let p = predicate_term(store.resources.get_graph_element(quad.predicate));
            let o = object_term(store.resources.get_graph_element(quad.obj));
            if let (Some(s), Some(p), Some(o)) = (s, p, o) {
                out.push_str(&format!("    {s} {p} {o} .\n"));
            }
        }
        out.push_str("}\n");
    }
    out
}

// ── Single-graph N-Quads / TriG ──────────────────────────────────────────────

/// Serialize one named graph as N-Quads, including the graph IRI as the 4th field.
///
/// For the default graph (`graph_id == DEFAULT_GRAPH_ELEMENT_ID`) the 4th field
/// is omitted, making each line a valid N-Triples line (which is also valid N-Quads).
pub fn serialize_nquads_graph(store: &Datastore, graph_id: GraphElementId) -> String {
    let g = if graph_id == DEFAULT_GRAPH_ELEMENT_ID {
        None
    } else {
        graph_term(store.resources.get_graph_element(graph_id))
    };
    let mut out = String::new();
    for quad in store.named_graphs.get_graph(graph_id) {
        let s = subject_term(store.resources.get_graph_element(quad.subject));
        let p = predicate_term(store.resources.get_graph_element(quad.predicate));
        let o = object_term(store.resources.get_graph_element(quad.obj));
        let (Some(s), Some(p), Some(o)) = (s, p, o) else {
            continue;
        };
        match &g {
            None => out.push_str(&format!("{s} {p} {o} .\n")),
            Some(g) => out.push_str(&format!("{s} {p} {o} {g} .\n")),
        }
    }
    out
}

/// Serialize one named graph as TriG.
///
/// Named graphs are wrapped in a `GRAPH <iri> { ... }` block.
/// For the default graph the output is bare triples (no `GRAPH` keyword),
/// which is valid TriG.
pub fn serialize_trig_graph(store: &Datastore, graph_id: GraphElementId) -> String {
    if graph_id == DEFAULT_GRAPH_ELEMENT_ID {
        return serialize_graph(store, graph_id);
    }
    let Some(g) = graph_term(store.resources.get_graph_element(graph_id)) else {
        return String::new();
    };
    let mut out = format!("GRAPH {g} {{\n");
    for quad in store.named_graphs.get_graph(graph_id) {
        let s = subject_term(store.resources.get_graph_element(quad.subject));
        let p = predicate_term(store.resources.get_graph_element(quad.predicate));
        let o = object_term(store.resources.get_graph_element(quad.obj));
        if let (Some(s), Some(p), Some(o)) = (s, p, o) {
            out.push_str(&format!("    {s} {p} {o} .\n"));
        }
    }
    out.push_str("}\n");
    out
}

// ── shared graph-term helper ──────────────────────────────────────────────────

fn graph_term(elem: &GraphElement) -> Option<String> {
    match elem {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            if iri.0 == DEFAULT_GRAPH_IRI {
                return None;
            }
            Some(format!("<{}>", escape_iri(&iri.0)))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => Some(format!("_:b{id}")),
        _ => None,
    }
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

    // ── N-Quads serializer tests ──────────────────────────────────────────────

    /// Default-graph triple must be written without the fourth (graph) field,
    /// matching N-Triples syntax (N-Quads §2).
    #[test]
    fn nquads_default_graph_triple_has_no_graph_field() {
        let mut ds = Datastore::new(64);
        let s = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/s".to_owned(),
        )));
        let p = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/p".to_owned(),
        )));
        let o = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/o".to_owned(),
        )));
        ds.add_triple(Triple {
            subject: s,
            predicate: p,
            obj: o,
        });
        let out = serialize_nquads(&ds);
        // Exactly one line, ends with " ." — no fourth field
        assert_eq!(out.lines().count(), 1, "expected 1 line, got: {out}");
        assert!(
            out.trim_end().ends_with("<http://example/o> ."),
            "line must end with object and dot, got: {out}"
        );
    }

    /// Named-graph triple must include the graph IRI as the fourth field (N-Quads §2).
    #[test]
    fn nquads_named_graph_triple_includes_graph_iri() {
        let mut ds = Datastore::new(64);
        let s = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/s".to_owned(),
        )));
        let p = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/p".to_owned(),
        )));
        let o = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/o".to_owned(),
        )));
        let g = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/g".to_owned(),
        )));
        ds.add_named_graph_triple(
            g,
            Triple {
                subject: s,
                predicate: p,
                obj: o,
            },
        );
        let out = serialize_nquads(&ds);
        assert_eq!(out.lines().count(), 1, "expected 1 line, got: {out}");
        assert!(
            out.contains("<http://example/g>"),
            "graph IRI must appear in output, got: {out}"
        );
        assert!(
            out.trim_end().ends_with("<http://example/g> ."),
            "line must end with graph IRI and dot, got: {out}"
        );
    }

    /// Language-tagged literals must survive an N-Quads roundtrip (N-Quads §2.4).
    #[test]
    fn nquads_lang_literal_roundtrip() {
        let nq = "<http://example/s> <http://example/p> \"Mona Lisa\"@en .\n";
        let mut ds = Datastore::new(64);
        crate::parse_nquads(&mut ds, nq.as_bytes()).unwrap();
        let out = serialize_nquads(&ds);
        let mut ds2 = Datastore::new(64);
        crate::parse_nquads(&mut ds2, out.as_bytes())
            .unwrap_or_else(|e| panic!("re-parse failed: {e}\noutput was:\n{out}"));
        assert_eq!(
            ds2.named_graphs.quad_count, 1,
            "roundtrip must preserve triple count"
        );
        assert!(
            out.contains("@en"),
            "language tag must survive roundtrip, got: {out}"
        );
    }

    /// Typed literals must survive an N-Quads roundtrip (N-Quads §2.5).
    #[test]
    fn nquads_typed_literal_roundtrip() {
        let nq = "<http://example/s> <http://example/p> \
                  \"1990-07-04\"^^<http://www.w3.org/2001/XMLSchema#date> \
                  <http://example/g> .\n";
        let mut ds = Datastore::new(64);
        crate::parse_nquads(&mut ds, nq.as_bytes()).unwrap();
        let out = serialize_nquads(&ds);
        let mut ds2 = Datastore::new(64);
        crate::parse_nquads(&mut ds2, out.as_bytes())
            .unwrap_or_else(|e| panic!("re-parse failed: {e}\noutput was:\n{out}"));
        assert_eq!(ds2.named_graphs.quad_count, 1);
        assert!(
            out.contains("XMLSchema#date"),
            "datatype IRI must survive roundtrip, got: {out}"
        );
    }

    /// W3C N-Quads spec §2 example: 8 triples in 2 named graphs.
    ///
    /// Source: <https://www.w3.org/TR/n-quads/#sec-examples>
    #[test]
    fn nquads_w3c_spec_example_roundtrip() {
        let nq = "\
<http://example.org/bob#me> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://xmlns.com/foaf/0.1/Person> <http://example.org/bob> .\n\
<http://example.org/bob#me> <http://xmlns.com/foaf/0.1/knows> <http://example.org/alice#me> <http://example.org/bob> .\n\
<http://example.org/bob#me> <http://schema.org/birthDate> \"1990-07-04\"^^<http://www.w3.org/2001/XMLSchema#date> <http://example.org/bob> .\n\
<http://example.org/bob#me> <http://xmlns.com/foaf/0.1/topic_interest> <http://www.wikidata.org/entity/Q12418> <http://example.org/bob> .\n\
<http://www.wikidata.org/entity/Q12418> <http://purl.org/dc/terms/title> \"Mona Lisa\" <http://example.org/bob> .\n\
<https://www.wikidata.org/wiki/Special:EntityData/Q12418> <http://schema.org/about> <http://www.wikidata.org/entity/Q12418> <http://example.org/bob> .\n\
<http://example.org/alice#me> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://xmlns.com/foaf/0.1/Person> <http://example.org/alice> .\n\
<http://example.org/alice#me> <http://xmlns.com/foaf/0.1/knows> <http://example.org/bob#me> <http://example.org/alice> .\n";

        let mut ds = Datastore::new(64);
        crate::parse_nquads(&mut ds, nq.as_bytes()).unwrap();
        assert_eq!(ds.named_graphs.quad_count, 8, "spec example has 8 triples");

        let out = serialize_nquads(&ds);
        let mut ds2 = Datastore::new(64);
        crate::parse_nquads(&mut ds2, out.as_bytes())
            .unwrap_or_else(|e| panic!("re-parse failed: {e}\noutput was:\n{out}"));

        assert_eq!(
            ds2.named_graphs.quad_count, 8,
            "roundtrip must preserve all 8 triples; output:\n{out}"
        );
        // Named graphs must be preserved
        assert!(
            ds2.lookup_named_graph_id("http://example.org/bob")
                .is_some(),
            "bob graph must survive roundtrip"
        );
        assert!(
            ds2.lookup_named_graph_id("http://example.org/alice")
                .is_some(),
            "alice graph must survive roundtrip"
        );
    }

    /// Serialize empty store produces empty N-Quads output.
    #[test]
    fn nquads_empty_store_is_empty() {
        let ds = Datastore::new(64);
        assert!(serialize_nquads(&ds).is_empty());
    }

    // ── TriG serializer tests ─────────────────────────────────────────────────

    /// Default-graph triples must be written without the GRAPH keyword (TriG §2.1).
    #[test]
    fn trig_default_graph_has_no_graph_keyword() {
        let mut ds = Datastore::new(64);
        let s = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/s".to_owned(),
        )));
        let p = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/p".to_owned(),
        )));
        let o = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/o".to_owned(),
        )));
        ds.add_triple(Triple {
            subject: s,
            predicate: p,
            obj: o,
        });
        let out = serialize_trig(&ds);
        assert!(
            !out.contains("GRAPH"),
            "default graph must not use GRAPH keyword, got: {out}"
        );
        assert!(
            out.contains("<http://example/s>"),
            "subject must appear, got: {out}"
        );
    }

    /// Named-graph triples must be wrapped in a GRAPH block (TriG §2.2).
    #[test]
    fn trig_named_graph_uses_graph_keyword() {
        let mut ds = Datastore::new(64);
        let s = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/s".to_owned(),
        )));
        let p = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/p".to_owned(),
        )));
        let o = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/o".to_owned(),
        )));
        let g = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example/g".to_owned(),
        )));
        ds.add_named_graph_triple(
            g,
            Triple {
                subject: s,
                predicate: p,
                obj: o,
            },
        );
        let out = serialize_trig(&ds);
        assert!(
            out.contains("GRAPH <http://example/g>"),
            "must have GRAPH block, got: {out}"
        );
        assert!(
            out.contains('{') && out.contains('}'),
            "block must have braces, got: {out}"
        );
    }

    /// W3C TriG spec §2 example: 8 triples in 2 named graphs, plus default graph.
    ///
    /// Source: <https://www.w3.org/TR/trig/#sec-examples>
    /// The TriG example is semantically equivalent to the N-Quads example above
    /// (same 8 triples, same 2 named graphs, no default-graph triples).
    #[test]
    fn trig_w3c_spec_example_roundtrip() {
        let trig = r#"
BASE <http://example.org/>
PREFIX rdf:    <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX xsd:    <http://www.w3.org/2001/XMLSchema#>
PREFIX foaf:   <http://xmlns.com/foaf/0.1/>
PREFIX dc:     <http://purl.org/dc/terms/>
PREFIX schema: <http://schema.org/>

<bob>
{
    <bob#me>
        rdf:type foaf:Person ;
        foaf:knows <alice#me> ;
        schema:birthDate "1990-07-04"^^xsd:date ;
        foaf:topic_interest <http://www.wikidata.org/entity/Q12418> .
    <http://www.wikidata.org/entity/Q12418>
        dc:title "Mona Lisa" .
    <https://www.wikidata.org/wiki/Special:EntityData/Q12418>
        schema:about <http://www.wikidata.org/entity/Q12418> .
}

<alice>
{
    <alice#me>
        rdf:type foaf:Person ;
        foaf:knows <bob#me> .
}
"#;
        let mut ds = Datastore::new(64);
        crate::parse_trig(&mut ds, trig.as_bytes()).unwrap();
        assert_eq!(ds.named_graphs.quad_count, 8, "spec example has 8 triples");

        let out = serialize_trig(&ds);
        let mut ds2 = Datastore::new(64);
        crate::parse_trig(&mut ds2, out.as_bytes())
            .unwrap_or_else(|e| panic!("re-parse failed: {e}\noutput was:\n{out}"));

        assert_eq!(
            ds2.named_graphs.quad_count, 8,
            "roundtrip must preserve all 8 triples; output:\n{out}"
        );
        assert!(
            ds2.lookup_named_graph_id("http://example.org/bob")
                .is_some(),
            "bob graph must survive TriG roundtrip"
        );
        assert!(
            ds2.lookup_named_graph_id("http://example.org/alice")
                .is_some(),
            "alice graph must survive TriG roundtrip"
        );
    }

    /// Multiple named graphs with default-graph triples all survive a TriG roundtrip.
    #[test]
    fn trig_mixed_default_and_named_graphs_roundtrip() {
        let trig = r#"
@prefix ex: <http://example.org/> .

ex:Alice a ex:Person .

<http://example.org/g1> {
    ex:Bob a ex:Employee .
    ex:Bob ex:worksFor ex:Acme .
}

<http://example.org/g2> {
    ex:Alice ex:knows ex:Bob .
}
"#;
        let mut ds = Datastore::new(64);
        crate::parse_trig(&mut ds, trig.as_bytes()).unwrap();
        let original_count = ds.named_graphs.quad_count;
        assert_eq!(original_count, 4, "fixture has 4 triples total");

        let out = serialize_trig(&ds);
        let mut ds2 = Datastore::new(64);
        crate::parse_trig(&mut ds2, out.as_bytes())
            .unwrap_or_else(|e| panic!("re-parse failed: {e}\noutput was:\n{out}"));
        assert_eq!(
            ds2.named_graphs.quad_count, original_count,
            "all triples must survive TriG roundtrip; output:\n{out}"
        );
        assert!(
            ds2.lookup_named_graph_id("http://example.org/g1").is_some(),
            "g1 must survive"
        );
        assert!(
            ds2.lookup_named_graph_id("http://example.org/g2").is_some(),
            "g2 must survive"
        );
    }

    /// Serialize TriG → parse → serialize as N-Quads: quad counts must match.
    #[test]
    fn trig_to_nquads_cross_format_roundtrip() {
        let trig = r#"
@prefix ex: <http://example.org/> .
<http://example.org/g> {
    ex:s ex:p ex:o .
    ex:s ex:q "hello"@en .
}
"#;
        let mut ds = Datastore::new(64);
        crate::parse_trig(&mut ds, trig.as_bytes()).unwrap();
        assert_eq!(ds.named_graphs.quad_count, 2);

        let nq_out = serialize_nquads(&ds);
        let mut ds2 = Datastore::new(64);
        crate::parse_nquads(&mut ds2, nq_out.as_bytes())
            .unwrap_or_else(|e| panic!("N-Quads re-parse failed: {e}\noutput:\n{nq_out}"));
        assert_eq!(
            ds2.named_graphs.quad_count, 2,
            "cross-format roundtrip must preserve triple count"
        );
    }

    /// Empty store produces empty TriG output.
    #[test]
    fn trig_empty_store_is_empty() {
        let ds = Datastore::new(64);
        assert!(serialize_trig(&ds).is_empty());
    }
}

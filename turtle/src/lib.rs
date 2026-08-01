/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

pub mod serialize;
pub use serialize::{
    format_literal, serialize_graph, serialize_nquads, serialize_nquads_graph, serialize_trig,
    serialize_trig_graph,
};

use dag_rdf::{Datastore, GraphElementId, IriReference, RdfLiteral, RdfResource, Triple};
use oxrdf::{GraphName, Literal, NamedOrBlankNode, Term};
use oxttl::{NQuadsParser, NTriplesParser, TriGParser, TurtleParseError, TurtleParser};
use std::io::Read;

pub fn parse_turtle<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleParseError> {
    for result in TurtleParser::new().for_reader(reader) {
        let triple = result?;
        let subject = intern_subject(datastore, triple.subject);
        let predicate = intern_named_node(datastore, triple.predicate.into_string());
        if let Some(obj) = intern_term(datastore, triple.object) {
            datastore.add_triple(Triple {
                subject,
                predicate,
                obj,
            });
        }
    }
    Ok(())
}

/// Like [`parse_turtle`] but resolves relative IRIs (e.g. `<#tm>`) against `base_iri`.
///
/// Used by the RML loader so that mapping files can use fragment-IRI subjects
/// such as `<#TriplesMap1>`.
pub fn parse_turtle_with_base<R: Read>(
    datastore: &mut Datastore,
    reader: R,
    base_iri: &str,
) -> Result<(), TurtleParseError> {
    let parser = TurtleParser::new()
        .with_base_iri(base_iri)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    for result in parser.for_reader(reader) {
        let triple = result?;
        let subject = intern_subject(datastore, triple.subject);
        let predicate = intern_named_node(datastore, triple.predicate.into_string());
        if let Some(obj) = intern_term(datastore, triple.object) {
            datastore.add_triple(Triple {
                subject,
                predicate,
                obj,
            });
        }
    }
    Ok(())
}

pub fn parse_trig<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleParseError> {
    for result in TriGParser::new().for_reader(reader) {
        let quad = result?;
        let subject = intern_subject(datastore, quad.subject);
        let predicate = intern_named_node(datastore, quad.predicate.into_string());
        let Some(obj) = intern_term(datastore, quad.object) else {
            continue;
        };
        let triple = Triple {
            subject,
            predicate,
            obj,
        };
        match quad.graph_name {
            GraphName::DefaultGraph => datastore.add_triple(triple),
            GraphName::NamedNode(node) => {
                let graph_id = intern_named_node(datastore, node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
            GraphName::BlankNode(node) => {
                let graph_id = datastore
                    .resources
                    .get_or_create_named_anon_resource(node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
        }
    }
    Ok(())
}

pub fn parse_ntriples<R: Read>(
    datastore: &mut Datastore,
    reader: R,
) -> Result<(), TurtleParseError> {
    for result in NTriplesParser::new().for_reader(reader) {
        let triple = result?;
        let subject = intern_subject(datastore, triple.subject);
        let predicate = intern_named_node(datastore, triple.predicate.into_string());
        if let Some(obj) = intern_term(datastore, triple.object) {
            datastore.add_triple(Triple {
                subject,
                predicate,
                obj,
            });
        }
    }
    Ok(())
}

pub fn parse_nquads<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), TurtleParseError> {
    for result in NQuadsParser::new().for_reader(reader) {
        let quad = result?;
        let subject = intern_subject(datastore, quad.subject);
        let predicate = intern_named_node(datastore, quad.predicate.into_string());
        let Some(obj) = intern_term(datastore, quad.object) else {
            continue;
        };
        let triple = Triple {
            subject,
            predicate,
            obj,
        };
        match quad.graph_name {
            GraphName::DefaultGraph => datastore.add_triple(triple),
            GraphName::NamedNode(node) => {
                let graph_id = intern_named_node(datastore, node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
            GraphName::BlankNode(node) => {
                let graph_id = datastore
                    .resources
                    .get_or_create_named_anon_resource(node.into_string());
                datastore.add_named_graph_triple(graph_id, triple);
            }
        }
    }
    Ok(())
}

fn intern_named_node(datastore: &mut Datastore, iri: String) -> GraphElementId {
    datastore.add_node_resource(RdfResource::Iri(IriReference(iri)))
}

fn intern_subject(datastore: &mut Datastore, subject: NamedOrBlankNode) -> GraphElementId {
    match subject {
        NamedOrBlankNode::NamedNode(node) => intern_named_node(datastore, node.into_string()),
        NamedOrBlankNode::BlankNode(node) => datastore
            .resources
            .get_or_create_named_anon_resource(node.into_string()),
    }
}

fn intern_term(datastore: &mut Datastore, term: Term) -> Option<GraphElementId> {
    match term {
        Term::NamedNode(node) => Some(intern_named_node(datastore, node.into_string())),
        Term::BlankNode(node) => Some(
            datastore
                .resources
                .get_or_create_named_anon_resource(node.into_string()),
        ),
        Term::Literal(lit) => Some(datastore.add_literal_resource(convert_literal(lit))),
        // RDF 1.2 triple term (`<<( s p o )>>`), object position only: oxrdf
        // 0.3.3's `Triple::subject` is typed `NamedOrBlankNode`, which cannot
        // itself hold a triple term, so `oxttl`'s grammar never emits a
        // `Term::Triple` whose inner subject is nested further. Recursing on
        // subject/object here is still correct for the object side, and the
        // subject side degrades gracefully to `intern_subject`.
        // Related: RDF 1.2 epic #143, Turtle parser #145.
        Term::Triple(triple) => {
            let s = intern_subject(datastore, triple.subject);
            let p = intern_named_node(datastore, triple.predicate.into_string());
            intern_term(datastore, triple.object).map(|o| datastore.add_triple_term(s, p, o))
        }
    }
}

fn convert_literal(lit: Literal) -> RdfLiteral {
    if let Some(lang) = lit.language() {
        return RdfLiteral::LangLiteral {
            literal: lit.value().to_owned(),
            lang: lang.to_owned(),
        };
    }
    let datatype = lit.datatype().into_owned().into_string();
    if datatype == "http://www.w3.org/2001/XMLSchema#string" {
        RdfLiteral::LiteralString(lit.value().to_owned())
    } else {
        RdfLiteral::TypedLiteral {
            literal: lit.value().to_owned(),
            type_iri: IriReference(datatype),
        }
    }
}

/// Parse a single Turtle literal term — e.g. `"5"^^<http://www.w3.org/2001/XMLSchema#integer>`,
/// `"hello"@en`, or a plain `"hello"` — back into an [`RdfLiteral`]. The
/// inverse of [`serialize::format_literal`].
///
/// Reuses the existing `oxttl` Turtle parser rather than hand-rolling a
/// second literal grammar: `literal_turtle` is embedded as the object of a
/// throwaway triple (`<urn:x-shacl:s> <urn:x-shacl:p> {literal} .`) and
/// parsed for real. Returns `None` if `literal_turtle` is not valid Turtle
/// literal syntax, does not parse to *exactly* one triple (guarding against
/// e.g. `"a" . <b> <c> <d>` smuggling extra statements past the wrapper), or
/// the resulting term is not a literal (an IRI/blank node/triple-term is not
/// a well-formed input for this function).
///
/// Only produces the three variants `oxttl`'s `Literal` can carry —
/// `LiteralString`, `LangLiteral`, `TypedLiteral` — never the built-in
/// numeric/boolean/temporal variants (`IntegerLiteral`, `BooleanLiteral`,
/// …), matching `convert_literal`'s (this module's) behaviour for parsed Turtle documents.
/// See [#337](https://github.com/daghovland/rdf-datalog/issues/337).
pub fn parse_literal_term(literal_turtle: &str) -> Option<RdfLiteral> {
    let wrapped = format!("<urn:x-shacl:s> <urn:x-shacl:p> {literal_turtle} .");
    let mut triples = TurtleParser::new().for_reader(wrapped.as_bytes());
    let first = triples.next()?.ok()?;
    // Must be exactly one triple: a second statement means literal_turtle
    // contained a terminating `.` followed by more input, which is not a
    // single well-formed literal term.
    if triples.next().is_some() {
        return None;
    }
    match first.object {
        Term::Literal(lit) => Some(convert_literal(lit)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;

    #[test]
    fn parse_simple_turtle() {
        let ttl = r#"
            @prefix ex: <http://example.org/> .
            ex:Alice a ex:Person .
            ex:Alice ex:name "Alice" .
        "#;
        let mut ds = Datastore::new(1000);
        parse_turtle(&mut ds, ttl.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    #[test]
    fn parse_trig_default_graph() {
        let trig = r#"
            @prefix ex: <http://example.org/> .
            ex:Alice a ex:Person .
        "#;
        let mut ds = Datastore::new(1000);
        parse_trig(&mut ds, trig.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    #[test]
    fn parse_trig_named_graph() {
        let trig = r#"
            @prefix ex: <http://example.org/> .
            <http://example.org/graph1> {
                ex:Alice a ex:Person .
                ex:Bob a ex:Person .
            }
        "#;
        let mut ds = Datastore::new(1000);
        parse_trig(&mut ds, trig.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    #[test]
    fn parse_trig_mixed_graphs() {
        let trig = r#"
            @prefix ex: <http://example.org/> .
            ex:Alice a ex:Person .
            <http://example.org/graph1> {
                ex:Bob a ex:Employee .
            }
        "#;
        let mut ds = Datastore::new(1000);
        parse_trig(&mut ds, trig.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    #[test]
    fn parse_ntriples_basic() {
        let nt = "<http://example.org/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/Person> .\n";
        let mut ds = Datastore::new(1000);
        parse_ntriples(&mut ds, nt.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    #[test]
    fn parse_nquads_default_graph() {
        let nq = "<http://example.org/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/Person> .\n";
        let mut ds = Datastore::new(1000);
        parse_nquads(&mut ds, nq.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    #[test]
    fn parse_nquads_named_graph() {
        let nq = "<http://example.org/Alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/Person> <http://example.org/g> .\n";
        let mut ds = Datastore::new(1000);
        parse_nquads(&mut ds, nq.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    // ── `format_literal` / `parse_literal_term` round trip (#337) ────────────

    /// A plain string literal, including embedded quote/newline/backslash
    /// requiring `escape_str` escaping, must format then parse back to the
    /// exact same `RdfLiteral::LiteralString`.
    #[test]
    fn literal_roundtrip_plain_string_with_escapes() {
        let lit = RdfLiteral::LiteralString("say \"hi\"\nbye\\end".to_string());
        let formatted = serialize::format_literal(&lit);
        assert_eq!(
            parse_literal_term(&formatted),
            Some(lit),
            "formatted was: {formatted}"
        );
    }

    /// A language-tagged literal must format then parse back to the exact
    /// same `RdfLiteral::LangLiteral`.
    #[test]
    fn literal_roundtrip_lang_literal() {
        let lit = RdfLiteral::LangLiteral {
            lang: "en".to_string(),
            literal: "hello".to_string(),
        };
        let formatted = serialize::format_literal(&lit);
        assert_eq!(formatted, "\"hello\"@en");
        assert_eq!(parse_literal_term(&formatted), Some(lit));
    }

    /// A typed literal must format then parse back to the exact same
    /// `RdfLiteral::TypedLiteral`.
    #[test]
    fn literal_roundtrip_typed_literal() {
        let lit = RdfLiteral::TypedLiteral {
            type_iri: IriReference("http://www.w3.org/2001/XMLSchema#integer".to_string()),
            literal: "5".to_string(),
        };
        let formatted = serialize::format_literal(&lit);
        assert_eq!(
            formatted,
            "\"5\"^^<http://www.w3.org/2001/XMLSchema#integer>"
        );
        assert_eq!(parse_literal_term(&formatted), Some(lit));
    }

    /// Built-in numeric/boolean/temporal variants don't round-trip to the
    /// *same* variant (only `LiteralString`/`LangLiteral`/`TypedLiteral` can
    /// come out of `parse_literal_term`, mirroring `convert_literal`'s
    /// behaviour for parsed Turtle documents) — but the semantic content
    /// (datatype IRI + lexical form) must be preserved as a `TypedLiteral`.
    #[test]
    fn literal_roundtrip_integer_literal_becomes_typed_literal() {
        let lit = RdfLiteral::IntegerLiteral(5.into());
        let formatted = serialize::format_literal(&lit);
        assert_eq!(
            formatted,
            "\"5\"^^<http://www.w3.org/2001/XMLSchema#integer>"
        );
        assert_eq!(
            parse_literal_term(&formatted),
            Some(RdfLiteral::TypedLiteral {
                type_iri: IriReference("http://www.w3.org/2001/XMLSchema#integer".to_string()),
                literal: "5".to_string(),
            })
        );
    }

    /// Trailing garbage after the literal (a second statement smuggled past
    /// the wrapper triple) must be rejected, not silently truncated.
    #[test]
    fn parse_literal_term_rejects_trailing_garbage() {
        let malicious = "\"a\" . <urn:x:evil-s> <urn:x:evil-p> <urn:x:evil-o>";
        assert_eq!(parse_literal_term(malicious), None);
    }

    /// Non-literal input (e.g. an IRI) is not a valid literal term.
    #[test]
    fn parse_literal_term_rejects_non_literal() {
        assert_eq!(parse_literal_term("<http://example.org/x>"), None);
    }
}

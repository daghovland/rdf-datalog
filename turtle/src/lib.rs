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
use oxrdfxml::{RdfXmlParseError, RdfXmlParser};
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

/// Parse an RDF/XML document (e.g. a `.rdf` file) into `datastore`'s default
/// graph.
///
/// Mirrors [`parse_turtle`]: `oxrdfxml`'s `RdfXmlParser` yields the same
/// `oxrdf::Triple { subject: NamedOrBlankNode, predicate: NamedNode, object:
/// Term }` shape `oxttl`'s parsers do, so the existing `intern_subject`/
/// `intern_named_node`/`intern_term` conversion helpers (written for Turtle)
/// apply unchanged — no second term-model adapter needed. See
/// `docs/plans/RDFXML_PARSER_PLAN.md` and epic
/// [#240](https://github.com/daghovland/rdf-datalog/issues/240).
pub fn parse_rdfxml<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), RdfXmlParseError> {
    for result in RdfXmlParser::new().for_reader(reader) {
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

/// Like [`parse_rdfxml`] but resolves relative IRIs against `base_iri`,
/// analogous to [`parse_turtle_with_base`] (PR
/// [#102](https://github.com/daghovland/rdf-datalog/issues/102)). An
/// in-document `xml:base` attribute, when present, still takes precedence
/// within its scope per the RDF/XML spec — this only supplies the base used
/// before any `xml:base` is seen (or when none is present at all).
pub fn parse_rdfxml_with_base<R: Read>(
    datastore: &mut Datastore,
    reader: R,
    base_iri: &str,
) -> Result<(), RdfXmlParseError> {
    let parser = RdfXmlParser::new()
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

    // ── RDF/XML (#750, epic #240) ─────────────────────────────────────────
    //
    // Edge cases per docs/plans/RDFXML_PARSER_PLAN.md and the epic's survey:
    // striped syntax, parseType="Collection"/"Literal", rdf:ID vs rdf:about
    // vs rdf:resource, xml:lang/xml:base scoping.

    const RDF_NS: &str = "xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"";

    /// Baseline smoke test: plain `rdf:about` subject with a nested
    /// `rdf:type` property element and a `schema:name` child-element
    /// literal, mirroring `parse_simple_turtle`.
    #[test]
    fn parse_rdfxml_basic_about_and_type() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:about="http://example.org/Alice">
                <rdf:type rdf:resource="http://example.org/Person"/>
                <ex:name>Alice</ex:name>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    /// Bare property attributes directly on the subject element (no nested
    /// property element) — common real-world shorthand.
    #[test]
    fn parse_rdfxml_bare_property_attributes() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <ex:Person rdf:about="http://example.org/Bar" ex:name="Bar"/>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        // rdf:type ex:Person (from the element name) + ex:name "Bar"
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    /// Blank node subjects via `rdf:nodeID`.
    #[test]
    fn parse_rdfxml_blank_node_subject() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:nodeID="b1">
                <ex:name>Anon</ex:name>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
    }

    /// `rdf:ID` mints a fragment IRI relative to the document base, distinct
    /// from `rdf:about` (full/relative IRI) and `rdf:resource` (object IRI).
    #[test]
    fn parse_rdfxml_id_about_and_resource() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/" xml:base="http://example.org/base/">
              <rdf:Description rdf:ID="alice">
                <ex:knows rdf:resource="http://example.org/base/#bob"/>
              </rdf:Description>
              <rdf:Description rdf:about="http://example.org/base/#bob">
                <ex:name>Bob</ex:name>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        // ex:knows (alice->bob) + ex:name "Bob" = 2 triples
        assert_eq!(ds.named_graphs.quad_count, 2);
        let alice = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/base/#alice".to_string(),
        )));
        assert!(
            ds.get_triples_with_subject(alice).next().is_some(),
            "rdf:ID should resolve against xml:base to a fragment IRI"
        );
    }

    /// `xml:lang` applies to a literal and is inherited down the element
    /// tree per XML scoping rules, until overridden by a nested `xml:lang`.
    #[test]
    fn parse_rdfxml_lang_scoping() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:about="http://example.org/Alice" xml:lang="en">
                <ex:name>Alice</ex:name>
                <ex:nickname xml:lang="fr">Alicette</ex:nickname>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 2);
        let name_en = ds.add_literal_resource(RdfLiteral::LangLiteral {
            literal: "Alice".to_string(),
            lang: "en".to_string(),
        });
        assert!(
            ds.get_triples_with_object(name_en).next().is_some(),
            "inherited xml:lang should produce an 'en'-tagged literal"
        );
        let nickname_fr = ds.add_literal_resource(RdfLiteral::LangLiteral {
            literal: "Alicette".to_string(),
            lang: "fr".to_string(),
        });
        assert!(
            ds.get_triples_with_object(nickname_fr).next().is_some(),
            "nested xml:lang should override the inherited tag"
        );
    }

    /// Striped syntax: a property element's value is itself a nested
    /// `rdf:Description`, alternating node/property layers.
    #[test]
    fn parse_rdfxml_striped_syntax() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:about="http://example.org/Alice">
                <ex:knows>
                  <rdf:Description rdf:about="http://example.org/Bob">
                    <ex:name>Bob</ex:name>
                  </rdf:Description>
                </ex:knows>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        // ex:knows (alice->bob) + ex:name "Bob" = 2 triples
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    /// `rdf:parseType="Resource"` collapses striping: the property element
    /// itself becomes an (anonymous) resource without an explicit nested
    /// `rdf:Description`.
    #[test]
    fn parse_rdfxml_parse_type_resource() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:about="http://example.org/Alice">
                <ex:address rdf:parseType="Resource">
                  <ex:city>Oslo</ex:city>
                </ex:address>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        // ex:address (alice->_:b) + ex:city "Oslo" = 2 triples
        assert_eq!(ds.named_graphs.quad_count, 2);
    }

    /// `rdf:parseType="Collection"` builds an RDF list (`rdf:first`/
    /// `rdf:rest`/`rdf:nil`) from XML child elements with no explicit list
    /// syntax of its own.
    #[test]
    fn parse_rdfxml_parse_type_collection() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:about="http://example.org/Alice">
                <ex:favorites rdf:parseType="Collection">
                  <rdf:Description rdf:about="http://example.org/Red"/>
                  <rdf:Description rdf:about="http://example.org/Blue"/>
                </ex:favorites>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        // ex:favorites (alice->list head) + 2x rdf:first + 2x rdf:rest = 5 triples
        assert_eq!(ds.named_graphs.quad_count, 5);
    }

    /// `rdf:parseType="Literal"` produces an `rdf:XMLLiteral`-typed literal
    /// from the element's inner XML content, verbatim (not parsed as RDF).
    #[test]
    fn parse_rdfxml_parse_type_literal() {
        let rdf = format!(
            r#"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:about="http://example.org/Alice">
                <ex:bio rdf:parseType="Literal"><b>Hi</b></ex:bio>
              </rdf:Description>
            </rdf:RDF>"#
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml(&mut ds, rdf.as_bytes()).expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
        // oxrdfxml canonicalizes in-scope XML namespace declarations onto the
        // literal's root element (so the `rdf:XMLLiteral` lexical form is
        // self-contained even outside its original document context) — it
        // is not a byte-for-byte copy of the source markup.
        let bio = ds.add_literal_resource(RdfLiteral::TypedLiteral {
            literal: "<b xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" xmlns:ex=\"http://example.org/\">Hi</b>".to_string(),
            type_iri: IriReference(
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#XMLLiteral".to_string(),
            ),
        });
        assert!(
            ds.get_triples_with_object(bio).next().is_some(),
            "parseType=Literal should produce a verbatim rdf:XMLLiteral"
        );
    }

    /// Malformed input (unclosed tag) must return `Err`, not panic —
    /// baseline error-path coverage mirroring other formats in this file.
    #[test]
    fn parse_rdfxml_rejects_malformed_input() {
        let rdf = format!(r#"<?xml version="1.0"?><rdf:RDF {RDF_NS}><rdf:Description"#);
        let mut ds = Datastore::new(1000);
        assert!(parse_rdfxml(&mut ds, rdf.as_bytes()).is_err());
    }

    /// `parse_rdfxml_with_base`: a caller-supplied base IRI is used to
    /// resolve `rdf:about`'s relative IRI when the document has no
    /// `xml:base` of its own.
    #[test]
    fn parse_rdfxml_with_base_resolves_relative_about() {
        let rdf = format!(
            r##"<?xml version="1.0"?>
            <rdf:RDF {RDF_NS} xmlns:ex="http://example.org/">
              <rdf:Description rdf:about="#alice">
                <ex:name>Alice</ex:name>
              </rdf:Description>
            </rdf:RDF>"##
        );
        let mut ds = Datastore::new(1000);
        parse_rdfxml_with_base(&mut ds, rdf.as_bytes(), "http://example.org/doc")
            .expect("parse should succeed");
        assert_eq!(ds.named_graphs.quad_count, 1);
        let alice = ds.add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/doc#alice".to_string(),
        )));
        assert!(
            ds.get_triples_with_subject(alice).next().is_some(),
            "relative rdf:about should resolve against the caller-supplied base"
        );
    }
}

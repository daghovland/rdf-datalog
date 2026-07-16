/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! IRI parsing and resolution: `fullIRI`, `abbreviatedIRI` (prefixed names,
//! including the default `:` prefix), and `simpleIRI` (bare names, resolved
//! against the default prefix). All three forms resolve to
//! [`owl_ontology::FullIri`] via [`ParserContext`].

use crate::tokens::{identifier, is_ident_char, punct, sp, tok};
use ingress::IriReference;
use nom::IResult;
use owl_ontology::FullIri;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

/// Parsing state threaded through the whole document: prefix map (keyed by
/// prefix name, `""` for the default `:` prefix) and blank-node-label ->
/// anonymous-individual-id assignment (stable within one parse call).
#[derive(Default)]
pub struct ParserContext {
    pub prefixes: RefCell<HashMap<String, String>>,
    next_anon_individual: Cell<u32>,
    blank_node_labels: RefCell<HashMap<String, u32>>,
    /// IRIs (fully resolved) known to be `DataProperty:` frames, populated by
    /// a pre-scan pass before frame parsing. Used to disambiguate
    /// `some`/`only`/`min`/`max`/`exactly` restrictions between object- and
    /// data-property readings; see `class_expr.rs` module docs.
    data_property_iris: RefCell<HashSet<String>>,
}

impl ParserContext {
    /// A fresh context with the standard `rdf:`, `rdfs:`, `owl:`, `xsd:`
    /// prefixes pre-declared (mirroring `sparql_parser`/`datalog_parser`'s
    /// built-in prefixes); a document's own `Prefix:` declarations may
    /// override any of these.
    pub fn new() -> Self {
        let ctx = Self::default();
        ctx.declare_prefix("rdf", ingress::RDF);
        ctx.declare_prefix("rdfs", ingress::RDFS);
        ctx.declare_prefix("owl", ingress::OWL);
        ctx.declare_prefix("xsd", ingress::XSD);
        ctx
    }

    pub fn declare_prefix(&self, name: &str, iri: &str) {
        self.prefixes
            .borrow_mut()
            .insert(name.to_string(), iri.to_string());
    }

    /// Resolve a prefixed name's namespace part (`prefix`) to a full IRI
    /// string, or `None` if the prefix is undeclared.
    fn resolve_prefix(&self, prefix: &str) -> Option<String> {
        self.prefixes.borrow().get(prefix).cloned()
    }

    /// Assign (or look up) a stable numeric id for a blank node label
    /// (`_:label`), for use as `Individual::AnonymousIndividual`.
    pub fn anon_individual_for_label(&self, label: &str) -> u32 {
        if let Some(id) = self.blank_node_labels.borrow().get(label) {
            return *id;
        }
        let id = self.next_anon_individual.get();
        self.next_anon_individual.set(id + 1);
        self.blank_node_labels
            .borrow_mut()
            .insert(label.to_string(), id);
        id
    }

    /// Record `iri` (a fully-resolved IRI string) as a known data property.
    pub fn mark_data_property(&self, iri: &str) {
        self.data_property_iris.borrow_mut().insert(iri.to_string());
    }

    /// Whether `iri` (a fully-resolved IRI string) was pre-scanned as a
    /// `DataProperty:` frame header.
    pub fn is_known_data_property(&self, iri: &str) -> bool {
        self.data_property_iris.borrow().contains(iri)
    }
}

/// `fullIRI ::= '<' ... '>'`
pub(crate) fn full_iri(input: &str) -> IResult<&str, String> {
    let (input, _) = nom::character::complete::char('<')(input)?;
    let end = input.find('>').ok_or_else(|| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
    })?;
    let iri = input[..end].to_string();
    let input = &input[end + 1..];
    let (input, ()) = sp(input)?;
    Ok((input, iri))
}

/// `abbreviatedIRI ::= prefixName ':' identifier`, including the default
/// (empty) prefix name, e.g. `:Pizza` or `owl:Thing`.
fn prefixed_name(input: &str) -> IResult<&str, (String, String)> {
    // Prefix part: zero or more identifier characters (may be empty for `:`).
    let prefix_end = input
        .find(|c: char| !is_ident_char(c))
        .unwrap_or(input.len());
    let prefix = &input[..prefix_end];
    let rest = &input[prefix_end..];
    let (rest, _) = punct(':')(rest)?;
    // Local part: zero or more identifier characters (may be empty, e.g. `ex:`).
    let local_end = rest.find(|c: char| !is_ident_char(c)).unwrap_or(rest.len());
    let local = &rest[..local_end];
    let rest = &rest[local_end..];
    let (rest, ()) = sp(rest)?;
    Ok((rest, (prefix.to_string(), local.to_string())))
}

/// Parse any IRI form and resolve it to a [`FullIri`] using `ctx`'s prefix map.
///
/// `simpleIRI` (a bare identifier with no `:`) is resolved against the
/// default (`""`) prefix.
pub(crate) fn iri<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, FullIri> {
    move |input: &'a str| {
        if input.starts_with('<') {
            let (input, s) = full_iri(input)?;
            return Ok((input, FullIri(IriReference(s))));
        }
        // Try prefixed name (contains ':' before any non-identifier char).
        if let Ok((rest, (prefix, local))) = prefixed_name(input)
            && let Some(ns) = ctx.resolve_prefix(&prefix)
        {
            return Ok((rest, FullIri(IriReference(format!("{ns}{local}")))));
        }
        // Fall back to simpleIRI: bare identifier resolved against default prefix.
        // Reserved frame/section keywords never resolve as a simpleIRI (see
        // `tokens::is_reserved_simple_name`) — this disambiguates e.g. an
        // absent, optional cardinality filler from the start of the next frame.
        let (rest, name) = identifier(input)?;
        if crate::tokens::is_reserved_simple_name(name) {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
        match ctx.resolve_prefix("") {
            Some(ns) => Ok((rest, FullIri(IriReference(format!("{ns}{name}"))))),
            None => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            ))),
        }
    }
}

/// A `nodeID` (blank node label), e.g. `_:x`.
pub(crate) fn node_id(input: &str) -> IResult<&str, String> {
    let (input, _) = nom::bytes::complete::tag("_:")(input)?;
    tok(|i: &str| {
        let end = i.find(|c: char| !is_ident_char(c)).unwrap_or(i.len());
        if end == 0 {
            return Err(nom::Err::Error(nom::error::Error::new(
                i,
                nom::error::ErrorKind::Alpha,
            )));
        }
        Ok((&i[end..], i[..end].to_string()))
    })(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_full_iri() {
        let ctx = ParserContext::new();
        let (rest, i) = iri(&ctx)("<http://example.org/Pizza> more").unwrap();
        assert_eq!(i.0.0, "http://example.org/Pizza");
        assert_eq!(rest, "more");
    }

    #[test]
    fn resolves_prefixed_and_default_name() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        ctx.declare_prefix("owl", "http://www.w3.org/2002/07/owl#");
        let (rest, i) = iri(&ctx)(":Pizza rest").unwrap();
        assert_eq!(i.0.0, "http://example.org/Pizza");
        assert_eq!(rest, "rest");
        let (_, i2) = iri(&ctx)("owl:Thing").unwrap();
        assert_eq!(i2.0.0, "http://www.w3.org/2002/07/owl#Thing");
    }

    #[test]
    fn resolves_simple_iri_against_default_prefix() {
        let ctx = ParserContext::new();
        ctx.declare_prefix("", "http://example.org/");
        let (rest, i) = iri(&ctx)("Pizza rest").unwrap();
        assert_eq!(i.0.0, "http://example.org/Pizza");
        assert_eq!(rest, "rest");
    }
}

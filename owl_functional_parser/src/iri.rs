/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `IRI ::= fullIRI | abbreviatedIRI`. Unlike Manchester Syntax, there is no
//! bare unprefixed `simpleIRI` production in Functional-Style Syntax, so IRI
//! resolution only has two cases.

use crate::tokens::{is_ident_char, punct, sp, tok};
use ingress::IriReference;
use nom::IResult;
use owl_ontology::FullIri;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Parsing state threaded through a whole document: prefix map (keyed by
/// prefix name, `""` for the default `:` prefix) and blank-node-label ->
/// anonymous-individual-id assignment (stable within one parse call).
/// Mirrors `manchester_parser::iri::ParserContext`, minus its
/// `data_property_iris` pre-scan set: Functional-Style Syntax's
/// `Keyword(args...)` tagging means object- vs. data-property positions are
/// never ambiguous (the keyword itself disambiguates), so no pre-scan pass
/// is needed here.
#[derive(Default)]
pub struct ParserContext {
    pub prefixes: RefCell<HashMap<String, String>>,
    next_anon_individual: Cell<u32>,
    blank_node_labels: RefCell<HashMap<String, u32>>,
}

impl ParserContext {
    /// A fresh context with the standard `rdf:`, `rdfs:`, `owl:`, `xsd:`
    /// prefixes pre-declared; a document's own `Prefix(...)` declarations
    /// may override any of these.
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

/// `abbreviatedIRI ::= PNAME_LN`, e.g. `:Pizza` or `owl:Thing` (including the
/// default, empty prefix name).
fn prefixed_name(input: &str) -> IResult<&str, (String, String)> {
    let prefix_end = input
        .find(|c: char| !is_ident_char(c))
        .unwrap_or(input.len());
    let prefix = &input[..prefix_end];
    let rest = &input[prefix_end..];
    let (rest, _) = punct(':')(rest)?;
    let local_end = rest.find(|c: char| !is_ident_char(c)).unwrap_or(rest.len());
    let local = &rest[..local_end];
    let rest = &rest[local_end..];
    let (rest, ()) = sp(rest)?;
    Ok((rest, (prefix.to_string(), local.to_string())))
}

/// Parse `IRI ::= fullIRI | abbreviatedIRI` and resolve it to a [`FullIri`]
/// using `ctx`'s prefix map.
pub(crate) fn iri<'a>(ctx: &'a ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, FullIri> {
    move |input: &'a str| {
        if input.starts_with('<') {
            let (input, s) = full_iri(input)?;
            return Ok((input, FullIri(IriReference(s))));
        }
        let (rest, (prefix, local)) = prefixed_name(input)?;
        match ctx.resolve_prefix(&prefix) {
            Some(ns) => Ok((rest, FullIri(IriReference(format!("{ns}{local}"))))),
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
        let (rest, i) = iri(&ctx)(":Pizza rest").unwrap();
        assert_eq!(i.0.0, "http://example.org/Pizza");
        assert_eq!(rest, "rest");
        let (_, i2) = iri(&ctx)("owl:Thing").unwrap();
        assert_eq!(i2.0.0, "http://www.w3.org/2002/07/owl#Thing");
    }

    #[test]
    fn rejects_undeclared_prefix() {
        let ctx = ParserContext::new();
        assert!(iri(&ctx)("nope:Thing").is_err());
    }
}

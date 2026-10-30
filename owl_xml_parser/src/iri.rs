/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `<Prefix>` collection, `IRI=`/`abbreviatedIRI=` resolution to
//! [`owl_ontology::FullIri`], and `<AnonymousIndividual nodeID="...">`
//! label assignment. See `docs/plans/OWL_XML_PLAN.md`'s "Intermediate
//! design" / "#608" sections: unlike `manchester_parser`/
//! `owl_functional_parser`'s `ParserContext`, prefixes don't need interior
//! mutability -- the whole document is already parsed into a tree by
//! `roxmltree::Document::parse`, so they're collected once, up front.
//! `AnonymousIndividual` label -> numeric-id assignment (#608) does need
//! interior mutability, since it's discovered lazily while descending into
//! `Individual`/`AnnotationValue` positions rather than collected in one
//! up-front pass like prefixes are -- so `Prefixes` (kept as one struct
//! threaded everywhere a `&Prefixes` was already threaded, rather than
//! introducing a second parameter into every function in this crate) grew
//! a `next_anon`/`labels` pair mirroring
//! `owl_functional_parser::iri::ParserContext::anon_individual_for_label`.

use owl_ontology::FullIri;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Prefix map (`""` for the default `:` prefix -> namespace IRI) plus
/// per-document `<AnonymousIndividual nodeID="...">` label -> numeric-id
/// assignment. One instance is built per [`crate::parse`] call, so ids are
/// scoped per document automatically.
#[derive(Default)]
pub(crate) struct Prefixes {
    map: HashMap<String, String>,
    next_anon: Cell<u32>,
    labels: RefCell<HashMap<String, u32>>,
}

impl Prefixes {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&String> {
        self.map.get(name)
    }

    pub(crate) fn insert(&mut self, name: String, iri: String) {
        self.map.insert(name, iri);
    }

    /// Assign (or look up) a stable numeric id for an `<AnonymousIndividual
    /// nodeID="...">` label, for use as `Individual::AnonymousIndividual`
    /// / `GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(..))`.
    /// The same label always resolves to the same id within one document.
    pub(crate) fn anon_individual_for_label(&self, label: &str) -> u32 {
        if let Some(id) = self.labels.borrow().get(label) {
            return *id;
        }
        let id = self.next_anon.get();
        self.next_anon.set(id + 1);
        self.labels.borrow_mut().insert(label.to_string(), id);
        id
    }
}

/// Collect every `<Prefix name="..." IRI="..."/>` that is a direct child of
/// the `<Ontology>` root element.
pub(crate) fn collect_prefixes(root: roxmltree::Node) -> Prefixes {
    let mut prefixes = Prefixes::new();
    for child in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Prefix")
    {
        let name = child.attribute("name").unwrap_or("").to_string();
        if let Some(iri) = child.attribute("IRI") {
            prefixes.insert(name, iri.to_string());
        }
    }
    prefixes
}

/// Resolve the `IRI=`/`abbreviatedIRI=` attribute on `node` to a
/// [`FullIri`]. Exactly one of the two attributes is expected, per the
/// OWL/XML spec's `IRI` production.
///
/// `IRI=` values are returned as-is (full-IRI-only for #605 -- relative-IRI
/// resolution against `xml:base`/the ontology IRI is deferred, see the plan
/// doc's "Deferred follow-up").
pub(crate) fn resolve_iri(node: roxmltree::Node, prefixes: &Prefixes) -> Result<FullIri, String> {
    if let Some(full) = node.attribute("IRI") {
        return Ok(FullIri(ingress::IriReference(full.to_string())));
    }
    if let Some(abbrev) = node.attribute("abbreviatedIRI") {
        let (prefix_name, local) = abbrev.split_once(':').ok_or_else(|| {
            format!(
                "abbreviatedIRI {abbrev:?} on <{}> has no ':'",
                node.tag_name().name()
            )
        })?;
        let ns = prefixes.get(prefix_name).ok_or_else(|| {
            format!(
                "abbreviatedIRI {abbrev:?} on <{}> uses undeclared prefix {prefix_name:?}",
                node.tag_name().name()
            )
        })?;
        return Ok(FullIri(ingress::IriReference(format!("{ns}{local}"))));
    }
    Err(format!(
        "<{}> has neither IRI= nor abbreviatedIRI=",
        node.tag_name().name()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn resolves_full_iri_attribute() {
        let d = doc(r#"<Class IRI="http://example.org/pizza#Pizza"/>"#);
        let iri = resolve_iri(d.root_element(), &Prefixes::new()).unwrap();
        assert_eq!(iri.0.0, "http://example.org/pizza#Pizza");
    }

    #[test]
    fn resolves_abbreviated_iri_against_prefix_map() {
        let mut prefixes = Prefixes::new();
        prefixes.insert("".to_string(), "http://example.org/pizza#".to_string());
        let d = doc(r#"<Class abbreviatedIRI=":Pizza"/>"#);
        let iri = resolve_iri(d.root_element(), &prefixes).unwrap();
        assert_eq!(iri.0.0, "http://example.org/pizza#Pizza");
    }

    #[test]
    fn errors_on_undeclared_prefix() {
        let d = doc(r#"<Class abbreviatedIRI="owl:Thing"/>"#);
        assert!(resolve_iri(d.root_element(), &Prefixes::new()).is_err());
    }
}

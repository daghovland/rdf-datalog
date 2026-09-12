/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `<Prefix>` collection and `IRI=`/`abbreviatedIRI=` resolution to
//! [`owl_ontology::FullIri`]. See `docs/plans/OWL_XML_PLAN.md`'s
//! "Intermediate design" section: unlike `manchester_parser`/
//! `owl_functional_parser`'s `ParserContext`, no interior mutability is
//! needed here — the whole document is already parsed into a tree by
//! `roxmltree::Document::parse`, so prefixes are collected once, up front,
//! into a plain `HashMap`.

use owl_ontology::FullIri;
use std::collections::HashMap;

/// Prefix name (`""` for the default `:` prefix) -> namespace IRI.
pub(crate) type Prefixes = HashMap<String, String>;

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

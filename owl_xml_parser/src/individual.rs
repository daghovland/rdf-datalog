/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `Individual ::= NamedIndividual | AnonymousIndividual`
//!
//! `<AnonymousIndividual nodeID="...">` resolves through
//! [`Prefixes::anon_individual_for_label`], assigning a stable per-document
//! numeric id the same `nodeID` always maps back to (#608).

use crate::iri::{Prefixes, resolve_iri};
use owl_ontology::Individual;

/// Parse a `<NamedIndividual>`/`<AnonymousIndividual>` element into an
/// [`Individual`].
pub(crate) fn individual(node: roxmltree::Node, prefixes: &Prefixes) -> Result<Individual, String> {
    match node.tag_name().name() {
        "NamedIndividual" => Ok(Individual::NamedIndividual(resolve_iri(node, prefixes)?)),
        "AnonymousIndividual" => {
            let node_id = node
                .attribute("nodeID")
                .ok_or_else(|| "<AnonymousIndividual> has no nodeID attribute".to_string())?;
            Ok(Individual::AnonymousIndividual(
                prefixes.anon_individual_for_label(node_id),
            ))
        }
        other => Err(format!("<{other}> is not a valid Individual")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_named_individual() {
        let d = doc(r#"<NamedIndividual IRI="http://example.org/Alice"/>"#);
        let i = individual(d.root_element(), &Prefixes::new()).unwrap();
        assert_eq!(
            i,
            Individual::NamedIndividual(owl_ontology::FullIri(ingress::IriReference(
                "http://example.org/Alice".to_string()
            )))
        );
    }

    #[test]
    fn parses_anonymous_individual() {
        let d = doc(r#"<AnonymousIndividual nodeID="x"/>"#);
        let i = individual(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(i, Individual::AnonymousIndividual(_)));
    }

    #[test]
    fn same_node_id_is_the_same_anonymous_individual() {
        let prefixes = Prefixes::new();
        let d1 = doc(r#"<AnonymousIndividual nodeID="x"/>"#);
        let d2 = doc(r#"<AnonymousIndividual nodeID="x"/>"#);
        let i1 = individual(d1.root_element(), &prefixes).unwrap();
        let i2 = individual(d2.root_element(), &prefixes).unwrap();
        assert_eq!(i1, i2);
    }

    #[test]
    fn different_node_ids_are_different_anonymous_individuals() {
        let prefixes = Prefixes::new();
        let d1 = doc(r#"<AnonymousIndividual nodeID="x"/>"#);
        let d2 = doc(r#"<AnonymousIndividual nodeID="y"/>"#);
        let i1 = individual(d1.root_element(), &prefixes).unwrap();
        let i2 = individual(d2.root_element(), &prefixes).unwrap();
        assert_ne!(i1, i2);
    }

    #[test]
    fn errors_on_missing_node_id() {
        let d = doc(r#"<AnonymousIndividual/>"#);
        assert!(individual(d.root_element(), &Prefixes::new()).is_err());
    }
}

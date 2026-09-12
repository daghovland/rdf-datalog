/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `Individual ::= NamedIndividual | AnonymousIndividual`
//!
//! Only `<NamedIndividual>` is implemented here -- `<AnonymousIndividual
//! nodeID="...">` needs a per-document label->id counter (matching
//! `manchester_parser`/`owl_functional_parser`'s `ParserContext::
//! anon_individual_for_label`), which is out of scope until
//! [#608](https://github.com/daghovland/rdf-datalog/issues/608) introduces
//! ABox/individual handling in full (see `docs/plans/OWL_XML_PLAN.md`'s
//! "#606" section).

use crate::iri::{Prefixes, resolve_iri};
use owl_ontology::Individual;

/// Parse a `<NamedIndividual>` element into an [`Individual`]. Errors with a
/// clear pointer to #608 on `<AnonymousIndividual>`.
pub(crate) fn individual(node: roxmltree::Node, prefixes: &Prefixes) -> Result<Individual, String> {
    match node.tag_name().name() {
        "NamedIndividual" => Ok(Individual::NamedIndividual(resolve_iri(node, prefixes)?)),
        "AnonymousIndividual" => {
            Err("<AnonymousIndividual> is not yet supported (see #608)".to_string())
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
    fn errors_on_anonymous_individual_with_608_reference() {
        let d = doc(r#"<AnonymousIndividual nodeID="x"/>"#);
        let err = individual(d.root_element(), &Prefixes::new()).unwrap_err();
        assert!(err.contains("608"));
    }
}

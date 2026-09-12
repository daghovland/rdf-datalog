/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `ObjectPropertyExpression ::= ObjectProperty | ObjectInverseOf`
//! `DataPropertyExpression ::= DataProperty`
//!
//! Unlike Functional-Style Syntax, both are always their own distinctly
//! named element (`<ObjectProperty>`, `<ObjectInverseOf>`, `<DataProperty>`)
//! -- there is no bare-IRI-ambiguity to disambiguate.

use crate::iri::{Prefixes, resolve_iri};
use owl_ontology::{DataProperty, ObjectPropertyExpression};

/// Parse an `<ObjectProperty>` or `<ObjectInverseOf>` element into an
/// [`ObjectPropertyExpression`].
pub(crate) fn object_property_expression(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<ObjectPropertyExpression, String> {
    match node.tag_name().name() {
        "ObjectProperty" => Ok(ObjectPropertyExpression::NamedObjectProperty(resolve_iri(
            node, prefixes,
        )?)),
        "ObjectInverseOf" => {
            let child = node
                .children()
                .find(|n| n.is_element())
                .ok_or_else(|| "<ObjectInverseOf> has no child element".to_string())?;
            Ok(ObjectPropertyExpression::InverseObjectProperty(Box::new(
                object_property_expression(child, prefixes)?,
            )))
        }
        other => Err(format!("<{other}> is not a valid ObjectPropertyExpression")),
    }
}

/// Parse a `<DataProperty>` element into a [`DataProperty`].
/// `DataPropertyExpression ::= DataProperty` -- always a bare element, no
/// wrapper keyword.
pub(crate) fn data_property_expression(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<DataProperty, String> {
    if node.tag_name().name() != "DataProperty" {
        return Err(format!(
            "<{}> is not a valid DataPropertyExpression",
            node.tag_name().name()
        ));
    }
    resolve_iri(node, prefixes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_named_object_property() {
        let d = doc(r#"<ObjectProperty IRI="http://example.org/hasTopping"/>"#);
        let p = object_property_expression(d.root_element(), &Prefixes::new()).unwrap();
        assert_eq!(
            p,
            ObjectPropertyExpression::NamedObjectProperty(owl_ontology::FullIri(
                ingress::IriReference("http://example.org/hasTopping".to_string())
            ))
        );
    }

    #[test]
    fn parses_object_inverse_of() {
        let d = doc(
            r#"<ObjectInverseOf><ObjectProperty IRI="http://example.org/hasTopping"/></ObjectInverseOf>"#,
        );
        let p = object_property_expression(d.root_element(), &Prefixes::new()).unwrap();
        match p {
            ObjectPropertyExpression::InverseObjectProperty(inner) => {
                assert!(matches!(
                    *inner,
                    ObjectPropertyExpression::NamedObjectProperty(_)
                ));
            }
            other => panic!("expected InverseObjectProperty, got {other:?}"),
        }
    }

    #[test]
    fn parses_data_property() {
        let d = doc(r#"<DataProperty IRI="http://example.org/hasCalories"/>"#);
        let p = data_property_expression(d.root_element(), &Prefixes::new()).unwrap();
        assert_eq!(p.0.0, "http://example.org/hasCalories");
    }

    #[test]
    fn errors_on_wrong_tag() {
        let d = doc(r#"<ObjectProperty IRI="http://example.org/hasTopping"/>"#);
        assert!(data_property_expression(d.root_element(), &Prefixes::new()).is_err());
    }
}

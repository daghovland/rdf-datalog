/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `<Annotation>` -> `owl_ontology::Annotation` (`(AnnotationProperty,
//! AnnotationValue)`).
//!
//! `AnnotationSubject ::= IRI | AnonymousIndividual` /
//! `AnnotationValue ::= AnonymousIndividual | IRI | Literal` per the spec's
//! §10 -- only the `IRI` and `Literal` value shapes are covered here (an
//! `AnonymousIndividual` value needs `individual.rs`, which doesn't exist
//! until [#608](https://github.com/daghovland/rdf-datalog/issues/608)
//! introduces ABox/individual handling).

use crate::iri::{Prefixes, resolve_iri};
use ingress::{GraphElement, IriReference, RdfLiteral};
use owl_ontology::{Annotation, AnnotationValue};

/// Parse a `<Literal>` element's text content (plus optional `xml:lang`/
/// `datatypeIRI` attributes) into a [`GraphElement::GraphLiteral`]. Shared
/// with `class_expr.rs`/`data_range.rs` (`DataHasValue`/`DataOneOf`/
/// `FacetRestriction` values are the same `<Literal>` production).
pub(crate) fn parse_literal(node: roxmltree::Node) -> GraphElement {
    let text = node.text().unwrap_or("").to_string();
    if let Some(lang) = node.attribute(("http://www.w3.org/XML/1998/namespace", "lang")) {
        return GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
            lang: lang.to_string(),
            literal: text,
        });
    }
    if let Some(datatype) = node.attribute("datatypeIRI") {
        return GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
            type_iri: IriReference(datatype.to_string()),
            literal: text,
        });
    }
    GraphElement::GraphLiteral(RdfLiteral::LiteralString(text))
}

/// Parse an `<Annotation>` element: an `<AnnotationProperty>` child (the
/// property), followed by exactly one value child (`<Literal>`, `<IRI>`, or
/// `<AbbreviatedIRI>`).
pub(crate) fn parse_annotation(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<Annotation, String> {
    let mut property = None;
    let mut value = None;

    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "AnnotationProperty" => {
                property = Some(resolve_iri(child, prefixes)?);
            }
            "Literal" => {
                value = Some(AnnotationValue::LiteralAnnotation(parse_literal(child)));
            }
            "IRI" => {
                let text = child.text().unwrap_or("").trim().to_string();
                value = Some(AnnotationValue::IriAnnotation(owl_ontology::FullIri(
                    IriReference(text),
                )));
            }
            "AbbreviatedIRI" => {
                let text = child.text().unwrap_or("").trim();
                let (prefix_name, local) = text
                    .split_once(':')
                    .ok_or_else(|| format!("<AbbreviatedIRI>{text}</AbbreviatedIRI> has no ':'"))?;
                let ns = prefixes.get(prefix_name).ok_or_else(|| {
                    format!("<AbbreviatedIRI> uses undeclared prefix {prefix_name:?}")
                })?;
                value = Some(AnnotationValue::IriAnnotation(owl_ontology::FullIri(
                    IriReference(format!("{ns}{local}")),
                )));
            }
            other => {
                return Err(format!(
                    "<Annotation> value element <{other}> not yet supported (see #608)"
                ));
            }
        }
    }

    let property =
        property.ok_or_else(|| "<Annotation> has no <AnnotationProperty>".to_string())?;
    let value = value.ok_or_else(|| "<Annotation> has no value element".to_string())?;
    Ok((property, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_literal_annotation() {
        let d = doc(r#"<Annotation>
                 <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                 <Literal>hello</Literal>
               </Annotation>"#);
        let (prop, value) = parse_annotation(d.root_element(), &Prefixes::new()).unwrap();
        assert_eq!(prop.0.0, "http://www.w3.org/2000/01/rdf-schema#comment");
        assert_eq!(
            value,
            AnnotationValue::LiteralAnnotation(GraphElement::GraphLiteral(
                RdfLiteral::LiteralString("hello".to_string())
            ))
        );
    }
}

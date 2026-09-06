/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `<Declaration>` -> `owl_ontology::Axiom::AxiomDeclaration`, and the
//! `Entity` element dispatch (`<Class>`, `<Datatype>`, `<ObjectProperty>`,
//! `<DataProperty>`, `<AnnotationProperty>`, `<NamedIndividual>`) it wraps.

use crate::annotation::parse_annotation;
use crate::iri::{Prefixes, resolve_iri};
use owl_ontology::{Axiom, Entity, Individual};

/// Parse a `<Class>`/`<Datatype>`/`<ObjectProperty>`/`<DataProperty>`/
/// `<AnnotationProperty>`/`<NamedIndividual>` element into an [`Entity`].
fn parse_entity(node: roxmltree::Node, prefixes: &Prefixes) -> Result<Entity, String> {
    let iri = resolve_iri(node, prefixes)?;
    match node.tag_name().name() {
        "Class" => Ok(Entity::ClassDeclaration(iri)),
        "Datatype" => Ok(Entity::DatatypeDeclaration(iri)),
        "ObjectProperty" => Ok(Entity::ObjectPropertyDeclaration(iri)),
        "DataProperty" => Ok(Entity::DataPropertyDeclaration(iri)),
        "AnnotationProperty" => Ok(Entity::AnnotationPropertyDeclaration(iri)),
        "NamedIndividual" => Ok(Entity::NamedIndividualDeclaration(
            Individual::NamedIndividual(iri),
        )),
        other => Err(format!("<{other}> is not a valid Declaration entity")),
    }
}

/// Parse a `<Declaration>` element: zero or more leading `<Annotation>`
/// children, then exactly one entity element.
pub(crate) fn parse_declaration(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<Axiom, String> {
    let mut annotations = Vec::new();
    let mut entity = None;

    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "Annotation" => annotations.push(parse_annotation(child, prefixes)?),
            _ => {
                if entity.is_some() {
                    return Err("<Declaration> has more than one entity element".to_string());
                }
                entity = Some(parse_entity(child, prefixes)?);
            }
        }
    }

    let entity = entity.ok_or_else(|| "<Declaration> has no entity element".to_string())?;
    Ok(Axiom::AxiomDeclaration((annotations, entity)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_class_declaration() {
        let d = doc(r#"<Declaration><Class IRI="http://example.org/pizza#Pizza"/></Declaration>"#);
        let axiom = parse_declaration(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomDeclaration((anns, Entity::ClassDeclaration(c))) => {
                assert!(anns.is_empty());
                assert_eq!(c.0.0, "http://example.org/pizza#Pizza");
            }
            other => panic!("unexpected axiom: {other:?}"),
        }
    }

    #[test]
    fn errors_with_no_entity() {
        let d = doc("<Declaration></Declaration>");
        assert!(parse_declaration(d.root_element(), &Prefixes::new()).is_err());
    }
}

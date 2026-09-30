/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `SubObjectPropertyOf` / `EquivalentObjectProperties` /
//! `DisjointObjectProperties` / `ObjectPropertyDomain` /
//! `ObjectPropertyRange` / `InverseObjectProperties` / the seven
//! object-property-characteristic elements -> `owl_ontology::Axiom::AxiomObjectPropertyAxiom`.
//!
//! `SubDataPropertyOf` / `EquivalentDataProperties` /
//! `DisjointDataProperties` / `DataPropertyDomain` / `DataPropertyRange` /
//! `FunctionalDataProperty` -> `owl_ontology::Axiom::AxiomDataPropertyAxiom`.
//!
//! `HasKey` -> `owl_ontology::Axiom::AxiomHasKey`.
//!
//! Axiom-level `<Annotation>` children on any of these (as opposed to
//! `<Declaration>`'s own leading annotations, handled by #605) are out of
//! scope for this issue -- deferred to
//! [#608](https://github.com/daghovland/rdf-datalog/issues/608), matching
//! `axiom.rs`'s (#606) precedent for class axioms. Encountering one
//! produces a clear error rather than being silently dropped.

use crate::class_expr::class_expression;
use crate::iri::Prefixes;
use crate::property_expr::{data_property_expression, object_property_expression};
use owl_ontology::{Axiom, DataPropertyAxiom, ObjectPropertyAxiom, SubPropertyExpression};

fn element_children<'a>(node: roxmltree::Node<'a, 'a>) -> Vec<roxmltree::Node<'a, 'a>> {
    node.children().filter(|n| n.is_element()).collect()
}

fn reject_axiom_level_annotation(tag: &str, children: &[roxmltree::Node]) -> Result<(), String> {
    if children.iter().any(|n| n.tag_name().name() == "Annotation") {
        return Err(format!(
            "<{tag}> axiom-level <Annotation> is not yet supported (see #608)"
        ));
    }
    Ok(())
}

/// Parse a `SubObjectPropertyOf` LHS: either a bare `ObjectPropertyExpression`
/// or an `<ObjectPropertyChain>` of two or more.
fn sub_object_property_expression(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<SubPropertyExpression, String> {
    if node.tag_name().name() == "ObjectPropertyChain" {
        let chain = element_children(node)
            .into_iter()
            .map(|c| object_property_expression(c, prefixes))
            .collect::<Result<Vec<_>, _>>()?;
        if chain.len() < 2 {
            return Err(format!(
                "<ObjectPropertyChain> expects at least 2 ObjectPropertyExpression children, found {}",
                chain.len()
            ));
        }
        Ok(SubPropertyExpression::PropertyExpressionChain(chain))
    } else {
        Ok(SubPropertyExpression::SubObjectPropertyExpression(
            object_property_expression(node, prefixes)?,
        ))
    }
}

/// Parse an `ObjectPropertyAxiom` element into an
/// [`Axiom::AxiomObjectPropertyAxiom`].
pub(crate) fn parse_object_property_axiom(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<Axiom, String> {
    let tag = node.tag_name().name();
    let children = element_children(node);
    reject_axiom_level_annotation(tag, &children)?;

    let unary = |build: fn(
        Vec<owl_ontology::Annotation>,
        owl_ontology::ObjectPropertyExpression,
    ) -> ObjectPropertyAxiom|
     -> Result<Axiom, String> {
        if children.len() != 1 {
            return Err(format!(
                "<{tag}> expects exactly 1 ObjectPropertyExpression child, found {}",
                children.len()
            ));
        }
        let p = object_property_expression(children[0], prefixes)?;
        Ok(Axiom::AxiomObjectPropertyAxiom(build(Vec::new(), p)))
    };

    match tag {
        "SubObjectPropertyOf" => {
            if children.len() != 2 {
                return Err(format!(
                    "<SubObjectPropertyOf> expects exactly 2 children, found {}",
                    children.len()
                ));
            }
            let sub = sub_object_property_expression(children[0], prefixes)?;
            let sup = object_property_expression(children[1], prefixes)?;
            Ok(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::SubObjectPropertyOf(Vec::new(), sub, sup),
            ))
        }
        "EquivalentObjectProperties" => {
            if children.len() < 2 {
                return Err(format!(
                    "<EquivalentObjectProperties> expects at least 2 ObjectPropertyExpression children, found {}",
                    children.len()
                ));
            }
            let ps = children
                .into_iter()
                .map(|c| object_property_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::EquivalentObjectProperties(Vec::new(), ps),
            ))
        }
        "DisjointObjectProperties" => {
            if children.len() < 2 {
                return Err(format!(
                    "<DisjointObjectProperties> expects at least 2 ObjectPropertyExpression children, found {}",
                    children.len()
                ));
            }
            let ps = children
                .into_iter()
                .map(|c| object_property_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::DisjointObjectProperties(Vec::new(), ps),
            ))
        }
        "ObjectPropertyDomain" => {
            if children.len() != 2 {
                return Err(format!(
                    "<ObjectPropertyDomain> expects exactly 2 children, found {}",
                    children.len()
                ));
            }
            let p = object_property_expression(children[0], prefixes)?;
            let c = class_expression(children[1], prefixes)?;
            Ok(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::ObjectPropertyDomain(Vec::new(), p, c),
            ))
        }
        "ObjectPropertyRange" => {
            if children.len() != 2 {
                return Err(format!(
                    "<ObjectPropertyRange> expects exactly 2 children, found {}",
                    children.len()
                ));
            }
            let p = object_property_expression(children[0], prefixes)?;
            let c = class_expression(children[1], prefixes)?;
            Ok(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::ObjectPropertyRange(Vec::new(), p, c),
            ))
        }
        "InverseObjectProperties" => {
            if children.len() != 2 {
                return Err(format!(
                    "<InverseObjectProperties> expects exactly 2 ObjectPropertyExpression children, found {}",
                    children.len()
                ));
            }
            let p1 = object_property_expression(children[0], prefixes)?;
            let p2 = object_property_expression(children[1], prefixes)?;
            Ok(Axiom::AxiomObjectPropertyAxiom(
                ObjectPropertyAxiom::InverseObjectProperties(Vec::new(), p1, p2),
            ))
        }
        "FunctionalObjectProperty" => unary(ObjectPropertyAxiom::FunctionalObjectProperty),
        "InverseFunctionalObjectProperty" => {
            unary(ObjectPropertyAxiom::InverseFunctionalObjectProperty)
        }
        "ReflexiveObjectProperty" => unary(ObjectPropertyAxiom::ReflexiveObjectProperty),
        "IrreflexiveObjectProperty" => unary(ObjectPropertyAxiom::IrreflexiveObjectProperty),
        "SymmetricObjectProperty" => unary(ObjectPropertyAxiom::SymmetricObjectProperty),
        "AsymmetricObjectProperty" => unary(ObjectPropertyAxiom::AsymmetricObjectProperty),
        "TransitiveObjectProperty" => unary(ObjectPropertyAxiom::TransitiveObjectProperty),
        other => Err(format!("<{other}> is not a valid ObjectPropertyAxiom")),
    }
}

/// Parse a `DataPropertyAxiom` element into an
/// [`Axiom::AxiomDataPropertyAxiom`].
pub(crate) fn parse_data_property_axiom(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<Axiom, String> {
    let tag = node.tag_name().name();
    let children = element_children(node);
    reject_axiom_level_annotation(tag, &children)?;

    match tag {
        "SubDataPropertyOf" => {
            if children.len() != 2 {
                return Err(format!(
                    "<SubDataPropertyOf> expects exactly 2 DataProperty children, found {}",
                    children.len()
                ));
            }
            let sub = data_property_expression(children[0], prefixes)?;
            let sup = data_property_expression(children[1], prefixes)?;
            Ok(Axiom::AxiomDataPropertyAxiom(
                DataPropertyAxiom::SubDataPropertyOf(Vec::new(), sub, sup),
            ))
        }
        "EquivalentDataProperties" => {
            if children.len() < 2 {
                return Err(format!(
                    "<EquivalentDataProperties> expects at least 2 DataProperty children, found {}",
                    children.len()
                ));
            }
            let ps = children
                .into_iter()
                .map(|c| data_property_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Axiom::AxiomDataPropertyAxiom(
                DataPropertyAxiom::EquivalentDataProperties(Vec::new(), ps),
            ))
        }
        "DisjointDataProperties" => {
            if children.len() < 2 {
                return Err(format!(
                    "<DisjointDataProperties> expects at least 2 DataProperty children, found {}",
                    children.len()
                ));
            }
            let ps = children
                .into_iter()
                .map(|c| data_property_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Axiom::AxiomDataPropertyAxiom(
                DataPropertyAxiom::DisjointDataProperties(Vec::new(), ps),
            ))
        }
        "DataPropertyDomain" => {
            if children.len() != 2 {
                return Err(format!(
                    "<DataPropertyDomain> expects exactly 2 children, found {}",
                    children.len()
                ));
            }
            let p = data_property_expression(children[0], prefixes)?;
            let c = class_expression(children[1], prefixes)?;
            Ok(Axiom::AxiomDataPropertyAxiom(
                DataPropertyAxiom::DataPropertyDomain(Vec::new(), p, c),
            ))
        }
        "DataPropertyRange" => {
            if children.len() != 2 {
                return Err(format!(
                    "<DataPropertyRange> expects exactly 2 children, found {}",
                    children.len()
                ));
            }
            let p = data_property_expression(children[0], prefixes)?;
            let dr = crate::data_range::data_range(children[1], prefixes)?;
            Ok(Axiom::AxiomDataPropertyAxiom(
                DataPropertyAxiom::DataPropertyRange(Vec::new(), p, dr),
            ))
        }
        "FunctionalDataProperty" => {
            if children.len() != 1 {
                return Err(format!(
                    "<FunctionalDataProperty> expects exactly 1 DataProperty child, found {}",
                    children.len()
                ));
            }
            let p = data_property_expression(children[0], prefixes)?;
            Ok(Axiom::AxiomDataPropertyAxiom(
                DataPropertyAxiom::FunctionalDataProperty(Vec::new(), p),
            ))
        }
        other => Err(format!("<{other}> is not a valid DataPropertyAxiom")),
    }
}

/// Parse a `<HasKey>` element into an [`Axiom::AxiomHasKey`]. The leading
/// child is the keyed `ClassExpression`; the remaining children are told
/// apart into `ObjectPropertyExpression`s (`<ObjectProperty>`/
/// `<ObjectInverseOf>`) and `DataProperty`s (`<DataProperty>`) purely by
/// tag name, since OWL/XML has no ambiguity here (unlike Functional-Style
/// Syntax's two parenthesized groups).
pub(crate) fn parse_has_key(node: roxmltree::Node, prefixes: &Prefixes) -> Result<Axiom, String> {
    let children = element_children(node);
    reject_axiom_level_annotation("HasKey", &children)?;

    let class_node = children
        .first()
        .ok_or_else(|| "<HasKey> has no ClassExpression child".to_string())?;
    let class = class_expression(*class_node, prefixes)?;

    let mut object_properties = Vec::new();
    let mut data_properties = Vec::new();
    for child in &children[1..] {
        match child.tag_name().name() {
            "ObjectProperty" | "ObjectInverseOf" => {
                object_properties.push(object_property_expression(*child, prefixes)?);
            }
            "DataProperty" => {
                data_properties.push(data_property_expression(*child, prefixes)?);
            }
            other => {
                return Err(format!(
                    "<HasKey> child <{other}> is neither an ObjectPropertyExpression nor a DataPropertyExpression"
                ));
            }
        }
    }

    Ok(Axiom::AxiomHasKey(
        Vec::new(),
        class,
        object_properties,
        data_properties,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_sub_object_property_of() {
        let d = doc(r#"<SubObjectPropertyOf>
                 <ObjectProperty IRI="http://example.org/hasDog"/>
                 <ObjectProperty IRI="http://example.org/hasPet"/>
               </SubObjectPropertyOf>"#);
        let axiom = parse_object_property_axiom(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(_, _, _))
        ));
    }

    #[test]
    fn parses_sub_object_property_of_chain() {
        let d = doc(r#"<SubObjectPropertyOf>
                 <ObjectPropertyChain>
                   <ObjectProperty IRI="http://example.org/hasParent"/>
                   <ObjectProperty IRI="http://example.org/hasParent"/>
                 </ObjectPropertyChain>
                 <ObjectProperty IRI="http://example.org/hasGrandparent"/>
               </SubObjectPropertyOf>"#);
        let axiom = parse_object_property_axiom(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
                _,
                SubPropertyExpression::PropertyExpressionChain(chain),
                _,
            )) => {
                assert_eq!(chain.len(), 2);
            }
            other => panic!("expected chain SubObjectPropertyOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_transitive_object_property() {
        let d = doc(
            r#"<TransitiveObjectProperty><ObjectProperty IRI="http://example.org/hasPart"/></TransitiveObjectProperty>"#,
        );
        let axiom = parse_object_property_axiom(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::TransitiveObjectProperty(_, _))
        ));
    }

    #[test]
    fn parses_functional_data_property() {
        let d = doc(
            r#"<FunctionalDataProperty><DataProperty IRI="http://example.org/hasSSN"/></FunctionalDataProperty>"#,
        );
        let axiom = parse_data_property_axiom(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::FunctionalDataProperty(_, _))
        ));
    }

    #[test]
    fn parses_has_key() {
        let d = doc(r#"<HasKey>
                 <Class IRI="http://example.org/Person"/>
                 <ObjectProperty IRI="http://example.org/hasSSN"/>
                 <DataProperty IRI="http://example.org/hasName"/>
               </HasKey>"#);
        let axiom = parse_has_key(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomHasKey(_, _, ops, dps) => {
                assert_eq!(ops.len(), 1);
                assert_eq!(dps.len(), 1);
            }
            other => panic!("expected AxiomHasKey, got {other:?}"),
        }
    }

    #[test]
    fn errors_on_axiom_level_annotation() {
        let d = doc(r#"<ObjectPropertyDomain>
                 <Annotation>
                   <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                   <Literal>why</Literal>
                 </Annotation>
                 <ObjectProperty IRI="http://example.org/hasTopping"/>
                 <Class IRI="http://example.org/Pizza"/>
               </ObjectPropertyDomain>"#);
        let err = parse_object_property_axiom(d.root_element(), &Prefixes::new()).unwrap_err();
        assert!(err.contains("608"));
    }
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `ClassExpression` — every keyword form from
//! [the OWL 2 spec §8](https://www.w3.org/TR/owl2-syntax/#Class_Expressions),
//! in its OWL/XML element shape. See `docs/plans/OWL_XML_PLAN.md`'s "#606"
//! section for the exact grammar quoted from the XML serialization spec.
//! Every production is its own uniquely named element, so (unlike
//! Manchester Syntax's precedence-ladder grammar) this is a flat dispatch
//! on `node.tag_name().name()`, recursing into `node.children()` directly.

use crate::annotation::parse_literal;
use crate::data_range::data_range;
use crate::individual::individual;
use crate::iri::{Prefixes, resolve_iri};
use crate::property_expr::{data_property_expression, object_property_expression};
use num_bigint::BigInt;
use owl_ontology::ClassExpression;

/// Element tags that name a `DataRange` production (used to tell a
/// `<DataSomeValuesFrom>`/cardinality restriction's trailing `DataRange`
/// child apart from its leading `<DataProperty>` children -- OWL/XML has no
/// bare-IRI ambiguity here since every tag differs).
const DATA_RANGE_TAGS: &[&str] = &[
    "Datatype",
    "DataIntersectionOf",
    "DataUnionOf",
    "DataComplementOf",
    "DataOneOf",
    "DatatypeRestriction",
];

fn element_children<'a>(node: roxmltree::Node<'a, 'a>) -> Vec<roxmltree::Node<'a, 'a>> {
    node.children().filter(|n| n.is_element()).collect()
}

fn cardinality(node: roxmltree::Node) -> Result<BigInt, String> {
    let raw = node
        .attribute("cardinality")
        .ok_or_else(|| format!("<{}> has no cardinality= attribute", node.tag_name().name()))?;
    BigInt::parse_bytes(raw.as_bytes(), 10).ok_or_else(|| {
        format!(
            "<{}> cardinality={raw:?} is not a valid non-negative integer",
            node.tag_name().name()
        )
    })
}

/// Parse a `ClassExpression` element.
pub(crate) fn class_expression(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<ClassExpression, String> {
    let tag = node.tag_name().name();
    match tag {
        "Class" => Ok(ClassExpression::ClassName(resolve_iri(node, prefixes)?)),
        "ObjectIntersectionOf" => Ok(ClassExpression::ObjectIntersectionOf(
            element_children(node)
                .into_iter()
                .map(|c| class_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "ObjectUnionOf" => Ok(ClassExpression::ObjectUnionOf(
            element_children(node)
                .into_iter()
                .map(|c| class_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "ObjectComplementOf" => {
            let child = one_child(node)?;
            Ok(ClassExpression::ObjectComplementOf(Box::new(
                class_expression(child, prefixes)?,
            )))
        }
        "ObjectOneOf" => Ok(ClassExpression::ObjectOneOf(
            element_children(node)
                .into_iter()
                .map(|c| individual(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "ObjectSomeValuesFrom" | "ObjectAllValuesFrom" => {
            let (prop_node, class_node) = two_children(node)?;
            let prop = object_property_expression(prop_node, prefixes)?;
            let class = Box::new(class_expression(class_node, prefixes)?);
            Ok(if tag == "ObjectSomeValuesFrom" {
                ClassExpression::ObjectSomeValuesFrom(prop, class)
            } else {
                ClassExpression::ObjectAllValuesFrom(prop, class)
            })
        }
        "ObjectHasValue" => {
            let (prop_node, ind_node) = two_children(node)?;
            let prop = object_property_expression(prop_node, prefixes)?;
            let ind = individual(ind_node, prefixes)?;
            Ok(ClassExpression::ObjectHasValue(prop, ind))
        }
        "ObjectHasSelf" => {
            let prop_node = one_child(node)?;
            Ok(ClassExpression::ObjectHasSelf(object_property_expression(
                prop_node, prefixes,
            )?))
        }
        "ObjectMinCardinality" | "ObjectMaxCardinality" | "ObjectExactCardinality" => {
            let n = cardinality(node)?;
            let children = element_children(node);
            let prop_node = children
                .first()
                .ok_or_else(|| format!("<{tag}> has no ObjectPropertyExpression child"))?;
            let prop = object_property_expression(*prop_node, prefixes)?;
            let filler = match children.get(1) {
                Some(c) => Some(Box::new(class_expression(*c, prefixes)?)),
                None => None,
            };
            Ok(match (tag, filler) {
                ("ObjectMinCardinality", Some(c)) => {
                    ClassExpression::ObjectMinQualifiedCardinality(n, prop, c)
                }
                ("ObjectMinCardinality", None) => ClassExpression::ObjectMinCardinality(n, prop),
                ("ObjectMaxCardinality", Some(c)) => {
                    ClassExpression::ObjectMaxQualifiedCardinality(n, prop, c)
                }
                ("ObjectMaxCardinality", None) => ClassExpression::ObjectMaxCardinality(n, prop),
                ("ObjectExactCardinality", Some(c)) => {
                    ClassExpression::ObjectExactQualifiedCardinality(n, prop, c)
                }
                ("ObjectExactCardinality", None) => {
                    ClassExpression::ObjectExactCardinality(n, prop)
                }
                _ => unreachable!(),
            })
        }
        "DataSomeValuesFrom" | "DataAllValuesFrom" => {
            let (props, range) = data_properties_then_range(node, prefixes)?;
            Ok(if tag == "DataSomeValuesFrom" {
                ClassExpression::DataSomeValuesFrom(props, range)
            } else {
                ClassExpression::DataAllValuesFrom(props, range)
            })
        }
        "DataHasValue" => {
            let (prop_node, literal_node) = two_children(node)?;
            let prop = data_property_expression(prop_node, prefixes)?;
            if literal_node.tag_name().name() != "Literal" {
                return Err(format!(
                    "<DataHasValue>'s second child must be <Literal>, found <{}>",
                    literal_node.tag_name().name()
                ));
            }
            Ok(ClassExpression::DataHasValue(
                prop,
                parse_literal(literal_node),
            ))
        }
        "DataMinCardinality" | "DataMaxCardinality" | "DataExactCardinality" => {
            let n = cardinality(node)?;
            let children = element_children(node);
            let prop_node = children
                .first()
                .ok_or_else(|| format!("<{tag}> has no DataPropertyExpression child"))?;
            let prop = data_property_expression(*prop_node, prefixes)?;
            let filler = match children.get(1) {
                Some(c) => Some(data_range(*c, prefixes)?),
                None => None,
            };
            Ok(match (tag, filler) {
                ("DataMinCardinality", Some(dr)) => {
                    ClassExpression::DataMinQualifiedCardinality(n, prop, dr)
                }
                ("DataMinCardinality", None) => ClassExpression::DataMinCardinality(n, prop),
                ("DataMaxCardinality", Some(dr)) => {
                    ClassExpression::DataMaxQualifiedCardinality(n, prop, dr)
                }
                ("DataMaxCardinality", None) => ClassExpression::DataMaxCardinality(n, prop),
                ("DataExactCardinality", Some(dr)) => {
                    ClassExpression::DataExactQualifiedCardinality(n, prop, dr)
                }
                ("DataExactCardinality", None) => ClassExpression::DataExactCardinality(n, prop),
                _ => unreachable!(),
            })
        }
        other => Err(format!("<{other}> is not a valid ClassExpression")),
    }
}

fn one_child<'a>(node: roxmltree::Node<'a, 'a>) -> Result<roxmltree::Node<'a, 'a>, String> {
    element_children(node)
        .into_iter()
        .next()
        .ok_or_else(|| format!("<{}> has no child element", node.tag_name().name()))
}

fn two_children<'a>(
    node: roxmltree::Node<'a, 'a>,
) -> Result<(roxmltree::Node<'a, 'a>, roxmltree::Node<'a, 'a>), String> {
    let children = element_children(node);
    if children.len() != 2 {
        return Err(format!(
            "<{}> expects exactly 2 child elements, found {}",
            node.tag_name().name(),
            children.len()
        ));
    }
    Ok((children[0], children[1]))
}

/// `DataPropertyExpression+ DataRange` -- the shared body of
/// `DataSomeValuesFrom`/`DataAllValuesFrom`. No lookahead is needed (unlike
/// the Functional-Style Syntax equivalent): a `<DataProperty>` child is
/// always a property, and any child tagged with one of [`DATA_RANGE_TAGS`]
/// is the (single, trailing) `DataRange`.
fn data_properties_then_range(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<(Vec<owl_ontology::DataProperty>, owl_ontology::DataRange), String> {
    let mut props = Vec::new();
    let mut range = None;
    for child in element_children(node) {
        let tag = child.tag_name().name();
        if tag == "DataProperty" {
            props.push(data_property_expression(child, prefixes)?);
        } else if DATA_RANGE_TAGS.contains(&tag) {
            if range.is_some() {
                return Err(format!(
                    "<{}> has more than one DataRange child",
                    node.tag_name().name()
                ));
            }
            range = Some(data_range(child, prefixes)?);
        } else {
            return Err(format!(
                "<{}> child <{tag}> is neither a DataProperty nor a DataRange",
                node.tag_name().name()
            ));
        }
    }
    let range =
        range.ok_or_else(|| format!("<{}> has no DataRange child", node.tag_name().name()))?;
    if props.is_empty() {
        return Err(format!(
            "<{}> has no DataPropertyExpression child",
            node.tag_name().name()
        ));
    }
    Ok((props, range))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_class_name() {
        let d = doc(r#"<Class IRI="http://example.org/Pizza"/>"#);
        let ce = class_expression(d.root_element(), &Prefixes::new()).unwrap();
        assert_eq!(
            ce,
            ClassExpression::ClassName(owl_ontology::FullIri(ingress::IriReference(
                "http://example.org/Pizza".to_string()
            )))
        );
    }

    #[test]
    fn parses_intersection_and_union() {
        let d = doc(r#"<ObjectIntersectionOf>
                 <Class IRI="http://example.org/Food"/>
                 <Class IRI="http://example.org/Pizza"/>
               </ObjectIntersectionOf>"#);
        let ce = class_expression(d.root_element(), &Prefixes::new()).unwrap();
        match ce {
            ClassExpression::ObjectIntersectionOf(v) => assert_eq!(v.len(), 2),
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_expression() {
        let d = doc(r#"<ObjectIntersectionOf>
                 <Class IRI="http://example.org/Food"/>
                 <ObjectSomeValuesFrom>
                   <ObjectProperty IRI="http://example.org/hasTopping"/>
                   <Class IRI="http://example.org/Topping"/>
                 </ObjectSomeValuesFrom>
               </ObjectIntersectionOf>"#);
        let ce = class_expression(d.root_element(), &Prefixes::new()).unwrap();
        match ce {
            ClassExpression::ObjectIntersectionOf(v) => {
                assert_eq!(v.len(), 2);
                assert!(matches!(v[1], ClassExpression::ObjectSomeValuesFrom(_, _)));
            }
            other => panic!("expected ObjectIntersectionOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_object_cardinality_unqualified_and_qualified() {
        let d = doc(
            r#"<ObjectMinCardinality cardinality="1"><ObjectProperty IRI="http://example.org/hasTopping"/></ObjectMinCardinality>"#,
        );
        let ce = class_expression(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(ce, ClassExpression::ObjectMinCardinality(n, _) if n == BigInt::from(1)));

        let d2 = doc(r#"<ObjectMinCardinality cardinality="1">
                 <ObjectProperty IRI="http://example.org/hasTopping"/>
                 <Class IRI="http://example.org/Topping"/>
               </ObjectMinCardinality>"#);
        let ce2 = class_expression(d2.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            ce2,
            ClassExpression::ObjectMinQualifiedCardinality(_, _, _)
        ));
    }

    #[test]
    fn parses_data_some_values_from() {
        let d = doc(r#"<DataSomeValuesFrom>
                 <DataProperty IRI="http://example.org/hasAge"/>
                 <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
               </DataSomeValuesFrom>"#);
        let ce = class_expression(d.root_element(), &Prefixes::new()).unwrap();
        match ce {
            ClassExpression::DataSomeValuesFrom(props, _) => assert_eq!(props.len(), 1),
            other => panic!("expected DataSomeValuesFrom, got {other:?}"),
        }
    }
}

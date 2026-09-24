/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `DataRange ::= Datatype | DataIntersectionOf | DataUnionOf`
//! `           | DataComplementOf | DataOneOf | DatatypeRestriction`
//!
//! Each alternative is its own element tag, so (unlike Functional-Style
//! Syntax's `data_properties_then_range`, which needs lookahead to
//! disambiguate a trailing bare IRI as "one more `DataPropertyExpression`"
//! vs. "the final `DataRange`") no ambiguity exists here: `<DataProperty>`
//! and every `DataRange` production use distinct tag names.

use crate::annotation::parse_literal;
use crate::iri::{Prefixes, resolve_iri};
use owl_ontology::DataRange;

/// Parse a `DataRange` element (`<Datatype>`, `<DataIntersectionOf>`,
/// `<DataUnionOf>`, `<DataComplementOf>`, `<DataOneOf>`, or
/// `<DatatypeRestriction>`).
pub(crate) fn data_range(node: roxmltree::Node, prefixes: &Prefixes) -> Result<DataRange, String> {
    let children = || node.children().filter(|n| n.is_element());
    match node.tag_name().name() {
        "Datatype" => Ok(DataRange::NamedDataRange(resolve_iri(node, prefixes)?)),
        "DataIntersectionOf" => Ok(DataRange::DataIntersectionOf(
            children()
                .map(|c| data_range(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "DataUnionOf" => Ok(DataRange::DataUnionOf(
            children()
                .map(|c| data_range(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "DataComplementOf" => {
            let child = children()
                .next()
                .ok_or_else(|| "<DataComplementOf> has no child element".to_string())?;
            Ok(DataRange::DataComplementOf(Box::new(data_range(
                child, prefixes,
            )?)))
        }
        "DataOneOf" => Ok(DataRange::DataOneOf(
            children()
                .map(|c| {
                    if c.tag_name().name() != "Literal" {
                        return Err(format!(
                            "<DataOneOf> child <{}> is not a <Literal>",
                            c.tag_name().name()
                        ));
                    }
                    Ok(parse_literal(c))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "DatatypeRestriction" => {
            let mut children = children();
            let datatype_node = children
                .next()
                .ok_or_else(|| "<DatatypeRestriction> has no <Datatype> child".to_string())?;
            if datatype_node.tag_name().name() != "Datatype" {
                return Err(format!(
                    "<DatatypeRestriction>'s first child must be <Datatype>, found <{}>",
                    datatype_node.tag_name().name()
                ));
            }
            let datatype = resolve_iri(datatype_node, prefixes)?;
            let facets = children
                .map(|facet_node| {
                    if facet_node.tag_name().name() != "FacetRestriction" {
                        return Err(format!(
                            "<DatatypeRestriction> child <{}> is not a <FacetRestriction>",
                            facet_node.tag_name().name()
                        ));
                    }
                    let facet_iri = facet_node
                        .attribute("facet")
                        .ok_or_else(|| "<FacetRestriction> has no facet= attribute".to_string())?;
                    let literal_node = facet_node
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "Literal")
                        .ok_or_else(|| "<FacetRestriction> has no <Literal> child".to_string())?;
                    Ok((
                        owl_ontology::FullIri(ingress::IriReference(facet_iri.to_string())),
                        parse_literal(literal_node),
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if facets.is_empty() {
                return Err("<DatatypeRestriction> has no <FacetRestriction> children".to_string());
            }
            Ok(DataRange::DatatypeRestriction(datatype, facets))
        }
        other => Err(format!("<{other}> is not a valid DataRange")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_named_datatype() {
        let d = doc(r#"<Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>"#);
        let dr = data_range(d.root_element(), &Prefixes::new()).unwrap();
        assert_eq!(
            dr,
            DataRange::NamedDataRange(owl_ontology::FullIri(ingress::IriReference(
                "http://www.w3.org/2001/XMLSchema#integer".to_string()
            )))
        );
    }

    #[test]
    fn parses_intersection_union_complement() {
        let d = doc(r#"<DataIntersectionOf>
                 <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
                 <DataComplementOf><Datatype IRI="http://www.w3.org/2001/XMLSchema#string"/></DataComplementOf>
               </DataIntersectionOf>"#);
        let dr = data_range(d.root_element(), &Prefixes::new()).unwrap();
        match dr {
            DataRange::DataIntersectionOf(v) => {
                assert_eq!(v.len(), 2);
                assert!(matches!(v[1], DataRange::DataComplementOf(_)));
            }
            other => panic!("expected DataIntersectionOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_data_one_of() {
        let d = doc(r#"<DataOneOf><Literal>a</Literal><Literal>b</Literal></DataOneOf>"#);
        let dr = data_range(d.root_element(), &Prefixes::new()).unwrap();
        match dr {
            DataRange::DataOneOf(v) => assert_eq!(v.len(), 2),
            other => panic!("expected DataOneOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_datatype_restriction_with_facets() {
        let d = doc(r#"<DatatypeRestriction>
                 <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
                 <FacetRestriction facet="http://www.w3.org/2001/XMLSchema#minInclusive"><Literal>0</Literal></FacetRestriction>
                 <FacetRestriction facet="http://www.w3.org/2001/XMLSchema#maxInclusive"><Literal>10</Literal></FacetRestriction>
               </DatatypeRestriction>"#);
        let dr = data_range(d.root_element(), &Prefixes::new()).unwrap();
        match dr {
            DataRange::DatatypeRestriction(dt, facets) => {
                assert_eq!(dt.0.0, "http://www.w3.org/2001/XMLSchema#integer");
                assert_eq!(facets.len(), 2);
            }
            other => panic!("expected DatatypeRestriction, got {other:?}"),
        }
    }
}

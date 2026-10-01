/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `Assertion ::= ClassAssertion | ObjectPropertyAssertion |
//! NegativeObjectPropertyAssertion | DataPropertyAssertion |
//! NegativeDataPropertyAssertion | SameIndividual | DifferentIndividuals`
//! -> `owl_ontology::Axiom::AxiomAssertion` (#608).

use crate::annotation::{parse_literal, split_axiom_annotations};
use crate::axiom::element_children;
use crate::class_expr::class_expression;
use crate::individual::individual;
use crate::iri::Prefixes;
use crate::property_expr::{data_property_expression, object_property_expression};
use owl_ontology::{Assertion, Axiom};

/// Parse a `<ClassAssertion>`/`<ObjectPropertyAssertion>`/
/// `<NegativeObjectPropertyAssertion>`/`<DataPropertyAssertion>`/
/// `<NegativeDataPropertyAssertion>`/`<SameIndividual>`/
/// `<DifferentIndividuals>` element into an [`Axiom::AxiomAssertion`].
pub(crate) fn parse_assertion(node: roxmltree::Node, prefixes: &Prefixes) -> Result<Axiom, String> {
    let tag = node.tag_name().name();
    let all_children = element_children(node);
    let (anns, children) = split_axiom_annotations(&all_children, prefixes)?;

    match tag {
        "ClassAssertion" => {
            if children.len() != 2 {
                return Err(format!(
                    "<ClassAssertion> expects exactly 2 children (ClassExpression, Individual), found {}",
                    children.len()
                ));
            }
            let ce = class_expression(children[0], prefixes)?;
            let ind = individual(children[1], prefixes)?;
            Ok(Axiom::AxiomAssertion(Assertion::ClassAssertion(
                anns, ce, ind,
            )))
        }
        "ObjectPropertyAssertion" | "NegativeObjectPropertyAssertion" => {
            if children.len() != 3 {
                return Err(format!(
                    "<{tag}> expects exactly 3 children (ObjectPropertyExpression, Individual, Individual), found {}",
                    children.len()
                ));
            }
            let p = object_property_expression(children[0], prefixes)?;
            let s = individual(children[1], prefixes)?;
            let o = individual(children[2], prefixes)?;
            let axiom = if tag == "ObjectPropertyAssertion" {
                Assertion::ObjectPropertyAssertion(anns, p, s, o)
            } else {
                Assertion::NegativeObjectPropertyAssertion(anns, p, s, o)
            };
            Ok(Axiom::AxiomAssertion(axiom))
        }
        "DataPropertyAssertion" | "NegativeDataPropertyAssertion" => {
            if children.len() != 3 {
                return Err(format!(
                    "<{tag}> expects exactly 3 children (DataPropertyExpression, Individual, Literal), found {}",
                    children.len()
                ));
            }
            let p = data_property_expression(children[0], prefixes)?;
            let ind = individual(children[1], prefixes)?;
            let lit = parse_literal(children[2]);
            let axiom = if tag == "DataPropertyAssertion" {
                Assertion::DataPropertyAssertion(anns, p, ind, lit)
            } else {
                Assertion::NegativeDataPropertyAssertion(anns, p, ind, lit)
            };
            Ok(Axiom::AxiomAssertion(axiom))
        }
        "SameIndividual" | "DifferentIndividuals" => {
            if children.len() < 2 {
                return Err(format!(
                    "<{tag}> expects at least 2 Individual children, found {}",
                    children.len()
                ));
            }
            let inds = children
                .into_iter()
                .map(|c| individual(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            let axiom = if tag == "SameIndividual" {
                Assertion::SameIndividual(anns, inds)
            } else {
                Assertion::DifferentIndividuals(anns, inds)
            };
            Ok(Axiom::AxiomAssertion(axiom))
        }
        other => Err(format!("<{other}> is not a valid Assertion")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_class_assertion() {
        let d = doc(r#"<ClassAssertion>
                 <Class IRI="http://example.org/Pizza"/>
                 <NamedIndividual IRI="http://example.org/Margherita"/>
               </ClassAssertion>"#);
        let axiom = parse_assertion(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            Axiom::AxiomAssertion(Assertion::ClassAssertion(_, _, _))
        ));
    }

    #[test]
    fn parses_object_property_assertion() {
        let d = doc(r#"<ObjectPropertyAssertion>
                 <ObjectProperty IRI="http://example.org/hasTopping"/>
                 <NamedIndividual IRI="http://example.org/Margherita"/>
                 <NamedIndividual IRI="http://example.org/Mozzarella"/>
               </ObjectPropertyAssertion>"#);
        let axiom = parse_assertion(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(_, _, _, _))
        ));
    }

    #[test]
    fn parses_same_individual_with_three_members() {
        let d = doc(r#"<SameIndividual>
                 <NamedIndividual IRI="http://example.org/A"/>
                 <NamedIndividual IRI="http://example.org/B"/>
                 <NamedIndividual IRI="http://example.org/C"/>
               </SameIndividual>"#);
        let axiom = parse_assertion(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomAssertion(Assertion::SameIndividual(_, inds)) => {
                assert_eq!(inds.len(), 3);
            }
            other => panic!("expected SameIndividual, got {other:?}"),
        }
    }

    #[test]
    fn parses_data_property_assertion_with_literal() {
        let d = doc(r#"<DataPropertyAssertion>
                 <DataProperty IRI="http://example.org/hasName"/>
                 <NamedIndividual IRI="http://example.org/Margherita"/>
                 <Literal>Margherita</Literal>
               </DataPropertyAssertion>"#);
        let axiom = parse_assertion(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            Axiom::AxiomAssertion(Assertion::DataPropertyAssertion(_, _, _, _))
        ));
    }

    #[test]
    fn parses_axiom_level_annotation() {
        let d = doc(r#"<ClassAssertion>
                 <Annotation>
                   <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                   <Literal>why</Literal>
                 </Annotation>
                 <Class IRI="http://example.org/Pizza"/>
                 <NamedIndividual IRI="http://example.org/Margherita"/>
               </ClassAssertion>"#);
        let axiom = parse_assertion(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomAssertion(Assertion::ClassAssertion(anns, _, _)) => {
                assert_eq!(anns.len(), 1);
            }
            other => panic!("expected ClassAssertion, got {other:?}"),
        }
    }

    #[test]
    fn errors_on_wrong_child_count() {
        let d = doc(r#"<ClassAssertion><Class IRI="http://example.org/Pizza"/></ClassAssertion>"#);
        assert!(parse_assertion(d.root_element(), &Prefixes::new()).is_err());
    }
}

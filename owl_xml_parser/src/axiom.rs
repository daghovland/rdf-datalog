/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `SubClassOf` / `EquivalentClasses` / `DisjointClasses` / `DisjointUnion`
//! -> `owl_ontology::Axiom::AxiomClassAxiom`.
//!
//! Axiom-level `<Annotation>` children (the leading `axiomAnnotations` every
//! `Axiom` alternative carries per the spec) are parsed via
//! `annotation::split_axiom_annotations` (#608).

use crate::annotation::split_axiom_annotations;
use crate::class_expr::class_expression;
use crate::iri::{Prefixes, resolve_iri};
use owl_ontology::{Axiom, ClassAxiom};

pub(crate) fn element_children<'a>(node: roxmltree::Node<'a, 'a>) -> Vec<roxmltree::Node<'a, 'a>> {
    node.children().filter(|n| n.is_element()).collect()
}

/// Parse a `<SubClassOf>`/`<EquivalentClasses>`/`<DisjointClasses>`/
/// `<DisjointUnion>` element into an [`Axiom::AxiomClassAxiom`].
pub(crate) fn parse_class_axiom(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<Axiom, String> {
    let tag = node.tag_name().name();
    let all_children = element_children(node);
    let (anns, children) = split_axiom_annotations(&all_children, prefixes)?;

    match tag {
        "SubClassOf" => {
            if children.len() != 2 {
                return Err(format!(
                    "<SubClassOf> expects exactly 2 ClassExpression children, found {}",
                    children.len()
                ));
            }
            let sub = class_expression(children[0], prefixes)?;
            let sup = class_expression(children[1], prefixes)?;
            Ok(Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                anns, sub, sup,
            )))
        }
        "EquivalentClasses" => {
            if children.len() < 2 {
                return Err(format!(
                    "<EquivalentClasses> expects at least 2 ClassExpression children, found {}",
                    children.len()
                ));
            }
            let ces = children
                .into_iter()
                .map(|c| class_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                anns, ces,
            )))
        }
        "DisjointClasses" => {
            if children.len() < 2 {
                return Err(format!(
                    "<DisjointClasses> expects at least 2 ClassExpression children, found {}",
                    children.len()
                ));
            }
            let ces = children
                .into_iter()
                .map(|c| class_expression(c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
                anns, ces,
            )))
        }
        "DisjointUnion" => {
            if children.len() < 3 {
                return Err(format!(
                    "<DisjointUnion> expects a Class followed by at least 2 ClassExpression children, found {}",
                    children.len()
                ));
            }
            let class = resolve_iri(children[0], prefixes)?;
            let ces = children[1..]
                .iter()
                .map(|c| class_expression(*c, prefixes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(
                anns, class, ces,
            )))
        }
        other => Err(format!("<{other}> is not a valid ClassAxiom")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    fn parses_sub_class_of() {
        let d = doc(r#"<SubClassOf>
                 <Class IRI="http://example.org/Pizza"/>
                 <Class IRI="http://example.org/Food"/>
               </SubClassOf>"#);
        let axiom = parse_class_axiom(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(_, _, _))
        ));
    }

    #[test]
    fn parses_axiom_level_annotation() {
        let d = doc(r#"<SubClassOf>
                 <Annotation>
                   <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                   <Literal>why</Literal>
                 </Annotation>
                 <Class IRI="http://example.org/Pizza"/>
                 <Class IRI="http://example.org/Food"/>
               </SubClassOf>"#);
        let axiom = parse_class_axiom(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(anns, _, _)) => {
                assert_eq!(anns.len(), 1);
            }
            other => panic!("expected SubClassOf, got {other:?}"),
        }
    }

    #[test]
    fn parses_disjoint_union() {
        let d = doc(r#"<DisjointUnion>
                 <Class IRI="http://example.org/Pizza"/>
                 <Class IRI="http://example.org/MeatPizza"/>
                 <Class IRI="http://example.org/VeggiePizza"/>
               </DisjointUnion>"#);
        let axiom = parse_class_axiom(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(_, class, ces)) => {
                assert_eq!(class.0.0, "http://example.org/Pizza");
                assert_eq!(ces.len(), 2);
            }
            other => panic!("expected DisjointUnion, got {other:?}"),
        }
    }
}

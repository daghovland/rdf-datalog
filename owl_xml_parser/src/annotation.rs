/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `<Annotation>` -> `owl_ontology::Annotation` (`(AnnotationProperty,
//! AnnotationValue)`); `axiomAnnotations` (the leading run of `<Annotation>`
//! children shared by every `Axiom` alternative); and the remaining
//! `AnnotationAxiom` elements (`<AnnotationAssertion>`,
//! `<SubAnnotationPropertyOf>`, `<AnnotationPropertyDomain>`,
//! `<AnnotationPropertyRange>`) (#608).
//!
//! `AnnotationSubject ::= IRI | AnonymousIndividual` /
//! `AnnotationValue ::= AnonymousIndividual | IRI | Literal` per the spec's
//! §10. A nested `<Annotation>` inside another `<Annotation>`'s own leading
//! annotations (a "meta-annotation") is a genuine type-model gap --
//! `owl_ontology::Annotation` is a flat `(AnnotationProperty,
//! AnnotationValue)` pair with no slot for annotations on an annotation,
//! the same limitation `owl_functional_parser`/`manchester_parser` already
//! document for their own crates -- so it errors rather than being silently
//! dropped, pointing at
//! [#695](https://github.com/daghovland/rdf-datalog/issues/695).

use crate::iri::{Prefixes, resolve_iri};
use ingress::{GraphElement, IriReference, RdfLiteral, RdfResource};
use owl_ontology::{Annotation, AnnotationValue, FullIri, Individual};

/// Parse a `<Literal>` element's text content (plus optional `xml:lang`/
/// `datatypeIRI` attributes) into a [`GraphElement::GraphLiteral`]. Shared
/// with `class_expr.rs`/`data_range.rs` (`DataHasValue`/`DataOneOf`/
/// `FacetRestriction` values are the same `<Literal>` production) and
/// `assertion.rs` (`DataPropertyAssertion`/`NegativeDataPropertyAssertion`).
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

/// Parse a plain `IRI ::= <IRI>fullIRI</IRI> | <AbbreviatedIRI>abbrev</AbbreviatedIRI>`
/// *text-content* element -- the shape used for `AnnotationPropertyDomain`/
/// `Range`'s second argument and as one alternative of
/// `AnnotationSubject`/`AnnotationValue`. This is distinct from
/// `iri::resolve_iri`, which reads the `IRI=`/`abbreviatedIRI=` *attribute*
/// form used by every entity reference (`<Class IRI="..."/>`,
/// `<AnnotationProperty IRI="..."/>`, ...).
pub(crate) fn parse_iri_text_element(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<FullIri, String> {
    match node.tag_name().name() {
        "IRI" => {
            let text = node.text().unwrap_or("").trim().to_string();
            Ok(FullIri(IriReference(text)))
        }
        "AbbreviatedIRI" => {
            let text = node.text().unwrap_or("").trim();
            let (prefix_name, local) = text
                .split_once(':')
                .ok_or_else(|| format!("<AbbreviatedIRI>{text}</AbbreviatedIRI> has no ':'"))?;
            let ns = prefixes.get(prefix_name).ok_or_else(|| {
                format!("<AbbreviatedIRI> uses undeclared prefix {prefix_name:?}")
            })?;
            Ok(FullIri(IriReference(format!("{ns}{local}"))))
        }
        other => Err(format!("<{other}> is not a valid IRI element")),
    }
}

/// Parse an `<AnonymousIndividual nodeID="...">` element's `nodeID`
/// attribute, resolving it through [`Prefixes::anon_individual_for_label`].
fn parse_anonymous_individual_node_id(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<u32, String> {
    let node_id = node
        .attribute("nodeID")
        .ok_or_else(|| "<AnonymousIndividual> has no nodeID attribute".to_string())?;
    Ok(prefixes.anon_individual_for_label(node_id))
}

/// Parse a shared `AnnotationSubject`/`AnnotationValue` filler element
/// (`<IRI>`, `<AbbreviatedIRI>`, `<AnonymousIndividual>`, or -- value only
/// per the spec, not separately validated here -- `<Literal>`) into the
/// `owl_ontology::GraphElement` slot `AnnotationAssertion`'s subject/value
/// fields actually carry (the same type-model shape
/// `owl_functional_parser`'s `annotation_subject`/
/// `annotation_value_as_graph_element` lower to).
pub(crate) fn parse_annotation_filler_as_graph_element(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<GraphElement, String> {
    match node.tag_name().name() {
        "Literal" => Ok(parse_literal(node)),
        "IRI" | "AbbreviatedIRI" => {
            let FullIri(iri) = parse_iri_text_element(node, prefixes)?;
            Ok(GraphElement::NodeOrEdge(RdfResource::Iri(iri)))
        }
        "AnonymousIndividual" => {
            let id = parse_anonymous_individual_node_id(node, prefixes)?;
            Ok(GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(
                id,
            )))
        }
        other => Err(format!(
            "<{other}> is not a valid AnnotationSubject/AnnotationValue"
        )),
    }
}

/// Parse an `<Annotation>` element: an `<AnnotationProperty>` child (the
/// property), followed by exactly one value child (`<Literal>`, `<IRI>`,
/// `<AbbreviatedIRI>`, or `<AnonymousIndividual>`).
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
            "IRI" | "AbbreviatedIRI" => {
                value = Some(AnnotationValue::IriAnnotation(parse_iri_text_element(
                    child, prefixes,
                )?));
            }
            "AnonymousIndividual" => {
                let id = parse_anonymous_individual_node_id(child, prefixes)?;
                value = Some(AnnotationValue::IndividualAnnotation(
                    Individual::AnonymousIndividual(id),
                ));
            }
            other => {
                return Err(format!(
                    "<Annotation> nested inside <{other}> (a meta-annotation) is not representable (see #695)"
                ));
            }
        }
    }

    let property =
        property.ok_or_else(|| "<Annotation> has no <AnnotationProperty>".to_string())?;
    let value = value.ok_or_else(|| "<Annotation> has no value element".to_string())?;
    Ok((property, value))
}

/// Split an axiom element's already-collected element children into its
/// `axiomAnnotations` (the leading run of `<Annotation>` children, per the
/// spec's uniform `Axiom ::= axiomAnnotations, ...` production -- fixed
/// ordering, so an `<Annotation>` appearing after the content children
/// belongs to a nested construct, not a second annotations group and is
/// left in the remainder for the caller's own children to reject) and the
/// remaining content children.
pub(crate) fn split_axiom_annotations<'a, 'input>(
    children: &[roxmltree::Node<'a, 'input>],
    prefixes: &Prefixes,
) -> Result<(Vec<Annotation>, Vec<roxmltree::Node<'a, 'input>>), String> {
    let mut anns = Vec::new();
    let mut i = 0;
    while i < children.len() && children[i].tag_name().name() == "Annotation" {
        anns.push(parse_annotation(children[i], prefixes)?);
        i += 1;
    }
    Ok((anns, children[i..].to_vec()))
}

/// Parse an `<AnnotationAssertion>`/`<SubAnnotationPropertyOf>`/
/// `<AnnotationPropertyDomain>`/`<AnnotationPropertyRange>` element into an
/// [`owl_ontology::Axiom::AxiomAnnotationAxiom`].
pub(crate) fn parse_annotation_axiom(
    node: roxmltree::Node,
    prefixes: &Prefixes,
) -> Result<owl_ontology::Axiom, String> {
    use owl_ontology::{AnnotationAxiom, Axiom};

    let tag = node.tag_name().name();
    let children: Vec<_> = node.children().filter(|n| n.is_element()).collect();
    let (anns, rest) = split_axiom_annotations(&children, prefixes)?;

    match tag {
        "AnnotationAssertion" => {
            if rest.len() != 3 {
                return Err(format!(
                    "<AnnotationAssertion> expects exactly 3 children (AnnotationProperty, AnnotationSubject, AnnotationValue), found {}",
                    rest.len()
                ));
            }
            let prop = resolve_iri(rest[0], prefixes)?;
            let subj = parse_annotation_filler_as_graph_element(rest[1], prefixes)?;
            let val = parse_annotation_filler_as_graph_element(rest[2], prefixes)?;
            Ok(Axiom::AxiomAnnotationAxiom(
                AnnotationAxiom::AnnotationAssertion(anns, prop, subj, val),
            ))
        }
        "SubAnnotationPropertyOf" => {
            if rest.len() != 2 {
                return Err(format!(
                    "<SubAnnotationPropertyOf> expects exactly 2 AnnotationProperty children, found {}",
                    rest.len()
                ));
            }
            let sub = resolve_iri(rest[0], prefixes)?;
            let sup = resolve_iri(rest[1], prefixes)?;
            Ok(Axiom::AxiomAnnotationAxiom(
                AnnotationAxiom::SubAnnotationPropertyOf(anns, sub, sup),
            ))
        }
        "AnnotationPropertyDomain" => {
            if rest.len() != 2 {
                return Err(format!(
                    "<AnnotationPropertyDomain> expects exactly 2 children, found {}",
                    rest.len()
                ));
            }
            let prop = resolve_iri(rest[0], prefixes)?;
            let dom = parse_iri_text_element(rest[1], prefixes)?;
            Ok(Axiom::AxiomAnnotationAxiom(
                AnnotationAxiom::AnnotationPropertyDomain(anns, prop, dom),
            ))
        }
        "AnnotationPropertyRange" => {
            if rest.len() != 2 {
                return Err(format!(
                    "<AnnotationPropertyRange> expects exactly 2 children, found {}",
                    rest.len()
                ));
            }
            let prop = resolve_iri(rest[0], prefixes)?;
            let rng = parse_iri_text_element(rest[1], prefixes)?;
            Ok(Axiom::AxiomAnnotationAxiom(
                AnnotationAxiom::AnnotationPropertyRange(anns, prop, rng),
            ))
        }
        other => Err(format!("<{other}> is not a valid AnnotationAxiom")),
    }
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

    #[test]
    fn parses_anonymous_individual_annotation_value() {
        let d = doc(r#"<Annotation>
                 <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#seeAlso"/>
                 <AnonymousIndividual nodeID="b0"/>
               </Annotation>"#);
        let (_, value) = parse_annotation(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            value,
            AnnotationValue::IndividualAnnotation(Individual::AnonymousIndividual(_))
        ));
    }

    #[test]
    fn errors_on_meta_annotation_with_695_reference() {
        let d = doc(r#"<Annotation>
                 <Annotation>
                   <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                   <Literal>meta</Literal>
                 </Annotation>
                 <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                 <Literal>hello</Literal>
               </Annotation>"#);
        let err = parse_annotation(d.root_element(), &Prefixes::new()).unwrap_err();
        assert!(err.contains("695"));
    }

    #[test]
    fn splits_leading_axiom_annotations() {
        let d = doc(r#"<SubClassOf>
                 <Annotation>
                   <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                   <Literal>why</Literal>
                 </Annotation>
                 <Class IRI="http://example.org/A"/>
                 <Class IRI="http://example.org/B"/>
               </SubClassOf>"#);
        let children: Vec<_> = d
            .root_element()
            .children()
            .filter(|n| n.is_element())
            .collect();
        let (anns, rest) = split_axiom_annotations(&children, &Prefixes::new()).unwrap();
        assert_eq!(anns.len(), 1);
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn parses_annotation_assertion() {
        let d = doc(r#"<AnnotationAssertion>
                 <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#label"/>
                 <IRI>http://example.org/Pizza</IRI>
                 <Literal>Pizza</Literal>
               </AnnotationAssertion>"#);
        let axiom = parse_annotation_axiom(d.root_element(), &Prefixes::new()).unwrap();
        assert!(matches!(
            axiom,
            owl_ontology::Axiom::AxiomAnnotationAxiom(
                owl_ontology::AnnotationAxiom::AnnotationAssertion(_, _, _, _)
            )
        ));
    }
}

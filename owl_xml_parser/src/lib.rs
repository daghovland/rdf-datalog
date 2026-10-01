/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! OWL 2 XML Serialization parser.
//!
//! Parses [OWL 2 XML Serialization](https://www.w3.org/TR/owl2-xml-serialization/)
//! (`.owx`/`.owl`) documents into an [`owl_ontology::Ontology`].
//!
//! See `docs/plans/OWL_XML_PLAN.md` for the grammar subset this parser
//! covers and the module layout. CLI/notebook wiring is tracked in
//! [#609](https://github.com/daghovland/rdf-datalog/issues/609);
//! `<DatatypeDefinition>` in
//! [#694](https://github.com/daghovland/rdf-datalog/issues/694); and
//! meta-annotations (annotations on annotations) in
//! [#695](https://github.com/daghovland/rdf-datalog/issues/695).

mod annotation;
mod assertion;
mod axiom;
mod class_expr;
mod data_range;
mod declaration;
mod individual;
mod iri;
mod property_axiom;
mod property_expr;

use owl_ontology::Ontology;

/// Parse an OWL/XML document (`.owx`/`.owl`) and produce an
/// [`owl_ontology::Ontology`].
///
/// Covers the full subset described in `docs/plans/OWL_XML_PLAN.md`: the
/// ontology header, `<Prefix>`/`<Import>` declarations, ontology-level
/// `<Annotation>`s, `<Declaration>`, class axioms, object-/data-property
/// axioms (including `<HasKey>`), ABox (individual) axioms, the remaining
/// annotation axioms, `<AnonymousIndividual>` fillers, and axiom-level
/// `<Annotation>` children on every axiom kind. `<DatatypeDefinition>` is
/// not yet recognized (see
/// [#694](https://github.com/daghovland/rdf-datalog/issues/694)) and will
/// cause a parse error if present.
pub fn parse(input: &str) -> Result<Ontology, String> {
    let doc = roxmltree::Document::parse(input).map_err(|e| format!("XML parse error: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "Ontology" {
        return Err(format!(
            "expected <Ontology> root element, found <{}>",
            root.tag_name().name()
        ));
    }

    let prefixes = iri::collect_prefixes(root);

    let ontology_iri = root.attribute("ontologyIRI").map(|s| s.to_string());
    let version_iri = root.attribute("versionIRI").map(|s| s.to_string());

    let mut imports = Vec::new();
    let mut ontology_annotations = Vec::new();
    let mut axioms = Vec::new();

    for child in root.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "Prefix" => {
                // Already collected in the up-front `iri::collect_prefixes` pass.
            }
            "Import" => {
                let text = child.text().unwrap_or("").trim().to_string();
                imports.push(ingress::IriReference(text));
            }
            "Annotation" => {
                let ann = annotation::parse_annotation(child, &prefixes)?;
                ontology_annotations.push(ann);
            }
            "Declaration" => {
                let axiom = declaration::parse_declaration(child, &prefixes)?;
                axioms.push(axiom);
            }
            "SubClassOf" | "EquivalentClasses" | "DisjointClasses" | "DisjointUnion" => {
                let axiom = axiom::parse_class_axiom(child, &prefixes)?;
                axioms.push(axiom);
            }
            "SubObjectPropertyOf"
            | "EquivalentObjectProperties"
            | "DisjointObjectProperties"
            | "ObjectPropertyDomain"
            | "ObjectPropertyRange"
            | "InverseObjectProperties"
            | "FunctionalObjectProperty"
            | "InverseFunctionalObjectProperty"
            | "ReflexiveObjectProperty"
            | "IrreflexiveObjectProperty"
            | "SymmetricObjectProperty"
            | "AsymmetricObjectProperty"
            | "TransitiveObjectProperty" => {
                let axiom = property_axiom::parse_object_property_axiom(child, &prefixes)?;
                axioms.push(axiom);
            }
            "SubDataPropertyOf"
            | "EquivalentDataProperties"
            | "DisjointDataProperties"
            | "DataPropertyDomain"
            | "DataPropertyRange"
            | "FunctionalDataProperty" => {
                let axiom = property_axiom::parse_data_property_axiom(child, &prefixes)?;
                axioms.push(axiom);
            }
            "HasKey" => {
                let axiom = property_axiom::parse_has_key(child, &prefixes)?;
                axioms.push(axiom);
            }
            "ClassAssertion"
            | "ObjectPropertyAssertion"
            | "NegativeObjectPropertyAssertion"
            | "DataPropertyAssertion"
            | "NegativeDataPropertyAssertion"
            | "SameIndividual"
            | "DifferentIndividuals" => {
                let axiom = assertion::parse_assertion(child, &prefixes)?;
                axioms.push(axiom);
            }
            "AnnotationAssertion"
            | "SubAnnotationPropertyOf"
            | "AnnotationPropertyDomain"
            | "AnnotationPropertyRange" => {
                let axiom = annotation::parse_annotation_axiom(child, &prefixes)?;
                axioms.push(axiom);
            }
            other => {
                return Err(format!(
                    "unsupported OWL/XML axiom element <{other}> (see #694)"
                ));
            }
        }
    }

    let version = match (ontology_iri, version_iri) {
        (Some(o), Some(v)) => ingress::OntologyVersion::VersionedOntology {
            ontology_iri: ingress::IriReference(o),
            version_iri: ingress::IriReference(v),
        },
        (Some(o), None) => ingress::OntologyVersion::NamedOntology(ingress::IriReference(o)),
        (None, _) => ingress::OntologyVersion::UnNamedOntology,
    };

    Ok(Ontology::new(
        imports,
        version,
        ontology_annotations,
        axioms,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unnamed_empty_ontology() {
        let onto = parse(
            r#"<?xml version="1.0"?><Ontology xmlns="http://www.w3.org/2002/07/owl#"></Ontology>"#,
        )
        .unwrap();
        assert_eq!(onto.version, ingress::OntologyVersion::UnNamedOntology);
        assert!(onto.axioms.is_empty());
    }

    #[test]
    fn rejects_non_ontology_root() {
        match parse(r#"<?xml version="1.0"?><rdf:RDF xmlns:rdf="x"></rdf:RDF>"#) {
            Err(e) => assert!(e.contains("Ontology")),
            Ok(_) => panic!("expected an error for a non-<Ontology> root"),
        }
    }
}

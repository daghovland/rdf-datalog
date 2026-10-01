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
//! covers, the module layout, and what's deferred (tracked in
//! [#606](https://github.com/daghovland/rdf-datalog/issues/606)-[#609](https://github.com/daghovland/rdf-datalog/issues/609)).
//! Issue [#605](https://github.com/daghovland/rdf-datalog/issues/605) tracks
//! this issue's own scope: the `<Ontology>` header, `<Prefix>`, `<Import>`,
//! ontology-level `<Annotation>`, and `<Declaration>`.

mod annotation;
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
/// Only the subset described in `docs/plans/OWL_XML_PLAN.md` is supported:
/// the ontology header, `<Prefix>`/`<Import>` declarations, ontology-level
/// `<Annotation>`s, `<Declaration>`, class axioms, and object-/data-property
/// axioms (including `<HasKey>`). ABox (individual) axioms and
/// non-`Declaration` axiom-level `<Annotation>` children are not yet
/// recognized and will cause a parse error if present — see
/// [#608](https://github.com/daghovland/rdf-datalog/issues/608).
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
            other => {
                return Err(format!(
                    "unsupported OWL/XML axiom element <{other}> (see #608)"
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

/// Cheap content sniff used by `dagalog`'s CLI/notebook-kernel wiring
/// ([#609](https://github.com/daghovland/rdf-datalog/issues/609)) to decide
/// whether a `.owl`-extensioned file is OWL/XML Serialization (this crate's
/// format) rather than, say, a Turtle-serialized ontology — the `.owl`
/// extension is ambiguous and used for both elsewhere in this repository's
/// own test fixtures (`tests/testdata/equality.owl` and friends are Turtle).
///
/// Returns `true` only when `src` parses as XML *and* its root element's tag
/// name is literally `Ontology` (the OWL/XML Serialization spec's root
/// element, <https://www.w3.org/TR/owl2-xml-serialization/>). Returns
/// `false` for anything else, including malformed XML, Turtle text, and
/// RDF/XML (whose root element is `rdf:RDF`, not `Ontology`) — this function
/// makes no attempt to recognize RDF/XML, which this codebase does not parse
/// under any extension today.
///
/// This is a sniff, not a validator: a `.owl` file that passes this check
/// may still fail `parse` for an unrelated reason (e.g. an unsupported axiom
/// element), and that failure should surface as a normal parse error rather
/// than a silent fallback to a different parser.
pub fn looks_like_owl_xml(src: &str) -> bool {
    match roxmltree::Document::parse(src) {
        Ok(doc) => doc.root_element().tag_name().name() == "Ontology",
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── looks_like_owl_xml, see #609 ─────────────────────────────────────

    #[test]
    fn sniff_recognizes_owl_xml_root() {
        assert!(looks_like_owl_xml(
            r#"<?xml version="1.0"?><Ontology xmlns="http://www.w3.org/2002/07/owl#"></Ontology>"#
        ));
    }

    #[test]
    fn sniff_recognizes_owl_xml_root_with_attributes() {
        assert!(looks_like_owl_xml(
            r#"<Ontology xmlns="http://www.w3.org/2002/07/owl#" ontologyIRI="http://example.org/o"><Declaration><Class IRI="http://example.org/C"/></Declaration></Ontology>"#
        ));
    }

    #[test]
    fn sniff_rejects_turtle_text() {
        assert!(!looks_like_owl_xml(
            "@prefix ex: <http://example.org/> .\nex:a a ex:B .\n"
        ));
    }

    #[test]
    fn sniff_rejects_turtle_starting_with_an_iri() {
        // A bare IRI subject is not a legal XML tag name, so this fails to
        // parse as XML at all rather than being confused for one.
        assert!(!looks_like_owl_xml(
            "<http://example.org/a> a <http://example.org/B> .\n"
        ));
    }

    #[test]
    fn sniff_rejects_rdf_xml_root() {
        assert!(!looks_like_owl_xml(
            r#"<?xml version="1.0"?><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"></rdf:RDF>"#
        ));
    }

    #[test]
    fn sniff_rejects_malformed_xml() {
        assert!(!looks_like_owl_xml("<Ontology><unclosed></Ontology>"));
    }

    #[test]
    fn sniff_rejects_empty_string() {
        assert!(!looks_like_owl_xml(""));
    }

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

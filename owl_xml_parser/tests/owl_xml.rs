/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `owl_xml_parser::parse`, one OWL/XML snippet per
//! test. See `docs/plans/OWL_XML_PLAN.md` for the phase this coverage maps
//! to (phases 1-6, this issue's #605 scope only: ontology header, prefixes,
//! imports, ontology-level annotations, declarations — no class/property/
//! ABox axioms yet, those are #606-608).

use ingress::IriReference;
use owl_ontology::{Entity, FullIri};

fn iri(s: &str) -> FullIri {
    FullIri(IriReference(s.to_string()))
}

const NS: &str = r#"xmlns="http://www.w3.org/2002/07/owl#" xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#" xmlns:xsd="http://www.w3.org/2001/XMLSchema#""#;

fn wrap(attrs: &str, body: &str) -> String {
    format!(r#"<?xml version="1.0"?><Ontology {NS} {attrs}>{body}</Ontology>"#)
}

// --- Phase 2: ontology header ----------------------------------------------

#[test]
#[ignore] // #605
fn empty_unnamed_ontology() {
    let onto = owl_xml_parser::parse(&wrap("", "")).unwrap();
    assert_eq!(onto.version, ingress::OntologyVersion::UnNamedOntology);
    assert!(onto.axioms.is_empty());
}

#[test]
#[ignore] // #605
fn named_ontology() {
    let onto =
        owl_xml_parser::parse(&wrap(r#"ontologyIRI="http://example.org/pizza""#, "")).unwrap();
    assert_eq!(
        onto.try_get_ontology_iri(),
        Some(&IriReference("http://example.org/pizza".to_string()))
    );
    assert_eq!(onto.try_get_version_iri(), None);
}

#[test]
#[ignore] // #605
fn named_ontology_with_version_iri() {
    let onto = owl_xml_parser::parse(&wrap(
        r#"ontologyIRI="http://example.org/pizza" versionIRI="http://example.org/pizza/1.0""#,
        "",
    ))
    .unwrap();
    assert_eq!(
        onto.try_get_ontology_iri(),
        Some(&IriReference("http://example.org/pizza".to_string()))
    );
    assert_eq!(
        onto.try_get_version_iri(),
        Some(&IriReference("http://example.org/pizza/1.0".to_string()))
    );
}

// --- Phase 3: Prefix + Import -----------------------------------------------

#[test]
#[ignore] // #605
fn import_declaration() {
    let onto = owl_xml_parser::parse(&wrap(
        "",
        r#"<Import>http://example.org/imported</Import>"#,
    ))
    .unwrap();
    assert_eq!(
        onto.directly_imports_documents,
        vec![IriReference("http://example.org/imported".to_string())]
    );
}

#[test]
#[ignore] // #605
fn multiple_imports() {
    let onto = owl_xml_parser::parse(&wrap(
        "",
        r#"<Import>http://example.org/a</Import><Import>http://example.org/b</Import>"#,
    ))
    .unwrap();
    assert_eq!(
        onto.directly_imports_documents,
        vec![
            IriReference("http://example.org/a".to_string()),
            IriReference("http://example.org/b".to_string()),
        ]
    );
}

#[test]
#[ignore] // #605
fn prefix_declaration_used_by_declaration_abbreviated_iri() {
    let src = wrap(
        r#"ontologyIRI="http://example.org/pizza""#,
        r#"<Prefix name="" IRI="http://example.org/pizza#"/>
           <Declaration><Class abbreviatedIRI=":Pizza"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 1);
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::ClassDeclaration(c))) => {
            assert_eq!(*c, iri("http://example.org/pizza#Pizza"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

#[test]
#[ignore] // #605
fn prefix_declaration_non_default_prefix() {
    let src = wrap(
        "",
        r#"<Prefix name="owl" IRI="http://www.w3.org/2002/07/owl#"/>
           <Declaration><Class abbreviatedIRI="owl:Thing"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::ClassDeclaration(c))) => {
            assert_eq!(*c, iri("http://www.w3.org/2002/07/owl#Thing"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

// --- Phase 4: Declaration, all six Entity variants --------------------------

#[test]
#[ignore] // #605
fn declaration_class_full_iri() {
    let src = wrap(
        "",
        r#"<Declaration><Class IRI="http://example.org/pizza#Pizza"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 1);
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((anns, Entity::ClassDeclaration(c))) => {
            assert!(anns.is_empty());
            assert_eq!(*c, iri("http://example.org/pizza#Pizza"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

#[test]
#[ignore] // #605
fn declaration_object_property() {
    let src = wrap(
        "",
        r#"<Declaration><ObjectProperty IRI="http://example.org/pizza#hasTopping"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::ObjectPropertyDeclaration(p))) => {
            assert_eq!(*p, iri("http://example.org/pizza#hasTopping"));
        }
        other => panic!("expected AxiomDeclaration(ObjectPropertyDeclaration), got {other:?}"),
    }
}

#[test]
#[ignore] // #605
fn declaration_data_property() {
    let src = wrap(
        "",
        r#"<Declaration><DataProperty IRI="http://example.org/pizza#hasCalories"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::DataPropertyDeclaration(p))) => {
            assert_eq!(*p, iri("http://example.org/pizza#hasCalories"));
        }
        other => panic!("expected AxiomDeclaration(DataPropertyDeclaration), got {other:?}"),
    }
}

#[test]
#[ignore] // #605
fn declaration_annotation_property() {
    let src = wrap(
        "",
        r#"<Declaration><AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::AnnotationPropertyDeclaration(p))) => {
            assert_eq!(*p, iri("http://www.w3.org/2000/01/rdf-schema#comment"));
        }
        other => panic!(
            "expected AxiomDeclaration(AnnotationPropertyDeclaration), got {other:?}"
        ),
    }
}

#[test]
#[ignore] // #605
fn declaration_named_individual() {
    let src = wrap(
        "",
        r#"<Declaration><NamedIndividual IRI="http://example.org/pizza#margherita1"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((
            _,
            Entity::NamedIndividualDeclaration(owl_ontology::Individual::NamedIndividual(i)),
        )) => {
            assert_eq!(*i, iri("http://example.org/pizza#margherita1"));
        }
        other => panic!("expected AxiomDeclaration(NamedIndividualDeclaration), got {other:?}"),
    }
}

#[test]
#[ignore] // #605
fn declaration_datatype() {
    let src = wrap(
        "",
        r#"<Declaration><Datatype IRI="http://www.w3.org/2001/XMLSchema#positiveInteger"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((_, Entity::DatatypeDeclaration(d))) => {
            assert_eq!(*d, iri("http://www.w3.org/2001/XMLSchema#positiveInteger"));
        }
        other => panic!("expected AxiomDeclaration(DatatypeDeclaration), got {other:?}"),
    }
}

#[test]
#[ignore] // #605
fn multiple_declarations_preserve_order() {
    let src = wrap(
        "",
        r#"<Declaration><Class IRI="http://example.org/pizza#Pizza"/></Declaration>
           <Declaration><Class IRI="http://example.org/pizza#Food"/></Declaration>
           <Declaration><ObjectProperty IRI="http://example.org/pizza#hasTopping"/></Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 3);
}

// --- Phase 5: annotations (ontology-level, and on a Declaration) -----------

#[test]
#[ignore] // #605
fn ontology_level_annotation() {
    let src = wrap(
        "",
        r#"<Annotation>
             <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
             <Literal>An example ontology</Literal>
           </Annotation>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.annotations.len(), 1);
    assert_eq!(onto.annotations[0].0, iri("http://www.w3.org/2000/01/rdf-schema#comment"));
}

#[test]
#[ignore] // #605
fn declaration_with_leading_annotation() {
    let src = wrap(
        "",
        r#"<Declaration>
             <Annotation>
               <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
               <Literal>A pizza</Literal>
             </Annotation>
             <Class IRI="http://example.org/pizza#Pizza"/>
           </Declaration>"#,
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(onto.axioms.len(), 1);
    match &onto.axioms[0] {
        owl_ontology::Axiom::AxiomDeclaration((anns, Entity::ClassDeclaration(c))) => {
            assert_eq!(anns.len(), 1);
            assert_eq!(*c, iri("http://example.org/pizza#Pizza"));
        }
        other => panic!("expected AxiomDeclaration(ClassDeclaration), got {other:?}"),
    }
}

// --- Phase 6: full-document integration -------------------------------------

#[test]
#[ignore] // #605
fn pizza_style_header_integration() {
    let src = format!(
        r#"<?xml version="1.0"?><Ontology {NS} ontologyIRI="http://example.org/pizza">
           <Prefix name="" IRI="http://example.org/pizza#"/>
           <Prefix name="owl" IRI="http://www.w3.org/2002/07/owl#"/>
           <Import>http://example.org/imported</Import>
           <Annotation>
             <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
             <Literal>An example ontology</Literal>
           </Annotation>
           <Declaration><Class abbreviatedIRI=":Pizza"/></Declaration>
           <Declaration><Class abbreviatedIRI=":Food"/></Declaration>
           <Declaration><ObjectProperty abbreviatedIRI=":hasTopping"/></Declaration>
           </Ontology>"#
    );
    let onto = owl_xml_parser::parse(&src).unwrap();
    assert_eq!(
        onto.try_get_ontology_iri(),
        Some(&IriReference("http://example.org/pizza".to_string()))
    );
    assert_eq!(
        onto.directly_imports_documents,
        vec![IriReference("http://example.org/imported".to_string())]
    );
    assert_eq!(onto.annotations.len(), 1);
    assert_eq!(onto.axioms.len(), 3);
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `DatatypeDefinition := axiomAnnotations Datatype DataRange`
//! -> `owl_ontology::Axiom::AxiomDatatypeDefinition`.
//!
//! Stubbed per [#694](https://github.com/daghovland/rdf-datalog/issues/694):
//! tests below are `#[ignore]`d pending implementation, following this
//! repo's TDD workflow (plan -> ignored tests + compiling stub ->
//! implement test-by-test).
//!
//! Axiom-level `<Annotation>` children on `<DatatypeDefinition>` (as opposed
//! to `<Declaration>`'s own leading annotations, handled by #605) are out of
//! scope for this issue -- deferred to
//! [#608](https://github.com/daghovland/rdf-datalog/issues/608), matching
//! `axiom.rs`'s (#606) and `property_axiom.rs`'s (#607) precedent for every
//! other non-`Declaration` axiom. Encountering one produces a clear error
//! rather than being silently dropped.

use crate::iri::Prefixes;
use owl_ontology::Axiom;

/// Parse a `<DatatypeDefinition>` element into an
/// [`Axiom::AxiomDatatypeDefinition`].
///
/// Stub for [#694](https://github.com/daghovland/rdf-datalog/issues/694):
/// not yet implemented.
pub(crate) fn parse_datatype_definition(
    _node: roxmltree::Node,
    _prefixes: &Prefixes,
) -> Result<Axiom, String> {
    Err("<DatatypeDefinition> not yet implemented (see #694)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(src).unwrap()
    }

    #[test]
    #[ignore = "not yet implemented, see #694"]
    fn parses_simple_datatype_definition() {
        let d = doc(
            r#"<DatatypeDefinition>
                 <Datatype IRI="http://example.org/AdultAge"/>
                 <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
               </DatatypeDefinition>"#,
        );
        let axiom = parse_datatype_definition(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomDatatypeDefinition(anns, dt, dr) => {
                assert!(anns.is_empty());
                assert_eq!(dt.0.0, "http://example.org/AdultAge");
                assert_eq!(
                    dr,
                    owl_ontology::DataRange::NamedDataRange(owl_ontology::FullIri(
                        ingress::IriReference(
                            "http://www.w3.org/2001/XMLSchema#integer".to_string()
                        )
                    ))
                );
            }
            other => panic!("expected AxiomDatatypeDefinition, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "not yet implemented, see #694"]
    fn parses_datatype_definition_with_restriction() {
        let d = doc(
            r#"<DatatypeDefinition>
                 <Datatype IRI="http://example.org/NegInt"/>
                 <DatatypeRestriction>
                   <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
                   <FacetRestriction facet="http://www.w3.org/2001/XMLSchema#maxExclusive"><Literal>0</Literal></FacetRestriction>
                 </DatatypeRestriction>
               </DatatypeDefinition>"#,
        );
        let axiom = parse_datatype_definition(d.root_element(), &Prefixes::new()).unwrap();
        match axiom {
            Axiom::AxiomDatatypeDefinition(_, dt, dr) => {
                assert_eq!(dt.0.0, "http://example.org/NegInt");
                assert!(matches!(
                    dr,
                    owl_ontology::DataRange::DatatypeRestriction(_, _)
                ));
            }
            other => panic!("expected AxiomDatatypeDefinition, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "not yet implemented, see #694"]
    fn errors_on_wrong_child_count() {
        let d = doc(
            r#"<DatatypeDefinition><Datatype IRI="http://example.org/AdultAge"/></DatatypeDefinition>"#,
        );
        let err = parse_datatype_definition(d.root_element(), &Prefixes::new()).unwrap_err();
        assert!(err.contains("exactly"));
    }

    #[test]
    #[ignore = "not yet implemented, see #694"]
    fn errors_on_axiom_level_annotation() {
        let d = doc(
            r#"<DatatypeDefinition>
                 <Annotation>
                   <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
                   <Literal>why</Literal>
                 </Annotation>
                 <Datatype IRI="http://example.org/AdultAge"/>
                 <Datatype IRI="http://www.w3.org/2001/XMLSchema#integer"/>
               </DatatypeDefinition>"#,
        );
        let err = parse_datatype_definition(d.root_element(), &Prefixes::new()).unwrap_err();
        assert!(err.contains("608"));
    }
}

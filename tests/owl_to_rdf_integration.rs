/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! End-to-end test of the general OWL 2 → RDF structural mapping
//! ([`owl2rl2datalog::owl2rdf`], [#177](https://github.com/daghovland/rdf-datalog/issues/177)).
//!
//! A Manchester Syntax ontology has no RDF triple representation of its own —
//! its TBox lives only in the parsed `owl_ontology::Ontology`, so the web UI's
//! class-hierarchy query (`?child rdfs:subClassOf ?parent`) finds nothing for a
//! `.omn`-loaded ontology. `owl2rdf` closes that gap. This test walks the whole
//! path: Manchester text → `Ontology` → RDF quads → SPARQL, and back out again
//! through `rdf_owl_translator::rdf2owl`.
//!
//! Run just this file: `cargo test --test owl_to_rdf_integration`

use dag_rdf::Datastore;
use dagalog::run_sparql_query;
use owl_ontology::{Axiom, ClassAxiom, ClassExpression, FullIri, Ontology};
use owl2rl2datalog::owl2rdf;
use rdf_owl_translator::rdf2owl;

/// An OWL 2 Functional-Style Syntax ontology whose `SubClassOf` axiom's own
/// annotation (`rdfs:label "why"`) itself carries a meta-annotation
/// (`rdfs:comment "src"`) — see
/// `owl_functional_parser/tests/serialize_roundtrip.rs::roundtrips_subclassof_with_meta_annotation`
/// for the same fixture used to pin down parsing/serialization of this
/// shape.
const OFN_META_ANNOTATION: &str = r#"
Ontology(<http://example.org/onto>
    Declaration(Class(<http://example.org/Dog>))
    Declaration(Class(<http://example.org/Animal>))
    Declaration(AnnotationProperty(<http://www.w3.org/2000/01/rdf-schema#label>))
    Declaration(AnnotationProperty(<http://www.w3.org/2000/01/rdf-schema#comment>))
    SubClassOf(Annotation(Annotation(<http://www.w3.org/2000/01/rdf-schema#comment> "src") <http://www.w3.org/2000/01/rdf-schema#label> "why") <http://example.org/Dog> <http://example.org/Animal>)
)
"#;

const OMN: &str = r#"
Prefix: : <http://example.org/>
Ontology:
Class: Animal
Class: Dog
    SubClassOf: Animal
ObjectProperty: hasPet
    Domain: Person
    Range: Animal
Class: Person
Individual: fido
    Types: Dog
"#;

fn manchester_to_rdf() -> (Datastore, Ontology) {
    let ontology = manchester_parser::parse(OMN).expect("Manchester source must parse");
    let mut ds = Datastore::new(1000);
    let report = owl2rdf(&mut ds, &ontology);
    assert!(
        report.triples_added > 0,
        "the ontology must produce triples; skipped: {:?}",
        report.skipped
    );
    (ds, ontology)
}

/// The class hierarchy of a Manchester-parsed ontology must be visible to a
/// plain SPARQL query once `owl2rdf` has run — this is exactly the query the
/// web UI's class-hierarchy view issues.
#[test]
fn manchester_tbox_is_queryable_as_rdf() {
    let (ds, _) = manchester_to_rdf();

    let rows = run_sparql_query(
        &ds,
        "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> \
         SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }",
    )
    .expect("query must succeed")
    .rows;
    assert_eq!(rows.len(), 1, "exactly one subclass edge: Dog ⊑ Animal");

    let domain_rows = run_sparql_query(
        &ds,
        "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> \
         PREFIX : <http://example.org/> \
         SELECT ?d WHERE { :hasPet rdfs:domain ?d }",
    )
    .expect("query must succeed")
    .rows;
    assert_eq!(domain_rows.len(), 1, "hasPet must have an rdfs:domain");

    let type_rows = run_sparql_query(
        &ds,
        "PREFIX : <http://example.org/> \
         SELECT ?t WHERE { :fido a ?t }",
    )
    .expect("query must succeed")
    .rows;
    assert!(
        !type_rows.is_empty(),
        "the ABox assertion must be materialised too"
    );
}

/// Round trip: `Ontology` → RDF → `Ontology`. The subclass axiom that only
/// existed inside the Manchester-parsed structure must come back out of the
/// emitted triples via `rdf2owl`.
#[test]
fn owl_to_rdf_to_owl_round_trip_recovers_subclass_axiom() {
    let (mut ds, _) = manchester_to_rdf();
    let recovered = rdf2owl(&mut ds).unwrap().ontology;

    let named = |local: &str| {
        ClassExpression::ClassName(FullIri(dag_rdf::IriReference(format!(
            "http://example.org/{local}"
        ))))
    };
    let expected = Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
        vec![],
        named("Dog"),
        named("Animal"),
    ));
    assert!(
        recovered.axioms.contains(&expected),
        "rdf2owl must recover SubClassOf(Dog, Animal) from the emitted triples; got {:?}",
        recovered.axioms
    );
}

/// A meta-annotation (annotation on an axiom's own annotation, #711/#589):
/// `owl2rdf` must reify the nested `Annotation(...)` via an
/// `owl:Annotation`-typed blank node per the W3C mapping spec's recursive
/// `TANN` rule (<https://www.w3.org/TR/owl2-mapping-to-rdf/#Translation_of_Annotations>),
/// and `rdf_owl_translator::rdf2owl` must recover it symmetrically — not
/// just the top-level axiom annotation, which the `owl:Axiom` reification
/// already handled before this issue.
#[test]
fn meta_annotation_round_trips_through_rdf() {
    let ontology = owl_functional_parser::parse(OFN_META_ANNOTATION)
        .expect("functional-syntax source must parse");
    let mut ds = Datastore::new(1000);
    let report = owl2rdf(&mut ds, &ontology);
    assert!(
        report.triples_added > 0,
        "the ontology must produce triples; skipped: {:?}",
        report.skipped
    );

    let recovered = rdf2owl(&mut ds).unwrap().ontology;
    let anns = recovered
        .axioms
        .iter()
        .find_map(|ax| match ax {
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(anns, _, _)) => Some(anns),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected a SubClassOf axiom; got {:?}", recovered.axioms));

    assert_eq!(anns.len(), 1, "exactly one top-level annotation");
    assert_eq!(
        anns[0].property.0.0,
        "http://www.w3.org/2000/01/rdf-schema#label"
    );
    assert_eq!(
        anns[0].annotations.len(),
        1,
        "the top-level annotation must carry its own meta-annotation, round-tripped through RDF"
    );
    assert_eq!(
        anns[0].annotations[0].property.0.0,
        "http://www.w3.org/2000/01/rdf-schema#comment"
    );
}

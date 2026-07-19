/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! End-to-end test that an ontology parsed from OWL 2 Manchester Syntax
//! ([#139](https://github.com/daghovland/rdf-datalog/issues/139)) actually
//! drives OWL-RL reasoning, not just that it parses into an in-memory
//! `owl_ontology::Ontology`.
//!
//! Manchester `Individual:`/`Types:`/`Facts:` sections parse into OWL
//! `Assertion` axioms (`owl_ontology::axioms::Assertion`). Those axioms are
//! materialised into `Datastore` quads by
//! [`owl2rl2datalog::assert_abox`] ([#159](https://github.com/daghovland/rdf-datalog/issues/159)),
//! the ABox counterpart to `owl2datalog`, which only compiles TBox-style
//! axioms (`SubClassOf`, etc.) into inference rules. The full Manchester →
//! `Datastore` → OWL-RL → SPARQL path is exercised below with no
//! manually-seeded quads.
//!
//! Run just this file: `cargo test --test manchester_owl_reasoning`

use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use dagalog::run_sparql_query;
use datalog::evaluate_rules;
use owl2rl2datalog::{assert_abox, owl2datalog};

const EX: &str = "http://example.org/";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

fn iri(local: &str) -> IriReference {
    IriReference(format!("{EX}{local}"))
}

fn has_triple(ds: &Datastore, subj: &str, pred: &str, obj: &str) -> bool {
    let get = |i: &IriReference| {
        ds.resources
            .resource_map
            .get(&GraphElement::NodeOrEdge(RdfResource::Iri(i.clone())))
            .copied()
    };
    match (
        get(&iri(subj)),
        get(&IriReference(pred.to_string())),
        get(&iri(obj)),
    ) {
        (Some(s), Some(p), Some(o)) => !ds
            .quads_matching(None, Some(s), Some(p), Some(o))
            .is_empty(),
        _ => false,
    }
}

/// A Manchester-parsed `SubClassOf:` axiom, run through `owl2datalog` +
/// `evaluate_rules`, must derive a new `rdf:type` triple for an individual
/// whose `Individual:`/`Types:` assertion is materialised into the
/// `Datastore` by `assert_abox` — the simplest possible OWL-RL inference
/// (`cax-sco` in the OWL 2 RL profile: `C1 SubClassOf C2` and `x rdf:type C1`
/// implies `x rdf:type C2`), driven purely from Manchester Syntax input.
#[test]
fn manchester_subclassof_drives_type_inference() {
    let omn = r#"
Prefix: : <http://example.org/>
Ontology:
Class: Animal
Class: Dog
    SubClassOf: Animal
Individual: fido
    Types: Dog
"#;
    let ontology = manchester_parser::parse(omn).expect("Manchester Syntax must parse");
    assert!(
        !ontology.axioms.is_empty(),
        "expected at least the Dog SubClassOf Animal axiom"
    );

    let mut ds = Datastore::new(1_000);

    // ABox fact materialised from the parsed Manchester `Individual:` frame.
    let added = assert_abox(&mut ds, &ontology);
    assert_eq!(
        added, 1,
        "one ClassAssertion (fido a Dog) must be materialised"
    );
    assert!(has_triple(&ds, "fido", RDF_TYPE, "Dog"));
    assert!(
        !has_triple(&ds, "fido", RDF_TYPE, "Animal"),
        "sanity check: the Animal typing must not already be present before reasoning"
    );

    // TBox from the parsed Manchester ontology drives the actual reasoning.
    let rules = owl2datalog(&mut ds.resources, &ontology);
    assert!(
        !rules.is_empty(),
        "SubClassOf must compile to at least one rule"
    );
    evaluate_rules(rules, &mut ds);

    assert!(
        has_triple(&ds, "fido", RDF_TYPE, "Animal"),
        "OWL-RL reasoning over the Manchester-parsed SubClassOf axiom must infer \
         that fido, asserted as a Dog, is also an Animal"
    );
}

/// Full pipeline: a Manchester `Individual:` frame with `Types:` and `Facts:`
/// (both an object-property and a data-property fact) must materialise into
/// `Datastore` quads via `assert_abox`, then OWL-RL reasoning must infer the
/// superclass typing, and finally a SPARQL query must see both the asserted
/// facts and the inferred one — end-to-end from Manchester text with no
/// manually-seeded quads. Covers all three atomic assertion kinds:
/// `ClassAssertion`, `ObjectPropertyAssertion`, `DataPropertyAssertion`.
#[test]
fn manchester_individual_frame_materialises_abox_and_reasons() {
    let omn = r#"
Prefix: : <http://example.org/>
Ontology:
Class: Animal
Class: Dog
    SubClassOf: Animal
ObjectProperty: hasOwner
DataProperty: hasName
Individual: fido
    Types: Dog
    Facts: hasOwner alice, hasName "Rex"
Individual: alice
"#;
    let ontology = manchester_parser::parse(omn).expect("Manchester Syntax must parse");

    let mut ds = Datastore::new(1_000);

    // Materialise the ABox: 1 ClassAssertion + 1 ObjectPropertyAssertion +
    // 1 DataPropertyAssertion = 3 ground triples.
    let added = assert_abox(&mut ds, &ontology);
    assert_eq!(
        added, 3,
        "fido a Dog, fido hasOwner alice, fido hasName \"Rex\" must all be materialised"
    );

    // Asserted object-graph facts are present as quads.
    assert!(has_triple(&ds, "fido", RDF_TYPE, "Dog"));
    assert!(has_triple(&ds, "fido", &format!("{EX}hasOwner"), "alice"));
    assert!(
        !has_triple(&ds, "fido", RDF_TYPE, "Animal"),
        "sanity check: superclass typing must not be present before reasoning"
    );

    // The data-property literal is best checked through SPARQL.
    let name_rows = run_sparql_query(
        &ds,
        r#"PREFIX : <http://example.org/> SELECT ?n WHERE { :fido :hasName ?n }"#,
    )
    .expect("query must succeed")
    .rows;
    assert_eq!(
        name_rows.len(),
        1,
        "fido must have exactly one hasName fact"
    );

    // Reason over the parsed TBox.
    let rules = owl2datalog(&mut ds.resources, &ontology);
    evaluate_rules(rules, &mut ds);

    assert!(
        has_triple(&ds, "fido", RDF_TYPE, "Animal"),
        "OWL-RL must infer fido a Animal from the materialised Dog typing"
    );

    // End-to-end SPARQL: query for all Animals must return the inferred fido.
    let animal_rows = run_sparql_query(
        &ds,
        r#"PREFIX : <http://example.org/>
           SELECT ?a WHERE { ?a a :Animal }"#,
    )
    .expect("query must succeed")
    .rows;
    assert_eq!(
        animal_rows.len(),
        1,
        "exactly fido should be inferred as an Animal"
    );

    // And the object-property fact is queryable end-to-end.
    let owner_rows = run_sparql_query(
        &ds,
        r#"PREFIX : <http://example.org/>
           SELECT ?o WHERE { :fido :hasOwner ?o }"#,
    )
    .expect("query must succeed")
    .rows;
    assert_eq!(
        owner_rows.len(),
        1,
        "fido must have exactly one hasOwner fact"
    );
}

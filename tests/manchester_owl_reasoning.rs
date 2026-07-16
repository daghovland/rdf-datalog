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
//! `Assertion` axioms (`owl_ontology::axioms::Assertion`), but nothing in the
//! `owl2rl2datalog` pipeline currently turns `Assertion` axioms into
//! `Datastore` quads — `owl2datalog` only compiles TBox-style axioms
//! (`SubClassOf`, etc.) into inference rules, exactly mirroring how
//! `tests/owl_integration.rs`'s RDF-based tests get their ABox facts from the
//! quads the Turtle/RDF-XML parser already wrote into the `Datastore`, not
//! from `rdf2owl`'s extracted axioms. So this test follows that same split:
//! the TBox (`SubClassOf`) comes from parsing Manchester Syntax; the ABox
//! fact is seeded directly as a quad. Wiring Manchester ABox assertions into
//! the `Datastore` is tracked in
//! [#159](https://github.com/daghovland/rdf-datalog/issues/159).

use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource, Triple};
use datalog::evaluate_rules;
use owl2rl2datalog::owl2datalog;

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
/// already asserted as an instance of the subclass — the simplest possible
/// OWL-RL inference (`cax-sco` in the OWL 2 RL profile: `C1 SubClassOf C2`
/// and `x rdf:type C1` implies `x rdf:type C2`).
#[test]
fn manchester_subclassof_drives_type_inference() {
    let omn = r#"
Prefix: : <http://example.org/>
Ontology:
Class: Animal
Class: Dog
    SubClassOf: Animal
"#;
    let ontology = manchester_parser::parse(omn).expect("Manchester Syntax must parse");
    assert!(
        !ontology.axioms.is_empty(),
        "expected at least the Dog SubClassOf Animal axiom"
    );

    let mut ds = Datastore::new(1_000);

    // ABox fact seeded directly (see module docs: Manchester `Individual:`
    // assertions aren't wired into the Datastore yet, tracked in #159).
    let fido = ds.add_node_resource(RdfResource::Iri(iri("fido")));
    let dog = ds.add_node_resource(RdfResource::Iri(iri("Dog")));
    let rdf_type = ds.add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_string())));
    ds.add_triple(Triple {
        subject: fido,
        predicate: rdf_type,
        obj: dog,
    });
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

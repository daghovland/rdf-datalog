/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Materialise OWL 2 ABox assertions into `Datastore` quads.
//!
//! Frame-based OWL syntaxes such as Manchester Syntax
//! ([#139](https://github.com/daghovland/rdf-datalog/issues/139)) parse
//! `Individual:`/`Types:`/`Facts:` sections into
//! [`owl_ontology::Assertion`] axioms rather than into RDF quads. The RDF
//! pipelines (Turtle, RDF/XML, JSON-LD) get their ABox facts as quads straight
//! from the parser, so [`crate::owl2datalog`] only compiles TBox-style axioms
//! (`SubClassOf`, property axioms, …) into inference rules and never looks at
//! `Assertion` axioms. This module fills that gap by walking the ontology's
//! `Axiom::AxiomAssertion(..)` axioms and interning the corresponding ground
//! triples into a [`Datastore`], so the reasoner has ABox facts to work from.
//! Tracked in [#159](https://github.com/daghovland/rdf-datalog/issues/159).

use dag_rdf::{Datastore, GraphElementId, RdfResource, Triple};
use ingress::{IriReference, RDF_TYPE};
use owl_ontology::{
    Assertion, Axiom, ClassExpression, FullIri, Individual, ObjectPropertyExpression, Ontology,
};

/// Intern an `Individual` as a `GraphElementId`.
///
/// Named individuals become IRI nodes. Anonymous individuals are routed
/// through [`GraphElementManager::get_or_create_named_anon_resource`] keyed
/// by a namespaced string derived from the parser-assigned id (rather than
/// reusing that raw `u32` directly as the `AnonymousBlankNode` id). That
/// method dedups by string key — so repeated references to the same
/// anonymous individual within one ontology still intern to the same node —
/// and on a cache miss allocates a fresh id from
/// `GraphElementManager::anon_resource_count`, the single monotonic counter
/// that also backs [`Datastore::new_anonymous_blank_node`] (used by
/// Turtle/TriG/N-Triples/JSON-LD blank-node ingestion). Since both sources
/// draw from that one counter, an anonymous individual's id can never
/// numerically collide with an RDF-ingested blank node's id regardless of
/// allocation order, fixing [#183](https://github.com/daghovland/rdf-datalog/issues/183).
/// The `owl-anon-individual#` prefix guards against a string-key collision
/// with a raw Turtle blank-node label happening to equal the bare id.
fn intern_individual(datastore: &mut Datastore, individual: &Individual) -> GraphElementId {
    match individual {
        Individual::NamedIndividual(FullIri(iri)) => {
            datastore.add_node_resource(RdfResource::Iri(iri.clone()))
        }
        Individual::AnonymousIndividual(id) => datastore
            .resources
            .get_or_create_named_anon_resource(format!("owl-anon-individual#{id}")),
    }
}

/// Materialise the ABox assertions of `ontology` as ground quads in
/// `datastore`.
///
/// Only assertions that correspond to a single ground RDF triple are
/// materialised:
///
/// * `ClassAssertion(_, ClassName(C), i)` → `i rdf:type C`
/// * `ObjectPropertyAssertion(_, NamedObjectProperty(p), s, o)` → `s p o`
/// * `DataPropertyAssertion(_, p, s, lit)` → `s p lit`
///
/// Assertions whose class or property is a *complex* expression (a union,
/// intersection, restriction, inverse property, property chain, …) do not
/// correspond to a single ground triple and are skipped with a `log::warn!`,
/// mirroring how [`crate::owl2datalog`] and `rdf_owl_translator` report
/// unsupported/complex constructs. `SameIndividual`, `DifferentIndividuals`,
/// and the negative-assertion variants are likewise out of scope for OWL-RL
/// ground-triple materialisation and are skipped.
///
/// Returns the number of quads added to the datastore.
pub fn assert_abox(datastore: &mut Datastore, ontology: &Ontology) -> usize {
    let mut added = 0usize;
    for axiom in &ontology.axioms {
        let Axiom::AxiomAssertion(assertion) = axiom else {
            continue;
        };
        match assertion {
            Assertion::ClassAssertion(_, ClassExpression::ClassName(FullIri(class_iri)), ind) => {
                let subject = intern_individual(datastore, ind);
                let predicate = datastore
                    .add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_owned())));
                let obj = datastore.add_node_resource(RdfResource::Iri(class_iri.clone()));
                datastore.add_triple(Triple {
                    subject,
                    predicate,
                    obj,
                });
                added += 1;
            }
            Assertion::ObjectPropertyAssertion(
                _,
                ObjectPropertyExpression::NamedObjectProperty(FullIri(prop_iri)),
                source,
                target,
            ) => {
                let subject = intern_individual(datastore, source);
                let predicate = datastore.add_node_resource(RdfResource::Iri(prop_iri.clone()));
                let obj = intern_individual(datastore, target);
                datastore.add_triple(Triple {
                    subject,
                    predicate,
                    obj,
                });
                added += 1;
            }
            Assertion::DataPropertyAssertion(_, FullIri(prop_iri), source, value) => {
                let subject = intern_individual(datastore, source);
                let predicate = datastore.add_node_resource(RdfResource::Iri(prop_iri.clone()));
                let obj = datastore.add_resource(value.clone());
                datastore.add_triple(Triple {
                    subject,
                    predicate,
                    obj,
                });
                added += 1;
            }
            Assertion::ClassAssertion(_, class_expr, _) => {
                log::warn!(
                    "Skipping ClassAssertion with non-atomic class expression (no single ground \
                     triple): {class_expr:?}"
                );
            }
            Assertion::ObjectPropertyAssertion(_, prop_expr, _, _) => {
                log::warn!(
                    "Skipping ObjectPropertyAssertion with non-atomic property expression (no \
                     single ground triple): {prop_expr:?}"
                );
            }
            other => {
                log::warn!(
                    "Skipping ABox assertion not materialisable as a single ground triple: \
                     {other:?}"
                );
            }
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::GraphElement;
    use ingress::OntologyVersion;

    /// Regression test for [#183](https://github.com/daghovland/rdf-datalog/issues/183):
    /// an anonymous individual materialised by `assert_abox` must never
    /// collide with an unrelated RDF-ingested blank node in the same
    /// `Datastore`, even when the OWL parser's own anonymous-individual
    /// counter and `Datastore::new_anonymous_blank_node`'s counter would,
    /// coincidentally, assign the same raw `u32`.
    ///
    /// `Datastore::new_anonymous_blank_node` (i.e.
    /// `GraphElementManager::create_unnamed_anon_resource`) increments its
    /// counter *before* minting, so the first call always produces
    /// `RdfResource::AnonymousBlankNode(1)`. We hand-build an ontology whose
    /// single anonymous individual carries that exact raw id (`1`), which is
    /// the only way to deterministically reproduce the collision: a
    /// parser-driven id can't be pinned down this precisely.
    #[test]
    fn anonymous_individual_does_not_collide_with_rdf_blank_node() {
        let mut ds = Datastore::new(100);

        // An RDF-ingested blank node, e.g. from Turtle. This is the exact
        // same primitive `GraphElementManager::create_unnamed_anon_resource`
        // that backs `get_or_create_named_anon_resource`, so it faithfully
        // stands in for a blank node parsed from RDF data. It is always
        // `AnonymousBlankNode(1)` for a freshly-created `Datastore`.
        let rdf_blank_node_id = ds.new_anonymous_blank_node();

        // An ontology with one anonymous individual asserted as a `:Thing`,
        // carrying the raw id `1` — matching the RDF blank node's raw id
        // above, to reproduce the collision precisely.
        let class_iri = IriReference("http://example.org/Thing".to_string());
        let ontology = Ontology::new(
            vec![],
            OntologyVersion::UnNamedOntology,
            vec![],
            vec![Axiom::AxiomAssertion(Assertion::ClassAssertion(
                vec![],
                ClassExpression::ClassName(FullIri(class_iri.clone())),
                Individual::AnonymousIndividual(1),
            ))],
        );

        let added = assert_abox(&mut ds, &ontology);
        assert_eq!(added, 1, "the ClassAssertion must materialise one triple");

        // Find the subject of the materialised `?s rdf:type :Thing` triple —
        // that's the GraphElementId the anonymous individual was interned to.
        let rdf_type_id = ds
            .resources
            .resource_map
            .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                RDF_TYPE.to_owned(),
            ))))
            .copied()
            .expect("rdf:type must have been interned by assert_abox");
        let class_id = ds
            .resources
            .resource_map
            .get(&GraphElement::NodeOrEdge(RdfResource::Iri(class_iri)))
            .copied()
            .expect(":Thing must have been interned by assert_abox");
        let matches = ds.quads_matching(None, None, Some(rdf_type_id), Some(class_id));
        assert_eq!(matches.len(), 1, "exactly one individual must be typed");
        let anon_individual_id = matches[0].subject;

        assert_ne!(
            anon_individual_id, rdf_blank_node_id,
            "an anonymous individual materialised by assert_abox must not collide with an \
             unrelated RDF-ingested blank node in the same Datastore"
        );
    }
}

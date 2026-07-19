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
/// Named individuals become IRI nodes; anonymous individuals become blank
/// nodes reusing the parser-assigned id. Note that this shares the `u32` blank
/// node space with [`Datastore::new_anonymous_blank_node`], so an anonymous
/// individual could in principle collide with a blank node ingested from RDF
/// into the same store; giving anonymous individuals a distinct namespace is
/// left to [#183](https://github.com/daghovland/rdf-datalog/issues/183)
/// follow-up work.
fn intern_individual(datastore: &mut Datastore, individual: &Individual) -> GraphElementId {
    match individual {
        Individual::NamedIndividual(FullIri(iri)) => {
            datastore.add_node_resource(RdfResource::Iri(iri.clone()))
        }
        Individual::AnonymousIndividual(id) => {
            datastore.add_node_resource(RdfResource::AnonymousBlankNode(*id))
        }
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

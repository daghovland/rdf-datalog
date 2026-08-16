/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Translate an [`owl_ontology::Ontology`] into RDF triples in a [`Datastore`],
//! following the W3C structural mapping
//! <https://www.w3.org/TR/owl2-mapping-to-rdf/>.
//!
//! Frame-based OWL syntaxes (Manchester Syntax) parse straight into an
//! `Ontology` and never produce RDF triples, so a Manchester-loaded ontology's
//! schema is invisible to direct SPARQL querying and cannot be exported as
//! Turtle/TriG. RDF-native input (Turtle, JSON-LD) has those same axioms as
//! real triples from the moment it is parsed. This module removes that
//! asymmetry. Tracked in
//! [#177](https://github.com/daghovland/rdf-datalog/issues/177).
//!
//! This is a *separate, additional* capability: [`crate::owl2datalog`]'s direct
//! `Ontology -> Vec<Rule>` path for OWL-RL reasoning is unaffected and is still
//! the way axioms reach the reasoner — re-deriving rules from freshly emitted
//! RDF would be a needless, lossy round-trip.
//!
//! # Scope of this pass
//!
//! Axioms over *named* (atomic) classes, properties, datatypes and individuals
//! are translated. Axioms mentioning a complex class expression (`ObjectUnionOf`,
//! `ObjectSomeValuesFrom`, cardinality restrictions, …) need the spec's
//! blank-node structural encoding (`rdf:List`s, `owl:Restriction` nodes) and are
//! **not** translated here; they are recorded in
//! [`RdfTranslationReport::skipped`] rather than silently dropped. That general
//! encoding is deferred to
//! [#509](https://github.com/daghovland/rdf-datalog/issues/509), split out of
//! the still-open parent tracking issue
//! [#373](https://github.com/daghovland/rdf-datalog/issues/373) (see that
//! issue's other follow-ups too: property chains
//! [#510](https://github.com/daghovland/rdf-datalog/issues/510), `HasKey`
//! [#511](https://github.com/daghovland/rdf-datalog/issues/511),
//! `DatatypeDefinition`
//! [#512](https://github.com/daghovland/rdf-datalog/issues/512), n-ary
//! disjoint/different constructs
//! [#513](https://github.com/daghovland/rdf-datalog/issues/513), annotation
//! axioms [#514](https://github.com/daghovland/rdf-datalog/issues/514), and
//! ontology header triples
//! [#515](https://github.com/daghovland/rdf-datalog/issues/515)).
//!
//! `DisjointUnionOf` over atomic (named) class-expression members *is*
//! translated in this pass — the `owl:disjointUnionOf`/`rdf:List` encoding
//! doesn't need the general complex-class-expression machinery when every
//! member is a plain named class, which is the common case. A `DisjointUnion`
//! whose members include a genuinely complex class expression still falls
//! back to [#509](https://github.com/daghovland/rdf-datalog/issues/509).

use dag_rdf::{Datastore, GraphElementId, RdfResource, Triple};
use ingress::{
    IriReference, OWL_ANNOTATION_PROPERTY, OWL_ASYMMETRIC_PROPERTY, OWL_CLASS,
    OWL_DATATYPE_PROPERTY, OWL_DIFFERENT_FROM, OWL_DISJOINT_UNION_OF, OWL_DISJOINT_WITH,
    OWL_EQUIVALENT_CLASS, OWL_EQUIVALENT_PROPERTY, OWL_FUNCTIONAL_PROPERTY,
    OWL_INVERSE_FUNCTIONAL_PROPERTY, OWL_IRREFLEXIVE_PROPERTY, OWL_NAMED_INDIVIDUAL,
    OWL_OBJECT_INVERSE_OF, OWL_OBJECT_PROPERTY, OWL_PROPERTY_DISJOINT_WITH, OWL_REFLEXIVE_PROPERTY,
    OWL_SAME_AS, OWL_SYMMETRIC_PROPERTY, OWL_TRANSITIVE_PROPERTY, RDF_FIRST, RDF_NIL, RDF_REST,
    RDF_TYPE, RDFS_DATATYPE, RDFS_DOMAIN, RDFS_RANGE, RDFS_SUB_CLASS_OF, RDFS_SUB_PROPERTY_OF,
};
use owl_ontology::{
    Assertion, Axiom, ClassAxiom, ClassExpression, DataPropertyAxiom, DataRange, Entity, FullIri,
    Individual, ObjectPropertyAxiom, ObjectPropertyExpression, Ontology, SubPropertyExpression,
};

/// What [`owl2rdf`] did, and what it could not do.
///
/// `skipped` holds one human-readable description per axiom that was **not**
/// translated (because it mentions a construct whose RDF encoding is not
/// implemented yet). Callers that care about faithfulness can inspect or
/// surface it instead of having to scrape the log — see
/// [#366](https://github.com/daghovland/rdf-datalog/issues/366).
#[must_use]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RdfTranslationReport {
    /// Number of triples emitted into the datastore.
    pub triples_added: usize,
    /// One description per untranslated axiom.
    pub skipped: Vec<String>,
}

/// Translate every axiom of `ontology` into RDF triples in `datastore`.
///
/// See the module documentation for what is and is not covered.
pub fn owl2rdf(datastore: &mut Datastore, ontology: &Ontology) -> RdfTranslationReport {
    let mut translator = Translator::new(datastore);
    for axiom in &ontology.axioms {
        translator.axiom(axiom);
    }
    translator.report
}

/// Intern an `Individual` as a `GraphElementId`.
///
/// Named individuals become IRI nodes. Anonymous individuals are routed
/// through [`dag_rdf::GraphElementManager::get_or_create_named_anon_resource`]
/// keyed by a namespaced string derived from the parser-assigned id (rather
/// than reusing that raw `u32` directly as the `AnonymousBlankNode` id). That
/// method dedups by string key — so repeated references to the same anonymous
/// individual within one ontology still intern to the same node — and on a
/// cache miss allocates a fresh id from
/// `GraphElementManager::anon_resource_count`, the single monotonic counter
/// that also backs [`Datastore::new_anonymous_blank_node`] (used by
/// Turtle/TriG/N-Triples/JSON-LD blank-node ingestion). Since both sources draw
/// from that one counter, an anonymous individual's id can never numerically
/// collide with an RDF-ingested blank node's id regardless of allocation order,
/// fixing [#183](https://github.com/daghovland/rdf-datalog/issues/183). The
/// `owl-anon-individual#` prefix guards against a string-key collision with a
/// raw Turtle blank-node label happening to equal the bare id.
pub(crate) fn intern_individual(
    datastore: &mut Datastore,
    individual: &Individual,
) -> GraphElementId {
    match individual {
        Individual::NamedIndividual(FullIri(iri)) => {
            datastore.add_node_resource(RdfResource::Iri(iri.clone()))
        }
        Individual::AnonymousIndividual(id) => datastore
            .resources
            .get_or_create_named_anon_resource(format!("owl-anon-individual#{id}")),
    }
}

/// Emit the ground triple for an assertion that maps to exactly one triple.
///
/// Returns `true` if `assertion` was one of the three single-ground-triple
/// forms (`ClassAssertion` / `ObjectPropertyAssertion` / `DataPropertyAssertion`
/// over named entities) and a triple was added, `false` otherwise. Shared by
/// [`owl2rdf`] and [`crate::assert_abox`] so the two never drift.
pub(crate) fn atomic_assertion_triple(datastore: &mut Datastore, assertion: &Assertion) -> bool {
    match assertion {
        Assertion::ClassAssertion(_, ClassExpression::ClassName(FullIri(class_iri)), ind) => {
            let subject = intern_individual(datastore, ind);
            let predicate =
                datastore.add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_owned())));
            let obj = datastore.add_node_resource(RdfResource::Iri(class_iri.clone()));
            datastore.add_triple(Triple {
                subject,
                predicate,
                obj,
            });
            true
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
            true
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
            true
        }
        _ => false,
    }
}

/// Mutable translation state: the target datastore plus the running report.
struct Translator<'a> {
    datastore: &'a mut Datastore,
    report: RdfTranslationReport,
}

impl<'a> Translator<'a> {
    fn new(datastore: &'a mut Datastore) -> Self {
        Self {
            datastore,
            report: RdfTranslationReport::default(),
        }
    }

    // ── low-level helpers ────────────────────────────────────────────────

    fn iri(&mut self, iri: &str) -> GraphElementId {
        self.datastore
            .add_node_resource(RdfResource::Iri(IriReference(iri.to_owned())))
    }

    fn full_iri(&mut self, iri: &FullIri) -> GraphElementId {
        self.datastore
            .add_node_resource(RdfResource::Iri(iri.0.clone()))
    }

    fn triple(&mut self, subject: GraphElementId, predicate: GraphElementId, obj: GraphElementId) {
        self.datastore.add_triple(Triple {
            subject,
            predicate,
            obj,
        });
        self.report.triples_added += 1;
    }

    /// `subject <predicate-iri> object`, interning the predicate IRI.
    fn triple_p(&mut self, subject: GraphElementId, predicate_iri: &str, obj: GraphElementId) {
        let predicate = self.iri(predicate_iri);
        self.triple(subject, predicate, obj);
    }

    /// `subject rdf:type <type-iri>`.
    fn type_triple(&mut self, subject: GraphElementId, type_iri: &str) {
        let obj = self.iri(type_iri);
        self.triple_p(subject, RDF_TYPE, obj);
    }

    fn skip(&mut self, what: &str, detail: impl std::fmt::Debug) {
        let message = format!("{what}: {detail:?}");
        log::warn!("owl2rdf: no RDF encoding implemented yet for {message}");
        self.report.skipped.push(message);
    }

    /// The node id of a *named* class expression, or `None` for a complex one.
    fn named_class(&mut self, expr: &ClassExpression) -> Option<GraphElementId> {
        match expr {
            ClassExpression::ClassName(iri) => Some(self.full_iri(iri)),
            ClassExpression::AnonymousClass(id) => Some(
                self.datastore
                    .add_node_resource(RdfResource::AnonymousBlankNode(*id)),
            ),
            _ => None,
        }
    }

    /// The node id of a *named* object property, or `None` for an inverse
    /// property expression or a property chain.
    fn named_object_property(&mut self, prop: &ObjectPropertyExpression) -> Option<GraphElementId> {
        match prop {
            ObjectPropertyExpression::NamedObjectProperty(iri) => Some(self.full_iri(iri)),
            ObjectPropertyExpression::AnonymousObjectProperty(id) => Some(
                self.datastore
                    .add_node_resource(RdfResource::AnonymousBlankNode(*id)),
            ),
            _ => None,
        }
    }

    /// Emit `a <predicate> b` for every consecutive pair of `items`.
    ///
    /// This is the OWL 2 RDF mapping's encoding of an n-ary
    /// `EquivalentClasses` / `EquivalentObjectProperties` / `SameIndividual`
    /// axiom: a chain of binary triples. `rdf_owl_translator::rdf2owl` reads
    /// each such triple back as its own binary axiom, which is
    /// logically equivalent to the n-ary original.
    fn chain(&mut self, items: &[GraphElementId], predicate_iri: &str) {
        for pair in items.windows(2) {
            self.triple_p(pair[0], predicate_iri, pair[1]);
        }
    }

    /// Build an `rdf:List` structural encoding of `items` and return the id of
    /// its head node (`rdf:nil` for an empty list, per the spec's `T(SEQ ...)`
    /// mapping).
    ///
    /// Each list cell is a fresh blank node with `rdf:first` pointing at the
    /// element and `rdf:rest` at the next cell (or `rdf:nil` for the last).
    fn rdf_list(&mut self, items: &[GraphElementId]) -> GraphElementId {
        let nil = self.iri(RDF_NIL);
        let mut tail = nil;
        for item in items.iter().rev() {
            let cell = self.datastore.new_anonymous_blank_node();
            self.triple_p(cell, RDF_FIRST, *item);
            self.triple_p(cell, RDF_REST, tail);
            tail = cell;
        }
        tail
    }

    /// Resolve every element of a list of class expressions, or `None` if any
    /// of them is complex.
    fn named_classes(&mut self, exprs: &[ClassExpression]) -> Option<Vec<GraphElementId>> {
        exprs.iter().map(|e| self.named_class(e)).collect()
    }

    fn named_object_properties(
        &mut self,
        props: &[ObjectPropertyExpression],
    ) -> Option<Vec<GraphElementId>> {
        props
            .iter()
            .map(|p| self.named_object_property(p))
            .collect()
    }

    // ── axiom dispatch ───────────────────────────────────────────────────

    fn axiom(&mut self, axiom: &Axiom) {
        match axiom {
            Axiom::AxiomDeclaration((_, entity)) => self.declaration(entity),
            Axiom::AxiomClassAxiom(class_axiom) => self.class_axiom(class_axiom),
            Axiom::AxiomObjectPropertyAxiom(prop_axiom) => self.object_property_axiom(prop_axiom),
            Axiom::AxiomDataPropertyAxiom(prop_axiom) => self.data_property_axiom(prop_axiom),
            Axiom::AxiomAssertion(assertion) => self.assertion(assertion),
            other => self.skip("axiom", other),
        }
    }

    fn declaration(&mut self, entity: &Entity) {
        let (iri, type_iri) = match entity {
            Entity::ClassDeclaration(iri) => (iri, OWL_CLASS),
            Entity::ObjectPropertyDeclaration(iri) => (iri, OWL_OBJECT_PROPERTY),
            Entity::DataPropertyDeclaration(iri) => (iri, OWL_DATATYPE_PROPERTY),
            Entity::DatatypeDeclaration(iri) => (iri, RDFS_DATATYPE),
            Entity::AnnotationPropertyDeclaration(iri) => (iri, OWL_ANNOTATION_PROPERTY),
            Entity::NamedIndividualDeclaration(Individual::NamedIndividual(iri)) => {
                (iri, OWL_NAMED_INDIVIDUAL)
            }
            Entity::NamedIndividualDeclaration(individual) => {
                // An anonymous individual has no declaration in RDF.
                self.skip("declaration of anonymous individual", individual);
                return;
            }
        };
        let subject = self.full_iri(iri);
        self.type_triple(subject, type_iri);
    }

    fn class_axiom(&mut self, axiom: &ClassAxiom) {
        match axiom {
            ClassAxiom::SubClassOf(_, sub, sup) => {
                match (self.named_class(sub), self.named_class(sup)) {
                    (Some(sub_id), Some(sup_id)) => {
                        self.triple_p(sub_id, RDFS_SUB_CLASS_OF, sup_id)
                    }
                    _ => self.skip("SubClassOf with complex class expression", axiom),
                }
            }
            ClassAxiom::EquivalentClasses(_, classes) => match self.named_classes(classes) {
                Some(ids) => self.chain(&ids, OWL_EQUIVALENT_CLASS),
                None => self.skip("EquivalentClasses with complex class expression", axiom),
            },
            ClassAxiom::DisjointClasses(_, classes) if classes.len() == 2 => {
                match self.named_classes(classes) {
                    Some(ids) => self.triple_p(ids[0], OWL_DISJOINT_WITH, ids[1]),
                    None => self.skip("DisjointClasses with complex class expression", axiom),
                }
            }
            // n > 2 needs an `owl:AllDisjointClasses` blank node with an
            // `owl:members` rdf:List — deferred to
            // https://github.com/daghovland/rdf-datalog/issues/513.
            ClassAxiom::DisjointUnion(_, class, members) => match self.named_classes(members) {
                Some(ids) => {
                    let class_id = self.full_iri(class);
                    self.type_triple(class_id, OWL_CLASS);
                    let list_head = self.rdf_list(&ids);
                    self.triple_p(class_id, OWL_DISJOINT_UNION_OF, list_head);
                }
                // The disjoint union's members are themselves complex class
                // expressions — needs the general blank-node structural
                // encoding, deferred to
                // https://github.com/daghovland/rdf-datalog/issues/509.
                None => self.skip("DisjointUnion with complex class expression member", axiom),
            },
            other => self.skip("class axiom", other),
        }
    }

    fn object_property_axiom(&mut self, axiom: &ObjectPropertyAxiom) {
        match axiom {
            ObjectPropertyAxiom::ObjectPropertyDomain(prop, domain) => {
                match (self.named_object_property(prop), self.named_class(domain)) {
                    (Some(prop_id), Some(class_id)) => {
                        self.triple_p(prop_id, RDFS_DOMAIN, class_id)
                    }
                    _ => self.skip("ObjectPropertyDomain with complex expression", axiom),
                }
            }
            ObjectPropertyAxiom::ObjectPropertyRange(prop, range) => {
                match (self.named_object_property(prop), self.named_class(range)) {
                    (Some(prop_id), Some(class_id)) => self.triple_p(prop_id, RDFS_RANGE, class_id),
                    _ => self.skip("ObjectPropertyRange with complex expression", axiom),
                }
            }
            ObjectPropertyAxiom::SubObjectPropertyOf(
                _,
                SubPropertyExpression::SubObjectPropertyExpression(sub),
                sup,
            ) => match (
                self.named_object_property(sub),
                self.named_object_property(sup),
            ) {
                (Some(sub_id), Some(sup_id)) => self.triple_p(sub_id, RDFS_SUB_PROPERTY_OF, sup_id),
                _ => self.skip("SubObjectPropertyOf with complex expression", axiom),
            },
            ObjectPropertyAxiom::EquivalentObjectProperties(_, props) => {
                match self.named_object_properties(props) {
                    Some(ids) => self.chain(&ids, OWL_EQUIVALENT_PROPERTY),
                    None => self.skip("EquivalentObjectProperties with complex expression", axiom),
                }
            }
            ObjectPropertyAxiom::DisjointObjectProperties(_, props) if props.len() == 2 => {
                match self.named_object_properties(props) {
                    Some(ids) => self.triple_p(ids[0], OWL_PROPERTY_DISJOINT_WITH, ids[1]),
                    None => self.skip("DisjointObjectProperties with complex expression", axiom),
                }
            }
            ObjectPropertyAxiom::InverseObjectProperties(_, first, second) => match (
                self.named_object_property(first),
                self.named_object_property(second),
            ) {
                (Some(a), Some(b)) => self.triple_p(a, OWL_OBJECT_INVERSE_OF, b),
                _ => self.skip("InverseObjectProperties with complex expression", axiom),
            },
            ObjectPropertyAxiom::FunctionalObjectProperty(_, prop) => {
                self.property_characteristic(prop, OWL_FUNCTIONAL_PROPERTY, axiom)
            }
            ObjectPropertyAxiom::InverseFunctionalObjectProperty(_, prop) => {
                self.property_characteristic(prop, OWL_INVERSE_FUNCTIONAL_PROPERTY, axiom)
            }
            ObjectPropertyAxiom::ReflexiveObjectProperty(_, prop) => {
                self.property_characteristic(prop, OWL_REFLEXIVE_PROPERTY, axiom)
            }
            ObjectPropertyAxiom::IrreflexiveObjectProperty(_, prop) => {
                self.property_characteristic(prop, OWL_IRREFLEXIVE_PROPERTY, axiom)
            }
            ObjectPropertyAxiom::SymmetricObjectProperty(_, prop) => {
                self.property_characteristic(prop, OWL_SYMMETRIC_PROPERTY, axiom)
            }
            ObjectPropertyAxiom::AsymmetricObjectProperty(_, prop) => {
                self.property_characteristic(prop, OWL_ASYMMETRIC_PROPERTY, axiom)
            }
            ObjectPropertyAxiom::TransitiveObjectProperty(_, prop) => {
                self.property_characteristic(prop, OWL_TRANSITIVE_PROPERTY, axiom)
            }
            other => self.skip("object property axiom", other),
        }
    }

    fn property_characteristic(
        &mut self,
        prop: &ObjectPropertyExpression,
        type_iri: &str,
        axiom: &ObjectPropertyAxiom,
    ) {
        match self.named_object_property(prop) {
            Some(prop_id) => self.type_triple(prop_id, type_iri),
            None => self.skip("property characteristic of complex expression", axiom),
        }
    }

    fn data_property_axiom(&mut self, axiom: &DataPropertyAxiom) {
        match axiom {
            DataPropertyAxiom::SubDataPropertyOf(_, sub, sup) => {
                let sub_id = self.full_iri(sub);
                let sup_id = self.full_iri(sup);
                self.triple_p(sub_id, RDFS_SUB_PROPERTY_OF, sup_id);
            }
            DataPropertyAxiom::EquivalentDataProperties(_, props) => {
                let ids: Vec<_> = props.iter().map(|p| self.full_iri(p)).collect();
                self.chain(&ids, OWL_EQUIVALENT_PROPERTY);
            }
            DataPropertyAxiom::DisjointDataProperties(_, props) if props.len() == 2 => {
                let ids: Vec<_> = props.iter().map(|p| self.full_iri(p)).collect();
                self.triple_p(ids[0], OWL_PROPERTY_DISJOINT_WITH, ids[1]);
            }
            DataPropertyAxiom::DataPropertyDomain(_, prop, domain) => {
                let prop_id = self.full_iri(prop);
                match self.named_class(domain) {
                    Some(class_id) => self.triple_p(prop_id, RDFS_DOMAIN, class_id),
                    None => self.skip("DataPropertyDomain with complex class expression", axiom),
                }
            }
            DataPropertyAxiom::DataPropertyRange(_, prop, DataRange::NamedDataRange(datatype)) => {
                let prop_id = self.full_iri(prop);
                let range_id = self.full_iri(datatype);
                self.triple_p(prop_id, RDFS_RANGE, range_id);
            }
            DataPropertyAxiom::FunctionalDataProperty(_, prop) => {
                let prop_id = self.full_iri(prop);
                self.type_triple(prop_id, OWL_FUNCTIONAL_PROPERTY);
            }
            other => self.skip("data property axiom", other),
        }
    }

    fn assertion(&mut self, assertion: &Assertion) {
        if atomic_assertion_triple(self.datastore, assertion) {
            self.report.triples_added += 1;
            return;
        }
        match assertion {
            Assertion::SameIndividual(_, individuals) => {
                let ids: Vec<_> = individuals
                    .iter()
                    .map(|i| intern_individual(self.datastore, i))
                    .collect();
                self.chain(&ids, OWL_SAME_AS);
            }
            Assertion::DifferentIndividuals(_, individuals) if individuals.len() == 2 => {
                let ids: Vec<_> = individuals
                    .iter()
                    .map(|i| intern_individual(self.datastore, i))
                    .collect();
                self.triple_p(ids[0], OWL_DIFFERENT_FROM, ids[1]);
            }
            other => self.skip("assertion", other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::GraphElement;
    use ingress::OntologyVersion;
    use owl_ontology::Ontology;

    const EX: &str = "http://example.org/";

    fn ex(local: &str) -> IriReference {
        IriReference(format!("{EX}{local}"))
    }

    fn full(local: &str) -> FullIri {
        FullIri(ex(local))
    }

    fn class(local: &str) -> ClassExpression {
        ClassExpression::ClassName(full(local))
    }

    fn obj_prop(local: &str) -> ObjectPropertyExpression {
        ObjectPropertyExpression::NamedObjectProperty(full(local))
    }

    fn ontology_of(axioms: Vec<Axiom>) -> Ontology {
        Ontology::new(vec![], OntologyVersion::UnNamedOntology, vec![], axioms)
    }

    fn id_of(ds: &Datastore, iri: &IriReference) -> Option<GraphElementId> {
        ds.resources
            .resource_map
            .get(&GraphElement::NodeOrEdge(RdfResource::Iri(iri.clone())))
            .copied()
    }

    /// Is `<subject-iri> <predicate-iri> <object-iri>` in the default graph?
    fn has_triple(
        ds: &Datastore,
        subject: &IriReference,
        predicate: &str,
        obj: &IriReference,
    ) -> bool {
        let predicate = IriReference(predicate.to_owned());
        match (id_of(ds, subject), id_of(ds, &predicate), id_of(ds, obj)) {
            (Some(s), Some(p), Some(o)) => !ds
                .quads_matching(None, Some(s), Some(p), Some(o))
                .is_empty(),
            _ => false,
        }
    }

    fn translate(axioms: Vec<Axiom>) -> (Datastore, RdfTranslationReport) {
        let mut ds = Datastore::new(100);
        let report = owl2rdf(&mut ds, &ontology_of(axioms));
        (ds, report)
    }

    /// Follow an `rdf:List` starting at `head`, returning the resolved IRIs
    /// of its `rdf:first` elements in order. Panics on a malformed list
    /// (missing `rdf:first`/`rdf:rest`) since tests only feed well-formed
    /// output of [`Translator::rdf_list`].
    fn read_rdf_list(ds: &Datastore, head: GraphElementId) -> Vec<IriReference> {
        let nil = id_of(ds, &IriReference(RDF_NIL.to_owned())).expect("rdf:nil interned");
        let first_pred = id_of(ds, &IriReference(RDF_FIRST.to_owned())).expect("rdf:first");
        let rest_pred = id_of(ds, &IriReference(RDF_REST.to_owned())).expect("rdf:rest");
        let mut items = Vec::new();
        let mut node = head;
        while node != nil {
            let first_quads = ds.quads_matching(None, Some(node), Some(first_pred), None);
            assert_eq!(first_quads.len(), 1, "each list cell has one rdf:first");
            let elem_id = first_quads[0].obj;
            let elem = ds.resources.get_graph_element(elem_id);
            match elem {
                GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => items.push(iri.clone()),
                other => panic!("expected an IRI list element, got {other:?}"),
            }
            let rest_quads = ds.quads_matching(None, Some(node), Some(rest_pred), None);
            assert_eq!(rest_quads.len(), 1, "each list cell has one rdf:rest");
            node = rest_quads[0].obj;
        }
        items
    }

    /// The head-node id of `<subject-iri> <predicate-iri> ?object`, assuming
    /// exactly one such triple.
    fn object_of(
        ds: &Datastore,
        subject: &IriReference,
        predicate_iri: &str,
    ) -> Option<GraphElementId> {
        let predicate = IriReference(predicate_iri.to_owned());
        let (s, p) = (id_of(ds, subject)?, id_of(ds, &predicate)?);
        let quads = ds.quads_matching(None, Some(s), Some(p), None);
        assert!(quads.len() <= 1, "expected at most one match");
        quads.first().map(|q| q.obj)
    }

    #[test]
    fn disjoint_union_of_two_named_classes_becomes_disjoint_union_of_list() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(
            vec![],
            full("Pet"),
            vec![class("Dog"), class("Cat")],
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(
            &ds,
            &ex("Pet"),
            RDF_TYPE,
            &IriReference(OWL_CLASS.to_owned())
        ));
        let list_head = object_of(&ds, &ex("Pet"), OWL_DISJOINT_UNION_OF)
            .expect("owl:disjointUnionOf triple must exist");
        assert_eq!(read_rdf_list(&ds, list_head), vec![ex("Dog"), ex("Cat")]);
        // rdf:type owl:Class + owl:disjointUnionOf + 2 list cells * 2 triples
        assert_eq!(report.triples_added, 6);
    }

    #[test]
    fn disjoint_union_of_three_or_more_named_classes_preserves_list_order() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(
            vec![],
            full("Pet"),
            vec![class("Dog"), class("Cat"), class("Bird"), class("Fish")],
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let list_head = object_of(&ds, &ex("Pet"), OWL_DISJOINT_UNION_OF)
            .expect("owl:disjointUnionOf triple must exist");
        assert_eq!(
            read_rdf_list(&ds, list_head),
            vec![ex("Dog"), ex("Cat"), ex("Bird"), ex("Fish")]
        );
    }

    /// A `DisjointUnion` whose members include a complex (non-atomic) class
    /// expression cannot be encoded without the general blank-node structural
    /// mapping — deferred to
    /// [#509](https://github.com/daghovland/rdf-datalog/issues/509) — and
    /// must be reported as skipped rather than partially/incorrectly emitted.
    #[test]
    fn disjoint_union_with_complex_member_is_reported_not_silently_dropped() {
        let (_ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(
            vec![],
            full("Pet"),
            vec![
                class("Dog"),
                ClassExpression::ObjectUnionOf(vec![class("Cat"), class("Bird")]),
            ],
        ))]);
        assert_eq!(report.triples_added, 0);
        assert_eq!(report.skipped.len(), 1, "skipped: {:?}", report.skipped);
    }

    #[test]
    fn subclassof_between_named_classes_becomes_rdfs_subclassof() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("Dog"),
            class("Animal"),
        ))]);
        assert_eq!(report.triples_added, 1);
        assert!(report.skipped.is_empty());
        assert!(has_triple(
            &ds,
            &ex("Dog"),
            RDFS_SUB_CLASS_OF,
            &ex("Animal")
        ));
    }

    #[test]
    fn equivalent_classes_becomes_owl_equivalent_class_chain() {
        let (ds, report) = translate(vec![
            Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                vec![],
                vec![class("Dog"), class("Canine")],
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
                vec![],
                vec![class("A"), class("B"), class("C")],
            )),
        ]);
        // one triple for the binary axiom, two for the ternary chain
        assert_eq!(report.triples_added, 3);
        assert!(has_triple(
            &ds,
            &ex("Dog"),
            OWL_EQUIVALENT_CLASS,
            &ex("Canine")
        ));
        assert!(has_triple(&ds, &ex("A"), OWL_EQUIVALENT_CLASS, &ex("B")));
        assert!(has_triple(&ds, &ex("B"), OWL_EQUIVALENT_CLASS, &ex("C")));
    }

    #[test]
    fn disjoint_classes_pair_becomes_owl_disjoint_with() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::DisjointClasses(
            vec![],
            vec![class("Dog"), class("Cat")],
        ))]);
        assert_eq!(report.triples_added, 1);
        assert!(has_triple(&ds, &ex("Dog"), OWL_DISJOINT_WITH, &ex("Cat")));
    }

    #[test]
    fn object_property_domain_and_range_become_rdfs_domain_and_range() {
        let (ds, report) = translate(vec![
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyDomain(
                obj_prop("hasPet"),
                class("Person"),
            )),
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::ObjectPropertyRange(
                obj_prop("hasPet"),
                class("Animal"),
            )),
        ]);
        assert_eq!(report.triples_added, 2);
        assert!(has_triple(&ds, &ex("hasPet"), RDFS_DOMAIN, &ex("Person")));
        assert!(has_triple(&ds, &ex("hasPet"), RDFS_RANGE, &ex("Animal")));
    }

    #[test]
    fn declarations_become_rdf_type_triples() {
        let decl = |entity| Axiom::AxiomDeclaration((vec![], entity));
        let (ds, report) = translate(vec![
            decl(Entity::ClassDeclaration(full("Dog"))),
            decl(Entity::ObjectPropertyDeclaration(full("hasPet"))),
            decl(Entity::DataPropertyDeclaration(full("age"))),
            decl(Entity::DatatypeDeclaration(full("Weight"))),
            decl(Entity::AnnotationPropertyDeclaration(full("note"))),
            decl(Entity::NamedIndividualDeclaration(
                Individual::NamedIndividual(full("fido")),
            )),
        ]);
        assert_eq!(report.triples_added, 6);
        assert!(has_triple(
            &ds,
            &ex("Dog"),
            RDF_TYPE,
            &IriReference(OWL_CLASS.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &ex("hasPet"),
            RDF_TYPE,
            &IriReference(OWL_OBJECT_PROPERTY.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &ex("age"),
            RDF_TYPE,
            &IriReference(OWL_DATATYPE_PROPERTY.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &ex("Weight"),
            RDF_TYPE,
            &IriReference(RDFS_DATATYPE.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &ex("note"),
            RDF_TYPE,
            &IriReference(OWL_ANNOTATION_PROPERTY.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &ex("fido"),
            RDF_TYPE,
            &IriReference(OWL_NAMED_INDIVIDUAL.to_owned())
        ));
    }

    #[test]
    fn object_property_axioms_and_characteristics() {
        let (ds, report) = translate(vec![
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SubObjectPropertyOf(
                vec![],
                SubPropertyExpression::SubObjectPropertyExpression(obj_prop("hasDog")),
                obj_prop("hasPet"),
            )),
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::EquivalentObjectProperties(
                vec![],
                vec![obj_prop("hasPet"), obj_prop("ownsPet")],
            )),
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::InverseObjectProperties(
                vec![],
                obj_prop("hasPet"),
                obj_prop("petOf"),
            )),
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::TransitiveObjectProperty(
                vec![],
                obj_prop("ancestorOf"),
            )),
            Axiom::AxiomObjectPropertyAxiom(ObjectPropertyAxiom::SymmetricObjectProperty(
                vec![],
                obj_prop("siblingOf"),
            )),
        ]);
        assert_eq!(report.triples_added, 5);
        assert!(has_triple(
            &ds,
            &ex("hasDog"),
            RDFS_SUB_PROPERTY_OF,
            &ex("hasPet")
        ));
        assert!(has_triple(
            &ds,
            &ex("hasPet"),
            OWL_EQUIVALENT_PROPERTY,
            &ex("ownsPet")
        ));
        assert!(has_triple(
            &ds,
            &ex("hasPet"),
            OWL_OBJECT_INVERSE_OF,
            &ex("petOf")
        ));
        assert!(has_triple(
            &ds,
            &ex("ancestorOf"),
            RDF_TYPE,
            &IriReference(OWL_TRANSITIVE_PROPERTY.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &ex("siblingOf"),
            RDF_TYPE,
            &IriReference(OWL_SYMMETRIC_PROPERTY.to_owned())
        ));
    }

    #[test]
    fn data_property_axioms() {
        let (ds, report) = translate(vec![
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyDomain(
                vec![],
                full("age"),
                class("Person"),
            )),
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::DataPropertyRange(
                vec![],
                full("age"),
                DataRange::NamedDataRange(FullIri(IriReference(
                    "http://www.w3.org/2001/XMLSchema#integer".to_owned(),
                ))),
            )),
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::SubDataPropertyOf(
                vec![],
                full("ageInYears"),
                full("age"),
            )),
            Axiom::AxiomDataPropertyAxiom(DataPropertyAxiom::FunctionalDataProperty(
                vec![],
                full("age"),
            )),
        ]);
        assert_eq!(report.triples_added, 4);
        assert!(has_triple(&ds, &ex("age"), RDFS_DOMAIN, &ex("Person")));
        assert!(has_triple(
            &ds,
            &ex("age"),
            RDFS_RANGE,
            &IriReference("http://www.w3.org/2001/XMLSchema#integer".to_owned())
        ));
        assert!(has_triple(
            &ds,
            &ex("ageInYears"),
            RDFS_SUB_PROPERTY_OF,
            &ex("age")
        ));
        assert!(has_triple(
            &ds,
            &ex("age"),
            RDF_TYPE,
            &IriReference(OWL_FUNCTIONAL_PROPERTY.to_owned())
        ));
    }

    /// The ABox logic absorbed from `assert_abox` must still materialise the
    /// same ground triples when driven through `owl2rdf`.
    #[test]
    fn abox_assertions_are_materialised() {
        let (ds, report) = translate(vec![
            Axiom::AxiomAssertion(Assertion::ClassAssertion(
                vec![],
                class("Dog"),
                Individual::NamedIndividual(full("fido")),
            )),
            Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(
                vec![],
                obj_prop("hasPet"),
                Individual::NamedIndividual(full("alice")),
                Individual::NamedIndividual(full("fido")),
            )),
            Axiom::AxiomAssertion(Assertion::SameIndividual(
                vec![],
                vec![
                    Individual::NamedIndividual(full("fido")),
                    Individual::NamedIndividual(full("rex")),
                ],
            )),
        ]);
        assert_eq!(report.triples_added, 3);
        assert!(has_triple(&ds, &ex("fido"), RDF_TYPE, &ex("Dog")));
        assert!(has_triple(&ds, &ex("alice"), EX_HAS_PET, &ex("fido")));
        assert!(has_triple(&ds, &ex("fido"), OWL_SAME_AS, &ex("rex")));
    }

    const EX_HAS_PET: &str = "http://example.org/hasPet";

    /// Axioms whose RDF encoding is not implemented yet must be *reported*,
    /// not silently dropped — the reporting half of
    /// [#366](https://github.com/daghovland/rdf-datalog/issues/366).
    #[test]
    fn complex_expressions_are_reported_not_silently_dropped() {
        let (_ds, report) = translate(vec![
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Dog"),
                ClassExpression::ObjectSomeValuesFrom(
                    obj_prop("hasPet"),
                    Box::new(class("Animal")),
                ),
            )),
            Axiom::AxiomAssertion(Assertion::ClassAssertion(
                vec![],
                ClassExpression::ObjectUnionOf(vec![class("Dog"), class("Cat")]),
                Individual::NamedIndividual(full("fido")),
            )),
        ]);
        assert_eq!(report.triples_added, 0);
        assert_eq!(report.skipped.len(), 2, "skipped: {:?}", report.skipped);
    }
}

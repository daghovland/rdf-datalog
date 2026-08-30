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
    IriReference, OWL_ALL_VALUES_FROM, OWL_ANNOTATED_PROPERTY, OWL_ANNOTATED_SOURCE,
    OWL_ANNOTATED_TARGET, OWL_ANNOTATION_PROPERTY, OWL_ASYMMETRIC_PROPERTY, OWL_AXIOM,
    OWL_CARDINALITY, OWL_CLASS, OWL_COMPLEMENT_OF, OWL_DATATYPE_PROPERTY, OWL_DIFFERENT_FROM,
    OWL_DISJOINT_UNION_OF, OWL_DISJOINT_WITH, OWL_EQUIVALENT_CLASS, OWL_EQUIVALENT_PROPERTY,
    OWL_FUNCTIONAL_PROPERTY, OWL_HAS_KEY, OWL_HAS_SELF, OWL_HAS_VALUE, OWL_IMPORT,
    OWL_INTERSECTION_OF, OWL_INVERSE_FUNCTIONAL_PROPERTY, OWL_IRREFLEXIVE_PROPERTY,
    OWL_MAX_CARDINALITY, OWL_MAX_QUALIFIED_CARDINALITY, OWL_MIN_CARDINALITY,
    OWL_MIN_QUALIFIED_CARDINALITY, OWL_NAMED_INDIVIDUAL, OWL_OBJECT_INVERSE_OF,
    OWL_OBJECT_PROPERTY, OWL_ON_CLASS, OWL_ON_DATA_RANGE, OWL_ON_PROPERTIES, OWL_ON_PROPERTY,
    OWL_ONE_OF, OWL_ONTOLOGY, OWL_PROPERTY_DISJOINT_WITH, OWL_QUALIFIED_CARDINALITY,
    OWL_REFLEXIVE_PROPERTY, OWL_RESTRICTION, OWL_SAME_AS, OWL_SOME_VALUES_FROM,
    OWL_SYMMETRIC_PROPERTY, OWL_TRANSITIVE_PROPERTY, OWL_UNION_OF, OWL_VERSION_IRI,
    OntologyVersion, RDF_FIRST, RDF_NIL, RDF_REST, RDF_TYPE, RDFS_DATATYPE, RDFS_DOMAIN,
    RDFS_RANGE, RDFS_SUB_CLASS_OF, RDFS_SUB_PROPERTY_OF, RdfLiteral, XSD_NON_NEGATIVE_INTEGER,
};
use owl_ontology::{
    Annotation, AnnotationAxiom, AnnotationValue, Assertion, Axiom, ClassAxiom, ClassExpression,
    DataProperty, DataPropertyAxiom, DataRange, Entity, FullIri, Individual, ObjectPropertyAxiom,
    ObjectPropertyExpression, Ontology, SubPropertyExpression,
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
    translator.ontology_header(ontology);
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
/// Returns `Some((subject, predicate, obj))` if `assertion` was one of the
/// three single-ground-triple forms (`ClassAssertion` /
/// `ObjectPropertyAssertion` / `DataPropertyAssertion` over named entities)
/// and a triple was added, `None` otherwise. The returned ids let
/// [`Translator::assertion`] attach an `owl:Axiom` reification for the
/// assertion's own annotations without re-deriving the triple. Shared by
/// [`owl2rdf`] and [`crate::assert_abox`] so the two never drift.
pub(crate) fn atomic_assertion_triple(
    datastore: &mut Datastore,
    assertion: &Assertion,
) -> Option<(GraphElementId, GraphElementId, GraphElementId)> {
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
            Some((subject, predicate, obj))
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
            Some((subject, predicate, obj))
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
            Some((subject, predicate, obj))
        }
        _ => None,
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

    /// Resolve an `AnnotationValue` to a `GraphElementId`: an IRI, a literal,
    /// or an anonymous individual (interned the same way as any other
    /// [`Individual`], via [`intern_individual`]).
    fn annotation_value(&mut self, value: &AnnotationValue) -> GraphElementId {
        match value {
            AnnotationValue::IriAnnotation(iri) => self.full_iri(iri),
            AnnotationValue::IndividualAnnotation(ind) => intern_individual(self.datastore, ind),
            AnnotationValue::LiteralAnnotation(elem) => self.datastore.add_resource(elem.clone()),
        }
    }

    /// `owl:Axiom` reification of the ground triple `(subject, predicate,
    /// obj)`, per the W3C mapping's axiom-annotation rule
    /// (<https://www.w3.org/TR/owl2-mapping-to-rdf/#Translation_of_Annotations>):
    /// a fresh blank node typed `owl:Axiom`, `owl:annotatedSource` /
    /// `owl:annotatedProperty` / `owl:annotatedTarget` triples pointing back
    /// at the three components, and one triple per annotation `(_:x, AP,
    /// T(av))` on that blank node.
    ///
    /// A no-op when `annotations` is empty — callers must never emit
    /// spurious reification triples for an axiom that carries no
    /// annotations.
    fn emit_axiom_annotations(
        &mut self,
        subject: GraphElementId,
        predicate: GraphElementId,
        obj: GraphElementId,
        annotations: &[Annotation],
    ) {
        if annotations.is_empty() {
            return;
        }
        let reification = self.datastore.new_anonymous_blank_node();
        self.type_triple(reification, OWL_AXIOM);
        self.triple_p(reification, OWL_ANNOTATED_SOURCE, subject);
        self.triple_p(reification, OWL_ANNOTATED_PROPERTY, predicate);
        self.triple_p(reification, OWL_ANNOTATED_TARGET, obj);
        for (ap, av) in annotations {
            let ap_id = self.full_iri(ap);
            let av_id = self.annotation_value(av);
            self.triple(reification, ap_id, av_id);
        }
    }

    /// `subject <predicate-iri> object`, interning the predicate IRI, plus
    /// the `owl:Axiom` reification of that triple if `annotations` is
    /// non-empty (see [`Translator::emit_axiom_annotations`]).
    fn triple_p_annotated(
        &mut self,
        subject: GraphElementId,
        predicate_iri: &str,
        obj: GraphElementId,
        annotations: &[Annotation],
    ) {
        let predicate = self.iri(predicate_iri);
        self.triple(subject, predicate, obj);
        self.emit_axiom_annotations(subject, predicate, obj, annotations);
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
    ///
    /// `annotations` is repeated on every triple in the chain (each gets its
    /// own `owl:Axiom` reification), per the mapping spec's §2.3.2: "each of
    /// the RDF triples obtained by the translation of ax' is transformed...
    /// and the annotations are repeated for each of the triples obtained".
    fn chain(&mut self, items: &[GraphElementId], predicate_iri: &str, annotations: &[Annotation]) {
        for pair in items.windows(2) {
            self.triple_p_annotated(pair[0], predicate_iri, pair[1], annotations);
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
    /// Resolve every element of a list of class expressions via the general
    /// [`Translator::class_expression`] resolver, so complex members are
    /// translated (blank-node encoded) rather than causing the whole axiom
    /// to be skipped. `None` if any element is unsupported.
    fn class_expressions(&mut self, exprs: &[ClassExpression]) -> Option<Vec<GraphElementId>> {
        exprs.iter().map(|e| self.class_expression(e)).collect()
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

    // ── complex class-expression blank-node encoding (#509) ────────────────
    //
    // <https://www.w3.org/TR/owl2-mapping-to-rdf/> §2.1's "Translation of
    // Class Expressions" table. See
    // docs/plans/OWL2RDF_COMPLEX_CLASS_EXPRESSIONS_PLAN.md for the exact
    // triple shapes and citations.

    /// A `"n"^^xsd:nonNegativeInteger` literal node for a cardinality value.
    ///
    /// Distinct from the codebase's default `RdfLiteral::IntegerLiteral`
    /// mapping (`xsd:integer`) — the spec requires `xsd:nonNegativeInteger`
    /// specifically for every cardinality-restriction triple.
    fn non_negative_integer(&mut self, n: &impl std::fmt::Display) -> GraphElementId {
        self.datastore
            .add_resource(dag_rdf::GraphElement::GraphLiteral(
                RdfLiteral::TypedLiteral {
                    type_iri: IriReference(XSD_NON_NEGATIVE_INTEGER.to_owned()),
                    literal: n.to_string(),
                },
            ))
    }

    /// A fresh `owl:Restriction` blank node with `owl:onProperty T(OPE)`, or
    /// `None` if `ope` is a still-unsupported expression (inverse property /
    /// property chain, [#510](https://github.com/daghovland/rdf-datalog/issues/510)).
    fn object_restriction_node(
        &mut self,
        ope: &ObjectPropertyExpression,
    ) -> Option<GraphElementId> {
        let prop_id = self.named_object_property(ope)?;
        let node = self.datastore.new_anonymous_blank_node();
        self.type_triple(node, OWL_RESTRICTION);
        self.triple_p(node, OWL_ON_PROPERTY, prop_id);
        Some(node)
    }

    /// A fresh `owl:Restriction` blank node with `owl:onProperty T(DPE)` for
    /// a single data property, or `owl:onProperties T(SEQ DPE1 ... DPEn)`
    /// (an `rdf:List`) for two or more, per the spec's `DataSomeValuesFrom`/
    /// `DataAllValuesFrom` rows.
    fn data_restriction_node(&mut self, props: &[DataProperty]) -> GraphElementId {
        let node = self.datastore.new_anonymous_blank_node();
        self.type_triple(node, OWL_RESTRICTION);
        let ids: Vec<_> = props.iter().map(|p| self.full_iri(p)).collect();
        if let [only] = ids[..] {
            self.triple_p(node, OWL_ON_PROPERTY, only);
        } else {
            let list_head = self.rdf_list(&ids);
            self.triple_p(node, OWL_ON_PROPERTIES, list_head);
        }
        node
    }

    /// `T(DR)` for a `DataRange`: just the datatype IRI for a named
    /// datatype. A complex `DataRange` (`DataUnionOf`, `DataIntersectionOf`,
    /// `DataComplementOf`, `DataOneOf`, `DatatypeRestriction`) has no RDF
    /// encoding yet — that's [#512](https://github.com/daghovland/rdf-datalog/issues/512)'s
    /// scope, not this issue's — so this returns `None` for those.
    fn named_data_range(&mut self, range: &DataRange) -> Option<GraphElementId> {
        match range {
            DataRange::NamedDataRange(datatype) => Some(self.full_iri(datatype)),
            _ => None,
        }
    }

    /// Build an `owl:Class` blank node `_:x rdf:type owl:Class ;
    /// <predicate-iri> T(SEQ members...)` for `ObjectUnionOf`/
    /// `ObjectIntersectionOf`. `None` if any member is itself unsupported.
    fn class_expression_list(
        &mut self,
        members: &[ClassExpression],
        predicate_iri: &str,
    ) -> Option<GraphElementId> {
        let ids: Option<Vec<_>> = members.iter().map(|m| self.class_expression(m)).collect();
        let ids = ids?;
        let list_head = self.rdf_list(&ids);
        let node = self.datastore.new_anonymous_blank_node();
        self.type_triple(node, OWL_CLASS);
        self.triple_p(node, predicate_iri, list_head);
        Some(node)
    }

    /// The node id of any class expression — named or complex — building
    /// the spec's blank-node structural encoding recursively for complex
    /// cases. `None` if `expr` (or a nested part of it) mentions a construct
    /// with no RDF encoding yet (a complex `DataRange`, or an
    /// `ObjectPropertyExpression` other than a named/anonymous property) —
    /// kept fallible so those propagate up via `?` *before* any blank node
    /// for the enclosing expression is minted, rather than leaving a
    /// partially-formed node in the datastore.
    fn class_expression(&mut self, expr: &ClassExpression) -> Option<GraphElementId> {
        if let Some(id) = self.named_class(expr) {
            return Some(id);
        }
        match expr {
            ClassExpression::ClassName(_) | ClassExpression::AnonymousClass(_) => {
                unreachable!("named_class handles both atomic cases above")
            }
            ClassExpression::ObjectUnionOf(members) => {
                self.class_expression_list(members, OWL_UNION_OF)
            }
            ClassExpression::ObjectIntersectionOf(members) => {
                self.class_expression_list(members, OWL_INTERSECTION_OF)
            }
            ClassExpression::ObjectComplementOf(inner) => {
                let inner_id = self.class_expression(inner)?;
                let node = self.datastore.new_anonymous_blank_node();
                self.type_triple(node, OWL_CLASS);
                self.triple_p(node, OWL_COMPLEMENT_OF, inner_id);
                Some(node)
            }
            ClassExpression::ObjectOneOf(individuals) => {
                let ids: Vec<_> = individuals
                    .iter()
                    .map(|i| intern_individual(self.datastore, i))
                    .collect();
                let list_head = self.rdf_list(&ids);
                let node = self.datastore.new_anonymous_blank_node();
                self.type_triple(node, OWL_CLASS);
                self.triple_p(node, OWL_ONE_OF, list_head);
                Some(node)
            }
            ClassExpression::ObjectSomeValuesFrom(ope, filler) => {
                let filler_id = self.class_expression(filler)?;
                let node = self.object_restriction_node(ope)?;
                self.triple_p(node, OWL_SOME_VALUES_FROM, filler_id);
                Some(node)
            }
            ClassExpression::ObjectAllValuesFrom(ope, filler) => {
                let filler_id = self.class_expression(filler)?;
                let node = self.object_restriction_node(ope)?;
                self.triple_p(node, OWL_ALL_VALUES_FROM, filler_id);
                Some(node)
            }
            ClassExpression::ObjectHasValue(ope, individual) => {
                let value_id = intern_individual(self.datastore, individual);
                let node = self.object_restriction_node(ope)?;
                self.triple_p(node, OWL_HAS_VALUE, value_id);
                Some(node)
            }
            ClassExpression::ObjectHasSelf(ope) => {
                let node = self.object_restriction_node(ope)?;
                let true_id = self
                    .datastore
                    .add_resource(dag_rdf::GraphElement::GraphLiteral(
                        RdfLiteral::BooleanLiteral(true),
                    ));
                self.triple_p(node, OWL_HAS_SELF, true_id);
                Some(node)
            }
            ClassExpression::ObjectMinCardinality(n, ope) => {
                let node = self.object_restriction_node(ope)?;
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MIN_CARDINALITY, n_id);
                Some(node)
            }
            ClassExpression::ObjectMaxCardinality(n, ope) => {
                let node = self.object_restriction_node(ope)?;
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MAX_CARDINALITY, n_id);
                Some(node)
            }
            ClassExpression::ObjectExactCardinality(n, ope) => {
                let node = self.object_restriction_node(ope)?;
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_CARDINALITY, n_id);
                Some(node)
            }
            ClassExpression::ObjectMinQualifiedCardinality(n, ope, on_class) => {
                let on_class_id = self.class_expression(on_class)?;
                let node = self.object_restriction_node(ope)?;
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MIN_QUALIFIED_CARDINALITY, n_id);
                self.triple_p(node, OWL_ON_CLASS, on_class_id);
                Some(node)
            }
            ClassExpression::ObjectMaxQualifiedCardinality(n, ope, on_class) => {
                let on_class_id = self.class_expression(on_class)?;
                let node = self.object_restriction_node(ope)?;
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MAX_QUALIFIED_CARDINALITY, n_id);
                self.triple_p(node, OWL_ON_CLASS, on_class_id);
                Some(node)
            }
            ClassExpression::ObjectExactQualifiedCardinality(n, ope, on_class) => {
                let on_class_id = self.class_expression(on_class)?;
                let node = self.object_restriction_node(ope)?;
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_QUALIFIED_CARDINALITY, n_id);
                self.triple_p(node, OWL_ON_CLASS, on_class_id);
                Some(node)
            }
            ClassExpression::DataSomeValuesFrom(props, range) => {
                let range_id = self.named_data_range(range)?;
                let node = self.data_restriction_node(props);
                self.triple_p(node, OWL_SOME_VALUES_FROM, range_id);
                Some(node)
            }
            ClassExpression::DataAllValuesFrom(props, range) => {
                let range_id = self.named_data_range(range)?;
                let node = self.data_restriction_node(props);
                self.triple_p(node, OWL_ALL_VALUES_FROM, range_id);
                Some(node)
            }
            ClassExpression::DataHasValue(prop, value) => {
                let value_id = self.datastore.add_resource(value.clone());
                let node = self.data_restriction_node(std::slice::from_ref(prop));
                self.triple_p(node, OWL_HAS_VALUE, value_id);
                Some(node)
            }
            ClassExpression::DataMinCardinality(n, prop) => {
                let node = self.data_restriction_node(std::slice::from_ref(prop));
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MIN_CARDINALITY, n_id);
                Some(node)
            }
            ClassExpression::DataMaxCardinality(n, prop) => {
                let node = self.data_restriction_node(std::slice::from_ref(prop));
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MAX_CARDINALITY, n_id);
                Some(node)
            }
            ClassExpression::DataExactCardinality(n, prop) => {
                let node = self.data_restriction_node(std::slice::from_ref(prop));
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_CARDINALITY, n_id);
                Some(node)
            }
            ClassExpression::DataMinQualifiedCardinality(n, prop, range) => {
                let range_id = self.named_data_range(range)?;
                let node = self.data_restriction_node(std::slice::from_ref(prop));
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MIN_QUALIFIED_CARDINALITY, n_id);
                self.triple_p(node, OWL_ON_DATA_RANGE, range_id);
                Some(node)
            }
            ClassExpression::DataMaxQualifiedCardinality(n, prop, range) => {
                let range_id = self.named_data_range(range)?;
                let node = self.data_restriction_node(std::slice::from_ref(prop));
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_MAX_QUALIFIED_CARDINALITY, n_id);
                self.triple_p(node, OWL_ON_DATA_RANGE, range_id);
                Some(node)
            }
            ClassExpression::DataExactQualifiedCardinality(n, prop, range) => {
                let range_id = self.named_data_range(range)?;
                let node = self.data_restriction_node(std::slice::from_ref(prop));
                let n_id = self.non_negative_integer(n);
                self.triple_p(node, OWL_QUALIFIED_CARDINALITY, n_id);
                self.triple_p(node, OWL_ON_DATA_RANGE, range_id);
                Some(node)
            }
        }
    }

    // ── axiom dispatch ───────────────────────────────────────────────────

    fn axiom(&mut self, axiom: &Axiom) {
        match axiom {
            Axiom::AxiomDeclaration((annotations, entity)) => self.declaration(entity, annotations),
            Axiom::AxiomClassAxiom(class_axiom) => self.class_axiom(class_axiom),
            Axiom::AxiomObjectPropertyAxiom(prop_axiom) => self.object_property_axiom(prop_axiom),
            Axiom::AxiomDataPropertyAxiom(prop_axiom) => self.data_property_axiom(prop_axiom),
            Axiom::AxiomAssertion(assertion) => self.assertion(assertion),
            Axiom::AxiomHasKey(annotations, class_expr, obj_props, data_props) => {
                self.has_key(axiom, class_expr, obj_props, data_props, annotations)
            }
            Axiom::AxiomAnnotationAxiom(annotation_axiom) => {
                self.annotation_axiom(annotation_axiom)
            }
            other => self.skip("axiom", other),
        }
    }

    /// Translate `ontology`'s header — the ontology IRI declaration,
    /// `owl:versionIRI`, `owl:imports`, and ontology-level annotations —
    /// per the W3C mapping's `T(Ontology(...))` rule
    /// (<https://www.w3.org/TR/owl2-mapping-to-rdf/> §2.1, Table 1). See
    /// docs/plans/OWL2RDF_ONTOLOGY_HEADER_PLAN.md for the exact triple
    /// shapes and citations, including a deliberate, documented deviation
    /// from the literal spec text for the fully-bare anonymous case (below).
    ///
    /// A *named* (or versioned) ontology always gets at least the
    /// `rdf:type owl:Ontology` triple, since there's a real IRI to make a
    /// statement about. An *anonymous* ontology with something to attach
    /// (imports and/or annotations) gets a fresh blank node as its header
    /// subject, per the spec's `_:x` rule, with every header triple hanging
    /// off that same node — this is the case exercised by
    /// `anonymous_ontology_gets_type_declaration_and_imports_on_blank_node`.
    ///
    /// A **completely bare anonymous ontology** (no IRI, no imports, no
    /// annotations) emits *nothing at all* — deliberately not the literal
    /// spec base case of a lone `_:x rdf:type owl:Ontology` triple. Plain
    /// RDF that never declared itself an OWL ontology document in the first
    /// place (no `<x> rdf:type owl:Ontology` triple anywhere) parses via
    /// `rdf_owl_translator::rdf2owl` into exactly this shape
    /// (`OntologyVersion::UnNamedOntology`, no imports/annotations); always
    /// synthesizing a content-free blank node here would silently add an
    /// `owl:Ontology` node on every such round trip that was never actually
    /// present in the source data, breaking graph-isomorphism round-trip
    /// tests (`rdf_starting_roundtrip_preserves_graph_isomorphism` in
    /// `tests/manchester_roundtrip.rs`) for no informational gain — a bare
    /// unattached blank node carries nothing a caller could not already
    /// infer from `ontology.version` being `UnNamedOntology` with empty
    /// imports/annotations.
    ///
    /// Returns the header subject id, if one was created, in case a future
    /// caller wants to attach more to the same node.
    fn ontology_header(&mut self, ontology: &Ontology) -> Option<GraphElementId> {
        let bare_anonymous = matches!(ontology.version, OntologyVersion::UnNamedOntology)
            && ontology.directly_imports_documents.is_empty()
            && ontology.annotations.is_empty();
        if bare_anonymous {
            return None;
        }
        let subject = match &ontology.version {
            OntologyVersion::UnNamedOntology => self.datastore.new_anonymous_blank_node(),
            OntologyVersion::NamedOntology(iri) => self.iri(&iri.0),
            OntologyVersion::VersionedOntology { ontology_iri, .. } => self.iri(&ontology_iri.0),
        };
        self.type_triple(subject, OWL_ONTOLOGY);
        if let OntologyVersion::VersionedOntology { version_iri, .. } = &ontology.version {
            let version_id = self.iri(&version_iri.0);
            self.triple_p(subject, OWL_VERSION_IRI, version_id);
        }
        for import in &ontology.directly_imports_documents {
            let import_id = self.iri(&import.0);
            self.triple_p(subject, OWL_IMPORT, import_id);
        }
        for (ap, av) in &ontology.annotations {
            let ap_id = self.full_iri(ap);
            let av_id = self.annotation_value(av);
            self.triple(subject, ap_id, av_id);
        }
        Some(subject)
    }

    fn declaration(&mut self, entity: &Entity, annotations: &[Annotation]) {
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
        let type_id = self.iri(type_iri);
        self.triple_p_annotated(subject, RDF_TYPE, type_id, annotations);
    }

    fn class_axiom(&mut self, axiom: &ClassAxiom) {
        match axiom {
            ClassAxiom::SubClassOf(annotations, sub, sup) => {
                match (self.class_expression(sub), self.class_expression(sup)) {
                    (Some(sub_id), Some(sup_id)) => {
                        self.triple_p_annotated(sub_id, RDFS_SUB_CLASS_OF, sup_id, annotations)
                    }
                    _ => self.skip("SubClassOf with unsupported class expression", axiom),
                }
            }
            ClassAxiom::EquivalentClasses(annotations, classes) => {
                match self.class_expressions(classes) {
                    Some(ids) => self.chain(&ids, OWL_EQUIVALENT_CLASS, annotations),
                    None => self.skip("EquivalentClasses with unsupported class expression", axiom),
                }
            }
            ClassAxiom::DisjointClasses(annotations, classes) if classes.len() == 2 => {
                match self.class_expressions(classes) {
                    Some(ids) => {
                        self.triple_p_annotated(ids[0], OWL_DISJOINT_WITH, ids[1], annotations)
                    }
                    None => self.skip("DisjointClasses with unsupported class expression", axiom),
                }
            }
            // n > 2 needs an `owl:AllDisjointClasses` blank node with an
            // `owl:members` rdf:List — deferred to
            // https://github.com/daghovland/rdf-datalog/issues/513.
            ClassAxiom::DisjointUnion(annotations, class, members) => {
                match self.class_expressions(members) {
                    Some(ids) => {
                        let class_id = self.full_iri(class);
                        self.type_triple(class_id, OWL_CLASS);
                        let list_head = self.rdf_list(&ids);
                        self.triple_p_annotated(
                            class_id,
                            OWL_DISJOINT_UNION_OF,
                            list_head,
                            annotations,
                        );
                    }
                    None => self.skip(
                        "DisjointUnion with unsupported class expression member",
                        axiom,
                    ),
                }
            }
            other => self.skip("class axiom", other),
        }
    }

    fn object_property_axiom(&mut self, axiom: &ObjectPropertyAxiom) {
        match axiom {
            // `ObjectPropertyDomain`/`ObjectPropertyRange` have no
            // `Vec<Annotation>` field in `owl_ontology::ObjectPropertyAxiom`
            // (unlike every other variant here) — axiom annotations on these
            // two forms have no source data to reify.
            ObjectPropertyAxiom::ObjectPropertyDomain(prop, domain) => {
                match (
                    self.named_object_property(prop),
                    self.class_expression(domain),
                ) {
                    (Some(prop_id), Some(class_id)) => {
                        self.triple_p(prop_id, RDFS_DOMAIN, class_id)
                    }
                    _ => self.skip("ObjectPropertyDomain with unsupported expression", axiom),
                }
            }
            ObjectPropertyAxiom::ObjectPropertyRange(prop, range) => {
                match (
                    self.named_object_property(prop),
                    self.class_expression(range),
                ) {
                    (Some(prop_id), Some(class_id)) => self.triple_p(prop_id, RDFS_RANGE, class_id),
                    _ => self.skip("ObjectPropertyRange with unsupported expression", axiom),
                }
            }
            ObjectPropertyAxiom::SubObjectPropertyOf(
                annotations,
                SubPropertyExpression::SubObjectPropertyExpression(sub),
                sup,
            ) => match (
                self.named_object_property(sub),
                self.named_object_property(sup),
            ) {
                (Some(sub_id), Some(sup_id)) => {
                    self.triple_p_annotated(sub_id, RDFS_SUB_PROPERTY_OF, sup_id, annotations)
                }
                _ => self.skip("SubObjectPropertyOf with complex expression", axiom),
            },
            ObjectPropertyAxiom::EquivalentObjectProperties(annotations, props) => {
                match self.named_object_properties(props) {
                    Some(ids) => self.chain(&ids, OWL_EQUIVALENT_PROPERTY, annotations),
                    None => self.skip("EquivalentObjectProperties with complex expression", axiom),
                }
            }
            ObjectPropertyAxiom::DisjointObjectProperties(annotations, props)
                if props.len() == 2 =>
            {
                match self.named_object_properties(props) {
                    Some(ids) => self.triple_p_annotated(
                        ids[0],
                        OWL_PROPERTY_DISJOINT_WITH,
                        ids[1],
                        annotations,
                    ),
                    None => self.skip("DisjointObjectProperties with complex expression", axiom),
                }
            }
            ObjectPropertyAxiom::InverseObjectProperties(annotations, first, second) => match (
                self.named_object_property(first),
                self.named_object_property(second),
            ) {
                (Some(a), Some(b)) => {
                    self.triple_p_annotated(a, OWL_OBJECT_INVERSE_OF, b, annotations)
                }
                _ => self.skip("InverseObjectProperties with complex expression", axiom),
            },
            ObjectPropertyAxiom::FunctionalObjectProperty(annotations, prop) => {
                self.property_characteristic(prop, OWL_FUNCTIONAL_PROPERTY, annotations, axiom)
            }
            ObjectPropertyAxiom::InverseFunctionalObjectProperty(annotations, prop) => self
                .property_characteristic(prop, OWL_INVERSE_FUNCTIONAL_PROPERTY, annotations, axiom),
            ObjectPropertyAxiom::ReflexiveObjectProperty(annotations, prop) => {
                self.property_characteristic(prop, OWL_REFLEXIVE_PROPERTY, annotations, axiom)
            }
            ObjectPropertyAxiom::IrreflexiveObjectProperty(annotations, prop) => {
                self.property_characteristic(prop, OWL_IRREFLEXIVE_PROPERTY, annotations, axiom)
            }
            ObjectPropertyAxiom::SymmetricObjectProperty(annotations, prop) => {
                self.property_characteristic(prop, OWL_SYMMETRIC_PROPERTY, annotations, axiom)
            }
            ObjectPropertyAxiom::AsymmetricObjectProperty(annotations, prop) => {
                self.property_characteristic(prop, OWL_ASYMMETRIC_PROPERTY, annotations, axiom)
            }
            ObjectPropertyAxiom::TransitiveObjectProperty(annotations, prop) => {
                self.property_characteristic(prop, OWL_TRANSITIVE_PROPERTY, annotations, axiom)
            }
            other => self.skip("object property axiom", other),
        }
    }

    fn property_characteristic(
        &mut self,
        prop: &ObjectPropertyExpression,
        type_iri: &str,
        annotations: &[Annotation],
        axiom: &ObjectPropertyAxiom,
    ) {
        match self.named_object_property(prop) {
            Some(prop_id) => {
                let type_id = self.iri(type_iri);
                self.triple_p_annotated(prop_id, RDF_TYPE, type_id, annotations)
            }
            None => self.skip("property characteristic of complex expression", axiom),
        }
    }

    fn data_property_axiom(&mut self, axiom: &DataPropertyAxiom) {
        match axiom {
            DataPropertyAxiom::SubDataPropertyOf(annotations, sub, sup) => {
                let sub_id = self.full_iri(sub);
                let sup_id = self.full_iri(sup);
                self.triple_p_annotated(sub_id, RDFS_SUB_PROPERTY_OF, sup_id, annotations);
            }
            DataPropertyAxiom::EquivalentDataProperties(annotations, props) => {
                let ids: Vec<_> = props.iter().map(|p| self.full_iri(p)).collect();
                self.chain(&ids, OWL_EQUIVALENT_PROPERTY, annotations);
            }
            DataPropertyAxiom::DisjointDataProperties(annotations, props) if props.len() == 2 => {
                let ids: Vec<_> = props.iter().map(|p| self.full_iri(p)).collect();
                self.triple_p_annotated(ids[0], OWL_PROPERTY_DISJOINT_WITH, ids[1], annotations);
            }
            DataPropertyAxiom::DataPropertyDomain(annotations, prop, domain) => {
                let prop_id = self.full_iri(prop);
                match self.class_expression(domain) {
                    Some(class_id) => {
                        self.triple_p_annotated(prop_id, RDFS_DOMAIN, class_id, annotations)
                    }
                    None => self.skip(
                        "DataPropertyDomain with unsupported class expression",
                        axiom,
                    ),
                }
            }
            DataPropertyAxiom::DataPropertyRange(
                annotations,
                prop,
                DataRange::NamedDataRange(datatype),
            ) => {
                let prop_id = self.full_iri(prop);
                let range_id = self.full_iri(datatype);
                self.triple_p_annotated(prop_id, RDFS_RANGE, range_id, annotations);
            }
            DataPropertyAxiom::FunctionalDataProperty(annotations, prop) => {
                let prop_id = self.full_iri(prop);
                let type_id = self.iri(OWL_FUNCTIONAL_PROPERTY);
                self.triple_p_annotated(prop_id, RDF_TYPE, type_id, annotations);
            }
            other => self.skip("data property axiom", other),
        }
    }

    fn assertion(&mut self, assertion: &Assertion) {
        if let Some((subject, predicate, obj)) = atomic_assertion_triple(self.datastore, assertion)
        {
            self.report.triples_added += 1;
            let annotations = match assertion {
                Assertion::ClassAssertion(annotations, ..)
                | Assertion::ObjectPropertyAssertion(annotations, ..)
                | Assertion::DataPropertyAssertion(annotations, ..) => annotations,
                _ => unreachable!("atomic_assertion_triple only succeeds for these three forms"),
            };
            self.emit_axiom_annotations(subject, predicate, obj, annotations);
            return;
        }
        match assertion {
            // `atomic_assertion_triple` only handles a `ClassExpression::ClassName`
            // subject; a complex (non-atomic) class expression still needs the
            // general blank-node structural encoding — the other half of
            // #366 (https://github.com/daghovland/rdf-datalog/issues/366),
            // fixed by #509 (https://github.com/daghovland/rdf-datalog/issues/509).
            Assertion::ClassAssertion(annotations, class_expr, ind) => {
                match self.class_expression(class_expr) {
                    Some(class_id) => {
                        let subject = intern_individual(self.datastore, ind);
                        let type_id = self.iri(RDF_TYPE);
                        self.triple(subject, type_id, class_id);
                        self.emit_axiom_annotations(subject, type_id, class_id, annotations);
                    }
                    None => self.skip(
                        "ClassAssertion with unsupported class expression",
                        assertion,
                    ),
                }
            }
            Assertion::SameIndividual(annotations, individuals) => {
                let ids: Vec<_> = individuals
                    .iter()
                    .map(|i| intern_individual(self.datastore, i))
                    .collect();
                self.chain(&ids, OWL_SAME_AS, annotations);
            }
            Assertion::DifferentIndividuals(annotations, individuals) if individuals.len() == 2 => {
                let ids: Vec<_> = individuals
                    .iter()
                    .map(|i| intern_individual(self.datastore, i))
                    .collect();
                self.triple_p_annotated(ids[0], OWL_DIFFERENT_FROM, ids[1], annotations);
            }
            other => self.skip("assertion", other),
        }
    }

    /// `HasKey(C (OPE1 ... OPEm) (DPE1 ... DPEn))` becomes
    /// `T(C) owl:hasKey T(SEQ OPE1 ... OPEm DPE1 ... DPEn)`, per the W3C
    /// mapping's `HasKey` row (<https://www.w3.org/TR/owl2-mapping-to-rdf/>).
    /// Unlike some other axiom-to-RDF mappings, `T(C)` for a named class is
    /// simply the class IRI — no blank node is needed for the subject side,
    /// only for the `rdf:List` cells encoding the key properties.
    ///
    /// A `C` that is itself a complex (non-atomic) class expression is
    /// resolved via [`Translator::class_expression`], the general blank-node
    /// structural encoding ([#509](https://github.com/daghovland/rdf-datalog/issues/509)).
    ///
    /// Only the main `T(C) owl:hasKey list-head` triple is reified when
    /// `annotations` is non-empty — the `rdf:first`/`rdf:rest` list-cell
    /// triples are emitted unchanged, per the mapping spec's §2.3.1 ("the
    /// first triple... is the main triple... the other triples... are output
    /// without any change").
    fn has_key(
        &mut self,
        axiom: &Axiom,
        class_expr: &ClassExpression,
        obj_props: &[ObjectPropertyExpression],
        data_props: &[DataProperty],
        annotations: &[Annotation],
    ) {
        match (
            self.class_expression(class_expr),
            self.named_object_properties(obj_props),
        ) {
            (Some(class_id), Some(obj_ids)) => {
                let mut key_ids = obj_ids;
                key_ids.extend(data_props.iter().map(|p| self.full_iri(p)));
                let list_head = self.rdf_list(&key_ids);
                self.triple_p_annotated(class_id, OWL_HAS_KEY, list_head, annotations);
            }
            _ => self.skip("HasKey with unsupported class expression", axiom),
        }
    }

    /// `Axiom::AxiomAnnotationAxiom` dispatch: `AnnotationAssertion`,
    /// `SubAnnotationPropertyOf`, `AnnotationPropertyDomain`,
    /// `AnnotationPropertyRange` — all single-ground-triple forms per Table 1
    /// of <https://www.w3.org/TR/owl2-mapping-to-rdf/>.
    fn annotation_axiom(&mut self, axiom: &AnnotationAxiom) {
        match axiom {
            AnnotationAxiom::AnnotationAssertion(annotations, ap, subject, value) => {
                let subject_id = self.datastore.add_resource(subject.clone());
                let ap_id = self.full_iri(ap);
                let value_id = self.datastore.add_resource(value.clone());
                self.triple(subject_id, ap_id, value_id);
                self.emit_axiom_annotations(subject_id, ap_id, value_id, annotations);
            }
            AnnotationAxiom::SubAnnotationPropertyOf(annotations, sub, sup) => {
                let sub_id = self.full_iri(sub);
                let sup_id = self.full_iri(sup);
                self.triple_p_annotated(sub_id, RDFS_SUB_PROPERTY_OF, sup_id, annotations);
            }
            AnnotationAxiom::AnnotationPropertyDomain(annotations, ap, domain) => {
                let ap_id = self.full_iri(ap);
                let domain_id = self.full_iri(domain);
                self.triple_p_annotated(ap_id, RDFS_DOMAIN, domain_id, annotations);
            }
            AnnotationAxiom::AnnotationPropertyRange(annotations, ap, range) => {
                let ap_id = self.full_iri(ap);
                let range_id = self.full_iri(range);
                self.triple_p_annotated(ap_id, RDFS_RANGE, range_id, annotations);
            }
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
    /// expression is now translated via the general blank-node structural
    /// mapping ([#509](https://github.com/daghovland/rdf-datalog/issues/509)):
    /// the complex member becomes its own `owl:Class`/`owl:unionOf` blank
    /// node inside the outer `owl:disjointUnionOf` list.
    #[test]
    fn disjoint_union_with_complex_member_is_now_translated() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::DisjointUnion(
            vec![],
            full("Pet"),
            vec![
                class("Dog"),
                ClassExpression::ObjectUnionOf(vec![class("Cat"), class("Bird")]),
            ],
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let list_head = object_of(&ds, &ex("Pet"), OWL_DISJOINT_UNION_OF)
            .expect("owl:disjointUnionOf triple must exist");
        let first_quads = ds.quads_matching(
            None,
            Some(list_head),
            id_of(&ds, &IriReference(RDF_FIRST.to_owned())),
            None,
        );
        assert_eq!(first_quads.len(), 1);
        let dog_id = id_of(&ds, &ex("Dog")).expect("Dog interned");
        assert_eq!(
            first_quads[0].obj, dog_id,
            "first member is the named class"
        );
        let rest_quads = ds.quads_matching(
            None,
            Some(list_head),
            id_of(&ds, &IriReference(RDF_REST.to_owned())),
            None,
        );
        assert_eq!(rest_quads.len(), 1);
        let second_cell = rest_quads[0].obj;
        let second_first = ds.quads_matching(
            None,
            Some(second_cell),
            id_of(&ds, &IriReference(RDF_FIRST.to_owned())),
            None,
        );
        assert_eq!(second_first.len(), 1);
        let union_node = second_first[0].obj;
        let union_list_head = ds
            .quads_matching(
                None,
                Some(union_node),
                id_of(&ds, &IriReference(OWL_UNION_OF.to_owned())),
                None,
            )
            .first()
            .expect("nested owl:unionOf triple must exist")
            .obj;
        assert_eq!(
            read_rdf_list(&ds, union_list_head),
            vec![ex("Cat"), ex("Bird")]
        );
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

    /// `HasKey` on a named class with a single object property key becomes
    /// `<class> owl:hasKey (<prop>)`, per the W3C mapping's
    /// `T(C) owl:hasKey T(SEQ OPE1 ... DPEn)` rule
    /// (<https://www.w3.org/TR/owl2-mapping-to-rdf/>). Tracked in
    /// [#511](https://github.com/daghovland/rdf-datalog/issues/511).
    #[test]
    fn has_key_on_named_class_with_single_object_property() {
        let (ds, report) = translate(vec![Axiom::AxiomHasKey(
            vec![],
            class("Person"),
            vec![obj_prop("hasSsn")],
            vec![],
        )]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let list_head =
            object_of(&ds, &ex("Person"), OWL_HAS_KEY).expect("owl:hasKey triple must exist");
        assert_eq!(read_rdf_list(&ds, list_head), vec![ex("hasSsn")]);
        // owl:hasKey + 1 list cell (rdf:first + rdf:rest) = 3 triples
        assert_eq!(report.triples_added, 3);
    }

    /// A mix of object and data properties: the `rdf:List` must preserve
    /// declared order, object properties first then data properties, per the
    /// spec's `SEQ OPE1 ... OPEm DPE1 ... DPEn` ordering.
    #[test]
    fn has_key_with_mixed_object_and_data_properties_preserves_order() {
        let (ds, report) = translate(vec![Axiom::AxiomHasKey(
            vec![],
            class("Person"),
            vec![obj_prop("hasSsn"), obj_prop("hasPassport")],
            vec![full("firstName"), full("lastName")],
        )]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let list_head =
            object_of(&ds, &ex("Person"), OWL_HAS_KEY).expect("owl:hasKey triple must exist");
        assert_eq!(
            read_rdf_list(&ds, list_head),
            vec![
                ex("hasSsn"),
                ex("hasPassport"),
                ex("firstName"),
                ex("lastName"),
            ]
        );
    }

    /// `HasKey` on a complex (non-named) class expression is now translated
    /// via the general blank-node structural encoding
    /// ([#509](https://github.com/daghovland/rdf-datalog/issues/509)).
    #[test]
    fn has_key_on_complex_class_expression_is_now_translated() {
        let (ds, report) = translate(vec![Axiom::AxiomHasKey(
            vec![],
            ClassExpression::ObjectUnionOf(vec![class("Person"), class("Organization")]),
            vec![obj_prop("hasSsn")],
            vec![],
        )]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let restriction_type = id_of(&ds, &IriReference(OWL_CLASS.to_owned())).expect("interned");
        let union_pred = id_of(&ds, &IriReference(OWL_UNION_OF.to_owned())).expect("interned");
        let type_pred = id_of(&ds, &IriReference(RDF_TYPE.to_owned())).expect("interned");
        let union_nodes = ds.quads_matching(None, None, Some(type_pred), Some(restriction_type));
        let union_node = union_nodes
            .iter()
            .find(|q| {
                !ds.quads_matching(None, Some(q.subject), Some(union_pred), None)
                    .is_empty()
            })
            .expect("owl:Class blank node with owl:unionOf must exist")
            .subject;
        let list_head = ds
            .quads_matching(None, Some(union_node), Some(union_pred), None)
            .first()
            .expect("owl:unionOf triple")
            .obj;
        assert_eq!(
            read_rdf_list(&ds, list_head),
            vec![ex("Person"), ex("Organization")]
        );
        // `owl:hasKey`'s subject is the union blank node, not a named IRI,
        // so it's looked up directly by subject id rather than via
        // `object_of` (which resolves a subject IRI).
        let has_key_pred = id_of(&ds, &IriReference(OWL_HAS_KEY.to_owned())).expect("interned");
        let has_key_quads = ds.quads_matching(None, Some(union_node), Some(has_key_pred), None);
        assert_eq!(
            has_key_quads.len(),
            1,
            "the union blank node carries owl:hasKey"
        );
        assert_eq!(read_rdf_list(&ds, has_key_quads[0].obj), vec![ex("hasSsn")]);
    }

    /// Axioms mentioning a complex class expression are now translated via
    /// the general blank-node structural encoding
    /// ([#509](https://github.com/daghovland/rdf-datalog/issues/509)) rather
    /// than being reported as skipped.
    #[test]
    fn subclassof_and_class_assertion_with_complex_class_expression_are_now_translated() {
        let (ds, report) = translate(vec![
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
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let list_head = object_of(&ds, &ex("Dog"), RDFS_SUB_CLASS_OF)
            .expect("Dog rdfs:subClassOf <restriction> triple must exist");
        let some_values_from =
            id_of(&ds, &IriReference(OWL_SOME_VALUES_FROM.to_owned())).expect("interned");
        let filler = ds
            .quads_matching(None, Some(list_head), Some(some_values_from), None)
            .first()
            .expect("owl:someValuesFrom triple")
            .obj;
        assert_eq!(filler, id_of(&ds, &ex("Animal")).expect("Animal interned"));

        let fido_id = id_of(&ds, &ex("fido")).expect("fido interned");
        let type_pred = id_of(&ds, &IriReference(RDF_TYPE.to_owned())).expect("interned");
        let type_quads = ds.quads_matching(None, Some(fido_id), Some(type_pred), None);
        let union_pred = id_of(&ds, &IriReference(OWL_UNION_OF.to_owned())).expect("interned");
        let union_node = type_quads
            .iter()
            .find(|q| {
                !ds.quads_matching(None, Some(q.obj), Some(union_pred), None)
                    .is_empty()
            })
            .expect("fido rdf:type <union blank node> triple must exist")
            .obj;
        let union_list_head = ds
            .quads_matching(None, Some(union_node), Some(union_pred), None)
            .first()
            .expect("owl:unionOf triple")
            .obj;
        assert_eq!(
            read_rdf_list(&ds, union_list_head),
            vec![ex("Dog"), ex("Cat")]
        );
    }

    // ── annotation axioms and axiom-annotation reification (#514) ─────────

    /// How many `owl:Axiom`-typed reification nodes exist in the store.
    fn axiom_reification_count(ds: &Datastore) -> usize {
        // `owl:Axiom` is only ever interned once at least one annotated
        // axiom has been translated — an ontology with none of those never
        // mints it, so treat "not interned" as zero reifications rather than
        // panicking.
        let Some(axiom_type) = id_of(ds, &IriReference(OWL_AXIOM.to_owned())) else {
            return 0;
        };
        let rdf_type = id_of(ds, &IriReference(RDF_TYPE.to_owned())).expect("rdf:type interned");
        ds.quads_matching(None, None, Some(rdf_type), Some(axiom_type))
            .len()
    }

    /// The `owl:Axiom` reification node for the ground triple `(s, p, o)`
    /// (by id), if one exists (i.e. a blank node typed `owl:Axiom` with
    /// matching `owl:annotatedSource` / `owl:annotatedProperty` /
    /// `owl:annotatedTarget`).
    fn find_reification_node_id(
        ds: &Datastore,
        s: GraphElementId,
        p: GraphElementId,
        o: GraphElementId,
    ) -> Option<GraphElementId> {
        let annotated_source =
            id_of(ds, &IriReference(OWL_ANNOTATED_SOURCE.to_owned())).expect("interned");
        let annotated_property =
            id_of(ds, &IriReference(OWL_ANNOTATED_PROPERTY.to_owned())).expect("interned");
        let annotated_target =
            id_of(ds, &IriReference(OWL_ANNOTATED_TARGET.to_owned())).expect("interned");
        ds.quads_matching(None, None, Some(annotated_source), Some(s))
            .into_iter()
            .map(|q| q.subject)
            .find(|&node| {
                !ds.quads_matching(None, Some(node), Some(annotated_property), Some(p))
                    .is_empty()
                    && !ds
                        .quads_matching(None, Some(node), Some(annotated_target), Some(o))
                        .is_empty()
            })
    }

    /// The `owl:Axiom` reification node for the ground triple
    /// `<subject-iri> <predicate-iri> <obj-iri>`, if one exists.
    fn find_reification_node(
        ds: &Datastore,
        subject: &IriReference,
        predicate_iri: &str,
        obj: &IriReference,
    ) -> Option<GraphElementId> {
        let s = id_of(ds, subject)?;
        let p = id_of(ds, &IriReference(predicate_iri.to_owned()))?;
        let o = id_of(ds, obj)?;
        find_reification_node_id(ds, s, p, o)
    }

    fn annotation(ap: &str, value: &str) -> Annotation {
        (
            full(ap),
            AnnotationValue::LiteralAnnotation(GraphElement::GraphLiteral(
                ingress::RdfLiteral::LangLiteral {
                    lang: "en".to_owned(),
                    literal: value.to_owned(),
                },
            )),
        )
    }

    #[test]
    fn annotation_assertion_on_named_individual_becomes_ground_triple() {
        let (ds, report) = translate(vec![Axiom::AxiomAnnotationAxiom(
            AnnotationAxiom::AnnotationAssertion(
                vec![],
                full("comment"),
                GraphElement::NodeOrEdge(RdfResource::Iri(ex("fido"))),
                GraphElement::GraphLiteral(ingress::RdfLiteral::LangLiteral {
                    lang: "en".to_owned(),
                    literal: "A good dog".to_owned(),
                }),
            ),
        )]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert_eq!(report.triples_added, 1);
        assert_eq!(
            axiom_reification_count(&ds),
            0,
            "no annotations, no reification"
        );
    }

    #[test]
    fn sub_annotation_property_of_becomes_rdfs_sub_property_of() {
        let (ds, report) = translate(vec![Axiom::AxiomAnnotationAxiom(
            AnnotationAxiom::SubAnnotationPropertyOf(vec![], full("shortComment"), full("comment")),
        )]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(
            &ds,
            &ex("shortComment"),
            RDFS_SUB_PROPERTY_OF,
            &ex("comment")
        ));
    }

    #[test]
    fn annotation_property_domain_and_range_become_rdfs_domain_and_range() {
        let (ds, report) = translate(vec![
            Axiom::AxiomAnnotationAxiom(AnnotationAxiom::AnnotationPropertyDomain(
                vec![],
                full("comment"),
                full("Thing"),
            )),
            Axiom::AxiomAnnotationAxiom(AnnotationAxiom::AnnotationPropertyRange(
                vec![],
                full("comment"),
                full("Literal"),
            )),
        ]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(&ds, &ex("comment"), RDFS_DOMAIN, &ex("Thing")));
        assert!(has_triple(&ds, &ex("comment"), RDFS_RANGE, &ex("Literal")));
    }

    #[test]
    fn subclassof_with_annotation_is_reified_via_owl_axiom() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![annotation("source", "a good textbook")],
            class("Dog"),
            class("Animal"),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(
            &ds,
            &ex("Dog"),
            RDFS_SUB_CLASS_OF,
            &ex("Animal")
        ));
        let node = find_reification_node(&ds, &ex("Dog"), RDFS_SUB_CLASS_OF, &ex("Animal"))
            .expect("owl:Axiom reification node must exist");
        let source_pred = id_of(&ds, &ex("source")).expect("annotation property interned");
        let quads = ds.quads_matching(None, Some(node), Some(source_pred), None);
        assert_eq!(
            quads.len(),
            1,
            "reification node must carry the annotation triple"
        );
        assert_eq!(axiom_reification_count(&ds), 1);
    }

    #[test]
    fn subclassof_with_empty_annotations_emits_no_reification() {
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
        assert_eq!(
            axiom_reification_count(&ds),
            0,
            "an axiom with no annotations must not get an owl:Axiom reification"
        );
    }

    #[test]
    fn equivalent_classes_annotations_repeat_on_every_chain_triple() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::EquivalentClasses(
            vec![annotation("source", "a good textbook")],
            vec![class("A"), class("B"), class("C")],
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(&ds, &ex("A"), OWL_EQUIVALENT_CLASS, &ex("B")));
        assert!(has_triple(&ds, &ex("B"), OWL_EQUIVALENT_CLASS, &ex("C")));
        assert!(find_reification_node(&ds, &ex("A"), OWL_EQUIVALENT_CLASS, &ex("B")).is_some());
        assert!(find_reification_node(&ds, &ex("B"), OWL_EQUIVALENT_CLASS, &ex("C")).is_some());
        assert_eq!(
            axiom_reification_count(&ds),
            2,
            "each chain triple gets its own owl:Axiom reification"
        );
    }

    #[test]
    fn has_key_with_annotation_reifies_only_the_main_triple() {
        let (ds, report) = translate(vec![Axiom::AxiomHasKey(
            vec![annotation("source", "a good textbook")],
            class("Person"),
            vec![obj_prop("hasSsn")],
            vec![],
        )]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let list_head =
            object_of(&ds, &ex("Person"), OWL_HAS_KEY).expect("owl:hasKey triple must exist");
        let person_id = id_of(&ds, &ex("Person")).expect("Person interned");
        let has_key_pred = id_of(&ds, &IriReference(OWL_HAS_KEY.to_owned())).expect("interned");
        assert!(
            find_reification_node_id(&ds, person_id, has_key_pred, list_head).is_some(),
            "the main owl:hasKey triple must be reified"
        );
        assert_eq!(
            axiom_reification_count(&ds),
            1,
            "only the main owl:hasKey triple is reified, not the rdf:List cells"
        );
        // Sanity: the list itself is unaffected by the annotation.
        assert_eq!(read_rdf_list(&ds, list_head), vec![ex("hasSsn")]);
    }

    // ── complex class-expression blank-node encoding (#509) ────────────────

    /// Resolve `A rdfs:subClassOf <blank node>` for a `SubClassOf(A, expr)`
    /// axiom, returning the blank node's id. Shared by the tests below,
    /// which each build one `SubClassOf` axiom whose superclass is the
    /// expression under test.
    fn sub_class_of_node(ds: &Datastore, sub_iri: &IriReference) -> GraphElementId {
        object_of(ds, sub_iri, RDFS_SUB_CLASS_OF).expect("A rdfs:subClassOf <node> must exist")
    }

    /// The single object of `subject <predicate-iri> ?object`, panicking if
    /// there isn't exactly one.
    fn single_object_by_id(
        ds: &Datastore,
        subject: GraphElementId,
        predicate_iri: &str,
    ) -> GraphElementId {
        let predicate = id_of(ds, &IriReference(predicate_iri.to_owned())).expect("interned");
        let quads = ds.quads_matching(None, Some(subject), Some(predicate), None);
        assert_eq!(
            quads.len(),
            1,
            "expected exactly one {predicate_iri} triple"
        );
        quads[0].obj
    }

    #[test]
    fn object_union_of_becomes_owl_class_with_union_of_list() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectUnionOf(vec![class("B"), class("C")]),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let type_pred = id_of(&ds, &IriReference(RDF_TYPE.to_owned())).expect("interned");
        let owl_class = id_of(&ds, &IriReference(OWL_CLASS.to_owned())).expect("interned");
        assert!(
            !ds.quads_matching(None, Some(node), Some(type_pred), Some(owl_class))
                .is_empty(),
            "blank node must be typed owl:Class"
        );
        let list_head = single_object_by_id(&ds, node, OWL_UNION_OF);
        assert_eq!(read_rdf_list(&ds, list_head), vec![ex("B"), ex("C")]);
    }

    #[test]
    fn object_intersection_of_becomes_owl_class_with_intersection_of_list() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectIntersectionOf(vec![class("B"), class("C")]),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let list_head = single_object_by_id(&ds, node, OWL_INTERSECTION_OF);
        assert_eq!(read_rdf_list(&ds, list_head), vec![ex("B"), ex("C")]);
    }

    #[test]
    fn object_complement_of_becomes_owl_class_with_complement_of() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectComplementOf(Box::new(class("B"))),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let complement = single_object_by_id(&ds, node, OWL_COMPLEMENT_OF);
        assert_eq!(complement, id_of(&ds, &ex("B")).expect("B interned"));
    }

    #[test]
    fn object_one_of_becomes_owl_class_with_one_of_list() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectOneOf(vec![
                Individual::NamedIndividual(full("i1")),
                Individual::NamedIndividual(full("i2")),
            ]),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let list_head = single_object_by_id(&ds, node, OWL_ONE_OF);
        assert_eq!(read_rdf_list(&ds, list_head), vec![ex("i1"), ex("i2")]);
    }

    #[test]
    fn object_some_values_from_becomes_owl_restriction() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectSomeValuesFrom(obj_prop("hasPet"), Box::new(class("Dog"))),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let type_pred = id_of(&ds, &IriReference(RDF_TYPE.to_owned())).expect("interned");
        let restriction = id_of(&ds, &IriReference(OWL_RESTRICTION.to_owned())).expect("interned");
        assert!(
            !ds.quads_matching(None, Some(node), Some(type_pred), Some(restriction))
                .is_empty()
        );
        let on_prop = single_object_by_id(&ds, node, OWL_ON_PROPERTY);
        assert_eq!(on_prop, id_of(&ds, &ex("hasPet")).expect("hasPet interned"));
        let filler = single_object_by_id(&ds, node, OWL_SOME_VALUES_FROM);
        assert_eq!(filler, id_of(&ds, &ex("Dog")).expect("Dog interned"));
    }

    #[test]
    fn object_all_values_from_becomes_owl_restriction() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectAllValuesFrom(obj_prop("hasPet"), Box::new(class("Dog"))),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let filler = single_object_by_id(&ds, node, OWL_ALL_VALUES_FROM);
        assert_eq!(filler, id_of(&ds, &ex("Dog")).expect("Dog interned"));
    }

    #[test]
    fn object_has_value_becomes_owl_restriction() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectHasValue(
                obj_prop("hasPet"),
                Individual::NamedIndividual(full("fido")),
            ),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let value = single_object_by_id(&ds, node, OWL_HAS_VALUE);
        assert_eq!(value, id_of(&ds, &ex("fido")).expect("fido interned"));
    }

    #[test]
    fn object_has_self_becomes_owl_restriction_with_true_literal() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectHasSelf(obj_prop("hasPet")),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let has_self_pred = id_of(&ds, &IriReference(OWL_HAS_SELF.to_owned())).expect("interned");
        let quads = ds.quads_matching(None, Some(node), Some(has_self_pred), None);
        assert_eq!(quads.len(), 1);
        let elem = ds.resources.get_graph_element(quads[0].obj);
        assert_eq!(
            elem,
            &dag_rdf::GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(true))
        );
    }

    #[test]
    fn object_unqualified_cardinality_restrictions_become_owl_restriction() {
        let (ds, report) = translate(vec![
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Min"),
                ClassExpression::ObjectMinCardinality(2.into(), obj_prop("hasPet")),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Max"),
                ClassExpression::ObjectMaxCardinality(3.into(), obj_prop("hasPet")),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Exact"),
                ClassExpression::ObjectExactCardinality(1.into(), obj_prop("hasPet")),
            )),
        ]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let min_node = sub_class_of_node(&ds, &ex("Min"));
        let min_val = single_object_by_id(&ds, min_node, OWL_MIN_CARDINALITY);
        let elem = ds.resources.get_graph_element(min_val);
        assert_eq!(
            elem,
            &dag_rdf::GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                type_iri: IriReference(XSD_NON_NEGATIVE_INTEGER.to_owned()),
                literal: "2".to_owned(),
            })
        );
        let max_node = sub_class_of_node(&ds, &ex("Max"));
        single_object_by_id(&ds, max_node, OWL_MAX_CARDINALITY);
        let exact_node = sub_class_of_node(&ds, &ex("Exact"));
        single_object_by_id(&ds, exact_node, OWL_CARDINALITY);
    }

    #[test]
    fn object_qualified_cardinality_restrictions_use_on_class() {
        let (ds, report) = translate(vec![
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Min"),
                ClassExpression::ObjectMinQualifiedCardinality(
                    2.into(),
                    obj_prop("hasPet"),
                    Box::new(class("Dog")),
                ),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Max"),
                ClassExpression::ObjectMaxQualifiedCardinality(
                    3.into(),
                    obj_prop("hasPet"),
                    Box::new(class("Dog")),
                ),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Exact"),
                ClassExpression::ObjectExactQualifiedCardinality(
                    1.into(),
                    obj_prop("hasPet"),
                    Box::new(class("Dog")),
                ),
            )),
        ]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let min_node = sub_class_of_node(&ds, &ex("Min"));
        single_object_by_id(&ds, min_node, OWL_MIN_QUALIFIED_CARDINALITY);
        let on_class = single_object_by_id(&ds, min_node, OWL_ON_CLASS);
        assert_eq!(on_class, id_of(&ds, &ex("Dog")).expect("Dog interned"));
        let max_node = sub_class_of_node(&ds, &ex("Max"));
        single_object_by_id(&ds, max_node, OWL_MAX_QUALIFIED_CARDINALITY);
        single_object_by_id(&ds, max_node, OWL_ON_CLASS);
        let exact_node = sub_class_of_node(&ds, &ex("Exact"));
        single_object_by_id(&ds, exact_node, OWL_QUALIFIED_CARDINALITY);
        single_object_by_id(&ds, exact_node, OWL_ON_CLASS);
    }

    #[test]
    fn data_some_values_from_single_property_uses_on_property() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::DataSomeValuesFrom(
                vec![full("age")],
                DataRange::NamedDataRange(FullIri(IriReference(
                    "http://www.w3.org/2001/XMLSchema#integer".to_owned(),
                ))),
            ),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let on_prop = single_object_by_id(&ds, node, OWL_ON_PROPERTY);
        assert_eq!(on_prop, id_of(&ds, &ex("age")).expect("age interned"));
        let filler = single_object_by_id(&ds, node, OWL_SOME_VALUES_FROM);
        assert_eq!(
            filler,
            id_of(
                &ds,
                &IriReference("http://www.w3.org/2001/XMLSchema#integer".to_owned())
            )
            .expect("xsd:integer interned")
        );
    }

    #[test]
    fn data_some_values_from_multiple_properties_uses_on_properties_list() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::DataSomeValuesFrom(
                vec![full("firstName"), full("lastName")],
                DataRange::NamedDataRange(FullIri(IriReference(
                    "http://www.w3.org/2001/XMLSchema#string".to_owned(),
                ))),
            ),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let node = sub_class_of_node(&ds, &ex("A"));
        let list_head = single_object_by_id(&ds, node, OWL_ON_PROPERTIES);
        assert_eq!(
            read_rdf_list(&ds, list_head),
            vec![ex("firstName"), ex("lastName")]
        );
    }

    #[test]
    fn data_cardinality_restrictions_become_owl_restriction() {
        let integer = || {
            DataRange::NamedDataRange(FullIri(IriReference(
                "http://www.w3.org/2001/XMLSchema#integer".to_owned(),
            )))
        };
        let (ds, report) = translate(vec![
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Min"),
                ClassExpression::DataMinCardinality(2.into(), full("age")),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("MinQ"),
                ClassExpression::DataMinQualifiedCardinality(2.into(), full("age"), integer()),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Max"),
                ClassExpression::DataMaxCardinality(3.into(), full("age")),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("MaxQ"),
                ClassExpression::DataMaxQualifiedCardinality(3.into(), full("age"), integer()),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("Exact"),
                ClassExpression::DataExactCardinality(1.into(), full("age")),
            )),
            Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
                vec![],
                class("ExactQ"),
                ClassExpression::DataExactQualifiedCardinality(1.into(), full("age"), integer()),
            )),
        ]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let min_node = sub_class_of_node(&ds, &ex("Min"));
        single_object_by_id(&ds, min_node, OWL_MIN_CARDINALITY);
        let min_q_node = sub_class_of_node(&ds, &ex("MinQ"));
        single_object_by_id(&ds, min_q_node, OWL_MIN_QUALIFIED_CARDINALITY);
        single_object_by_id(&ds, min_q_node, OWL_ON_DATA_RANGE);
        let max_node = sub_class_of_node(&ds, &ex("Max"));
        single_object_by_id(&ds, max_node, OWL_MAX_CARDINALITY);
        let max_q_node = sub_class_of_node(&ds, &ex("MaxQ"));
        single_object_by_id(&ds, max_q_node, OWL_MAX_QUALIFIED_CARDINALITY);
        single_object_by_id(&ds, max_q_node, OWL_ON_DATA_RANGE);
        let exact_node = sub_class_of_node(&ds, &ex("Exact"));
        single_object_by_id(&ds, exact_node, OWL_CARDINALITY);
        let exact_q_node = sub_class_of_node(&ds, &ex("ExactQ"));
        single_object_by_id(&ds, exact_q_node, OWL_QUALIFIED_CARDINALITY);
        single_object_by_id(&ds, exact_q_node, OWL_ON_DATA_RANGE);
    }

    /// A restriction whose `DataRange` is itself complex (not a plain named
    /// datatype) has no RDF encoding yet — that structural mapping is
    /// [#512](https://github.com/daghovland/rdf-datalog/issues/512)'s scope,
    /// not this issue's — so it must be reported skipped, not
    /// partially/incorrectly emitted.
    #[test]
    fn data_restriction_with_complex_data_range_is_reported_not_silently_dropped() {
        let (_ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::DataSomeValuesFrom(
                vec![full("age")],
                DataRange::DataUnionOf(vec![
                    DataRange::NamedDataRange(FullIri(IriReference(
                        "http://www.w3.org/2001/XMLSchema#integer".to_owned(),
                    ))),
                    DataRange::NamedDataRange(FullIri(IriReference(
                        "http://www.w3.org/2001/XMLSchema#string".to_owned(),
                    ))),
                ]),
            ),
        ))]);
        assert_eq!(report.triples_added, 0);
        assert_eq!(report.skipped.len(), 1, "skipped: {:?}", report.skipped);
    }

    /// Recursion: a union containing an intersection must translate the
    /// nested intersection as its own `owl:Class` blank node inside the
    /// outer union's `rdf:List`, not just one level deep.
    #[test]
    fn nested_union_of_union_and_intersection_recurses() {
        let (ds, report) = translate(vec![Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            class("A"),
            ClassExpression::ObjectUnionOf(vec![
                class("B"),
                ClassExpression::ObjectIntersectionOf(vec![class("C"), class("D")]),
            ]),
        ))]);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let outer_node = sub_class_of_node(&ds, &ex("A"));
        let outer_list_head = single_object_by_id(&ds, outer_node, OWL_UNION_OF);

        let nil = id_of(&ds, &IriReference(RDF_NIL.to_owned())).expect("rdf:nil interned");
        let first_pred = id_of(&ds, &IriReference(RDF_FIRST.to_owned())).expect("rdf:first");
        let rest_pred = id_of(&ds, &IriReference(RDF_REST.to_owned())).expect("rdf:rest");

        let first_cell_first =
            ds.quads_matching(None, Some(outer_list_head), Some(first_pred), None);
        assert_eq!(first_cell_first.len(), 1);
        assert_eq!(
            first_cell_first[0].obj,
            id_of(&ds, &ex("B")).expect("B interned"),
            "first union member is the named class B"
        );

        let rest = ds.quads_matching(None, Some(outer_list_head), Some(rest_pred), None);
        assert_eq!(rest.len(), 1);
        let second_cell = rest[0].obj;
        assert_ne!(second_cell, nil);

        let second_first = ds.quads_matching(None, Some(second_cell), Some(first_pred), None);
        assert_eq!(second_first.len(), 1);
        let intersection_node = second_first[0].obj;

        let type_pred = id_of(&ds, &IriReference(RDF_TYPE.to_owned())).expect("interned");
        let owl_class = id_of(&ds, &IriReference(OWL_CLASS.to_owned())).expect("interned");
        assert!(
            !ds.quads_matching(
                None,
                Some(intersection_node),
                Some(type_pred),
                Some(owl_class)
            )
            .is_empty()
        );
        let inner_list_head = single_object_by_id(&ds, intersection_node, OWL_INTERSECTION_OF);
        assert_eq!(read_rdf_list(&ds, inner_list_head), vec![ex("C"), ex("D")]);
    }

    // ── ontology header triples (#515) ──────────────────────────────────
    //
    // <https://www.w3.org/TR/owl2-mapping-to-rdf/> §2.1, Table 1's
    // `T(Ontology(...))` row. See
    // docs/plans/OWL2RDF_ONTOLOGY_HEADER_PLAN.md for the exact triple
    // shapes and citations.

    /// Translate a full [`Ontology`] (not just a bare axiom list, unlike
    /// [`translate`]) so header fields (version, imports, annotations) can
    /// vary per test.
    fn ontology_translate(ontology: &Ontology) -> (Datastore, RdfTranslationReport) {
        let mut ds = Datastore::new(100);
        let report = owl2rdf(&mut ds, ontology);
        (ds, report)
    }

    /// The id of the single node typed `owl:Ontology` in `ds`. Used for the
    /// anonymous-ontology-with-imports-or-annotations case, where the
    /// header's subject is a fresh blank node with no IRI to look up by.
    /// Panics if there isn't exactly one such node — callers only reach for
    /// this helper when they know their ontology isn't the fully-bare
    /// anonymous case that emits no header at all (see
    /// `bare_anonymous_ontology_gets_no_header_at_all`).
    fn header_node(ds: &Datastore) -> GraphElementId {
        let type_pred = id_of(ds, &IriReference(RDF_TYPE.to_owned())).expect("rdf:type interned");
        let owl_ontology =
            id_of(ds, &IriReference(OWL_ONTOLOGY.to_owned())).expect("owl:Ontology interned");
        let quads = ds.quads_matching(None, None, Some(type_pred), Some(owl_ontology));
        assert_eq!(
            quads.len(),
            1,
            "expected exactly one node typed owl:Ontology"
        );
        quads[0].subject
    }

    /// A named, unversioned, import-free, annotation-free, axiom-free
    /// ontology.
    fn named_ontology(iri: &str) -> Ontology {
        Ontology::new(
            vec![],
            OntologyVersion::NamedOntology(IriReference(iri.to_owned())),
            vec![],
            vec![],
        )
    }

    #[test]
    fn named_ontology_with_iri_only_gets_type_declaration() {
        let (ds, report) = ontology_translate(&named_ontology(&format!("{EX}onto")));
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            RDF_TYPE,
            &IriReference(OWL_ONTOLOGY.to_owned())
        ));
        assert_eq!(report.triples_added, 1);
    }

    #[test]
    fn named_ontology_with_version_iri_gets_version_triple() {
        let ontology = Ontology::new(
            vec![],
            OntologyVersion::VersionedOntology {
                ontology_iri: IriReference(format!("{EX}onto")),
                version_iri: IriReference(format!("{EX}onto/1.0")),
            },
            vec![],
            vec![],
        );
        let (ds, report) = ontology_translate(&ontology);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            RDF_TYPE,
            &IriReference(OWL_ONTOLOGY.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            OWL_VERSION_IRI,
            &IriReference(format!("{EX}onto/1.0"))
        ));
        assert_eq!(report.triples_added, 2);
    }

    #[test]
    fn named_ontology_with_imports_gets_one_imports_triple_per_import() {
        let mut ontology = named_ontology(&format!("{EX}onto"));
        ontology.directly_imports_documents = vec![
            IriReference(format!("{EX}import1")),
            IriReference(format!("{EX}import2")),
        ];
        let (ds, report) = ontology_translate(&ontology);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            OWL_IMPORT,
            &IriReference(format!("{EX}import1"))
        ));
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            OWL_IMPORT,
            &IriReference(format!("{EX}import2"))
        ));
        // type triple + 2 imports triples
        assert_eq!(report.triples_added, 3);
    }

    #[test]
    fn anonymous_ontology_gets_type_declaration_and_imports_on_blank_node() {
        let ontology = Ontology::new(
            vec![IriReference(format!("{EX}import1"))],
            OntologyVersion::UnNamedOntology,
            vec![],
            vec![],
        );
        let (ds, report) = ontology_translate(&ontology);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        let header = header_node(&ds);
        let import_id = id_of(&ds, &IriReference(format!("{EX}import1"))).expect("interned");
        let import_pred = id_of(&ds, &IriReference(OWL_IMPORT.to_owned())).expect("interned");
        assert!(
            !ds.quads_matching(None, Some(header), Some(import_pred), Some(import_id))
                .is_empty(),
            "owl:imports triple must hang off the same blank node as the type triple"
        );
        // type triple + 1 import triple
        assert_eq!(report.triples_added, 2);
    }

    #[test]
    fn named_ontology_with_version_imports_and_annotation_all_together() {
        let mut ontology = Ontology::new(
            vec![IriReference(format!("{EX}import1"))],
            OntologyVersion::VersionedOntology {
                ontology_iri: IriReference(format!("{EX}onto")),
                version_iri: IriReference(format!("{EX}onto/1.0")),
            },
            vec![annotation("comment", "An example ontology")],
            vec![],
        );
        ontology.axioms = vec![]; // sanity: header emission doesn't need axioms
        let (ds, report) = ontology_translate(&ontology);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            RDF_TYPE,
            &IriReference(OWL_ONTOLOGY.to_owned())
        ));
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            OWL_VERSION_IRI,
            &IriReference(format!("{EX}onto/1.0"))
        ));
        assert!(has_triple(
            &ds,
            &IriReference(format!("{EX}onto")),
            OWL_IMPORT,
            &IriReference(format!("{EX}import1"))
        ));
        let comment_pred = id_of(&ds, &ex("comment")).expect("interned");
        let onto_id = id_of(&ds, &IriReference(format!("{EX}onto"))).expect("interned");
        assert_eq!(
            ds.quads_matching(None, Some(onto_id), Some(comment_pred), None)
                .len(),
            1,
            "ontology-level annotation must be a plain, non-reified triple on the header node"
        );
        // type + version + import + annotation
        assert_eq!(report.triples_added, 4);
    }

    /// A completely bare anonymous ontology (no IRI, no imports, no
    /// annotations) — as produced by `rdf_owl_translator::rdf2owl` on plain
    /// RDF that never declared itself an OWL ontology document in the
    /// first place — must not gain a synthetic `_:x rdf:type owl:Ontology`
    /// node it never had. See `ontology_header`'s doc comment for the full
    /// rationale (this is a deliberate deviation from the literal spec
    /// text, driven by `rdf_starting_roundtrip_preserves_graph_isomorphism`
    /// in `tests/manchester_roundtrip.rs`).
    #[test]
    fn bare_anonymous_ontology_gets_no_header_at_all() {
        let ontology = Ontology::new(vec![], OntologyVersion::UnNamedOntology, vec![], vec![]);
        let (ds, report) = ontology_translate(&ontology);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert_eq!(report.triples_added, 0);
        // Nothing at all was emitted (not even interning owl:Ontology's IRI,
        // since it's never used as a triple component), so there must be no
        // quads whatsoever in the datastore.
        assert!(
            ds.quads_matching(None, None, None, None).is_empty(),
            "a fully bare anonymous ontology must produce zero triples"
        );
    }
}

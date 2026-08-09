/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Full-circle fidelity test for the Manchester Syntax pipeline
//! ([#179](https://github.com/daghovland/rdf-datalog/issues/179), epic
//! [#178](https://github.com/daghovland/rdf-datalog/issues/178)):
//!
//! 1. Parse Manchester Syntax text (`manchester_parser::parse`) → `Ontology`.
//! 2. Translate the ontology into `Datastore` quads via the general
//!    OWL→RDF structural mapping (`owl2rl2datalog::owl2rdf`,
//!    [#177](https://github.com/daghovland/rdf-datalog/issues/177)).
//! 3. Serialize those quads as Turtle (`turtle::serialize_graph`).
//! 4. Re-parse that Turtle text into a fresh `Datastore`
//!    (`turtle::parse_turtle`).
//! 5. Translate the re-parsed `Datastore` back into an `Ontology`
//!    (`rdf_owl_translator::rdf2owl`).
//! 6. Serialize *that* `Ontology` back to Manchester Syntax
//!    (`manchester_parser::serialize`, [#160](https://github.com/daghovland/rdf-datalog/issues/160)).
//! 7. Compare the round-tripped Manchester text's axioms against the
//!    original's.
//!
//! ## What "compare" means here
//!
//! Per the issue's own discussion, byte-for-byte text comparison is too
//! strict: axiom ordering, `Class:`/`ObjectProperty:`/... frame grouping, and
//! (for documents with anonymous individuals) blank-node numbering can all
//! legitimately differ across the round trip without indicating a real bug.
//! This test instead re-parses *both* the original and the round-tripped
//! Manchester text back into `Ontology` values (`manchester_parser::parse`
//! itself has no `PartialEq` bugs to compensate for — parsing is exercised
//! and trusted elsewhere) and compares their `axioms: Vec<Axiom>` as a
//! `HashSet<Axiom>`, exactly the pattern already used by
//! `manchester_parser/tests/serialize_roundtrip.rs` for the narrower
//! serializer-only round trip. `Axiom` derives `Eq`/`Hash`, so this is a
//! structural comparison, order-independent by construction.
//!
//! For the anonymous-individual case, blank-node/anonymous-individual ids are
//! *not* guaranteed to line up: an id assigned during Manchester parsing is
//! discarded once translated to an RDF blank node (`owl2rdf`), gets a
//! *different* id from the Turtle parser on re-ingestion, and is renumbered
//! again by `rdf2owl`. [`AnonRenumberer`] below canonicalises
//! `Individual::AnonymousIndividual` ids by first-occurrence order within
//! each axiom set (the "stable renumbering keyed by first appearance"
//! pattern the issue asks for) before the two sets are compared. It is a
//! small local axiom-tree walk rather than a reuse of `rdf_canon`:
//! `rdf_canon`'s canonicalisation operates over RDF quads/blank nodes, and
//! `Ontology`'s `Vec<Axiom>` has no quad-shaped representation to hand it
//! (that is exactly the asymmetry `owl2rdf`/`rdf2owl` exist to bridge, and
//! going via RDF quads *for the comparison itself* would just reintroduce
//! the numbering churn this canonicalisation step exists to remove).
//!
//! Three Manchester fixtures are used for the Manchester-starting direction:
//! - [`RICH_OMN`] exercises a broad spread of Manchester constructs that
//!   `owl2rdf`'s current scope actually translates (named-class TBox axioms,
//!   object/data property frames and characteristics, ABox assertions) with
//!   no anonymous individuals, so plain (non-canonicalising) axiom-set
//!   equality applies.
//! - [`ANON_OMN`] adds a single anonymous individual to specifically exercise
//!   the canonicalisation path with the minimal case.
//! - [`ANON_MULTI_OMN`] adds three anonymous individuals linked to each other
//!   and to named individuals via `Types:`, object `Facts:`, `SameAs:` and
//!   `DifferentFrom:`. It was originally built so frame-declaration order and
//!   first-reference order diverge, to exercise [`AnonRenumberer`]'s
//!   renumbering *order* directly — but doing so proved that comparison
//!   unsound (see [`full_pipeline_roundtrips_multiple_anonymous_individuals`]'s
//!   doc comment), so this fixture is instead compared via `rdf_canon`, the
//!   same strategy used for the RDF-starting direction below.
//!
//! A fourth test, [`rdf_starting_roundtrip_preserves_graph_isomorphism`],
//! starts from hand-written Turtle instead of Manchester Syntax and compares
//! RDF graphs directly via `rdf_canon`'s RDFC-1.0 canonicalisation rather
//! than the `AnonRenumberer` axiom-tree walk; see its own doc comment for
//! why that comparison strategy fits that direction better. The multi-anon
//! Manchester-starting test above ended up needing the same strategy, for
//! the same underlying reason: `rdf_canon`'s blank-node-identity-aware
//! canonicalisation is robust to exactly the kind of order-shuffling that
//! sank the naive first-occurrence approach.
//!
//! Constructs `owl2rdf` does not yet translate (non-atomic class
//! expressions: `and`/`or`/`not`, cardinality/`some`/`only` restrictions,
//! `{...}` nominals — see that module's doc comment) are deliberately kept
//! out of these fixtures: including them would just be asserting today's
//! `owl2rdf` gap rather than testing round-trip fidelity, and that gap is
//! already tracked by [#177](https://github.com/daghovland/rdf-datalog/issues/177)'s
//! own follow-up scope, not this issue.

use dag_rdf::{DEFAULT_GRAPH_ELEMENT_ID, Datastore};
use owl_ontology::{
    Annotation, AnnotationValue, Assertion, Axiom, ClassAxiom, ClassExpression, Entity, Individual,
};
use owl2rl2datalog::owl2rdf;
use rdf_owl_translator::rdf2owl;
use std::collections::{HashMap, HashSet};

/// A no-anonymous-individual fixture covering: class declarations and
/// `SubClassOf:`/`EquivalentTo:`/`DisjointWith:`; object property
/// declarations, `Domain:`/`Range:`/`SubPropertyOf:`/`EquivalentTo:`/
/// `DisjointWith:`/`InverseOf:`/`Characteristics:`; data property
/// declarations with the same section spread; and an `Individual:` frame
/// with `Types:`, object and data `Facts:`, `SameAs:`, `DifferentFrom:`.
const RICH_OMN: &str = r#"
Prefix: : <http://example.org/onto#>
Prefix: xsd: <http://www.w3.org/2001/XMLSchema#>
Ontology: <http://example.org/onto>

Class: Food

Class: Pizza
    SubClassOf: Food
    DisjointWith: Drink

Class: Drink

Class: Meal
    EquivalentTo: Food

ObjectProperty: hasBaseTopping

ObjectProperty: hasTopping
    Domain: Pizza
    Range: Topping
    SubPropertyOf: hasBaseTopping
    EquivalentTo: hasIngredient
    Characteristics: InverseFunctional

ObjectProperty: hasIngredient

ObjectProperty: isToppingOf
    InverseOf: hasTopping

Class: Topping

DataProperty: hasBaseAge

DataProperty: hasAge
    Domain: Person
    Range: xsd:integer
    Characteristics: Functional
    SubPropertyOf: hasBaseAge
    EquivalentTo: hasYears

DataProperty: hasYears

Class: Person

Individual: Margherita
    Types: Pizza
    Facts: hasTopping Mozzarella

Individual: Mozzarella
    Types: Topping

Individual: Alice
    Types: Person
    Facts: hasAge "30"^^xsd:integer
    SameAs: Alicia
    DifferentFrom: Bob

Individual: Alicia
Individual: Bob
"#;

/// A single anonymous individual, used only in a `Types:` section — the
/// minimal fixture exercising [`AnonRenumberer`]'s canonicalisation path.
const ANON_OMN: &str = r#"
Prefix: : <http://example.org/onto#>
Ontology: <http://example.org/onto>

Class: Person

Individual: _:x
    Types: Person
"#;

/// Three anonymous individuals (`_:a`, `_:b`, `_:c`) spread across several
/// axiom types — `Types:`, object `Facts:` (anon-to-anon *and* anon-to-named),
/// `SameAs:`, `DifferentFrom:` — exercising [`AnonRenumberer`]'s "stable
/// renumbering keyed by first appearance" logic with more than one anonymous
/// individual to actually renumber.
///
/// The declaration order of the individual *frames* is deliberately not the
/// same as the order the individuals are first *referenced* in the axiom
/// stream: `Carol`'s frame (the very first frame) references `_:a` in its
/// `Facts:` section before `_:b`'s own frame (which appears next) or `_:a`'s
/// own frame (which appears after `_:b`'s) do. So by first-occurrence-in-the-
/// axiom-list order — the order [`AnonRenumberer`] canonicalises by — `_:a`
/// is seen before `_:b`, even though `_:b`'s own `Individual:` frame precedes
/// `_:a`'s in the document text. A renumbering bug keyed off frame
/// declaration order (rather than true first occurrence) would not be caught
/// by a fixture where the two orders coincide; this fixture is built so they
/// don't.
const ANON_MULTI_OMN: &str = r#"
Prefix: : <http://example.org/onto#>
Ontology: <http://example.org/onto>

Class: Person
Class: Place

ObjectProperty: knows
ObjectProperty: livesIn

Individual: Earth
    Types: Place

Individual: Carol
    Types: Person
    Facts: knows _:a

Individual: _:b
    Types: Person
    Facts: knows _:a, livesIn Earth

Individual: _:a
    Types: Person
    SameAs: _:c
    DifferentFrom: Carol

Individual: _:c
    Types: Person
"#;

/// Run the full 7-step pipeline described in the module doc comment, and
/// return the round-tripped Manchester Syntax text.
///
/// `allowed_skips` lists the exact `RdfTranslationReport::skipped` messages
/// this fixture is expected to produce (e.g. the by-design "no RDF
/// declaration for an anonymous individual" gap for [`ANON_OMN`], see
/// `full_pipeline_roundtrips_single_anonymous_individual`'s doc comment) —
/// any other skip means the fixture drifted onto a construct `owl2rdf`
/// doesn't translate, which would make this a test of today's `owl2rdf` gaps
/// rather than of round-trip fidelity.
fn roundtrip(omn: &str, allowed_skips: &[&str]) -> String {
    let ontology = manchester_parser::parse(omn).unwrap_or_else(|e| panic!("parse failed: {e}"));

    let mut ds = Datastore::new(1_000);
    let report = owl2rdf(&mut ds, &ontology);
    let unexpected: Vec<&String> = report
        .skipped
        .iter()
        .filter(|s| !allowed_skips.contains(&s.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "fixture must only use axioms owl2rdf actually translates (beyond \
         `allowed_skips`); unexpected skips: {unexpected:?}"
    );

    let turtle_text = turtle::serialize_graph(&ds, DEFAULT_GRAPH_ELEMENT_ID);

    let mut reparsed_ds = Datastore::new(1_000);
    turtle::parse_turtle(&mut reparsed_ds, turtle_text.as_bytes())
        .unwrap_or_else(|e| panic!("re-parsing emitted Turtle failed: {e}\n---\n{turtle_text}"));

    let recovered_ontology = rdf2owl(&mut reparsed_ds)
        .unwrap_or_else(|e| panic!("rdf2owl failed: {e:?}"))
        .ontology;

    manchester_parser::serialize(&recovered_ontology)
}

/// Renumbers `Individual::AnonymousIndividual` ids by first-occurrence order
/// within an axiom list ("stable renumbering keyed by first appearance", as
/// suggested by [#179](https://github.com/daghovland/rdf-datalog/issues/179)),
/// so that two structurally-equivalent axiom sets compare equal even if the
/// raw anonymous-individual/blank-node ids differ. A minimal recursive walk
/// over every axiom shape that can mention an `Individual` (see the module
/// doc comment for why this is a small local walk rather than a reuse of
/// `rdf_canon`'s quad-shaped canonicalisation).
#[derive(Default)]
struct AnonRenumberer {
    map: HashMap<u32, u32>,
    next: u32,
}

impl AnonRenumberer {
    fn individual(&mut self, ind: &Individual) -> Individual {
        match ind {
            Individual::NamedIndividual(iri) => Individual::NamedIndividual(iri.clone()),
            Individual::AnonymousIndividual(id) => {
                let next = &mut self.next;
                let new_id = *self.map.entry(*id).or_insert_with(|| {
                    let assigned = *next;
                    *next += 1;
                    assigned
                });
                Individual::AnonymousIndividual(new_id)
            }
        }
    }

    fn annotation(&mut self, (prop, value): &Annotation) -> Annotation {
        let value = match value {
            AnnotationValue::IndividualAnnotation(ind) => {
                AnnotationValue::IndividualAnnotation(self.individual(ind))
            }
            other => other.clone(),
        };
        (prop.clone(), value)
    }

    fn annotations(&mut self, anns: &[Annotation]) -> Vec<Annotation> {
        anns.iter().map(|a| self.annotation(a)).collect()
    }

    fn class_expr(&mut self, ce: &ClassExpression) -> ClassExpression {
        use ClassExpression::*;
        match ce {
            ObjectOneOf(inds) => ObjectOneOf(inds.iter().map(|i| self.individual(i)).collect()),
            ObjectHasValue(p, ind) => ObjectHasValue(p.clone(), self.individual(ind)),
            ObjectUnionOf(list) => ObjectUnionOf(list.iter().map(|c| self.class_expr(c)).collect()),
            ObjectIntersectionOf(list) => {
                ObjectIntersectionOf(list.iter().map(|c| self.class_expr(c)).collect())
            }
            ObjectComplementOf(inner) => ObjectComplementOf(Box::new(self.class_expr(inner))),
            ObjectSomeValuesFrom(p, f) => {
                ObjectSomeValuesFrom(p.clone(), Box::new(self.class_expr(f)))
            }
            ObjectAllValuesFrom(p, f) => {
                ObjectAllValuesFrom(p.clone(), Box::new(self.class_expr(f)))
            }
            ObjectMinQualifiedCardinality(n, p, f) => {
                ObjectMinQualifiedCardinality(n.clone(), p.clone(), Box::new(self.class_expr(f)))
            }
            ObjectMaxQualifiedCardinality(n, p, f) => {
                ObjectMaxQualifiedCardinality(n.clone(), p.clone(), Box::new(self.class_expr(f)))
            }
            ObjectExactQualifiedCardinality(n, p, f) => {
                ObjectExactQualifiedCardinality(n.clone(), p.clone(), Box::new(self.class_expr(f)))
            }
            other => other.clone(),
        }
    }

    fn entity(&mut self, e: &Entity) -> Entity {
        match e {
            Entity::NamedIndividualDeclaration(ind) => {
                Entity::NamedIndividualDeclaration(self.individual(ind))
            }
            other => other.clone(),
        }
    }

    fn assertion(&mut self, a: &Assertion) -> Assertion {
        use Assertion::*;
        match a {
            SameIndividual(anns, list) => SameIndividual(
                self.annotations(anns),
                list.iter().map(|i| self.individual(i)).collect(),
            ),
            DifferentIndividuals(anns, list) => DifferentIndividuals(
                self.annotations(anns),
                list.iter().map(|i| self.individual(i)).collect(),
            ),
            ClassAssertion(anns, c, ind) => ClassAssertion(
                self.annotations(anns),
                self.class_expr(c),
                self.individual(ind),
            ),
            ObjectPropertyAssertion(anns, p, i1, i2) => ObjectPropertyAssertion(
                self.annotations(anns),
                p.clone(),
                self.individual(i1),
                self.individual(i2),
            ),
            NegativeObjectPropertyAssertion(anns, p, i1, i2) => NegativeObjectPropertyAssertion(
                self.annotations(anns),
                p.clone(),
                self.individual(i1),
                self.individual(i2),
            ),
            DataPropertyAssertion(anns, p, ind, lit) => DataPropertyAssertion(
                self.annotations(anns),
                p.clone(),
                self.individual(ind),
                lit.clone(),
            ),
            NegativeDataPropertyAssertion(anns, p, ind, lit) => NegativeDataPropertyAssertion(
                self.annotations(anns),
                p.clone(),
                self.individual(ind),
                lit.clone(),
            ),
        }
    }

    fn class_axiom(&mut self, a: &ClassAxiom) -> ClassAxiom {
        use ClassAxiom::*;
        match a {
            SubClassOf(anns, lhs, rhs) => SubClassOf(
                self.annotations(anns),
                self.class_expr(lhs),
                self.class_expr(rhs),
            ),
            EquivalentClasses(anns, list) => EquivalentClasses(
                self.annotations(anns),
                list.iter().map(|c| self.class_expr(c)).collect(),
            ),
            DisjointClasses(anns, list) => DisjointClasses(
                self.annotations(anns),
                list.iter().map(|c| self.class_expr(c)).collect(),
            ),
            DisjointUnion(anns, c, list) => DisjointUnion(
                self.annotations(anns),
                c.clone(),
                list.iter().map(|e| self.class_expr(e)).collect(),
            ),
        }
    }

    fn axiom(&mut self, ax: &Axiom) -> Axiom {
        use Axiom::*;
        match ax {
            AxiomDeclaration((anns, e)) => {
                AxiomDeclaration((self.annotations(anns), self.entity(e)))
            }
            AxiomClassAxiom(a) => AxiomClassAxiom(self.class_axiom(a)),
            // Object/data property axioms and `AxiomHasKey` never mention an
            // `Individual` in this crate's `ClassExpression`/property-axiom
            // shapes except through `Vec<Annotation>` and (for `HasKey`) a
            // `ClassExpression`, both handled generically below.
            AxiomObjectPropertyAxiom(a) => AxiomObjectPropertyAxiom(match a {
                owl_ontology::ObjectPropertyAxiom::ObjectPropertyDomain(p, c) => {
                    owl_ontology::ObjectPropertyAxiom::ObjectPropertyDomain(
                        p.clone(),
                        self.class_expr(c),
                    )
                }
                owl_ontology::ObjectPropertyAxiom::ObjectPropertyRange(p, c) => {
                    owl_ontology::ObjectPropertyAxiom::ObjectPropertyRange(
                        p.clone(),
                        self.class_expr(c),
                    )
                }
                other => other.clone(),
            }),
            AxiomDataPropertyAxiom(a) => AxiomDataPropertyAxiom(match a {
                owl_ontology::DataPropertyAxiom::DataPropertyDomain(anns, p, c) => {
                    owl_ontology::DataPropertyAxiom::DataPropertyDomain(
                        self.annotations(anns),
                        p.clone(),
                        self.class_expr(c),
                    )
                }
                other => other.clone(),
            }),
            AxiomDatatypeDefinition(anns, d, dr) => {
                AxiomDatatypeDefinition(self.annotations(anns), d.clone(), dr.clone())
            }
            AxiomHasKey(anns, c, ops, dps) => AxiomHasKey(
                self.annotations(anns),
                self.class_expr(c),
                ops.clone(),
                dps.clone(),
            ),
            AxiomAssertion(a) => AxiomAssertion(self.assertion(a)),
            AxiomAnnotationAxiom(a) => AxiomAnnotationAxiom(a.clone()),
        }
    }

    /// Canonicalise every axiom in `axioms`, in order (order matters: it
    /// determines "first occurrence").
    fn renumber(axioms: &[Axiom]) -> Vec<Axiom> {
        let mut r = Self::default();
        axioms.iter().map(|a| r.axiom(a)).collect()
    }
}

fn axiom_set(omn: &str) -> HashSet<Axiom> {
    let ontology = manchester_parser::parse(omn).unwrap_or_else(|e| panic!("parse failed: {e}"));
    AnonRenumberer::renumber(&ontology.axioms)
        .into_iter()
        .collect()
}

/// The full 7-step pipeline must preserve the axiom set of a
/// no-anonymous-individual Manchester document exercising a broad spread of
/// constructs `owl2rdf` translates: class hierarchy (`SubClassOf:`,
/// `EquivalentTo:`, `DisjointWith:`), object/data property frames
/// (`Domain:`/`Range:`/`SubPropertyOf:`/`EquivalentTo:`/`InverseOf:`/
/// `Characteristics:`), and ABox assertions (`Types:`, object/data `Facts:`,
/// `SameAs:`/`DifferentFrom:`).
#[test]
fn full_pipeline_roundtrips_rich_tbox_and_abox() {
    let roundtripped = roundtrip(RICH_OMN, &[]);

    let original = axiom_set(RICH_OMN);
    let round_tripped = axiom_set(&roundtripped);

    assert_eq!(
        original, round_tripped,
        "axiom sets differ after Manchester -> RDF -> Manchester round trip; \
         round-tripped text was:\n{roundtripped}"
    );
}

/// A single anonymous individual survives the full pipeline: `Types:
/// Person` on `_:x` must come back as a `ClassAssertion` on *some* anonymous
/// individual, once ids are canonicalised by first appearance.
///
/// `owl2rdf` does not translate the bare *declaration* of an anonymous
/// individual (`Entity::NamedIndividualDeclaration(AnonymousIndividual)`) —
/// RDF has no declaration triple for a blank node either, per the OWL 2
/// RDF mapping spec (declarations are for IRIs), so this is not a bug: the
/// declaration axiom is dropped by design, and only the ABox
/// `ClassAssertion` about the individual round-trips. The two axiom sets
/// are therefore compared after removing declaration axioms from both
/// sides, isolating the check to what the pipeline is actually meant to
/// preserve.
#[test]
fn full_pipeline_roundtrips_single_anonymous_individual() {
    let roundtripped = roundtrip(
        ANON_OMN,
        &["declaration of anonymous individual: AnonymousIndividual(0)"],
    );

    let without_individual_decls = |set: HashSet<Axiom>| -> HashSet<Axiom> {
        set.into_iter()
            .filter(|a| {
                !matches!(
                    a,
                    Axiom::AxiomDeclaration((_, Entity::NamedIndividualDeclaration(_)))
                )
            })
            .collect()
    };

    let original = without_individual_decls(axiom_set(ANON_OMN));
    let round_tripped = without_individual_decls(axiom_set(&roundtripped));

    assert_eq!(
        original, round_tripped,
        "axiom sets (excluding individual declarations) differ after \
         Manchester -> RDF -> Manchester round trip; round-tripped text was:\n{roundtripped}"
    );
    assert!(
        original
            .iter()
            .any(|a| matches!(a, Axiom::AxiomAssertion(Assertion::ClassAssertion(_, _, _)))),
        "sanity check: the fixture's Types: assertion must still be present"
    );
}

/// Three anonymous individuals, linked to each other and to named
/// individuals via `Types:`, object `Facts:`, `SameAs:` and
/// `DifferentFrom:`, survive the full pipeline.
///
/// This test was originally written to exercise [`AnonRenumberer`]'s
/// renumbering *order* directly (comparing axiom sets the same way
/// [`full_pipeline_roundtrips_single_anonymous_individual`] does), using a
/// fixture ([`ANON_MULTI_OMN`]) deliberately built so frame-declaration
/// order and first-reference order diverge. That approach turned out to be
/// unsound: `AnonRenumberer` canonicalises by first *occurrence* in the
/// `Vec<Axiom>` it walks, but Manchester frame order carries no semantics
/// and is not preserved by `manchester_parser::serialize` — the
/// round-tripped text legitimately emits `_:b1`'s frame before `Earth`'s and
/// `Carol`'s, which flips which of the *other* two anonymous individuals is
/// "first occurring" relative to the original text, even though the
/// document is semantically identical. First-occurrence renumbering is
/// therefore not a graph-isomorphism invariant once more than one
/// anonymous individual's relative order can shuffle across the round
/// trip — exactly the scenario this fixture was built to exercise, which is
/// how the unsoundness surfaced instead of the bug it was meant to catch.
///
/// So, like [`rdf_starting_roundtrip_preserves_graph_isomorphism`], this
/// test compares via `rdf_canon`'s RDFC-1.0 canonicalisation over the RDF
/// quads each ontology translates to, rather than via [`AnonRenumberer`]'s
/// axiom-tree walk. `AnonRenumberer` remains correct (and is kept) for
/// [`full_pipeline_roundtrips_single_anonymous_individual`], where a single
/// anonymous individual has no ordering ambiguity to exploit, and for
/// [`full_pipeline_roundtrips_rich_tbox_and_abox`], which has none at all.
#[test]
fn full_pipeline_roundtrips_multiple_anonymous_individuals() {
    let allowed_skips = [
        "declaration of anonymous individual: AnonymousIndividual(0)",
        "declaration of anonymous individual: AnonymousIndividual(1)",
        "declaration of anonymous individual: AnonymousIndividual(2)",
    ];
    let roundtripped = roundtrip(ANON_MULTI_OMN, &allowed_skips);

    // Sanity checks: the constructs this fixture specifically targets must
    // actually be present in the original ontology, so the canonical-RDF
    // comparison below isn't vacuously true because the fixture drifted.
    let original_ontology =
        manchester_parser::parse(ANON_MULTI_OMN).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let original_axioms: HashSet<Axiom> = original_ontology.axioms.iter().cloned().collect();

    let class_assertions = original_axioms
        .iter()
        .filter(|a| matches!(a, Axiom::AxiomAssertion(Assertion::ClassAssertion(_, _, _))))
        .count();
    assert_eq!(
        class_assertions, 5,
        "expected one ClassAssertion per individual (Earth, Carol, _:a, _:b, _:c)"
    );
    assert!(
        original_axioms.iter().any(|a| matches!(
            a,
            Axiom::AxiomAssertion(Assertion::ObjectPropertyAssertion(
                _,
                _,
                Individual::AnonymousIndividual(_),
                Individual::AnonymousIndividual(_)
            ))
        )),
        "sanity check: an anon-to-anon object property Facts: assertion must survive"
    );
    assert!(
        original_axioms
            .iter()
            .any(|a| matches!(a, Axiom::AxiomAssertion(Assertion::SameIndividual(_, _)))),
        "sanity check: the SameAs: assertion must survive"
    );
    assert!(
        original_axioms.iter().any(|a| matches!(
            a,
            Axiom::AxiomAssertion(Assertion::DifferentIndividuals(_, _))
        )),
        "sanity check: the DifferentFrom: assertion must survive"
    );

    // Canonical-RDF comparison: translate both the original and the
    // round-tripped ontology to RDF quads and compare RDFC-1.0 canonical
    // N-Quads, which is blank-node-identity-aware and does not depend on
    // Manchester frame order at all.
    let mut original_ds = Datastore::new(1_000);
    let original_report = owl2rdf(&mut original_ds, &original_ontology);
    let unexpected_original: Vec<&String> = original_report
        .skipped
        .iter()
        .filter(|s| !allowed_skips.contains(&s.as_str()))
        .collect();
    assert!(
        unexpected_original.is_empty(),
        "unexpected owl2rdf skips translating the original ontology: {unexpected_original:?}"
    );

    let roundtripped_ontology = manchester_parser::parse(&roundtripped)
        .unwrap_or_else(|e| panic!("re-parsing round-tripped text failed: {e}"));
    let mut roundtripped_ds = Datastore::new(1_000);
    let roundtripped_report = owl2rdf(&mut roundtripped_ds, &roundtripped_ontology);
    let unexpected_roundtripped: Vec<&String> = roundtripped_report
        .skipped
        .iter()
        .filter(|s| !allowed_skips.contains(&s.as_str()))
        .collect();
    assert!(
        unexpected_roundtripped.is_empty(),
        "unexpected owl2rdf skips translating the round-tripped ontology: \
         {unexpected_roundtripped:?}"
    );

    let original_canon = rdf_canon::canonicalize_dataset(&original_ds)
        .unwrap_or_else(|e| panic!("canonicalization of original dataset failed: {e}"));
    let roundtripped_canon = rdf_canon::canonicalize_dataset(&roundtripped_ds)
        .unwrap_or_else(|e| panic!("canonicalization of round-tripped dataset failed: {e}"));

    assert_eq!(
        original_canon, roundtripped_canon,
        "canonical N-Quads differ after Manchester -> RDF -> Manchester round trip;\n\
         round-tripped Manchester text was:\n{roundtripped}\n\
         --- original canonical N-Quads ---\n{original_canon}\
         --- round-tripped canonical N-Quads ---\n{roundtripped_canon}"
    );
}

/// A second, structurally different round trip: instead of starting from
/// Manchester Syntax text, this starts from hand-written Turtle/RDF
/// (exercising `rdf2owl`'s robustness against real Turtle-authored graphs,
/// which have no Manchester-parser stage in between) and compares RDF graphs
/// directly using `rdf_canon`'s blank-node-aware canonicalisation
/// ([`rdf_canon::canonicalize_dataset`]) rather than [`AnonRenumberer`]'s
/// axiom-tree walk.
///
/// Pipeline: Turtle (`turtle::parse_turtle`) -> `Datastore` -> `Ontology`
/// (`rdf_owl_translator::rdf2owl`) -> `Datastore` (`owl2rl2datalog::owl2rdf`)
/// -> compare against the original `Datastore` via RDFC-1.0 canonical
/// N-Quads equality. This is a genuinely different failure surface than the
/// Manchester-starting tests above: it validates the RDF -> RDF leg of the
/// pipeline directly (through `Ontology` and back) rather than comparing
/// `Ontology` axiom sets, and it exercises `rdf2owl` against triples that
/// never passed through the Manchester parser's own id-assignment or
/// serializer.
///
/// `rdf_canon` turned out to be a materially simpler fit for this direction
/// than [`AnonRenumberer`] is for the Manchester-starting direction: RDFC-1.0
/// canonicalises blank-node identity across an entire dataset for free
/// (Dag's hunch, confirmed) — no bespoke first-occurrence walk over a
/// bespoke tree shape is needed, because the comparison never leaves the RDF
/// quad representation `rdf_canon` already understands. The catch is the
/// flip side of that: `rdf_canon` demands the *whole* graph line up
/// (including every declaration triple, e.g. `owl:Class`/`owl:NamedIndividual`
/// typing), so the fixture below is deliberately restricted to constructs
/// that survive `owl2rdf` losslessly — notably, no `owl:NamedIndividual`
/// typing triple on the blank node, since `owl2rdf` intentionally never
/// re-emits one for an anonymous individual (see
/// `full_pipeline_roundtrips_single_anonymous_individual`'s doc comment).
#[test]
fn rdf_starting_roundtrip_preserves_graph_isomorphism() {
    const TURTLE_FIXTURE: &str = r#"
@prefix : <http://example.org/onto#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

:Person a owl:Class .
:Place a owl:Class .
:knows a owl:ObjectProperty .
:livesIn a owl:ObjectProperty .

:Alice a owl:NamedIndividual, :Person ;
    :knows _:b1 .

_:b1 a :Person ;
    :livesIn :Earth .

:Earth a owl:NamedIndividual, :Place .
"#;

    let mut original_ds = Datastore::new(1_000);
    turtle::parse_turtle(&mut original_ds, TURTLE_FIXTURE.as_bytes())
        .unwrap_or_else(|e| panic!("parsing Turtle fixture failed: {e}"));

    let ontology = rdf2owl(&mut original_ds)
        .unwrap_or_else(|e| panic!("rdf2owl failed: {e:?}"))
        .ontology;

    let mut round_tripped_ds = Datastore::new(1_000);
    let report = owl2rdf(&mut round_tripped_ds, &ontology);
    assert!(
        report.skipped.is_empty(),
        "fixture must only use axioms owl2rdf actually translates; unexpected skips: {:?}",
        report.skipped
    );

    let original_canon = rdf_canon::canonicalize_dataset(&original_ds)
        .unwrap_or_else(|e| panic!("canonicalization of original dataset failed: {e}"));
    let round_tripped_canon = rdf_canon::canonicalize_dataset(&round_tripped_ds)
        .unwrap_or_else(|e| panic!("canonicalization of round-tripped dataset failed: {e}"));

    assert_eq!(
        original_canon, round_tripped_canon,
        "canonical N-Quads differ after Turtle -> Ontology -> Turtle round trip;\n\
         --- original ---\n{original_canon}--- round-tripped ---\n{round_tripped_canon}"
    );
}

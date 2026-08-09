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
//! Two fixtures are used:
//! - [`RICH_OMN`] exercises a broad spread of Manchester constructs that
//!   `owl2rdf`'s current scope actually translates (named-class TBox axioms,
//!   object/data property frames and characteristics, ABox assertions) with
//!   no anonymous individuals, so plain (non-canonicalising) axiom-set
//!   equality applies.
//! - [`ANON_OMN`] adds a single anonymous individual to specifically exercise
//!   the canonicalisation path.
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

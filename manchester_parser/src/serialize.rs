/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serialize an [`owl_ontology::Ontology`] to OWL 2 Manchester Syntax text
//! (the reverse direction of [`crate::parse`]).
//!
//! Follow-up to the parser (issue
//! [#139](https://github.com/daghovland/rdf-datalog/issues/139)/PR
//! [#158](https://github.com/daghovland/rdf-datalog/pull/158)), tracked in
//! [#160](https://github.com/daghovland/rdf-datalog/issues/160). Mirrors the
//! parser's scope: see `docs/plans/MANCHESTER_SYNTAX_PLAN.md`'s "Serialiser"
//! section.
//!
//! ## Design
//!
//! - **No `Prefix:` declarations.** `Ontology` carries no prefix map (see the
//!   plan doc), so every IRI is emitted in full form (`<...>`), which the
//!   parser's `iri` production accepts in every position a Manchester IRI can
//!   appear. This sidesteps inventing a prefix-shortening scheme entirely.
//! - **One frame per entity**, not per axiom: axioms are grouped by their
//!   "natural" frame subject (the class/property/individual a section is
//!   about) in first-occurrence order, then each group is emitted as a single
//!   frame with one section line per axiom. Grouping (rather than one frame
//!   per axiom) matters for round-tripping declaration annotations: the
//!   parser folds every `Annotations:` section inside a frame into that
//!   entity's *single* `AxiomDeclaration`, so emitting the same entity's
//!   declaration annotations and its other axioms as separate same-named
//!   frames would reparse into two distinct (and unequal) declaration axioms.
//! - **Top-level `misc` forms** (`EquivalentClasses:`, `DisjointClasses:`,
//!   `SameIndividual:`, `DifferentIndividuals:`, `EquivalentProperties:`,
//!   `DisjointProperties:`) are used for n-ary axioms with more than two
//!   members, and as a fallback for binary axioms whose members can't serve
//!   as a frame subject (e.g. an `inverse P` object property expression).
//! - **Out-of-scope constructs are skipped with a `log::warn!`,  never
//!   silently emitted as invalid syntax.** This covers everything deferred by
//!   [#157](https://github.com/daghovland/rdf-datalog/issues/157)
//!   (`DisjointUnionOf:`, `HasKey:`, property chains, compound data ranges,
//!   the `Datatype:` frame) plus a few gaps specific to serialisation
//!   (standalone `AnnotationAssertion` axioms about an arbitrary subject: the
//!   frame grammar only lets `Annotations:` attach to a frame's own entity
//!   declaration, so an assertion about an unrelated subject has no frame
//!   form to serialize into).
//!
//! Anonymous individuals (`Individual::AnonymousIndividual(u32)`) are
//! serialized as `_:b<id>`; since ids are assigned by first-occurrence order
//! during parsing and this serializer preserves axiom order when building
//! frame groups, round-tripping a document with a single anonymous individual
//! reproduces the same id. Documents with several anonymous individuals are
//! not guaranteed to reproduce the *same* ids after a round-trip (grouping by
//! entity can reorder which `_:bN` label is seen first), only equivalent
//! *structure* once ids are considered abstractly — see the round-trip test
//! file for how this is scoped down to single-anonymous-individual fixtures.

use owl_ontology::{
    Annotation, AnnotationAxiom, AnnotationValue, Assertion, ClassAxiom, ClassExpression,
    DataPropertyAxiom, DataRange, Entity, FullIri, Individual, ObjectPropertyAxiom,
    ObjectPropertyExpression, Ontology, SubPropertyExpression,
};
use std::collections::HashMap;

/// Serialize `ontology` to OWL 2 Manchester Syntax text.
///
/// Only `ontology.axioms` is serialized (not `ontology.all_axioms()`'s
/// built-in `owl:Thing`/`xsd:integer`/... declarations, which are implicit
/// and never need restating).
pub fn serialize(ontology: &Ontology) -> String {
    let mut out = String::new();
    emit_header(&mut out, ontology);

    let mut order: Vec<FrameKey> = Vec::new();
    let mut frames: HashMap<FrameKey, FrameBody> = HashMap::new();
    let mut misc_lines: Vec<String> = Vec::new();

    for axiom in &ontology.axioms {
        match classify(axiom) {
            Some(Emission::FrameLine(key, line)) => {
                ensure_frame(&mut order, &mut frames, key.clone());
                frames.get_mut(&key).expect("just ensured").lines.push(line);
            }
            Some(Emission::FrameOnly(key)) => {
                ensure_frame(&mut order, &mut frames, key);
            }
            Some(Emission::FrameAnnotations(key, anns)) => {
                ensure_frame(&mut order, &mut frames, key.clone());
                frames
                    .get_mut(&key)
                    .expect("just ensured")
                    .decl_annotations
                    .extend(anns);
            }
            Some(Emission::Misc(line)) => misc_lines.push(line),
            None => {
                // `classify` (or one of its helpers) already logged a
                // `log::warn!` explaining why this axiom was skipped.
            }
        }
    }

    for key in &order {
        emit_frame(&mut out, key, &frames[key]);
    }
    for line in &misc_lines {
        out.push_str(line);
    }
    out
}

fn log_skip(reason: &str) {
    log::warn!("manchester_parser::serialize: skipping unsupported axiom: {reason}");
}

// ── Frame grouping ───────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Eq, Hash)]
enum IndividualKey {
    Named(String),
    Anon(u32),
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum FrameKey {
    Class(String),
    ObjectProperty(String),
    DataProperty(String),
    AnnotationProperty(String),
    Individual(IndividualKey),
}

#[derive(Default)]
struct FrameBody {
    decl_annotations: Vec<Annotation>,
    /// Fully-formatted `"    Section: ...\n"` lines, one per axiom.
    lines: Vec<String>,
}

enum Emission {
    /// Ensure the frame exists and append a section line to its body.
    FrameLine(FrameKey, String),
    /// Ensure the frame exists (a bare declaration with no annotations).
    FrameOnly(FrameKey),
    /// Ensure the frame exists and fold annotations into its declaration.
    FrameAnnotations(FrameKey, Vec<Annotation>),
    /// A top-level `misc` line (already `"Kw: ...\n"`-terminated).
    Misc(String),
}

fn ensure_frame(
    order: &mut Vec<FrameKey>,
    frames: &mut HashMap<FrameKey, FrameBody>,
    key: FrameKey,
) {
    use std::collections::hash_map::Entry;
    if let Entry::Vacant(entry) = frames.entry(key.clone()) {
        order.push(key);
        entry.insert(FrameBody::default());
    }
}

fn emit_frame(out: &mut String, key: &FrameKey, body: &FrameBody) {
    match key {
        FrameKey::Class(iri) => out.push_str(&format!("Class: <{iri}>\n")),
        FrameKey::ObjectProperty(iri) => out.push_str(&format!("ObjectProperty: <{iri}>\n")),
        FrameKey::DataProperty(iri) => out.push_str(&format!("DataProperty: <{iri}>\n")),
        FrameKey::AnnotationProperty(iri) => {
            out.push_str(&format!("AnnotationProperty: <{iri}>\n"))
        }
        FrameKey::Individual(k) => {
            out.push_str(&format!("Individual: {}\n", fmt_individual_key(k)))
        }
    }
    if !body.decl_annotations.is_empty()
        && let Some(s) = fmt_annotation_list(&body.decl_annotations)
    {
        out.push_str(&format!("    Annotations: {s}\n"));
    }
    for line in &body.lines {
        out.push_str(line);
    }
    out.push('\n');
}

fn emit_header(out: &mut String, o: &Ontology) {
    out.push_str("Ontology:");
    match &o.version {
        ingress::OntologyVersion::UnNamedOntology => {}
        ingress::OntologyVersion::NamedOntology(iri) => {
            out.push_str(&format!(" <{}>", iri.0));
        }
        ingress::OntologyVersion::VersionedOntology {
            ontology_iri,
            version_iri,
        } => {
            out.push_str(&format!(" <{}> <{}>", ontology_iri.0, version_iri.0));
        }
    }
    out.push('\n');
    for imp in &o.directly_imports_documents {
        out.push_str(&format!("Import: <{}>\n", imp.0));
    }
    if !o.annotations.is_empty() {
        if let Some(s) = fmt_annotation_list(&o.annotations) {
            out.push_str(&format!("Annotations: {s}\n"));
        } else {
            log::warn!(
                "manchester_parser::serialize: skipping ontology-level Annotations: (unsupported annotation value)"
            );
        }
    }
    out.push('\n');
}

// ── Axiom classification ─────────────────────────────────────────────────

fn classify(axiom: &owl_ontology::Axiom) -> Option<Emission> {
    use owl_ontology::Axiom::*;
    match axiom {
        AxiomDeclaration((anns, entity)) => classify_declaration(anns, entity),
        AxiomClassAxiom(a) => classify_class_axiom(a),
        AxiomObjectPropertyAxiom(a) => classify_object_property_axiom(a),
        AxiomDataPropertyAxiom(a) => classify_data_property_axiom(a),
        AxiomDatatypeDefinition(..) => {
            log_skip("Datatype: frame / DatatypeDefinition (#157, compound data ranges)");
            None
        }
        AxiomHasKey(..) => {
            log_skip("HasKey: (#157)");
            None
        }
        AxiomAssertion(a) => classify_assertion(a),
        AxiomAnnotationAxiom(a) => classify_annotation_axiom(a),
    }
}

fn classify_declaration(anns: &[Annotation], entity: &Entity) -> Option<Emission> {
    match entity {
        Entity::ClassDeclaration(iri) => {
            Some(decl_emission(FrameKey::Class(iri.0.0.clone()), anns))
        }
        Entity::ObjectPropertyDeclaration(iri) => Some(decl_emission(
            FrameKey::ObjectProperty(iri.0.0.clone()),
            anns,
        )),
        Entity::DataPropertyDeclaration(iri) => {
            Some(decl_emission(FrameKey::DataProperty(iri.0.0.clone()), anns))
        }
        Entity::AnnotationPropertyDeclaration(iri) => Some(decl_emission(
            FrameKey::AnnotationProperty(iri.0.0.clone()),
            anns,
        )),
        Entity::NamedIndividualDeclaration(ind) => Some(decl_emission(
            FrameKey::Individual(individual_key(ind)),
            anns,
        )),
        Entity::DatatypeDeclaration(_) => {
            log_skip("Datatype: frame (#157)");
            None
        }
    }
}

fn decl_emission(key: FrameKey, anns: &[Annotation]) -> Emission {
    if anns.is_empty() {
        Emission::FrameOnly(key)
    } else {
        Emission::FrameAnnotations(key, anns.to_vec())
    }
}

fn classify_class_axiom(a: &ClassAxiom) -> Option<Emission> {
    match a {
        ClassAxiom::SubClassOf(anns, lhs, rhs) => {
            let ClassExpression::ClassName(lhs_iri) = lhs else {
                log_skip(
                    "SubClassOf: with a non-atomic subject (no `Class:` frame header to attach it to)",
                );
                return None;
            };
            let rhs_s = fmt_class_expr(rhs)?;
            let ann = ann_prefix(anns)?;
            let line = format!("    SubClassOf: {ann}{rhs_s}\n");
            Some(Emission::FrameLine(
                FrameKey::Class(lhs_iri.0.0.clone()),
                line,
            ))
        }
        ClassAxiom::EquivalentClasses(anns, list) => {
            class_nary(anns, list, "EquivalentTo", "EquivalentClasses")
        }
        ClassAxiom::DisjointClasses(anns, list) => {
            class_nary(anns, list, "DisjointWith", "DisjointClasses")
        }
        ClassAxiom::DisjointUnion(..) => {
            log_skip("DisjointUnionOf: (#157)");
            None
        }
    }
}

/// `EquivalentClasses`/`DisjointClasses`: prefer the two-element `Class:`
/// frame section form (trying either element as the atomic frame subject,
/// since the relation is symmetric); fall back to the top-level `misc` form
/// otherwise (always used for genuinely n-ary lists).
fn class_nary(
    anns: &[Annotation],
    list: &[ClassExpression],
    frame_kw: &str,
    misc_kw: &str,
) -> Option<Emission> {
    if list.len() == 2 {
        for (subj, val) in [(&list[0], &list[1]), (&list[1], &list[0])] {
            if let ClassExpression::ClassName(iri) = subj
                && let Some(rhs) = fmt_class_expr(val)
                && let Some(ann) = ann_prefix(anns)
            {
                let line = format!("    {frame_kw}: {ann}{rhs}\n");
                return Some(Emission::FrameLine(FrameKey::Class(iri.0.0.clone()), line));
            }
        }
    }
    let items: Option<Vec<String>> = list.iter().map(fmt_class_expr).collect();
    let items = items?;
    if items.len() < 2 {
        return None;
    }
    let ann = ann_prefix(anns)?;
    Some(Emission::Misc(format!(
        "{misc_kw}: {ann}{}\n",
        items.join(", ")
    )))
}

fn classify_object_property_axiom(a: &ObjectPropertyAxiom) -> Option<Emission> {
    use ObjectPropertyAxiom::*;
    match a {
        ObjectPropertyDomain(p, c) => {
            let rhs = fmt_class_expr(c)?;
            obj_prop_frame_line(p, "Domain", &[], &rhs)
        }
        ObjectPropertyRange(p, c) => {
            let rhs = fmt_class_expr(c)?;
            obj_prop_frame_line(p, "Range", &[], &rhs)
        }
        SubObjectPropertyOf(anns, sub, sup) => {
            let SubPropertyExpression::SubObjectPropertyExpression(sub_expr) = sub else {
                log_skip("SubPropertyChain: (#157)");
                return None;
            };
            let ObjectPropertyExpression::NamedObjectProperty(sub_iri) = sub_expr else {
                log_skip("SubPropertyOf: with a non-named subject");
                return None;
            };
            let sup_s = fmt_obj_prop(sup)?;
            obj_prop_frame_line_named(sub_iri, "SubPropertyOf", anns, &sup_s)
        }
        EquivalentObjectProperties(anns, list) => {
            obj_prop_nary(anns, list, "EquivalentTo", "EquivalentProperties")
        }
        DisjointObjectProperties(anns, list) => {
            obj_prop_nary(anns, list, "DisjointWith", "DisjointProperties")
        }
        InverseObjectProperties(anns, p1, p2) => {
            let ObjectPropertyExpression::NamedObjectProperty(p1_iri) = p1 else {
                log_skip("InverseOf: with a non-named subject");
                return None;
            };
            let p2s = fmt_obj_prop(p2)?;
            obj_prop_frame_line_named(p1_iri, "InverseOf", anns, &p2s)
        }
        FunctionalObjectProperty(anns, p) => obj_prop_characteristic(p, "Functional", anns),
        InverseFunctionalObjectProperty(anns, p) => {
            obj_prop_characteristic(p, "InverseFunctional", anns)
        }
        ReflexiveObjectProperty(anns, p) => obj_prop_characteristic(p, "Reflexive", anns),
        IrreflexiveObjectProperty(anns, p) => obj_prop_characteristic(p, "Irreflexive", anns),
        SymmetricObjectProperty(anns, p) => obj_prop_characteristic(p, "Symmetric", anns),
        AsymmetricObjectProperty(anns, p) => obj_prop_characteristic(p, "Asymmetric", anns),
        TransitiveObjectProperty(anns, p) => obj_prop_characteristic(p, "Transitive", anns),
    }
}

fn obj_prop_frame_line(
    p: &ObjectPropertyExpression,
    kw: &str,
    anns: &[Annotation],
    value: &str,
) -> Option<Emission> {
    let ObjectPropertyExpression::NamedObjectProperty(iri) = p else {
        log_skip(&format!("{kw}: with a non-named object property"));
        return None;
    };
    obj_prop_frame_line_named(iri, kw, anns, value)
}

fn obj_prop_frame_line_named(
    iri: &FullIri,
    kw: &str,
    anns: &[Annotation],
    value: &str,
) -> Option<Emission> {
    let ann = ann_prefix(anns)?;
    let line = format!("    {kw}: {ann}{value}\n");
    Some(Emission::FrameLine(
        FrameKey::ObjectProperty(iri.0.0.clone()),
        line,
    ))
}

fn obj_prop_characteristic(
    p: &ObjectPropertyExpression,
    kw: &str,
    anns: &[Annotation],
) -> Option<Emission> {
    let ObjectPropertyExpression::NamedObjectProperty(iri) = p else {
        log_skip(&format!(
            "Characteristics: {kw} with a non-named object property"
        ));
        return None;
    };
    let ann = ann_prefix(anns)?;
    let line = format!("    Characteristics: {ann}{kw}\n");
    Some(Emission::FrameLine(
        FrameKey::ObjectProperty(iri.0.0.clone()),
        line,
    ))
}

fn obj_prop_nary(
    anns: &[Annotation],
    list: &[ObjectPropertyExpression],
    frame_kw: &str,
    misc_kw: &str,
) -> Option<Emission> {
    if list.len() == 2 {
        for (subj, val) in [(&list[0], &list[1]), (&list[1], &list[0])] {
            if let ObjectPropertyExpression::NamedObjectProperty(iri) = subj
                && let Some(rhs) = fmt_obj_prop(val)
                && let Some(ann) = ann_prefix(anns)
            {
                let line = format!("    {frame_kw}: {ann}{rhs}\n");
                return Some(Emission::FrameLine(
                    FrameKey::ObjectProperty(iri.0.0.clone()),
                    line,
                ));
            }
        }
    }
    // The top-level `EquivalentProperties:`/`DisjointProperties:` misc form
    // only accepts a list of plain named-property IRIs (see `frame.rs`'s
    // `equivalent_or_disjoint_properties`), not general
    // `objectPropertyExpression`s (e.g. `inverse P`).
    let items: Option<Vec<String>> = list
        .iter()
        .map(|e| match e {
            ObjectPropertyExpression::NamedObjectProperty(iri) => Some(fmt_iri(iri)),
            _ => None,
        })
        .collect();
    let items = items?;
    if items.len() < 2 {
        return None;
    }
    let ann = ann_prefix(anns)?;
    Some(Emission::Misc(format!(
        "{misc_kw}: {ann}{}\n",
        items.join(", ")
    )))
}

fn classify_data_property_axiom(a: &DataPropertyAxiom) -> Option<Emission> {
    use DataPropertyAxiom::*;
    match a {
        SubDataPropertyOf(anns, sub, sup) => {
            data_prop_frame_line(sub, "SubPropertyOf", anns, &fmt_iri(sup))
        }
        EquivalentDataProperties(anns, list) => {
            data_prop_nary(anns, list, "EquivalentTo", "EquivalentProperties")
        }
        DisjointDataProperties(anns, list) => {
            data_prop_nary(anns, list, "DisjointWith", "DisjointProperties")
        }
        DataPropertyDomain(anns, p, c) => {
            let rhs = fmt_class_expr(c)?;
            data_prop_frame_line(p, "Domain", anns, &rhs)
        }
        DataPropertyRange(anns, p, dr) => {
            let rhs = fmt_data_range(dr)?;
            data_prop_frame_line(p, "Range", anns, &rhs)
        }
        FunctionalDataProperty(anns, p) => {
            let ann = ann_prefix(anns)?;
            let line = format!("    Characteristics: {ann}Functional\n");
            Some(Emission::FrameLine(
                FrameKey::DataProperty(p.0.0.clone()),
                line,
            ))
        }
    }
}

fn data_prop_frame_line(
    p: &FullIri,
    kw: &str,
    anns: &[Annotation],
    value: &str,
) -> Option<Emission> {
    let ann = ann_prefix(anns)?;
    let line = format!("    {kw}: {ann}{value}\n");
    Some(Emission::FrameLine(
        FrameKey::DataProperty(p.0.0.clone()),
        line,
    ))
}

fn data_prop_nary(
    anns: &[Annotation],
    list: &[FullIri],
    frame_kw: &str,
    misc_kw: &str,
) -> Option<Emission> {
    if list.len() == 2 {
        let rhs = fmt_iri(&list[1]);
        return data_prop_frame_line(&list[0], frame_kw, anns, &rhs);
    }
    if list.len() < 2 {
        return None;
    }
    let ann = ann_prefix(anns)?;
    let items: Vec<String> = list.iter().map(fmt_iri).collect();
    Some(Emission::Misc(format!(
        "{misc_kw}: {ann}{}\n",
        items.join(", ")
    )))
}

fn classify_assertion(a: &Assertion) -> Option<Emission> {
    use Assertion::*;
    match a {
        SameIndividual(anns, list) => individual_nary(anns, list, "SameAs", "SameIndividual"),
        DifferentIndividuals(anns, list) => {
            individual_nary(anns, list, "DifferentFrom", "DifferentIndividuals")
        }
        ClassAssertion(anns, c, ind) => {
            let cs = fmt_class_expr(c)?;
            let ann = ann_prefix(anns)?;
            let line = format!("    Types: {ann}{cs}\n");
            Some(Emission::FrameLine(
                FrameKey::Individual(individual_key(ind)),
                line,
            ))
        }
        ObjectPropertyAssertion(anns, p, i1, i2) => fact_line(anns, p, i1, i2, false),
        NegativeObjectPropertyAssertion(anns, p, i1, i2) => fact_line(anns, p, i1, i2, true),
        DataPropertyAssertion(anns, p, ind, lit) => data_fact_line(anns, p, ind, lit, false),
        NegativeDataPropertyAssertion(anns, p, ind, lit) => data_fact_line(anns, p, ind, lit, true),
    }
}

fn fact_line(
    anns: &[Annotation],
    p: &ObjectPropertyExpression,
    subj: &Individual,
    obj: &Individual,
    negated: bool,
) -> Option<Emission> {
    let ObjectPropertyExpression::NamedObjectProperty(iri) = p else {
        log_skip("Facts: with a non-named object property (the `fact` grammar takes a plain IRI)");
        return None;
    };
    let ann = ann_prefix(anns)?;
    let neg = if negated { "not " } else { "" };
    let line = format!(
        "    Facts: {ann}{neg}{} {}\n",
        fmt_iri(iri),
        fmt_individual(obj)
    );
    Some(Emission::FrameLine(
        FrameKey::Individual(individual_key(subj)),
        line,
    ))
}

fn data_fact_line(
    anns: &[Annotation],
    p: &FullIri,
    subj: &Individual,
    lit: &ingress::GraphElement,
    negated: bool,
) -> Option<Emission> {
    let lit_s = fmt_literal(lit)?;
    let ann = ann_prefix(anns)?;
    let neg = if negated { "not " } else { "" };
    let line = format!("    Facts: {ann}{neg}{} {lit_s}\n", fmt_iri(p));
    Some(Emission::FrameLine(
        FrameKey::Individual(individual_key(subj)),
        line,
    ))
}

/// `SameAs:`/`DifferentFrom:` (binary) or the top-level `SameIndividual:`/
/// `DifferentIndividuals:` misc form (n-ary). Unlike class/property nary
/// axioms, any `Individual` (named or anonymous) is a valid frame subject, so
/// there's no fallible "is this an atomic subject" check here.
fn individual_nary(
    anns: &[Annotation],
    list: &[Individual],
    frame_kw: &str,
    misc_kw: &str,
) -> Option<Emission> {
    if list.len() == 2 {
        let ann = ann_prefix(anns)?;
        let line = format!("    {frame_kw}: {ann}{}\n", fmt_individual(&list[1]));
        return Some(Emission::FrameLine(
            FrameKey::Individual(individual_key(&list[0])),
            line,
        ));
    }
    if list.len() < 2 {
        return None;
    }
    let ann = ann_prefix(anns)?;
    let items: Vec<String> = list.iter().map(fmt_individual).collect();
    Some(Emission::Misc(format!(
        "{misc_kw}: {ann}{}\n",
        items.join(", ")
    )))
}

fn classify_annotation_axiom(a: &AnnotationAxiom) -> Option<Emission> {
    use AnnotationAxiom::*;
    match a {
        AnnotationAssertion(..) => {
            log_skip(
                "AnnotationAssertion about an arbitrary subject (the frame grammar only lets \
                 `Annotations:` attach to a frame's own entity declaration)",
            );
            None
        }
        SubAnnotationPropertyOf(anns, sub, sup) => {
            ann_prop_frame_line(sub, "SubPropertyOf", anns, &fmt_iri(sup))
        }
        AnnotationPropertyDomain(anns, p, target) => {
            ann_prop_frame_line(p, "Domain", anns, &fmt_iri(target))
        }
        AnnotationPropertyRange(anns, p, target) => {
            ann_prop_frame_line(p, "Range", anns, &fmt_iri(target))
        }
    }
}

fn ann_prop_frame_line(
    p: &FullIri,
    kw: &str,
    anns: &[Annotation],
    value: &str,
) -> Option<Emission> {
    let ann = ann_prefix(anns)?;
    let line = format!("    {kw}: {ann}{value}\n");
    Some(Emission::FrameLine(
        FrameKey::AnnotationProperty(p.0.0.clone()),
        line,
    ))
}

// ── Term formatting ───────────────────────────────────────────────────────

fn fmt_iri(iri: &FullIri) -> String {
    format!("<{}>", iri.0.0)
}

fn individual_key(ind: &Individual) -> IndividualKey {
    match ind {
        Individual::NamedIndividual(iri) => IndividualKey::Named(iri.0.0.clone()),
        Individual::AnonymousIndividual(id) => IndividualKey::Anon(*id),
    }
}

fn fmt_individual_key(k: &IndividualKey) -> String {
    match k {
        IndividualKey::Named(iri) => format!("<{iri}>"),
        IndividualKey::Anon(id) => format!("_:b{id}"),
    }
}

fn fmt_individual(ind: &Individual) -> String {
    fmt_individual_key(&individual_key(ind))
}

fn fmt_annotation(a: &Annotation) -> Option<String> {
    let (prop, value) = a;
    let val = match value {
        AnnotationValue::IriAnnotation(iri) => fmt_iri(iri),
        AnnotationValue::LiteralAnnotation(ge) => fmt_literal(ge)?,
        AnnotationValue::IndividualAnnotation(ind) => fmt_individual(ind),
    };
    Some(format!("{} {val}", fmt_iri(prop)))
}

fn fmt_annotation_list(anns: &[Annotation]) -> Option<String> {
    let items: Option<Vec<String>> = anns.iter().map(fmt_annotation).collect();
    Some(items?.join(", "))
}

/// `""` if `anns` is empty, else `"Annotations: a1, a2 "` (space-terminated,
/// ready to be followed directly by the annotated item).
fn ann_prefix(anns: &[Annotation]) -> Option<String> {
    if anns.is_empty() {
        Some(String::new())
    } else {
        Some(format!("Annotations: {} ", fmt_annotation_list(anns)?))
    }
}

fn fmt_literal(ge: &ingress::GraphElement) -> Option<String> {
    match ge {
        ingress::GraphElement::GraphLiteral(lit) => Some(fmt_rdf_literal(lit)),
        _ => None,
    }
}

fn fmt_rdf_literal(lit: &ingress::RdfLiteral) -> String {
    use ingress::RdfLiteral::*;
    match lit {
        LiteralString(s) => format!("\"{}\"", escape_str(s)),
        LangLiteral { lang, literal } => format!("\"{}\"@{lang}", escape_str(literal)),
        TypedLiteral { type_iri, literal } => {
            format!("\"{}\"^^<{}>", escape_str(literal), type_iri.0)
        }
        IntegerLiteral(i) => format!("{i}"),
        DecimalLiteral(d) => {
            let s = d.to_string();
            if s.contains('.') { s } else { format!("{s}.0") }
        }
        FloatLiteral(f) => format!("{}f", f.0),
        // Not reachable through this parser's `literal` production (only
        // string/typed/integer/decimal/float are); best-effort typed-literal
        // fallback so output stays syntactically valid for hand-built
        // `Ontology`s. Re-parsing such a fallback always yields a
        // `TypedLiteral`, not the original variant.
        BooleanLiteral(b) => format!("\"{b}\"^^<{}boolean>", ingress::XSD),
        DoubleLiteral(d) => format!("\"{d}\"^^<{}double>", ingress::XSD),
        DateTimeLiteral(dt) => format!("\"{}\"^^<{}dateTime>", dt.to_rfc3339(), ingress::XSD),
        DateLiteral(d) => format!("\"{}\"^^<{}date>", d.format("%Y-%m-%d"), ingress::XSD),
        TimeLiteral(t) => format!("\"{}\"^^<{}time>", t.format("%H:%M:%S"), ingress::XSD),
        DurationLiteral(dur) => format!("\"{dur}\"^^<{}duration>", ingress::XSD),
    }
}

fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

fn is_atomic_class(ce: &ClassExpression) -> bool {
    matches!(
        ce,
        ClassExpression::ClassName(_) | ClassExpression::ObjectOneOf(_)
    )
}

/// Format `ce` as a Manchester `primary`/`atomic`: bare if it's already
/// atomic (a class name or `{ind,...}`), else parenthesized. Always safe to
/// use for `and`/`or` operands and restriction fillers, regardless of what
/// the (looser) grammar would strictly require there.
fn fmt_atomic_class(ce: &ClassExpression) -> Option<String> {
    let s = fmt_class_expr(ce)?;
    if is_atomic_class(ce) {
        Some(s)
    } else {
        Some(format!("({s})"))
    }
}

fn join_class(list: &[ClassExpression], kw: &str) -> Option<String> {
    let items: Option<Vec<String>> = list.iter().map(fmt_atomic_class).collect();
    Some(items?.join(&format!(" {kw} ")))
}

fn fmt_class_expr(ce: &ClassExpression) -> Option<String> {
    match ce {
        ClassExpression::ClassName(iri) => Some(fmt_iri(iri)),
        ClassExpression::AnonymousClass(_) => {
            log_skip("anonymous class expression (not produced by this parser's grammar)");
            None
        }
        ClassExpression::ObjectComplementOf(inner) => {
            Some(format!("not {}", fmt_atomic_class(inner)?))
        }
        ClassExpression::ObjectIntersectionOf(list) => join_class(list, "and"),
        ClassExpression::ObjectUnionOf(list) => join_class(list, "or"),
        ClassExpression::ObjectOneOf(inds) => {
            let items: Vec<String> = inds.iter().map(fmt_individual).collect();
            Some(format!("{{ {} }}", items.join(", ")))
        }
        ClassExpression::ObjectSomeValuesFrom(p, filler) => Some(format!(
            "{} some {}",
            fmt_obj_prop(p)?,
            fmt_atomic_class(filler)?
        )),
        ClassExpression::ObjectAllValuesFrom(p, filler) => Some(format!(
            "{} only {}",
            fmt_obj_prop(p)?,
            fmt_atomic_class(filler)?
        )),
        ClassExpression::ObjectHasValue(p, ind) => Some(format!(
            "{} value {}",
            fmt_obj_prop(p)?,
            fmt_individual(ind)
        )),
        ClassExpression::ObjectHasSelf(p) => Some(format!("{} Self", fmt_obj_prop(p)?)),
        ClassExpression::ObjectMinQualifiedCardinality(n, p, f) => Some(format!(
            "{} min {n} {}",
            fmt_obj_prop(p)?,
            fmt_atomic_class(f)?
        )),
        ClassExpression::ObjectMaxQualifiedCardinality(n, p, f) => Some(format!(
            "{} max {n} {}",
            fmt_obj_prop(p)?,
            fmt_atomic_class(f)?
        )),
        ClassExpression::ObjectExactQualifiedCardinality(n, p, f) => Some(format!(
            "{} exactly {n} {}",
            fmt_obj_prop(p)?,
            fmt_atomic_class(f)?
        )),
        ClassExpression::ObjectMinCardinality(n, p) => {
            Some(format!("{} min {n}", fmt_obj_prop(p)?))
        }
        ClassExpression::ObjectMaxCardinality(n, p) => {
            Some(format!("{} max {n}", fmt_obj_prop(p)?))
        }
        ClassExpression::ObjectExactCardinality(n, p) => {
            Some(format!("{} exactly {n}", fmt_obj_prop(p)?))
        }
        ClassExpression::DataSomeValuesFrom(props, dr) => fmt_data_restriction(props, dr, "some"),
        ClassExpression::DataAllValuesFrom(props, dr) => fmt_data_restriction(props, dr, "only"),
        ClassExpression::DataHasValue(p, lit) => {
            Some(format!("{} value {}", fmt_iri(p), fmt_literal(lit)?))
        }
        ClassExpression::DataMinQualifiedCardinality(n, p, dr) => {
            Some(format!("{} min {n} {}", fmt_iri(p), fmt_data_range(dr)?))
        }
        ClassExpression::DataMaxQualifiedCardinality(n, p, dr) => {
            Some(format!("{} max {n} {}", fmt_iri(p), fmt_data_range(dr)?))
        }
        ClassExpression::DataExactQualifiedCardinality(n, p, dr) => Some(format!(
            "{} exactly {n} {}",
            fmt_iri(p),
            fmt_data_range(dr)?
        )),
        ClassExpression::DataMinCardinality(n, p) => Some(format!("{} min {n}", fmt_iri(p))),
        ClassExpression::DataMaxCardinality(n, p) => Some(format!("{} max {n}", fmt_iri(p))),
        ClassExpression::DataExactCardinality(n, p) => Some(format!("{} exactly {n}", fmt_iri(p))),
    }
}

fn fmt_data_restriction(props: &[FullIri], dr: &DataRange, kw: &str) -> Option<String> {
    if props.len() != 1 {
        log_skip(
            "data restriction over more than one data property (not supported by this parser's grammar)",
        );
        return None;
    }
    Some(format!(
        "{} {kw} {}",
        fmt_iri(&props[0]),
        fmt_data_range(dr)?
    ))
}

fn fmt_obj_prop(p: &ObjectPropertyExpression) -> Option<String> {
    match p {
        ObjectPropertyExpression::NamedObjectProperty(iri) => Some(fmt_iri(iri)),
        ObjectPropertyExpression::InverseObjectProperty(inner) => match &**inner {
            ObjectPropertyExpression::NamedObjectProperty(iri) => {
                Some(format!("inverse {}", fmt_iri(iri)))
            }
            _ => {
                log_skip(
                    "nested `inverse (inverse ...)` object property expression (not produced by this parser's grammar)",
                );
                None
            }
        },
        ObjectPropertyExpression::AnonymousObjectProperty(_) => {
            log_skip(
                "anonymous object property expression (not produced by this parser's grammar)",
            );
            None
        }
        ObjectPropertyExpression::ObjectPropertyChain(_) => {
            log_skip("object property chain (#157)");
            None
        }
    }
}

fn fmt_data_range(dr: &DataRange) -> Option<String> {
    match dr {
        DataRange::NamedDataRange(iri) => Some(fmt_iri(iri)),
        _ => {
            log_skip("compound data range (#157: and/or/not/{lit,...}/facets)");
            None
        }
    }
}

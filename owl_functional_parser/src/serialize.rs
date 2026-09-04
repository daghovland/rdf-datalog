/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serialize an [`owl_ontology::Ontology`] to OWL 2 Functional-Style Syntax
//! text (the reverse direction of [`crate::parse`]).
//!
//! Follow-up to the parser (issue
//! [#180](https://github.com/daghovland/rdf-datalog/issues/180)/PR
//! [#627](https://github.com/daghovland/rdf-datalog/pull/627)), tracked in
//! [#181](https://github.com/daghovland/rdf-datalog/issues/181). See
//! `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`'s "Serialiser" section
//! for the design: unlike `manchester_parser::serialize`, there is no
//! frame-grouping pass here -- every `owl_ontology::Axiom` maps to exactly
//! one `Keyword(...)` line, emitted in `ontology.axioms` order.

use owl_ontology::{
    Annotation, AnnotationAxiom, AnnotationValue, Assertion, Axiom, ClassAxiom, ClassExpression,
    DataPropertyAxiom, DataRange, Entity, FullIri, Individual, ObjectPropertyAxiom,
    ObjectPropertyExpression, Ontology, SubPropertyExpression,
};

/// Serialize `ontology` to OWL 2 Functional-Style Syntax text.
///
/// Only `ontology.axioms` is serialized (not `ontology.all_axioms()`'s
/// built-in `owl:Thing`/`xsd:integer`/... declarations, which are implicit
/// and never need restating).
pub fn serialize(ontology: &Ontology) -> String {
    let mut out = String::new();
    out.push_str("Ontology(");
    match &ontology.version {
        ingress::OntologyVersion::UnNamedOntology => {}
        ingress::OntologyVersion::NamedOntology(iri) => {
            out.push_str(&format!("<{}>", iri.0));
        }
        ingress::OntologyVersion::VersionedOntology {
            ontology_iri,
            version_iri,
        } => {
            out.push_str(&format!("<{}> <{}>", ontology_iri.0, version_iri.0));
        }
    }
    out.push('\n');
    for imp in &ontology.directly_imports_documents {
        out.push_str(&format!("    Import(<{}>)\n", imp.0));
    }
    for ann in &ontology.annotations {
        match fmt_annotation(ann) {
            Some(s) => out.push_str(&format!("    {s}\n")),
            None => log_skip("ontology-level Annotation (unsupported annotation value)"),
        }
    }
    for axiom in &ontology.axioms {
        match fmt_axiom(axiom) {
            Some(s) => out.push_str(&format!("    {s}\n")),
            None => {
                // `fmt_axiom` (or one of its helpers) already logged a
                // `log::warn!` explaining why this axiom was skipped.
            }
        }
    }
    out.push_str(")\n");
    out
}

fn log_skip(reason: &str) {
    log::warn!("owl_functional_parser::serialize: skipping unsupported axiom: {reason}");
}

// ── Axiom formatting ─────────────────────────────────────────────────────

fn fmt_axiom(axiom: &Axiom) -> Option<String> {
    match axiom {
        Axiom::AxiomDeclaration((anns, entity)) => fmt_declaration(anns, entity),
        Axiom::AxiomClassAxiom(a) => fmt_class_axiom(a),
        Axiom::AxiomObjectPropertyAxiom(a) => fmt_object_property_axiom(a),
        Axiom::AxiomDataPropertyAxiom(a) => fmt_data_property_axiom(a),
        Axiom::AxiomDatatypeDefinition(anns, dt, dr) => {
            let ann = fmt_axiom_annotations(anns);
            let dr_s = fmt_data_range(dr)?;
            Some(format!("DatatypeDefinition({ann}{} {dr_s})", fmt_iri(dt)))
        }
        Axiom::AxiomHasKey(anns, ce, ops, dps) => fmt_has_key(anns, ce, ops, dps),
        Axiom::AxiomAssertion(a) => fmt_assertion(a),
        Axiom::AxiomAnnotationAxiom(a) => fmt_annotation_axiom(a),
    }
}

fn fmt_declaration(anns: &[Annotation], entity: &Entity) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let inner = match entity {
        Entity::ClassDeclaration(iri) => format!("Class({})", fmt_iri(iri)),
        Entity::ObjectPropertyDeclaration(iri) => format!("ObjectProperty({})", fmt_iri(iri)),
        Entity::DataPropertyDeclaration(iri) => format!("DataProperty({})", fmt_iri(iri)),
        Entity::DatatypeDeclaration(iri) => format!("Datatype({})", fmt_iri(iri)),
        Entity::AnnotationPropertyDeclaration(iri) => {
            format!("AnnotationProperty({})", fmt_iri(iri))
        }
        Entity::NamedIndividualDeclaration(ind) => match ind {
            Individual::NamedIndividual(iri) => format!("NamedIndividual({})", fmt_iri(iri)),
            Individual::AnonymousIndividual(_) => {
                log_skip(
                    "Declaration(NamedIndividual(...)) with an anonymous individual \
                     (not produced by this parser's grammar -- NamedIndividual declarations are always named)",
                );
                return None;
            }
        },
    };
    Some(format!("Declaration({ann}{inner})"))
}

fn fmt_class_axiom(a: &ClassAxiom) -> Option<String> {
    match a {
        ClassAxiom::SubClassOf(anns, sub, sup) => {
            let ann = fmt_axiom_annotations(anns);
            let sub_s = fmt_class_expr(sub)?;
            let sup_s = fmt_class_expr(sup)?;
            Some(format!("SubClassOf({ann}{sub_s} {sup_s})"))
        }
        ClassAxiom::EquivalentClasses(anns, list) => {
            fmt_class_nary("EquivalentClasses", anns, list)
        }
        ClassAxiom::DisjointClasses(anns, list) => fmt_class_nary("DisjointClasses", anns, list),
        ClassAxiom::DisjointUnion(anns, class, list) => {
            let ann = fmt_axiom_annotations(anns);
            let items: Option<Vec<String>> = list.iter().map(fmt_class_expr).collect();
            let items = items?;
            Some(format!(
                "DisjointUnion({ann}{} {})",
                fmt_iri(class),
                items.join(" ")
            ))
        }
    }
}

fn fmt_class_nary(kw: &str, anns: &[Annotation], list: &[ClassExpression]) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let items: Option<Vec<String>> = list.iter().map(fmt_class_expr).collect();
    let items = items?;
    Some(format!("{kw}({ann}{})", items.join(" ")))
}

fn fmt_object_property_axiom(a: &ObjectPropertyAxiom) -> Option<String> {
    use ObjectPropertyAxiom::*;
    match a {
        ObjectPropertyDomain(p, c) => {
            let p_s = fmt_obj_prop(p)?;
            let c_s = fmt_class_expr(c)?;
            Some(format!("ObjectPropertyDomain({p_s} {c_s})"))
        }
        ObjectPropertyRange(p, c) => {
            let p_s = fmt_obj_prop(p)?;
            let c_s = fmt_class_expr(c)?;
            Some(format!("ObjectPropertyRange({p_s} {c_s})"))
        }
        SubObjectPropertyOf(anns, sub, sup) => {
            let ann = fmt_axiom_annotations(anns);
            let sub_s = fmt_sub_object_property_expression(sub)?;
            let sup_s = fmt_obj_prop(sup)?;
            Some(format!("SubObjectPropertyOf({ann}{sub_s} {sup_s})"))
        }
        EquivalentObjectProperties(anns, list) => {
            fmt_obj_prop_nary("EquivalentObjectProperties", anns, list)
        }
        DisjointObjectProperties(anns, list) => {
            fmt_obj_prop_nary("DisjointObjectProperties", anns, list)
        }
        InverseObjectProperties(anns, p1, p2) => {
            let ann = fmt_axiom_annotations(anns);
            let p1_s = fmt_obj_prop(p1)?;
            let p2_s = fmt_obj_prop(p2)?;
            Some(format!("InverseObjectProperties({ann}{p1_s} {p2_s})"))
        }
        FunctionalObjectProperty(anns, p) => {
            unary_obj_prop_axiom("FunctionalObjectProperty", anns, p)
        }
        InverseFunctionalObjectProperty(anns, p) => {
            unary_obj_prop_axiom("InverseFunctionalObjectProperty", anns, p)
        }
        ReflexiveObjectProperty(anns, p) => {
            unary_obj_prop_axiom("ReflexiveObjectProperty", anns, p)
        }
        IrreflexiveObjectProperty(anns, p) => {
            unary_obj_prop_axiom("IrreflexiveObjectProperty", anns, p)
        }
        SymmetricObjectProperty(anns, p) => {
            unary_obj_prop_axiom("SymmetricObjectProperty", anns, p)
        }
        AsymmetricObjectProperty(anns, p) => {
            unary_obj_prop_axiom("AsymmetricObjectProperty", anns, p)
        }
        TransitiveObjectProperty(anns, p) => {
            unary_obj_prop_axiom("TransitiveObjectProperty", anns, p)
        }
    }
}

fn unary_obj_prop_axiom(
    kw: &str,
    anns: &[Annotation],
    p: &ObjectPropertyExpression,
) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let p_s = fmt_obj_prop(p)?;
    Some(format!("{kw}({ann}{p_s})"))
}

fn fmt_obj_prop_nary(
    kw: &str,
    anns: &[Annotation],
    list: &[ObjectPropertyExpression],
) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let items: Option<Vec<String>> = list.iter().map(fmt_obj_prop).collect();
    let items = items?;
    Some(format!("{kw}({ann}{})", items.join(" ")))
}

fn fmt_sub_object_property_expression(sub: &SubPropertyExpression) -> Option<String> {
    match sub {
        SubPropertyExpression::SubObjectPropertyExpression(p) => fmt_obj_prop(p),
        SubPropertyExpression::PropertyExpressionChain(chain) => {
            let items: Option<Vec<String>> = chain.iter().map(fmt_obj_prop).collect();
            let items = items?;
            Some(format!("ObjectPropertyChain({})", items.join(" ")))
        }
    }
}

fn fmt_data_property_axiom(a: &DataPropertyAxiom) -> Option<String> {
    use DataPropertyAxiom::*;
    match a {
        SubDataPropertyOf(anns, sub, sup) => {
            let ann = fmt_axiom_annotations(anns);
            Some(format!(
                "SubDataPropertyOf({ann}{} {})",
                fmt_iri(sub),
                fmt_iri(sup)
            ))
        }
        EquivalentDataProperties(anns, list) => {
            fmt_data_prop_nary("EquivalentDataProperties", anns, list)
        }
        DisjointDataProperties(anns, list) => {
            fmt_data_prop_nary("DisjointDataProperties", anns, list)
        }
        DataPropertyDomain(anns, p, c) => {
            let ann = fmt_axiom_annotations(anns);
            let c_s = fmt_class_expr(c)?;
            Some(format!("DataPropertyDomain({ann}{} {c_s})", fmt_iri(p)))
        }
        DataPropertyRange(anns, p, dr) => {
            let ann = fmt_axiom_annotations(anns);
            let dr_s = fmt_data_range(dr)?;
            Some(format!("DataPropertyRange({ann}{} {dr_s})", fmt_iri(p)))
        }
        FunctionalDataProperty(anns, p) => {
            let ann = fmt_axiom_annotations(anns);
            Some(format!("FunctionalDataProperty({ann}{})", fmt_iri(p)))
        }
    }
}

fn fmt_data_prop_nary(kw: &str, anns: &[Annotation], list: &[FullIri]) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let items: Vec<String> = list.iter().map(fmt_iri).collect();
    Some(format!("{kw}({ann}{})", items.join(" ")))
}

fn fmt_has_key(
    anns: &[Annotation],
    ce: &ClassExpression,
    ops: &[ObjectPropertyExpression],
    dps: &[FullIri],
) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let ce_s = fmt_class_expr(ce)?;
    let ops_items: Option<Vec<String>> = ops.iter().map(fmt_obj_prop).collect();
    let ops_items = ops_items?;
    let dps_items: Vec<String> = dps.iter().map(fmt_iri).collect();
    Some(format!(
        "HasKey({ann}{ce_s} ({}) ({}))",
        ops_items.join(" "),
        dps_items.join(" ")
    ))
}

fn fmt_assertion(a: &Assertion) -> Option<String> {
    use Assertion::*;
    match a {
        SameIndividual(anns, list) => fmt_individual_nary("SameIndividual", anns, list),
        DifferentIndividuals(anns, list) => fmt_individual_nary("DifferentIndividuals", anns, list),
        ClassAssertion(anns, ce, ind) => {
            let ann = fmt_axiom_annotations(anns);
            let ce_s = fmt_class_expr(ce)?;
            Some(format!(
                "ClassAssertion({ann}{ce_s} {})",
                fmt_individual(ind)
            ))
        }
        ObjectPropertyAssertion(anns, p, i1, i2) => {
            fmt_object_fact("ObjectPropertyAssertion", anns, p, i1, i2)
        }
        NegativeObjectPropertyAssertion(anns, p, i1, i2) => {
            fmt_object_fact("NegativeObjectPropertyAssertion", anns, p, i1, i2)
        }
        DataPropertyAssertion(anns, p, ind, lit) => {
            fmt_data_fact("DataPropertyAssertion", anns, p, ind, lit)
        }
        NegativeDataPropertyAssertion(anns, p, ind, lit) => {
            fmt_data_fact("NegativeDataPropertyAssertion", anns, p, ind, lit)
        }
    }
}

fn fmt_object_fact(
    kw: &str,
    anns: &[Annotation],
    p: &ObjectPropertyExpression,
    i1: &Individual,
    i2: &Individual,
) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let p_s = fmt_obj_prop(p)?;
    Some(format!(
        "{kw}({ann}{p_s} {} {})",
        fmt_individual(i1),
        fmt_individual(i2)
    ))
}

fn fmt_data_fact(
    kw: &str,
    anns: &[Annotation],
    p: &FullIri,
    ind: &Individual,
    lit: &ingress::GraphElement,
) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let lit_s = fmt_literal(lit)?;
    Some(format!(
        "{kw}({ann}{} {} {lit_s})",
        fmt_iri(p),
        fmt_individual(ind)
    ))
}

fn fmt_individual_nary(kw: &str, anns: &[Annotation], list: &[Individual]) -> Option<String> {
    let ann = fmt_axiom_annotations(anns);
    let items: Vec<String> = list.iter().map(fmt_individual).collect();
    Some(format!("{kw}({ann}{})", items.join(" ")))
}

fn fmt_annotation_axiom(a: &AnnotationAxiom) -> Option<String> {
    use AnnotationAxiom::*;
    match a {
        AnnotationAssertion(anns, prop, subj, val) => {
            let ann = fmt_axiom_annotations(anns);
            let subj_s = fmt_graph_element_as_subject_or_value(subj)?;
            let val_s = fmt_graph_element_as_subject_or_value(val)?;
            Some(format!(
                "AnnotationAssertion({ann}{} {subj_s} {val_s})",
                fmt_iri(prop)
            ))
        }
        SubAnnotationPropertyOf(anns, sub, sup) => {
            let ann = fmt_axiom_annotations(anns);
            Some(format!(
                "SubAnnotationPropertyOf({ann}{} {})",
                fmt_iri(sub),
                fmt_iri(sup)
            ))
        }
        AnnotationPropertyDomain(anns, p, target) => {
            let ann = fmt_axiom_annotations(anns);
            Some(format!(
                "AnnotationPropertyDomain({ann}{} {})",
                fmt_iri(p),
                fmt_iri(target)
            ))
        }
        AnnotationPropertyRange(anns, p, target) => {
            let ann = fmt_axiom_annotations(anns);
            Some(format!(
                "AnnotationPropertyRange({ann}{} {})",
                fmt_iri(p),
                fmt_iri(target)
            ))
        }
    }
}

// ── Term formatting ───────────────────────────────────────────────────────

fn fmt_iri(iri: &FullIri) -> String {
    format!("<{}>", iri.0.0)
}

fn fmt_individual(ind: &Individual) -> String {
    match ind {
        Individual::NamedIndividual(iri) => fmt_iri(iri),
        Individual::AnonymousIndividual(id) => format!("_:b{id}"),
    }
}

fn fmt_annotation(a: &Annotation) -> Option<String> {
    let (prop, value) = a;
    let val = match value {
        AnnotationValue::IriAnnotation(iri) => fmt_iri(iri),
        AnnotationValue::LiteralAnnotation(ge) => fmt_literal(ge)?,
        AnnotationValue::IndividualAnnotation(ind) => fmt_individual(ind),
    };
    Some(format!("Annotation({} {val})", fmt_iri(prop)))
}

/// `""` if `anns` is empty, else every `Annotation(...)` form
/// space-separated and followed by a trailing space, ready to be followed
/// directly by an axiom's "real" arguments -- mirrors `axiomAnnotations ::=
/// { Annotation }`'s position immediately inside an axiom's opening paren.
/// A parsed annotation whose value can't be formatted is dropped with a
/// `log::warn!` rather than failing the whole axiom.
fn fmt_axiom_annotations(anns: &[Annotation]) -> String {
    if anns.is_empty() {
        return String::new();
    }
    let items: Vec<String> = anns
        .iter()
        .filter_map(|a| {
            let s = fmt_annotation(a);
            if s.is_none() {
                log_skip(
                    "axiom Annotation with an unsupported value, dropped from axiomAnnotations",
                );
            }
            s
        })
        .collect();
    if items.is_empty() {
        String::new()
    } else {
        format!("{} ", items.join(" "))
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
        IntegerLiteral(i) => format!("\"{i}\"^^<{}integer>", ingress::XSD),
        DecimalLiteral(d) => format!("\"{d}\"^^<{}decimal>", ingress::XSD),
        FloatLiteral(f) => format!("\"{}\"^^<{}float>", f.0, ingress::XSD),
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

/// Format a [`ingress::GraphElement`] in the position `AnnotationAssertion`'s
/// subject/value slots use it for -- an IRI, an anonymous individual, or a
/// literal (see `annotation.rs`'s `annotation_subject`/
/// `annotation_value_as_graph_element` on the parser side, which lower into
/// exactly this set of `GraphElement` variants).
fn fmt_graph_element_as_subject_or_value(ge: &ingress::GraphElement) -> Option<String> {
    match ge {
        ingress::GraphElement::NodeOrEdge(ingress::RdfResource::Iri(iri)) => {
            Some(format!("<{}>", iri.0))
        }
        ingress::GraphElement::NodeOrEdge(ingress::RdfResource::AnonymousBlankNode(id)) => {
            Some(format!("_:b{id}"))
        }
        ingress::GraphElement::GraphLiteral(lit) => Some(fmt_rdf_literal(lit)),
        ingress::GraphElement::TripleTerm(_) => {
            log_skip(
                "AnnotationAssertion subject/value as an RDF 1.2 triple term \
                 (not produced by this parser's grammar)",
            );
            None
        }
    }
}

fn fmt_class_expr(ce: &ClassExpression) -> Option<String> {
    match ce {
        ClassExpression::ClassName(iri) => Some(fmt_iri(iri)),
        ClassExpression::AnonymousClass(_) => {
            log_skip("anonymous class expression (not produced by this parser's grammar)");
            None
        }
        ClassExpression::ObjectComplementOf(inner) => {
            Some(format!("ObjectComplementOf({})", fmt_class_expr(inner)?))
        }
        ClassExpression::ObjectIntersectionOf(list) => join_class("ObjectIntersectionOf", list),
        ClassExpression::ObjectUnionOf(list) => join_class("ObjectUnionOf", list),
        ClassExpression::ObjectOneOf(inds) => {
            let items: Vec<String> = inds.iter().map(fmt_individual).collect();
            Some(format!("ObjectOneOf({})", items.join(" ")))
        }
        ClassExpression::ObjectSomeValuesFrom(p, filler) => Some(format!(
            "ObjectSomeValuesFrom({} {})",
            fmt_obj_prop(p)?,
            fmt_class_expr(filler)?
        )),
        ClassExpression::ObjectAllValuesFrom(p, filler) => Some(format!(
            "ObjectAllValuesFrom({} {})",
            fmt_obj_prop(p)?,
            fmt_class_expr(filler)?
        )),
        ClassExpression::ObjectHasValue(p, ind) => Some(format!(
            "ObjectHasValue({} {})",
            fmt_obj_prop(p)?,
            fmt_individual(ind)
        )),
        ClassExpression::ObjectHasSelf(p) => Some(format!("ObjectHasSelf({})", fmt_obj_prop(p)?)),
        ClassExpression::ObjectMinQualifiedCardinality(n, p, f) => Some(format!(
            "ObjectMinCardinality({n} {} {})",
            fmt_obj_prop(p)?,
            fmt_class_expr(f)?
        )),
        ClassExpression::ObjectMaxQualifiedCardinality(n, p, f) => Some(format!(
            "ObjectMaxCardinality({n} {} {})",
            fmt_obj_prop(p)?,
            fmt_class_expr(f)?
        )),
        ClassExpression::ObjectExactQualifiedCardinality(n, p, f) => Some(format!(
            "ObjectExactCardinality({n} {} {})",
            fmt_obj_prop(p)?,
            fmt_class_expr(f)?
        )),
        ClassExpression::ObjectMinCardinality(n, p) => {
            Some(format!("ObjectMinCardinality({n} {})", fmt_obj_prop(p)?))
        }
        ClassExpression::ObjectMaxCardinality(n, p) => {
            Some(format!("ObjectMaxCardinality({n} {})", fmt_obj_prop(p)?))
        }
        ClassExpression::ObjectExactCardinality(n, p) => {
            Some(format!("ObjectExactCardinality({n} {})", fmt_obj_prop(p)?))
        }
        ClassExpression::DataSomeValuesFrom(props, dr) => {
            fmt_data_restriction("DataSomeValuesFrom", props, dr)
        }
        ClassExpression::DataAllValuesFrom(props, dr) => {
            fmt_data_restriction("DataAllValuesFrom", props, dr)
        }
        ClassExpression::DataHasValue(p, lit) => Some(format!(
            "DataHasValue({} {})",
            fmt_iri(p),
            fmt_literal(lit)?
        )),
        ClassExpression::DataMinQualifiedCardinality(n, p, dr) => Some(format!(
            "DataMinCardinality({n} {} {})",
            fmt_iri(p),
            fmt_data_range(dr)?
        )),
        ClassExpression::DataMaxQualifiedCardinality(n, p, dr) => Some(format!(
            "DataMaxCardinality({n} {} {})",
            fmt_iri(p),
            fmt_data_range(dr)?
        )),
        ClassExpression::DataExactQualifiedCardinality(n, p, dr) => Some(format!(
            "DataExactCardinality({n} {} {})",
            fmt_iri(p),
            fmt_data_range(dr)?
        )),
        ClassExpression::DataMinCardinality(n, p) => {
            Some(format!("DataMinCardinality({n} {})", fmt_iri(p)))
        }
        ClassExpression::DataMaxCardinality(n, p) => {
            Some(format!("DataMaxCardinality({n} {})", fmt_iri(p)))
        }
        ClassExpression::DataExactCardinality(n, p) => {
            Some(format!("DataExactCardinality({n} {})", fmt_iri(p)))
        }
    }
}

fn join_class(kw: &str, list: &[ClassExpression]) -> Option<String> {
    let items: Option<Vec<String>> = list.iter().map(fmt_class_expr).collect();
    Some(format!("{kw}({})", items?.join(" ")))
}

fn fmt_data_restriction(kw: &str, props: &[FullIri], dr: &DataRange) -> Option<String> {
    let props_s: Vec<String> = props.iter().map(fmt_iri).collect();
    let dr_s = fmt_data_range(dr)?;
    Some(format!("{kw}({} {dr_s})", props_s.join(" ")))
}

fn fmt_obj_prop(p: &ObjectPropertyExpression) -> Option<String> {
    match p {
        ObjectPropertyExpression::NamedObjectProperty(iri) => Some(fmt_iri(iri)),
        ObjectPropertyExpression::InverseObjectProperty(inner) => {
            Some(format!("ObjectInverseOf({})", fmt_obj_prop(inner)?))
        }
        ObjectPropertyExpression::AnonymousObjectProperty(_) => {
            log_skip(
                "anonymous object property expression (not produced by this parser's grammar)",
            );
            None
        }
        ObjectPropertyExpression::ObjectPropertyChain(chain) => {
            // Only valid as a `SubObjectPropertyOf` LHS in the grammar; a
            // chain nested inside e.g. `ObjectSomeValuesFrom` is not
            // producible by this parser, but format it anyway (best-effort)
            // rather than fail the whole axiom.
            let items: Option<Vec<String>> = chain.iter().map(fmt_obj_prop).collect();
            Some(format!("ObjectPropertyChain({})", items?.join(" ")))
        }
    }
}

fn fmt_data_range(dr: &DataRange) -> Option<String> {
    match dr {
        DataRange::NamedDataRange(iri) => Some(fmt_iri(iri)),
        DataRange::DataIntersectionOf(list) => join_data_range("DataIntersectionOf", list),
        DataRange::DataUnionOf(list) => join_data_range("DataUnionOf", list),
        DataRange::DataComplementOf(inner) => {
            Some(format!("DataComplementOf({})", fmt_data_range(inner)?))
        }
        DataRange::DataOneOf(vals) => {
            let items: Option<Vec<String>> = vals.iter().map(fmt_literal).collect();
            Some(format!("DataOneOf({})", items?.join(" ")))
        }
        DataRange::DatatypeRestriction(dt, facets) => {
            let mut parts = vec![fmt_iri(dt)];
            for (facet, value) in facets {
                parts.push(fmt_iri(facet));
                parts.push(fmt_literal(value)?);
            }
            Some(format!("DatatypeRestriction({})", parts.join(" ")))
        }
    }
}

fn join_data_range(kw: &str, list: &[DataRange]) -> Option<String> {
    let items: Option<Vec<String>> = list.iter().map(fmt_data_range).collect();
    Some(format!("{kw}({})", items?.join(" ")))
}

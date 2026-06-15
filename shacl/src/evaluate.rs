/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Direct Rust evaluation of Phase 2 SHACL constraint components.
//!
//! These constraints require value-testing (datatype checks, comparisons, regex,
//! string-length, language tags) that are not expressible in the current Datalog
//! engine without built-in predicate extensions.  We evaluate them directly
//! against the **original** data graph before Datalog materialisation, just like
//! `sh:closed`, so that synthetic helper predicates never interfere.
//!
//! Spec: <https://www.w3.org/TR/shacl/#core-components>

use crate::{graph, shapes, vocab};
use dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID;
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral};
use ingress::RDF_TYPE;
use regex::Regex;
use std::collections::HashSet;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Evaluate all Phase 2 property constraints for every shape and add violation
/// triples to `work`.  Returns the violation-predicate IDs.
pub fn eval_all(
    parsed: &[shapes::ParsedShape],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let mut viol_preds = Vec::new();
    for shape in parsed {
        let targets = crate::data_targets(shape, data);
        for prop in &shape.property_shapes {
            for (ci, constraint) in prop.constraints.iter().enumerate() {
                let coord = ConstraintCoord {
                    si: shape.idx,
                    pi: prop.idx,
                    ci,
                };
                let new = eval_prop_constraint(
                    constraint,
                    coord,
                    &prop.path,
                    &targets,
                    data,
                    shapes_store,
                    work,
                );
                viol_preds.extend(new);
            }
        }
        // sh:nodeKind at node shape level — check each target node itself.
        if let Some(nk) = &shape.node_kind {
            let viol = graph::intern_iri(work, &vocab::viol_node_kind(shape.idx, usize::MAX));
            let nil = graph::intern_iri(work, vocab::INT_NIL);
            for node in &targets {
                if !matches_node_kind(data, *node, nk) {
                    add_viol(work, *node, viol, nil);
                }
            }
            viol_preds.push(viol);
        }

        // sh:xone at shape level:
        if !shape.xone_inners.is_empty() {
            let new = eval_xone(shape, &targets, data, shapes_store, work);
            viol_preds.extend(new);
        }
    }
    viol_preds
}

// ── Constraint coordinate ─────────────────────────────────────────────────────

/// Position of a constraint within the shapes graph, used to mint unique
/// violation IRI names via `vocab::viol_*`.
#[derive(Clone, Copy, Debug)]
struct ConstraintCoord {
    /// Index of the node shape in `parsed`.
    si: usize,
    /// Index of the property shape within that node shape.
    pi: usize,
    /// Index of the constraint within that property shape.
    ci: usize,
}

// ── Property constraint dispatch ──────────────────────────────────────────────

fn eval_prop_constraint(
    constraint: &shapes::PropConstraint,
    coord: ConstraintCoord,
    path: &str,
    targets: &[GraphElementId],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let ConstraintCoord { si, pi, ci } = coord;
    use shapes::PropConstraint::*;
    match constraint {
        // Phase 1 constraints are handled via Datalog — skip them here.
        MinCount(_) | MaxCount(_) | Class(_) | HasValue(_) | In(_) => vec![],

        // §4.1.2 sh:datatype
        Datatype(dt_iri) => {
            let viol = graph::intern_iri(work, &vocab::viol_datatype(si, pi));
            for node in targets {
                for val in path_values(data, *node, path) {
                    if !has_datatype(data, val, dt_iri) {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }

        // §4.1.3 sh:nodeKind
        NodeKind(nk) => {
            let viol = graph::intern_iri(work, &vocab::viol_node_kind(si, pi));
            for node in targets {
                for val in path_values(data, *node, path) {
                    if !matches_node_kind(data, val, nk) {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }

        // §4.3 value range
        MinInclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_min_inclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in path_values(data, *node, path) {
                    if let (Some(b), Some(v)) = (&bound_val, lit_comparable(data, val))
                        && v < *b
                    {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }
        MaxInclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_max_inclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in path_values(data, *node, path) {
                    if let (Some(b), Some(v)) = (&bound_val, lit_comparable(data, val))
                        && v > *b
                    {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }
        MinExclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_min_exclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in path_values(data, *node, path) {
                    if let (Some(b), Some(v)) = (&bound_val, lit_comparable(data, val))
                        && v <= *b
                    {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }
        MaxExclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_max_exclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in path_values(data, *node, path) {
                    if let (Some(b), Some(v)) = (&bound_val, lit_comparable(data, val))
                        && v >= *b
                    {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }

        // §4.4.1 sh:minLength
        MinLength(n) => {
            let viol = graph::intern_iri(work, &vocab::viol_min_length(si, pi));
            for node in targets {
                for val in path_values(data, *node, path) {
                    if let Some(s) = lexical_form(data, val)
                        && codepoint_len(&s) < *n as usize
                    {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }

        // §4.4.2 sh:maxLength
        MaxLength(n) => {
            let viol = graph::intern_iri(work, &vocab::viol_max_length(si, pi));
            for node in targets {
                for val in path_values(data, *node, path) {
                    if let Some(s) = lexical_form(data, val)
                        && codepoint_len(&s) > *n as usize
                    {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }

        // §4.4.3 sh:pattern
        Pattern(pat, flags) => {
            let viol = graph::intern_iri(work, &vocab::viol_pattern(si, pi));
            let full_pat = regex_with_flags(pat, flags.as_deref());
            match Regex::new(&full_pat) {
                Err(e) => {
                    log::warn!("sh:pattern regex '{}' invalid: {e}", pat);
                }
                Ok(re) => {
                    for node in targets {
                        for val in path_values(data, *node, path) {
                            if let Some(s) = lexical_form(data, val)
                                && !re.is_match(&s)
                            {
                                add_viol(work, *node, viol, val);
                            }
                        }
                    }
                }
            }
            vec![viol]
        }

        // §4.4.4 sh:languageIn
        LanguageIn(tags) => {
            let tag_set: HashSet<String> = tags.iter().map(|t| t.to_lowercase()).collect();
            let viol = graph::intern_iri(work, &vocab::viol_language_in(si, pi));
            for node in targets {
                for val in path_values(data, *node, path) {
                    // Language-tagged literal whose tag is not in the allowed set → violation.
                    // Non-language-tagged literals also violate (per SHACL spec §4.4.4).
                    // Non-literals are ignored.
                    let violates = match data.resources.get_graph_element(val) {
                        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) => {
                            !lang_matches(&tag_set, lang)
                        }
                        GraphElement::GraphLiteral(_) => true,
                        _ => false,
                    };
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![viol]
        }

        // §4.4.5 sh:uniqueLang
        UniqueLang => {
            let viol = graph::intern_iri(work, &vocab::viol_unique_lang(si, pi));
            for node in targets {
                let vals = path_values(data, *node, path);
                let mut seen_langs: HashSet<String> = HashSet::new();
                for val in &vals {
                    if let GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) =
                        data.resources.get_graph_element(*val)
                    {
                        let lower = lang.to_lowercase();
                        if !seen_langs.insert(lower) {
                            // Duplicate language tag → violation (focus-node level)
                            add_viol(work, *node, viol, *val);
                        }
                    }
                }
            }
            vec![viol]
        }

        // §4.5.1 sh:equals — value sets must be identical
        Equals(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_equals(si, pi));
            for node in targets {
                let path_vals: HashSet<GraphElementId> =
                    path_values(data, *node, path).into_iter().collect();
                let other_vals: HashSet<GraphElementId> =
                    path_values(data, *node, other_path).into_iter().collect();
                if path_vals != other_vals {
                    // One violation per focus node (not per differing value pair)
                    let nil = graph::intern_iri(work, vocab::INT_NIL);
                    add_viol(work, *node, viol, nil);
                }
            }
            vec![viol]
        }

        // §4.5.2 sh:disjoint — value sets must not overlap
        Disjoint(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_disjoint(si, pi));
            for node in targets {
                let path_vals: HashSet<GraphElementId> =
                    path_values(data, *node, path).into_iter().collect();
                let other_vals: HashSet<GraphElementId> =
                    path_values(data, *node, other_path).into_iter().collect();
                for shared in path_vals.intersection(&other_vals) {
                    add_viol(work, *node, viol, *shared);
                }
            }
            vec![viol]
        }

        // §4.5.3 sh:lessThan — every path value must be strictly < every other value
        LessThan(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_less_than(si, pi));
            for node in targets {
                'outer: for pv in path_values(data, *node, path) {
                    if let Some(pvc) = lit_comparable(data, pv) {
                        for ov in path_values(data, *node, other_path) {
                            if let Some(ovc) = lit_comparable(data, ov)
                                && pvc >= ovc
                            {
                                add_viol(work, *node, viol, pv);
                                continue 'outer;
                            }
                        }
                    }
                }
            }
            vec![viol]
        }

        // §4.5.4 sh:lessThanOrEquals
        LessThanOrEquals(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_less_than_or_equals(si, pi));
            for node in targets {
                'outer: for pv in path_values(data, *node, path) {
                    if let Some(pvc) = lit_comparable(data, pv) {
                        for ov in path_values(data, *node, other_path) {
                            if let Some(ovc) = lit_comparable(data, ov)
                                && pvc > ovc
                            {
                                add_viol(work, *node, viol, pv);
                                continue 'outer;
                            }
                        }
                    }
                }
            }
            vec![viol]
        }

        // §4.7.1 sh:node — values must conform to a referenced node shape
        shapes::PropConstraint::NodeShape(inner_shapes_id) => eval_node_shape(
            coord,
            *inner_shapes_id,
            path,
            targets,
            data,
            shapes_store,
            work,
        ),

        // §4.7.3 sh:qualifiedValueShape
        shapes::PropConstraint::QualifiedValueShape {
            shapes_id,
            min,
            max,
        } => eval_qualified_value(
            coord,
            QualifiedSpec {
                inner_shapes_id: *shapes_id,
                min: *min,
                max: *max,
            },
            path,
            targets,
            data,
            shapes_store,
            work,
        ),

        // Unimplemented — skip silently
        #[allow(unreachable_patterns)]
        _ => {
            log::debug!(
                "Phase 2 constraint {constraint:?} at ({si},{pi},{ci}) not yet implemented"
            );
            vec![]
        }
    }
}

// ── sh:xone ───────────────────────────────────────────────────────────────────

fn eval_xone(
    shape: &shapes::ParsedShape,
    targets: &[GraphElementId],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let si = shape.idx;
    let viol = graph::intern_iri(work, &vocab::viol_xone(si));
    let nil = graph::intern_iri(work, vocab::INT_NIL);

    for node in targets {
        let conforming_count = shape
            .xone_inners
            .iter()
            .filter(|inner_ref| {
                node_conforms_to_inner(*node, inner_ref.shapes_id, data, shapes_store)
            })
            .count();
        if conforming_count != 1 {
            add_viol(work, *node, viol, nil);
        }
    }
    vec![viol]
}

// ── sh:node ───────────────────────────────────────────────────────────────────

fn eval_node_shape(
    coord: ConstraintCoord,
    inner_shapes_id: GraphElementId,
    path: &str,
    targets: &[GraphElementId],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let viol = graph::intern_iri(work, &vocab::viol_node_shape(coord.si, coord.pi));
    for node in targets {
        for val in path_values(data, *node, path) {
            if !node_conforms_to_inner(val, inner_shapes_id, data, shapes_store) {
                add_viol(work, *node, viol, val);
            }
        }
    }
    vec![viol]
}

// ── sh:qualifiedValueShape ────────────────────────────────────────────────────

struct QualifiedSpec {
    inner_shapes_id: GraphElementId,
    min: Option<u64>,
    max: Option<u64>,
}

fn eval_qualified_value(
    coord: ConstraintCoord,
    spec: QualifiedSpec,
    path: &str,
    targets: &[GraphElementId],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let viol = graph::intern_iri(work, &vocab::viol_qualified_value(coord.si, coord.pi));
    let nil = graph::intern_iri(work, vocab::INT_NIL);

    for node in targets {
        let qualifying_count = path_values(data, *node, path)
            .iter()
            .filter(|&&val| node_conforms_to_inner(val, spec.inner_shapes_id, data, shapes_store))
            .count() as u64;

        let fails = spec.min.is_some_and(|n| qualifying_count < n)
            || spec.max.is_some_and(|n| qualifying_count > n);
        if fails {
            add_viol(work, *node, viol, nil);
        }
    }
    vec![viol]
}

// ── Inner shape conformance (for sh:node / sh:xone / sh:qualifiedValueShape) ──

/// Return `true` if `node` (in `data`) satisfies the inner shape `inner_id` (in `shapes_store`).
///
/// Phase 2 support: `sh:class C`, `sh:nodeKind NK`, `sh:property [sh:path P; sh:minCount 1]`.
fn node_conforms_to_inner(
    node: GraphElementId,
    inner_id: GraphElementId,
    data: &Datastore,
    shapes_store: &Datastore,
) -> bool {
    use crate::vocab::{SH_CLASS, SH_MIN_COUNT, SH_NODE_KIND, SH_PATH, SH_PROPERTY};

    // sh:class C — node must be instance of C
    if let Some(class_id) = graph::get_object(shapes_store, inner_id, SH_CLASS)
        && let Some(class_iri) = graph::iri_string(shapes_store, class_id)
    {
        let rdf_type_id = graph::lookup_iri(data, RDF_TYPE);
        let class_data_id = graph::lookup_iri(data, &class_iri);
        if let (Some(rt), Some(cd)) = (rdf_type_id, class_data_id) {
            if !data
                .get_triples_with_subject_predicate(node, rt)
                .any(|t| t.obj == cd)
            {
                return false;
            }
        } else {
            return false;
        }
    }

    // sh:nodeKind NK
    if let Some(nk_id) = graph::get_object(shapes_store, inner_id, SH_NODE_KIND)
        && let Some(nk_iri) = graph::iri_string(shapes_store, nk_id)
        && let Some(nk) = shapes::NodeKindValue::from_iri(&nk_iri)
        && !matches_node_kind(data, node, &nk)
    {
        return false;
    }

    // sh:property [ sh:path P; sh:minCount 1 ] — node must have ≥1 value for P
    for prop_node in graph::get_objects(shapes_store, inner_id, SH_PROPERTY) {
        if let Some(path_id) = graph::get_object(shapes_store, prop_node, SH_PATH)
            && let Some(path_iri) = graph::iri_string(shapes_store, path_id)
        {
            let min = graph::get_object(shapes_store, prop_node, SH_MIN_COUNT)
                .and_then(|id| graph::elem_to_u64(shapes_store, id))
                .unwrap_or(0);
            if min >= 1 && path_values(data, node, &path_iri).is_empty() {
                return false;
            }
        }
    }

    true
}

// ── Value / literal helpers ───────────────────────────────────────────────────

/// Return all values of the property `path_iri` for `node` in the default graph.
fn path_values(data: &Datastore, node: GraphElementId, path_iri: &str) -> Vec<GraphElementId> {
    let Some(path_id) = graph::lookup_iri(data, path_iri) else {
        return vec![];
    };
    data.get_triples_with_subject_predicate(node, path_id)
        .map(|t| t.obj)
        .collect()
}

/// Add a violation triple `(focus, viol_pred, value)` to the **default** graph of `work`.
fn add_viol(
    work: &mut Datastore,
    focus: GraphElementId,
    viol_pred: GraphElementId,
    value: GraphElementId,
) {
    work.named_graphs.add_quad(dag_rdf::ingress::Quad {
        triple_id: DEFAULT_GRAPH_ELEMENT_ID,
        subject: focus,
        predicate: viol_pred,
        obj: value,
    });
}

// ── sh:datatype check ─────────────────────────────────────────────────────────

/// Return `true` if the element `id` has the given RDF datatype IRI.
fn has_datatype(data: &Datastore, id: GraphElementId, dt_iri: &str) -> bool {
    match data.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(lit) => literal_datatype_iri(lit) == dt_iri,
        _ => false,
    }
}

fn literal_datatype_iri(lit: &RdfLiteral) -> &str {
    use ingress::{XSD_BOOLEAN, XSD_INTEGER};
    match lit {
        RdfLiteral::TypedLiteral { type_iri, .. } => &type_iri.0,
        RdfLiteral::LiteralString(_) | RdfLiteral::LangLiteral { .. } => {
            "http://www.w3.org/2001/XMLSchema#string"
        }
        RdfLiteral::BooleanLiteral(_) => XSD_BOOLEAN,
        RdfLiteral::IntegerLiteral(_) => XSD_INTEGER,
        RdfLiteral::DecimalLiteral(_) => "http://www.w3.org/2001/XMLSchema#decimal",
        RdfLiteral::FloatLiteral(_) => "http://www.w3.org/2001/XMLSchema#float",
        RdfLiteral::DoubleLiteral(_) => "http://www.w3.org/2001/XMLSchema#double",
        RdfLiteral::DurationLiteral(_) => "http://www.w3.org/2001/XMLSchema#duration",
        RdfLiteral::DateTimeLiteral(_) => "http://www.w3.org/2001/XMLSchema#dateTime",
        RdfLiteral::TimeLiteral(_) => "http://www.w3.org/2001/XMLSchema#time",
        RdfLiteral::DateLiteral(_) => "http://www.w3.org/2001/XMLSchema#date",
    }
}

// ── sh:nodeKind check ─────────────────────────────────────────────────────────

fn matches_node_kind(data: &Datastore, id: GraphElementId, nk: &shapes::NodeKindValue) -> bool {
    use shapes::NodeKindValue::*;
    let is_iri = graph::is_iri(data, id);
    let is_blank = graph::is_blank_node(data, id);
    let is_lit = !is_iri && !is_blank;
    match nk {
        IRI => is_iri,
        BlankNode => is_blank,
        Literal => is_lit,
        BlankNodeOrIRI => is_blank || is_iri,
        BlankNodeOrLiteral => is_blank || is_lit,
        IRIOrLiteral => is_iri || is_lit,
    }
}

// ── Comparable value (for range + lessThan) ───────────────────────────────────

/// An ordered value suitable for numeric/date comparisons.
#[derive(PartialEq)]
enum Comparable {
    Numeric(f64),
    Date(chrono::NaiveDate),
    DateTime(chrono::DateTime<chrono::Utc>),
}

impl Eq for Comparable {}
impl PartialOrd for Comparable {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Comparable {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Comparable::Numeric(a), Comparable::Numeric(b)) => {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Comparable::Date(a), Comparable::Date(b)) => a.cmp(b),
            (Comparable::DateTime(a), Comparable::DateTime(b)) => a.cmp(b),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

fn lit_comparable(data: &Datastore, id: GraphElementId) -> Option<Comparable> {
    match data.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(lit) => lit_to_comparable(lit),
        _ => None,
    }
}

fn lit_to_comparable(lit: &RdfLiteral) -> Option<Comparable> {
    use ingress::{XSD_DATE, XSD_DATE_TIME};
    use num_traits::ToPrimitive;
    match lit {
        RdfLiteral::IntegerLiteral(n) => n.to_f64().map(Comparable::Numeric),
        RdfLiteral::DecimalLiteral(d) => {
            use rust_decimal::prelude::ToPrimitive;
            d.to_f64().map(Comparable::Numeric)
        }
        RdfLiteral::FloatLiteral(f) => Some(Comparable::Numeric(f.0)),
        RdfLiteral::DoubleLiteral(d) => Some(Comparable::Numeric(d.0)),
        RdfLiteral::DateLiteral(d) => Some(Comparable::Date(*d)),
        RdfLiteral::DateTimeLiteral(dt) => Some(Comparable::DateTime(*dt)),
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            let iri = type_iri.0.as_str();
            if iri.contains("integer")
                || iri.contains("int")
                || iri.contains("decimal")
                || iri.contains("float")
                || iri.contains("double")
            {
                literal.parse::<f64>().ok().map(Comparable::Numeric)
            } else if iri == XSD_DATE {
                literal
                    .parse::<chrono::NaiveDate>()
                    .ok()
                    .map(Comparable::Date)
            } else if iri == XSD_DATE_TIME {
                literal
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .ok()
                    .map(Comparable::DateTime)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Resolve a shape-constraint bound (e.g. the value of `sh:minInclusive`) to a `Comparable`.
///
/// The bound IRI is looked up in `shapes_store` to get the literal; then the literal
/// is re-read from the **shapes** store (not the data store).
fn bound_to_comparable(
    _data: &Datastore,
    shapes_store: &Datastore,
    bound_elem: &shapes::ElemValue,
) -> Option<Comparable> {
    // The bound was stored in the shapes graph as a literal.
    // We look it up there rather than in the data graph.
    match bound_elem {
        shapes::ElemValue::Literal {
            value, datatype, ..
        } => {
            // Parse the literal value using the datatype hint.
            let dt = datatype.as_deref().unwrap_or("");
            if dt.contains("integer")
                || dt.contains("int")
                || dt.contains("decimal")
                || dt.contains("float")
                || dt.contains("double")
            {
                value.parse::<f64>().ok().map(Comparable::Numeric)
            } else if dt.contains("date") && !dt.contains("Time") {
                value
                    .parse::<chrono::NaiveDate>()
                    .ok()
                    .map(Comparable::Date)
            } else if dt.contains("dateTime") {
                value
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .ok()
                    .map(Comparable::DateTime)
            } else {
                // Plain number without explicit datatype
                value.parse::<f64>().ok().map(Comparable::Numeric)
            }
        }
        shapes::ElemValue::Iri(iri) => {
            // A bound given as an IRI is unusual; try looking up the literal in the shapes store
            if let Some(id) = graph::lookup_iri(shapes_store, iri)
                && let GraphElement::GraphLiteral(lit) =
                    shapes_store.resources.get_graph_element(id)
            {
                return lit_to_comparable(lit);
            }
            None
        }
        _ => None,
    }
}

// ── String / language helpers ─────────────────────────────────────────────────

/// Get the lexical form of a literal (the string value before datatype/lang processing).
fn lexical_form(data: &Datastore, id: GraphElementId) -> Option<String> {
    match data.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(lit) => Some(match lit {
            RdfLiteral::LiteralString(s) => s.clone(),
            RdfLiteral::LangLiteral { literal, .. } => literal.clone(),
            RdfLiteral::TypedLiteral { literal, .. } => literal.clone(),
            RdfLiteral::IntegerLiteral(n) => n.to_string(),
            RdfLiteral::BooleanLiteral(b) => b.to_string(),
            other => other.to_string(),
        }),
        _ => None,
    }
}

/// Count Unicode codepoints (not bytes) in a string.
fn codepoint_len(s: &str) -> usize {
    s.chars().count()
}

/// Build a regex pattern string that applies XSD/SHACL flags to the base pattern.
///
/// SHACL uses XPath regex flags: `i` (case-insensitive), `x` (extended), etc.
/// The `regex` crate uses `(?flags)` inline notation.
fn regex_with_flags(pattern: &str, flags: Option<&str>) -> String {
    match flags {
        None | Some("") => pattern.to_owned(),
        Some(f) => {
            // Map XPath flags to regex inline syntax
            let inline: String = f
                .chars()
                .filter(|&c| matches!(c, 'i' | 's' | 'm' | 'x'))
                .collect();
            if inline.is_empty() {
                pattern.to_owned()
            } else {
                format!("(?{inline}){pattern}")
            }
        }
    }
}

/// Check if `lang_tag` matches any of the allowed tags (BCP-47 prefix match).
fn lang_matches(allowed: &HashSet<String>, lang_tag: &str) -> bool {
    let lower = lang_tag.to_lowercase();
    allowed.contains(&lower)
        || allowed
            .iter()
            .any(|a| lower.starts_with(a.as_str()) && lower.as_bytes().get(a.len()) == Some(&b'-'))
}

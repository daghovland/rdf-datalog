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

use crate::{ViolMeta, graph, shapes, vocab};
use dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID;
use dag_rdf::{Datastore, GraphElement, GraphElementId, RdfLiteral, RdfResource};
use ingress::{RDF_TYPE, RDFS_SUB_CLASS_OF};
use regex::Regex;
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};

// ── Entry point ───────────────────────────────────────────────────────────────

/// Evaluate all Phase 2 property constraints for every shape and add violation
/// triples to `work`.  Returns the violation-predicate IDs paired with the
/// producing shape's `ViolMeta` (severity, source shape, path, constraint
/// component, message). See
/// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
pub fn eval_all(
    parsed: &[shapes::ParsedShape],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<(GraphElementId, ViolMeta)> {
    let mut viol_preds = Vec::new();
    for shape in parsed {
        // sh:deactivated — skip this shape's constraints entirely (SHACL §3).
        // See #262.
        if shape.deactivated {
            continue;
        }
        let targets = crate::data_targets(shape, data);
        for prop in &shape.property_shapes {
            if prop.deactivated {
                continue;
            }
            for (ci, constraint) in prop.constraints.iter().enumerate() {
                let coord = ConstraintCoord {
                    si: shape.idx,
                    pi: prop.idx,
                    ci,
                };
                let new = eval_prop_constraint(
                    constraint,
                    coord,
                    Some(&prop.path),
                    &targets,
                    data,
                    shapes_store,
                    work,
                );
                viol_preds.extend(new.into_iter().map(|(v, component)| {
                    (
                        v,
                        ViolMeta::new(
                            shapes_store,
                            shape,
                            prop.shapes_id,
                            Some(&prop.path),
                            component,
                        ),
                    )
                }));
            }
        }

        // Node-level (pathless) value constraints — sh:datatype/sh:in/sh:class/…
        // declared directly on the shape (no sh:path). These are checked against
        // each target node itself rather than a path-traversed value.
        // See #260.
        for (ci, constraint) in shape.node_constraints.iter().enumerate() {
            let coord = ConstraintCoord {
                si: shape.idx,
                pi: vocab::NODE_LEVEL_PI_BASE + ci,
                ci: 0,
            };
            let new =
                eval_prop_constraint(constraint, coord, None, &targets, data, shapes_store, work);
            viol_preds.extend(new.into_iter().map(|(v, component)| {
                (
                    v,
                    ViolMeta::new(shapes_store, shape, shape.shapes_id, None, component),
                )
            }));
        }

        // sh:nodeKind at node shape level — check each target node itself.
        // sh:value for a node-shape-scoped (pathless) constraint is the focus
        // node itself (SHACL §4.1.3/§3.4.1) — e.g. `sh:targetNode "true"^^
        // xsd:boolean ; sh:nodeKind sh:IRI` must report `sh:value
        // "true"^^xsd:boolean`, not omit it. Previously used `nil` here (the
        // "no value" sentinel), which under-reported this field for every
        // node-shape sh:nodeKind violation. See
        // https://github.com/daghovland/rdf-datalog/issues/310.
        if let Some(nk) = &shape.node_kind {
            let viol = graph::intern_iri(work, &vocab::viol_node_kind(shape.idx, usize::MAX));
            for node in &targets {
                if !matches_node_kind(data, *node, nk) {
                    add_viol(work, *node, viol, *node);
                }
            }
            viol_preds.push((
                viol,
                ViolMeta::new(
                    shapes_store,
                    shape,
                    shape.shapes_id,
                    None,
                    vocab::CC_NODE_KIND,
                ),
            ));
        }

        // sh:xone at shape level:
        if !shape.xone_inners.is_empty() {
            let new = eval_xone(shape, &targets, data, shapes_store, work);
            viol_preds.extend(new.into_iter().map(|v| {
                (
                    v,
                    ViolMeta::new(shapes_store, shape, shape.shapes_id, None, vocab::CC_XONE),
                )
            }));
        }

        // sh:not — violation iff the negated inner shape conforms. Evaluated here
        // (rather than as a Datalog rule, as it was before #258) because the full
        // inner-shape conformance check (`shape_conforms_for_node`) can involve
        // constraints — datatype, pattern, ranges, ... — that the Datalog engine
        // cannot express as rule bodies. Mirrors sh:xone's existing direct-eval
        // style above.
        if let Some(inner_ref) = &shape.not_inner {
            let viol = graph::intern_iri(work, &vocab::viol_not(shape.idx));
            for node in &targets {
                if shape_conforms_for_node(*node, inner_ref.shapes_id, data, shapes_store) {
                    // Node-shape-level (pathless) violation: sh:value is the
                    // focus node itself. See
                    // https://github.com/daghovland/rdf-datalog/issues/309.
                    add_viol(work, *node, viol, *node);
                }
            }
            viol_preds.push((
                viol,
                ViolMeta::new(shapes_store, shape, shape.shapes_id, None, vocab::CC_NOT),
            ));
        }

        // sh:or — violation iff NO disjunct's inner shape conforms. See sh:not above
        // for why this moved from Datalog-rule generation to direct evaluation.
        if !shape.or_inners.is_empty() {
            let viol = graph::intern_iri(work, &vocab::viol_or(shape.idx));
            for node in &targets {
                let any_conforms = shape.or_inners.iter().any(|inner_ref| {
                    shape_conforms_for_node(*node, inner_ref.shapes_id, data, shapes_store)
                });
                if !any_conforms {
                    // Node-shape-level (pathless) violation: sh:value is the
                    // focus node itself. See
                    // https://github.com/daghovland/rdf-datalog/issues/309.
                    add_viol(work, *node, viol, *node);
                }
            }
            viol_preds.push((
                viol,
                ViolMeta::new(shapes_store, shape, shape.shapes_id, None, vocab::CC_OR),
            ));
        }

        // sh:and — violation iff at least one inner shape does NOT conform for
        // the focus node. Per spec (§4.6.1), the reported violation is
        // sh:and's OWN constraint component (sh:AndConstraintComponent),
        // sourced from the enclosing shape itself — never the specific inner
        // branch/constraint that happened to fail. `shape_conforms_for_node`
        // (already used by sh:or/sh:not/sh:xone above) recursively covers
        // every constraint kind an inner shape can declare — Phase 1
        // (minCount/…), Phase 2 (datatype/pattern/…), and nested logical
        // combinators — so a single boolean check per inner shape is
        // sufficient; no separate Datalog-rule generation is needed for
        // sh:and (unlike the old, leaky implementation this replaced). See
        // https://github.com/daghovland/rdf-datalog/issues/309.
        if !shape.and_inners.is_empty() {
            let viol = graph::intern_iri(work, &vocab::viol_and(shape.idx));
            for node in &targets {
                let all_conform = shape.and_inners.iter().all(|inner_ref| {
                    shape_conforms_for_node(*node, inner_ref.shapes_id, data, shapes_store)
                });
                if !all_conform {
                    add_viol(work, *node, viol, *node);
                }
            }
            viol_preds.push((
                viol,
                ViolMeta::new(shapes_store, shape, shape.shapes_id, None, vocab::CC_AND),
            ));
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

/// Evaluate one Phase 2 property constraint, returning every violation
/// predicate it produced, each paired with its own `sh:sourceConstraintComponent`
/// IRI. For every constraint type except `sh:qualifiedValueShape` (see
/// `eval_qualified_value`) this is always zero or one predicate, tagged with
/// `constraint.component_iri()` — but the pairing lives here, inside the
/// match, rather than being applied uniformly by the caller, specifically so
/// `sh:qualifiedValueShape` can return up to two independently-tagged
/// predicates (one per bound) when both `sh:qualifiedMinCount` and
/// `sh:qualifiedMaxCount` are declared. See
/// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
fn eval_prop_constraint(
    constraint: &shapes::PropConstraint,
    coord: ConstraintCoord,
    path: Option<&str>,
    targets: &[GraphElementId],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<(GraphElementId, &'static str)> {
    let ConstraintCoord { si, pi, ci } = coord;
    use shapes::PropConstraint::*;
    let values_of = |node: GraphElementId| -> Vec<GraphElementId> { values_for(data, node, path) };
    match constraint {
        // Phase 1 constraints are handled via Datalog — skip them here.
        MinCount(_) | MaxCount(_) | Class(_) | HasValue(_) | In(_) => vec![],

        // §4.1.2 sh:datatype
        Datatype(dt_iri) => {
            let viol = graph::intern_iri(work, &vocab::viol_datatype(si, pi));
            for node in targets {
                for val in values_of(*node) {
                    if !has_datatype(data, val, dt_iri) {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.1.3 sh:nodeKind
        NodeKind(nk) => {
            let viol = graph::intern_iri(work, &vocab::viol_node_kind(si, pi));
            for node in targets {
                for val in values_of(*node) {
                    if !matches_node_kind(data, val, nk) {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.3 value range
        //
        // Per spec, a value node that cannot be compared to the bound (e.g.
        // not a literal, or a literal whose datatype isn't ordered against
        // the bound's) is itself a violation — the same "incomparable ⇒
        // violation" rule already applied to sh:lessThan (#303). Previously
        // these constraints silently skipped incomparable values instead of
        // reporting them. See
        // https://www.w3.org/TR/shacl/#ConstraintComponentsValueRange and
        // https://github.com/daghovland/rdf-datalog/issues/311.
        MinInclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_min_inclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in values_of(*node) {
                    let violates = match (&bound_val, lit_comparable(data, val)) {
                        (Some(b), Some(v)) => v < *b,
                        _ => true,
                    };
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }
        MaxInclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_max_inclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in values_of(*node) {
                    let violates = match (&bound_val, lit_comparable(data, val)) {
                        (Some(b), Some(v)) => v > *b,
                        _ => true,
                    };
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }
        MinExclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_min_exclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in values_of(*node) {
                    let violates = match (&bound_val, lit_comparable(data, val)) {
                        (Some(b), Some(v)) => v <= *b,
                        _ => true,
                    };
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }
        MaxExclusive(bound) => {
            let viol = graph::intern_iri(work, &vocab::viol_max_exclusive(si, pi));
            let bound_val = bound_to_comparable(data, shapes_store, bound);
            for node in targets {
                for val in values_of(*node) {
                    let violates = match (&bound_val, lit_comparable(data, val)) {
                        (Some(b), Some(v)) => v >= *b,
                        _ => true,
                    };
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.4.1 sh:minLength
        // Per spec: IRIs are tested by their string form (lexical_form
        // returns Some), blank nodes always violate (lexical_form returns
        // None) — see https://github.com/daghovland/rdf-datalog/issues/261
        MinLength(n) => {
            let viol = graph::intern_iri(work, &vocab::viol_min_length(si, pi));
            for node in targets {
                for val in values_of(*node) {
                    let violates = match lexical_form(data, val) {
                        Some(s) => codepoint_len(&s) < *n as usize,
                        None => true,
                    };
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.4.2 sh:maxLength
        // Per spec: IRIs are tested by their string form (lexical_form
        // returns Some), blank nodes always violate (lexical_form returns
        // None) — see https://github.com/daghovland/rdf-datalog/issues/261
        MaxLength(n) => {
            let viol = graph::intern_iri(work, &vocab::viol_max_length(si, pi));
            for node in targets {
                for val in values_of(*node) {
                    let violates = match lexical_form(data, val) {
                        Some(s) => codepoint_len(&s) > *n as usize,
                        None => true,
                    };
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.4.3 sh:pattern
        // Per spec: IRIs are tested by their string form (lexical_form
        // returns Some), blank nodes always violate (lexical_form returns
        // None) — see https://github.com/daghovland/rdf-datalog/issues/261
        Pattern(pat, flags) => {
            let viol = graph::intern_iri(work, &vocab::viol_pattern(si, pi));
            let full_pat = regex_with_flags(pat, flags.as_deref());
            match Regex::new(&full_pat) {
                Err(e) => {
                    log::warn!("sh:pattern regex '{}' invalid: {e}", pat);
                }
                Ok(re) => {
                    for node in targets {
                        for val in values_of(*node) {
                            let violates = match lexical_form(data, val) {
                                Some(s) => !re.is_match(&s),
                                None => true,
                            };
                            if violates {
                                add_viol(work, *node, viol, val);
                            }
                        }
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.4.4 sh:languageIn
        LanguageIn(tags) => {
            let tag_set: HashSet<String> = tags.iter().map(|t| t.to_lowercase()).collect();
            let viol = graph::intern_iri(work, &vocab::viol_language_in(si, pi));
            for node in targets {
                for val in values_of(*node) {
                    // Language-tagged literal whose tag is not in the allowed set → violation.
                    // Non-language-tagged literals also violate (per SHACL spec §4.4.4).
                    // Non-literals also violate — the spec's normative text is
                    // "For each value node that is either not a literal or
                    // that does not have a language tag matching ...", so a
                    // non-literal value node is not out of scope, it always
                    // violates. See
                    // https://www.w3.org/TR/shacl/#LanguageInConstraintComponent
                    // and https://github.com/daghovland/rdf-datalog/issues/266.
                    let violates = !matches!(
                        data.resources.get_graph_element(val),
                        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. })
                            if lang_matches(&tag_set, lang)
                    );
                    if violates {
                        add_viol(work, *node, viol, val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.4.5 sh:uniqueLang
        //
        // Per spec: one validation result per *language tag* that is used by
        // more than one value node — not one result per duplicate occurrence.
        // E.g. three values tagged "en" is one violation (for "en"), not
        // two. `sh:value` is reported as the first value node seen with that
        // tag (a representative, since the spec does not mandate which of
        // the several offending literals is used). See
        // https://www.w3.org/TR/shacl/#UniqueLangConstraintComponent and
        // https://github.com/daghovland/rdf-datalog/issues/311.
        UniqueLang => {
            let viol = graph::intern_iri(work, &vocab::viol_unique_lang(si, pi));
            for node in targets {
                let vals = values_of(*node);
                // Preserve first-seen order per language via a Vec of
                // (lang, first_value, count) rather than a HashMap, so
                // result order is deterministic without an extra sort.
                let mut by_lang: Vec<(String, GraphElementId, u32)> = Vec::new();
                for val in &vals {
                    if let GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) =
                        data.resources.get_graph_element(*val)
                    {
                        let lower = lang.to_lowercase();
                        match by_lang.iter_mut().find(|(l, _, _)| *l == lower) {
                            Some((_, _, count)) => *count += 1,
                            None => by_lang.push((lower, *val, 1)),
                        }
                    }
                }
                for (_, first_val, count) in by_lang {
                    if count > 1 {
                        add_viol(work, *node, viol, first_val);
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.5.1 sh:equals — value sets must be identical.
        // Per spec: "For each value node that does not exist as a value of
        // the property $equals at the focus node, there is a validation
        // result with the value node as sh:value. For each value of the
        // property $equals at the focus node that is not one of the value
        // nodes, there is a validation result with the value as sh:value."
        // — i.e. one result per member of the symmetric difference, not one
        // result per focus node. See
        // https://www.w3.org/TR/shacl/#EqualsConstraintComponent and
        // https://github.com/daghovland/rdf-datalog/issues/266.
        Equals(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_equals(si, pi));
            for node in targets {
                let path_vals: HashSet<GraphElementId> = values_of(*node).into_iter().collect();
                let other_vals: HashSet<GraphElementId> =
                    path_values(data, *node, other_path).into_iter().collect();
                // Sort the symmetric difference before emitting: HashSet
                // iteration order is nondeterministic (RandomState varies
                // per process), and we want the report's result order to be
                // stable across runs.
                let mut differing: Vec<GraphElementId> = path_vals
                    .symmetric_difference(&other_vals)
                    .copied()
                    .collect();
                differing.sort_unstable();
                for val in differing {
                    add_viol(work, *node, viol, val);
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.5.2 sh:disjoint — value sets must not overlap
        Disjoint(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_disjoint(si, pi));
            for node in targets {
                let path_vals: HashSet<GraphElementId> = values_of(*node).into_iter().collect();
                let other_vals: HashSet<GraphElementId> =
                    path_values(data, *node, other_path).into_iter().collect();
                for shared in path_vals.intersection(&other_vals) {
                    add_viol(work, *node, viol, *shared);
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.5.3 sh:lessThan — every path value must be strictly < every other value.
        // Per spec: "... or where the two values cannot be compared, there is
        // a validation result" — an incomparable pair (including a
        // cross-datatype pair, e.g. a number vs. a date) violates just like a
        // failed `<`. See
        // https://www.w3.org/TR/shacl/#LessThanConstraintComponent and
        // https://github.com/daghovland/rdf-datalog/issues/266.
        LessThan(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_less_than(si, pi));
            for node in targets {
                'outer: for pv in values_of(*node) {
                    for ov in path_values(data, *node, other_path) {
                        let ok = matches!(sparql_compare(data, pv, ov), Some(Ordering::Less));
                        if !ok {
                            add_viol(work, *node, viol, pv);
                            continue 'outer;
                        }
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
        }

        // §4.5.4 sh:lessThanOrEquals — same "cannot be compared ⇒ violation"
        // rule as sh:lessThan above.
        LessThanOrEquals(other_path) => {
            let viol = graph::intern_iri(work, &vocab::viol_less_than_or_equals(si, pi));
            for node in targets {
                'outer: for pv in values_of(*node) {
                    for ov in path_values(data, *node, other_path) {
                        let ok = matches!(
                            sparql_compare(data, pv, ov),
                            Some(Ordering::Less) | Some(Ordering::Equal)
                        );
                        if !ok {
                            add_viol(work, *node, viol, pv);
                            continue 'outer;
                        }
                    }
                }
            }
            vec![(viol, constraint.component_iri())]
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
        )
        .into_iter()
        .map(|v| (v, constraint.component_iri()))
        .collect(),

        // §4.7.3 sh:qualifiedValueShape — sh:qualifiedMinCount/sh:qualifiedMaxCount
        // are two independent constraint components (see `eval_qualified_value`),
        // which is why this arm, unlike every other, does not tag its result with
        // a single `constraint.component_iri()` — `eval_qualified_value` already
        // returns each predicate paired with its own correct component.
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

    for node in targets {
        let conforming_count = shape
            .xone_inners
            .iter()
            .filter(|inner_ref| {
                shape_conforms_for_node(*node, inner_ref.shapes_id, data, shapes_store)
            })
            .count();
        if conforming_count != 1 {
            // Node-shape-level (pathless) violation: sh:value is the focus
            // node itself. See
            // https://github.com/daghovland/rdf-datalog/issues/309.
            add_viol(work, *node, viol, *node);
        }
    }
    vec![viol]
}

// ── sh:node ───────────────────────────────────────────────────────────────────

fn eval_node_shape(
    coord: ConstraintCoord,
    inner_shapes_id: GraphElementId,
    path: Option<&str>,
    targets: &[GraphElementId],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let viol = graph::intern_iri(work, &vocab::viol_node_shape(coord.si, coord.pi));
    for node in targets {
        for val in values_for(data, *node, path) {
            if !shape_conforms_for_node(val, inner_shapes_id, data, shapes_store) {
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

/// `sh:qualifiedMinCount` and `sh:qualifiedMaxCount` are two independent SHACL
/// constraint components (`QualifiedMinCountConstraintComponent` /
/// `QualifiedMaxCountConstraintComponent` — there is no unified
/// "QualifiedValueShapeConstraintComponent" in the spec) that happen to share
/// one `sh:qualifiedValueShape` parameter in this crate's `PropConstraint`
/// representation. When a property shape declares both (an interval), each
/// bound is checked and reported **independently** — its own violation
/// predicate, its own correct `sh:sourceConstraintComponent` — rather than
/// merging into one ambiguous "fails" check that can only guess which bound
/// actually tripped. See PR #300 review / #264.
///
/// `qualifying_count` is recomputed once per bound when both are declared,
/// rather than once and reused — a small, deliberate duplication of work in
/// exchange for keeping each bound's check fully independent and simple to
/// read; the target sets involved are validation-time, not hot-loop, sized.
fn eval_qualified_value(
    coord: ConstraintCoord,
    spec: QualifiedSpec,
    path: Option<&str>,
    targets: &[GraphElementId],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<(GraphElementId, &'static str)> {
    let nil = graph::intern_iri(work, vocab::INT_NIL);
    let mut result = Vec::new();

    let qualifying_count = |node: GraphElementId| -> u64 {
        values_for(data, node, path)
            .iter()
            .filter(|&&val| shape_conforms_for_node(val, spec.inner_shapes_id, data, shapes_store))
            .count() as u64
    };

    if let Some(min) = spec.min {
        let viol = graph::intern_iri(work, &vocab::viol_qualified_min_count(coord.si, coord.pi));
        for node in targets {
            if qualifying_count(*node) < min {
                add_viol(work, *node, viol, nil);
            }
        }
        result.push((viol, vocab::CC_QUALIFIED_MIN_COUNT));
    }
    if let Some(max) = spec.max {
        let viol = graph::intern_iri(work, &vocab::viol_qualified_max_count(coord.si, coord.pi));
        for node in targets {
            if qualifying_count(*node) > max {
                add_viol(work, *node, viol, nil);
            }
        }
        result.push((viol, vocab::CC_QUALIFIED_MAX_COUNT));
    }
    result
}

// ── Inner shape conformance (shared by sh:not/sh:or/sh:node/sh:xone/sh:qualifiedValueShape) ──
//
// A single "does shape S hold for node N" predicate used everywhere a shape is
// referenced by another shape, instead of separate hand-rolled mini-checkers
// that only understood a subset of constraint components. See #258.

/// Return `true` if `node` (in `data`) satisfies every constraint of the shape
/// node `shape_id` (in `shapes_store`) — the FULL shape semantics: every
/// property-shape and node-level constraint, `sh:nodeKind`, and (recursively)
/// `sh:not`/`sh:and`/`sh:or`/`sh:xone`.
///
/// `shape_id` need not carry an `rdf:type sh:NodeShape`/`sh:PropertyShape`
/// triple — `shapes::parse_one_shape` works on any shape-graph node, which is
/// exactly what's needed here since inner shapes referenced via `sh:not`/
/// `sh:or`/`sh:node`/etc. are typically anonymous blank nodes.
///
/// No runtime cycle guard is needed here: `shapes::find_shape_reference_cycle`
/// statically rejects any cyclic shapes graph in `crate::validate` before
/// validation begins, so by the time this function runs, the shape-reference
/// graph reachable from any top-level shape is guaranteed acyclic. See
/// [#278](https://github.com/daghovland/rdf-datalog/issues/278).
fn shape_conforms_for_node(
    node: GraphElementId,
    shape_id: GraphElementId,
    data: &Datastore,
    shapes_store: &Datastore,
) -> bool {
    let parsed = shapes::parse_one_shape(shapes_store, shape_id, 0);

    // sh:deactivated — a deactivated shape is vacuously satisfied by every
    // node (SHACL §3: it must produce no results, which here means it never
    // blocks conformance when referenced by sh:not/sh:and/sh:or/sh:node/…).
    // See #262.
    if parsed.deactivated {
        return true;
    }

    if let Some(nk) = &parsed.node_kind
        && !matches_node_kind(data, node, nk)
    {
        return false;
    }

    for prop in &parsed.property_shapes {
        if prop.deactivated {
            continue;
        }
        for constraint in &prop.constraints {
            if !constraint_conforms(constraint, node, Some(&prop.path), data, shapes_store) {
                return false;
            }
        }
    }

    for constraint in &parsed.node_constraints {
        if !constraint_conforms(constraint, node, None, data, shapes_store) {
            return false;
        }
    }

    if let Some(inner_ref) = &parsed.not_inner
        && shape_conforms_for_node(node, inner_ref.shapes_id, data, shapes_store)
    {
        return false;
    }

    if !parsed
        .and_inners
        .iter()
        .all(|inner_ref| shape_conforms_for_node(node, inner_ref.shapes_id, data, shapes_store))
    {
        return false;
    }

    if !parsed.or_inners.is_empty()
        && !parsed
            .or_inners
            .iter()
            .any(|inner_ref| shape_conforms_for_node(node, inner_ref.shapes_id, data, shapes_store))
    {
        return false;
    }

    if !parsed.xone_inners.is_empty() {
        let conforming_count = parsed
            .xone_inners
            .iter()
            .filter(|inner_ref| {
                shape_conforms_for_node(node, inner_ref.shapes_id, data, shapes_store)
            })
            .count();
        if conforming_count != 1 {
            return false;
        }
    }

    true
}

/// Return `true` if every applicable value for `node` (path-traversed values
/// when `path` is `Some`, or the focus node itself when `path` is `None`)
/// satisfies `constraint`. The boolean, early-exit counterpart to
/// `eval_prop_constraint`'s violation-collecting loop — used by
/// `shape_conforms_for_node` to answer "does this shape hold", which does not
/// need per-violation reporting detail. Shares every atomic value-testing
/// primitive with `eval_prop_constraint` (`has_datatype`, `matches_node_kind`,
/// `lit_comparable`, `lexical_form`, `regex_with_flags`, `lang_matches`,
/// `path_values`/`values_for`).
fn constraint_conforms(
    constraint: &shapes::PropConstraint,
    node: GraphElementId,
    path: Option<&str>,
    data: &Datastore,
    shapes_store: &Datastore,
) -> bool {
    use shapes::PropConstraint::*;
    let values = values_for(data, node, path);

    match constraint {
        MinCount(n) => {
            let distinct: HashSet<GraphElementId> = values.iter().copied().collect();
            distinct.len() as u64 >= *n
        }
        MaxCount(n) => {
            let distinct: HashSet<GraphElementId> = values.iter().copied().collect();
            distinct.len() as u64 <= *n
        }
        Class(class_iri) => {
            let Some(rdf_type_id) = graph::lookup_iri(data, RDF_TYPE) else {
                return values.is_empty();
            };
            let Some(class_id) = graph::lookup_iri(data, class_iri) else {
                return values.is_empty();
            };
            values
                .iter()
                .all(|&v| is_instance_of_class_or_subclass(data, v, class_id, rdf_type_id))
        }
        Datatype(dt_iri) => values.iter().all(|&v| has_datatype(data, v, dt_iri)),
        NodeKind(nk) => values.iter().all(|&v| matches_node_kind(data, v, nk)),
        HasValue(elem) => {
            let Some(val_id) = lookup_elem_value(data, elem) else {
                return false;
            };
            values.contains(&val_id)
        }
        In(allowed) => {
            let allowed_ids: HashSet<GraphElementId> = allowed
                .iter()
                .filter_map(|e| lookup_elem_value(data, e))
                .collect();
            values.iter().all(|v| allowed_ids.contains(v))
        }
        // `is_some_and` returns false for a `None` lexical form (blank-node
        // value node), so this already treats blank nodes as violations per
        // SHACL §4.4.1/4.4.2, matching the evaluate_pattern fix in
        // https://github.com/daghovland/rdf-datalog/issues/261. IRIs still
        // get their string form from lexical_form and are tested normally.
        MinLength(n) => values
            .iter()
            .all(|&v| lexical_form(data, v).is_some_and(|s| codepoint_len(&s) >= *n as usize)),
        MaxLength(n) => values
            .iter()
            .all(|&v| lexical_form(data, v).is_some_and(|s| codepoint_len(&s) <= *n as usize)),
        Pattern(pat, flags) => {
            let full_pat = regex_with_flags(pat, flags.as_deref());
            match Regex::new(&full_pat) {
                Err(e) => {
                    log::warn!("sh:pattern regex '{}' invalid: {e}", pat);
                    true
                }
                Ok(re) => values
                    .iter()
                    .all(|&v| lexical_form(data, v).is_some_and(|s| re.is_match(&s))),
            }
        }
        LanguageIn(tags) => {
            let tag_set: HashSet<String> = tags.iter().map(|t| t.to_lowercase()).collect();
            // A non-literal value node never conforms — see the matching
            // eval_prop_constraint arm above for the spec citation
            // (https://github.com/daghovland/rdf-datalog/issues/266).
            values.iter().all(|&v| {
                matches!(
                    data.resources.get_graph_element(v),
                    GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. })
                        if lang_matches(&tag_set, lang)
                )
            })
        }
        UniqueLang => {
            let mut seen_langs: HashSet<String> = HashSet::new();
            values.iter().all(|&v| {
                if let GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) =
                    data.resources.get_graph_element(v)
                {
                    seen_langs.insert(lang.to_lowercase())
                } else {
                    true
                }
            })
        }
        Equals(other_path) => {
            let path_vals: HashSet<GraphElementId> = values.iter().copied().collect();
            let other_vals: HashSet<GraphElementId> =
                path_values(data, node, other_path).into_iter().collect();
            path_vals == other_vals
        }
        Disjoint(other_path) => {
            let other_vals: HashSet<GraphElementId> =
                path_values(data, node, other_path).into_iter().collect();
            values.iter().all(|v| !other_vals.contains(v))
        }
        // See the eval_prop_constraint LessThan/LessThanOrEquals arms for the
        // spec citation on why an incomparable pair does not conform
        // (https://github.com/daghovland/rdf-datalog/issues/266).
        LessThan(other_path) => values.iter().all(|&pv| {
            path_values(data, node, other_path)
                .iter()
                .all(|&ov| matches!(sparql_compare(data, pv, ov), Some(Ordering::Less)))
        }),
        LessThanOrEquals(other_path) => values.iter().all(|&pv| {
            path_values(data, node, other_path).iter().all(|&ov| {
                matches!(
                    sparql_compare(data, pv, ov),
                    Some(Ordering::Less) | Some(Ordering::Equal)
                )
            })
        }),
        MinInclusive(bound) => {
            let Some(b) = bound_to_comparable(data, shapes_store, bound) else {
                return true;
            };
            values
                .iter()
                .all(|&v| lit_comparable(data, v).is_none_or(|vc| vc >= b))
        }
        MaxInclusive(bound) => {
            let Some(b) = bound_to_comparable(data, shapes_store, bound) else {
                return true;
            };
            values
                .iter()
                .all(|&v| lit_comparable(data, v).is_none_or(|vc| vc <= b))
        }
        MinExclusive(bound) => {
            let Some(b) = bound_to_comparable(data, shapes_store, bound) else {
                return true;
            };
            values
                .iter()
                .all(|&v| lit_comparable(data, v).is_none_or(|vc| vc > b))
        }
        MaxExclusive(bound) => {
            let Some(b) = bound_to_comparable(data, shapes_store, bound) else {
                return true;
            };
            values
                .iter()
                .all(|&v| lit_comparable(data, v).is_none_or(|vc| vc < b))
        }
        NodeShape(inner_shapes_id) => values
            .iter()
            .all(|&v| shape_conforms_for_node(v, *inner_shapes_id, data, shapes_store)),
        QualifiedValueShape {
            shapes_id,
            min,
            max,
        } => {
            let qualifying_count = values
                .iter()
                .filter(|&&v| shape_conforms_for_node(v, *shapes_id, data, shapes_store))
                .count() as u64;
            !min.is_some_and(|n| qualifying_count < n) && !max.is_some_and(|n| qualifying_count > n)
        }
    }
}

/// Return `true` if `value` has `rdf:type class_id`, or `rdf:type` of any
/// class reachable from `class_id` by following `rdfs:subClassOf` edges
/// backwards (i.e. `value`'s asserted type is `class_id` or a transitive
/// subclass of it), using only subclass edges already present in `data` — no
/// external OWL-RL/RDFS reasoner is invoked. Per SHACL's "SHACL instance"
/// definition. See <https://github.com/daghovland/rdf-datalog/issues/265>.
///
/// Implemented as a BFS over each of `value`'s asserted types, walking
/// `t rdfs:subClassOf super` edges outward from `t` and checking whether
/// `class_id` is reached; a `visited` set guards against cycles in
/// malformed subclass data.
fn is_instance_of_class_or_subclass(
    data: &Datastore,
    value: GraphElementId,
    class_id: GraphElementId,
    rdf_type_id: GraphElementId,
) -> bool {
    let Some(sub_class_of_id) = graph::lookup_iri(data, RDFS_SUB_CLASS_OF) else {
        // No rdfs:subClassOf triples exist in the data at all — fall back to
        // the direct rdf:type check.
        return data
            .get_triples_with_subject_predicate(value, rdf_type_id)
            .any(|t| t.obj == class_id);
    };

    let types: Vec<GraphElementId> = data
        .get_triples_with_subject_predicate(value, rdf_type_id)
        .map(|t| t.obj)
        .collect();

    let mut visited: HashSet<GraphElementId> = HashSet::new();
    let mut queue: VecDeque<GraphElementId> = VecDeque::new();
    for t in types {
        if t == class_id {
            return true;
        }
        if visited.insert(t) {
            queue.push_back(t);
        }
    }

    while let Some(t) = queue.pop_front() {
        for parent in data
            .get_triples_with_subject_predicate(t, sub_class_of_id)
            .map(|tr| tr.obj)
        {
            if parent == class_id {
                return true;
            }
            if visited.insert(parent) {
                queue.push_back(parent);
            }
        }
    }
    false
}

/// Look up an `ElemValue` (from the shapes graph) as a `GraphElementId` in `data`,
/// without mutating `data` (unlike `translate::intern_elem`, which is only used
/// against the mutable working store during rule generation).
pub(crate) fn lookup_elem_value(
    data: &Datastore,
    elem: &shapes::ElemValue,
) -> Option<GraphElementId> {
    use dag_rdf::{GraphElement as GE, RdfResource};
    match elem {
        shapes::ElemValue::Iri(iri) => graph::lookup_iri(data, iri),
        shapes::ElemValue::BlankNode(n) => data
            .resources
            .resource_map
            .get(&GE::NodeOrEdge(RdfResource::AnonymousBlankNode(*n)))
            .copied(),
        shapes::ElemValue::Literal {
            value,
            datatype,
            lang,
        } => {
            let lit = if let Some(lang) = lang {
                RdfLiteral::LangLiteral {
                    lang: lang.clone(),
                    literal: value.clone(),
                }
            } else if let Some(dt) = datatype {
                RdfLiteral::TypedLiteral {
                    type_iri: ingress::IriReference(dt.clone()),
                    literal: value.clone(),
                }
            } else {
                RdfLiteral::LiteralString(value.clone())
            };
            data.resources
                .resource_map
                .get(&GE::GraphLiteral(lit))
                .copied()
        }
    }
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

/// Resolve the "values to test" for a focus node against a constraint: path-traversed
/// values for a property-shape constraint (`path = Some(iri)`), or just the focus node
/// itself for a node-level (pathless) constraint (`path = None`). See #260.
fn values_for(data: &Datastore, node: GraphElementId, path: Option<&str>) -> Vec<GraphElementId> {
    match path {
        Some(p) => path_values(data, node, p),
        None => vec![node],
    }
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
    use ingress::{RDF_LANG_STRING, XSD_BOOLEAN, XSD_INTEGER};
    match lit {
        RdfLiteral::TypedLiteral { type_iri, .. } => &type_iri.0,
        RdfLiteral::LiteralString(_) => "http://www.w3.org/2001/XMLSchema#string",
        RdfLiteral::LangLiteral { .. } => RDF_LANG_STRING,
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
            // Mismatched variants (e.g. Numeric vs. Date) are not actually
            // equal — they're a type error / "cannot be compared" per
            // SPARQL/XPath comparison semantics. This fallthrough is used by
            // `bound_to_comparable` for the sh:minInclusive/maxInclusive/
            // minExclusive/maxExclusive value-range components, which is a
            // known, tracked gap distinct from the sh:lessThan/
            // lessThanOrEquals fix in #266 (which replaced this Ord-based
            // path with `sparql_compare` for those two components only).
            // See https://github.com/daghovland/rdf-datalog/issues/304.
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

/// A value node classified for the SPARQL `<`/`<=` operator mapping used by
/// `sh:lessThan`/`sh:lessThanOrEquals` (SPARQL 1.1 §17.3): numeric-numeric,
/// simple-literal/xsd:string-string (codepoint collation), xsd:boolean-boolean,
/// and xsd:dateTime-dateTime pairs are comparable; anything else (including a
/// cross-type pair, e.g. numeric vs. date) is not. Distinct from `Comparable`
/// (used by the value-range constraints, `sh:minInclusive` &c.), which does
/// not cover strings/booleans and is intentionally left untouched — see
/// `sparql_compare` below and <https://github.com/daghovland/rdf-datalog/issues/266>.
enum SparqlCmpValue {
    Numeric(f64),
    Str(String),
    Bool(bool),
    Date(chrono::NaiveDate),
    DateTime(chrono::DateTime<chrono::Utc>),
}

fn sparql_cmp_value(data: &Datastore, id: GraphElementId) -> Option<SparqlCmpValue> {
    use ingress::{XSD_BOOLEAN, XSD_STRING};
    match data.resources.get_graph_element(id) {
        GraphElement::GraphLiteral(lit) => match lit {
            RdfLiteral::LiteralString(s) => Some(SparqlCmpValue::Str(s.clone())),
            RdfLiteral::BooleanLiteral(b) => Some(SparqlCmpValue::Bool(*b)),
            RdfLiteral::TypedLiteral { type_iri, literal } if type_iri.0 == XSD_STRING => {
                Some(SparqlCmpValue::Str(literal.clone()))
            }
            RdfLiteral::TypedLiteral { type_iri, literal } if type_iri.0 == XSD_BOOLEAN => {
                literal.parse::<bool>().ok().map(SparqlCmpValue::Bool)
            }
            other => lit_to_comparable(other).map(|c| match c {
                Comparable::Numeric(n) => SparqlCmpValue::Numeric(n),
                Comparable::Date(d) => SparqlCmpValue::Date(d),
                Comparable::DateTime(dt) => SparqlCmpValue::DateTime(dt),
            }),
        },
        _ => None,
    }
}

/// SPARQL `<`/`<=`/`>` comparison for `sh:lessThan`/`sh:lessThanOrEquals`
/// (see `SparqlCmpValue`). Returns `None` when the pair "cannot be compared"
/// per <https://www.w3.org/TR/shacl/#LessThanConstraintComponent> — either
/// value isn't a recognized comparable literal, or the two are of different
/// kinds (e.g. a number and a date) — which callers must treat as a
/// violation, not as "skip".
fn sparql_compare(
    data: &Datastore,
    a: GraphElementId,
    b: GraphElementId,
) -> Option<std::cmp::Ordering> {
    use SparqlCmpValue::*;
    match (sparql_cmp_value(data, a)?, sparql_cmp_value(data, b)?) {
        (Numeric(x), Numeric(y)) => x.partial_cmp(&y),
        (Str(x), Str(y)) => Some(x.cmp(&y)),
        (Bool(x), Bool(y)) => Some(x.cmp(&y)),
        (Date(x), Date(y)) => Some(x.cmp(&y)),
        (DateTime(x), DateTime(y)) => Some(x.cmp(&y)),
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

/// Get the string representation of a value node that `sh:minLength`,
/// `sh:maxLength`, and `sh:pattern` test against (SPARQL `str()` of the
/// value), or `None` if the value node must unconditionally violate those
/// constraints.
///
/// Per the normative SHACL §4.4.1-4.4.3 text (W3C SHACL spec, verified
/// against the spec's own SPARQL definitions which use `str($value)` guarded
/// by `!isBlank($value)`): these constraints "can be applied to any literals
/// and IRIs, but not to blank nodes" — a blank node always produces a
/// validation result regardless of the bound/pattern. So:
/// - literal → its lexical form (pre-datatype/lang string value)
/// - IRI → the IRI string itself (`str()` of an IRI is the IRI)
/// - blank node / triple term → `None`, meaning "always violates"
///
/// Before the fix, this returned `None` for *all* non-literals (including
/// IRIs), and callers treated `None` as "skip this value node" rather than
/// "always violates", so a non-matching IRI silently conformed and a blank
/// node was never flagged at all. See
/// <https://github.com/daghovland/rdf-datalog/issues/261>.
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
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => Some(iri.0.clone()),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(_))
        | GraphElement::TripleTerm(_) => None,
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

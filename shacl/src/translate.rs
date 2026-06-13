/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Translate parsed SHACL shapes into Datalog rules.
//!
//! Each `ParsedShape` is converted into a set of `datalog::Rule`s that:
//!   1. Mark target nodes with a per-shape synthetic predicate.
//!   2. Derive `has-value` helper triples for existence checks.
//!   3. Derive violation triples for each failing constraint.
//!
//! After `evaluate_rules` runs, every triple whose predicate is in `viol_preds`
//! represents one `ValidationResult`.
//!
//! # Cross-graph note
//! All IRI strings read from the shapes store are re-interned into the working
//! (data-clone) store via `graph::intern_iri` before being placed in rule bodies.
//! IDs from the shapes store are never used directly in working-store rules.

use crate::graph;
use crate::shapes::{ElemValue, NodeKindValue, ParsedShape, PropConstraint, Target};
use crate::vocab::*;
use dag_rdf::{Datastore, GraphElementId, Term};
use dag_rdf::query::get_default_graph_pattern;
use datalog::types::{Rule, RuleAtom, RuleHead};
use ingress::RDF_TYPE;

/// Translate all parsed shapes into Datalog rules.
///
/// Returns `(rules, viol_preds)` where `viol_preds` is the set of predicate IDs
/// whose triples in the working store after evaluation represent violations.
pub fn shapes_to_rules(
    parsed: &[ParsedShape],
    work: &mut Datastore,
) -> (Vec<Rule>, Vec<GraphElementId>) {
    let true_id = graph::intern_iri(work, INT_TRUE);
    let nil_id = graph::intern_iri(work, INT_NIL);
    let rdf_type_id = graph::intern_iri(work, RDF_TYPE);

    let mut rules: Vec<Rule> = Vec::new();
    let mut viol_preds: Vec<GraphElementId> = Vec::new();

    for shape in parsed {
        let si = shape.idx;
        let target_pred = graph::intern_iri(work, &int_target(si));

        // ── Target rules ─────────────────────────────────────────────────────
        rules.extend(target_rules(
            shape, target_pred, true_id, rdf_type_id, work,
        ));

        // ── Property shape constraint rules ──────────────────────────────────
        for prop in &shape.property_shapes {
            let pi = prop.idx;
            let path_id = graph::intern_iri(work, &prop.path);

            for constraint in &prop.constraints {
                let new_preds = property_constraint_rules(
                    constraint, si, pi, path_id, target_pred,
                    true_id, nil_id, rdf_type_id,
                    &mut rules, work,
                );
                viol_preds.extend(new_preds);
            }
        }

        // ── Node-level sh:nodeKind at the shape level ─────────────────────────
        // (e.g. sh:targetObjectsOf ex:knows; sh:nodeKind sh:IRI at node level)
        // Deferred to Phase 2 (requires built-in predicates).

        // ── sh:closed ─────────────────────────────────────────────────────────
        if let Some(allowed_iris) = &shape.closed {
            let allowed_pred = graph::intern_iri(work, &int_allowed_pred(si));
            // Fact for each allowed predicate
            for iri in allowed_iris {
                let pred_id = graph::intern_iri(work, iri);
                rules.push(fact(pred_id, allowed_pred, true_id));
            }
            let viol_pred = graph::intern_iri(work, &viol_closed(si));
            // violation: (node, ?p, ?v) where ?p is not allowed
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol_pred),
                    Term::Variable("p".into()),
                )),
                body: vec![
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Variable("p".into()),
                        Term::Variable("v".into()),
                    )),
                    RuleAtom::NotPattern(dgp(
                        Term::Variable("p".into()),
                        Term::Resource(allowed_pred),
                        Term::Resource(true_id),
                    )),
                ],
            });
            viol_preds.push(viol_pred);
        }

        // ── sh:not ────────────────────────────────────────────────────────────
        if let Some(inner) = &shape.not_shape {
            let new = not_rules(shape, inner, target_pred, true_id, nil_id, rdf_type_id, &mut rules, work);
            viol_preds.extend(new);
        }

        // ── sh:and ────────────────────────────────────────────────────────────
        if !shape.and_shapes.is_empty() {
            let new = and_rules(shape, target_pred, true_id, nil_id, rdf_type_id, &mut rules, work);
            viol_preds.extend(new);
        }

        // ── sh:or ─────────────────────────────────────────────────────────────
        if !shape.or_shapes.is_empty() {
            let new = or_rules(shape, target_pred, true_id, nil_id, rdf_type_id, &mut rules, work);
            viol_preds.extend(new);
        }

        // ── sh:xone ───────────────────────────────────────────────────────────
        // Phase 2 (requires counting conforming sub-shapes).
    }

    (rules, viol_preds)
}

// ── Target rules ──────────────────────────────────────────────────────────────

fn target_rules(
    shape: &ParsedShape,
    target_pred: GraphElementId,
    true_id: GraphElementId,
    rdf_type_id: GraphElementId,
    work: &mut Datastore,
) -> Vec<Rule> {
    let mut rules = Vec::new();
    for target in &shape.targets {
        match target {
            Target::Node(elem) => {
                let node_id = intern_elem(elem, work);
                rules.push(fact(node_id, target_pred, true_id));
            }
            Target::Class(class_iri) => {
                let class_id = graph::intern_iri(work, class_iri);
                // (node, target_pred, true) :- [node, rdf:type, class]
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(rdf_type_id),
                        Term::Resource(class_id),
                    ))],
                });
            }
            Target::ImplicitClass(class_iri) => {
                let class_id = graph::intern_iri(work, class_iri);
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(rdf_type_id),
                        Term::Resource(class_id),
                    ))],
                });
            }
            Target::SubjectsOf(pred_iri) => {
                let pred_id = graph::intern_iri(work, pred_iri);
                // (node, target_pred, true) :- [node, pred, ?o]
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(pred_id),
                        Term::Variable("o".into()),
                    ))],
                });
            }
            Target::ObjectsOf(pred_iri) => {
                let pred_id = graph::intern_iri(work, pred_iri);
                // (node, target_pred, true) :- [?s, pred, node]
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![RuleAtom::PositivePattern(dgp(
                        Term::Variable("s".into()),
                        Term::Resource(pred_id),
                        Term::Variable("n".into()),
                    ))],
                });
            }
        }
    }
    rules
}

// ── Property constraint rules ─────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn property_constraint_rules(
    constraint: &PropConstraint,
    si: usize,
    pi: usize,
    path_id: GraphElementId,
    target_pred: GraphElementId,
    true_id: GraphElementId,
    nil_id: GraphElementId,
    rdf_type_id: GraphElementId,
    rules: &mut Vec<Rule>,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    match constraint {
        // §4.2.1 sh:minCount
        PropConstraint::MinCount(n) => {
            if *n == 0 {
                return vec![]; // minCount 0 is trivially satisfied
            }
            if *n == 1 {
                // has-val helper: (node, has_val, true) :- target(node), [node, path, ?v]
                let has_val = graph::intern_iri(work, &int_has_val(si, pi));
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(has_val),
                        Term::Resource(true_id),
                    )),
                    body: vec![
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(target_pred),
                            Term::Resource(true_id),
                        )),
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(path_id),
                            Term::Variable("v".into()),
                        )),
                    ],
                });
                // violation: target(node), NOT has_val(node)
                let viol = graph::intern_iri(work, &viol_min_count(si, pi));
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(viol),
                        Term::Resource(nil_id),
                    )),
                    body: vec![
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(target_pred),
                            Term::Resource(true_id),
                        )),
                        RuleAtom::NotPattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(has_val),
                            Term::Resource(true_id),
                        )),
                    ],
                });
                vec![viol]
            } else {
                log::warn!("sh:minCount {n} > 1 not yet implemented (Phase 2)");
                vec![]
            }
        }

        // §4.2.2 sh:maxCount
        PropConstraint::MaxCount(n) => {
            if *n == 0 {
                // violation if any value exists
                let viol = graph::intern_iri(work, &viol_max_count(si, pi));
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(viol),
                        Term::Variable("v".into()),
                    )),
                    body: vec![
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(target_pred),
                            Term::Resource(true_id),
                        )),
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(path_id),
                            Term::Variable("v".into()),
                        )),
                    ],
                });
                vec![viol]
            } else if *n >= 1 {
                // violation if two DISTINCT values exist: v1 != v2
                let viol = graph::intern_iri(work, &viol_max_count(si, pi));
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(viol),
                        Term::Resource(true_id),
                    )),
                    body: vec![
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(target_pred),
                            Term::Resource(true_id),
                        )),
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(path_id),
                            Term::Variable("v1".into()),
                        )),
                        RuleAtom::PositivePattern(dgp(
                            Term::Variable("n".into()),
                            Term::Resource(path_id),
                            Term::Variable("v2".into()),
                        )),
                        RuleAtom::NotEqualsAtom(
                            Term::Variable("v1".into()),
                            Term::Variable("v2".into()),
                        ),
                    ],
                });
                // This handles maxCount 1 exactly. For maxCount N > 1, we'd need
                // N+1 distinct values, which requires counting (Phase 2).
                if *n > 1 {
                    log::warn!("sh:maxCount {n} > 1: using pair-inequality (detects any 2 distinct values, not N+1)");
                }
                vec![viol]
            } else {
                vec![]
            }
        }

        // §4.1.1 sh:class — value must be an instance of the given class
        PropConstraint::Class(class_iri) => {
            let class_id = graph::intern_iri(work, class_iri);
            // Helper: value_is_class(v) :- [v, rdf:type, class]
            let has_class = graph::intern_iri(work, &format!("urn:dagalog:shacl:hasClass:{si}:{pi}"));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("v".into()),
                    Term::Resource(has_class),
                    Term::Resource(true_id),
                )),
                body: vec![RuleAtom::PositivePattern(dgp(
                    Term::Variable("v".into()),
                    Term::Resource(rdf_type_id),
                    Term::Resource(class_id),
                ))],
            });
            let viol = graph::intern_iri(work, &viol_class(si, pi));
            // violation: target(n), [n, path, v], NOT has_class(v)
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Variable("v".into()),
                )),
                body: vec![
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v".into()),
                    )),
                    RuleAtom::NotPattern(dgp(
                        Term::Variable("v".into()),
                        Term::Resource(has_class),
                        Term::Resource(true_id),
                    )),
                ],
            });
            vec![viol]
        }

        // §4.8.2 sh:hasValue — value set must include the specified value
        PropConstraint::HasValue(elem) => {
            let val_id = intern_elem(elem, work);
            // has-val helper: (n, has_val, true) :- target(n), [n, path, val_id]
            let has_val = graph::intern_iri(work, &int_has_val(si, pi));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(has_val),
                    Term::Resource(true_id),
                )),
                body: vec![
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Resource(val_id),
                    )),
                ],
            });
            let viol = graph::intern_iri(work, &viol_has_value(si, pi));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Resource(nil_id),
                )),
                body: vec![
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    RuleAtom::NotPattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(has_val),
                        Term::Resource(true_id),
                    )),
                ],
            });
            vec![viol]
        }

        // §4.8.3 sh:in — each value must be one of the listed values
        PropConstraint::In(allowed) => {
            let in_list_pred = graph::intern_iri(work, &int_in_list(si, pi));
            // Fact for each allowed value
            for elem in allowed {
                let val_id = intern_elem(elem, work);
                rules.push(fact(val_id, in_list_pred, true_id));
            }
            let viol = graph::intern_iri(work, &viol_in(si, pi));
            // violation: target(n), [n, path, v], NOT in_list(v)
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Variable("v".into()),
                )),
                body: vec![
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    RuleAtom::PositivePattern(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v".into()),
                    )),
                    RuleAtom::NotPattern(dgp(
                        Term::Variable("v".into()),
                        Term::Resource(in_list_pred),
                        Term::Resource(true_id),
                    )),
                ],
            });
            vec![viol]
        }

        // §4.1.2 sh:datatype, §4.1.3 sh:nodeKind,
        // §4.3 value range, §4.4 string, §4.5 property pairs,
        // §4.7.3 qualifiedValueShape — deferred to Phase 2 (built-in predicates).
        PropConstraint::Datatype(_)
        | PropConstraint::NodeKind(_)
        | PropConstraint::MinLength(_)
        | PropConstraint::MaxLength(_)
        | PropConstraint::Pattern(_, _)
        | PropConstraint::LanguageIn(_)
        | PropConstraint::UniqueLang
        | PropConstraint::Equals(_)
        | PropConstraint::Disjoint(_)
        | PropConstraint::LessThan(_)
        | PropConstraint::LessThanOrEquals(_)
        | PropConstraint::QualifiedValueShape { .. } => {
            log::debug!("Constraint {constraint:?} not yet implemented (Phase 2)");
            vec![]
        }
    }
}

// ── Logical constraint rules ───────────────────────────────────────────────────

fn not_rules(
    shape: &ParsedShape,
    inner: &ElemValue,
    target_pred: GraphElementId,
    true_id: GraphElementId,
    nil_id: GraphElementId,
    rdf_type_id: GraphElementId,
    rules: &mut Vec<Rule>,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let si = shape.idx;

    // We need to check if the focus node "conforms" to the inner shape.
    // For the inner shape being a blank node `[ sh:class C ]`:
    // parse and inline the inner constraints.
    let inner_ok_pred = graph::intern_iri(work, &int_sub_ok(si, 0));
    let viol = graph::intern_iri(work, &viol_not(si));

    // Inline inner shape constraints:
    inline_inner_shape_rules(inner, inner_ok_pred, true_id, rdf_type_id, rules, work);

    // not-violation: target(n), inner_ok(n)
    rules.push(Rule {
        head: RuleHead::NormalHead(dgp(
            Term::Variable("n".into()),
            Term::Resource(viol),
            Term::Resource(nil_id),
        )),
        body: vec![
            RuleAtom::PositivePattern(dgp(
                Term::Variable("n".into()),
                Term::Resource(target_pred),
                Term::Resource(true_id),
            )),
            RuleAtom::PositivePattern(dgp(
                Term::Variable("n".into()),
                Term::Resource(inner_ok_pred),
                Term::Resource(true_id),
            )),
        ],
    });
    vec![viol]
}

fn and_rules(
    shape: &ParsedShape,
    target_pred: GraphElementId,
    true_id: GraphElementId,
    nil_id: GraphElementId,
    rdf_type_id: GraphElementId,
    rules: &mut Vec<Rule>,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let si = shape.idx;
    let mut viols = Vec::new();

    // sh:and — violation if ANY sub-shape constraint is violated.
    // Each sub-shape generates its own violation predicate.
    for (sub_idx, sub_elem) in shape.and_shapes.iter().enumerate() {
        let sub_viol = graph::intern_iri(work, &viol_and(si, sub_idx));
        // Derive violations for sub-shape and forward to sub_viol.
        // For each sub-shape we check its own constraints against target nodes.
        let sub_ok_pred = graph::intern_iri(work, &int_sub_ok(si, sub_idx));
        inline_inner_shape_rules(sub_elem, sub_ok_pred, true_id, rdf_type_id, rules, work);

        // and-sub-violation: target(n), NOT sub_ok(n)
        let has_val = graph::intern_iri(
            work,
            &format!("urn:dagalog:shacl:andHasOk:{si}:{sub_idx}"),
        );
        // has_ok helper so we can negate cleanly
        rules.push(Rule {
            head: RuleHead::NormalHead(dgp(
                Term::Variable("n".into()),
                Term::Resource(has_val),
                Term::Resource(true_id),
            )),
            body: vec![
                RuleAtom::PositivePattern(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(target_pred),
                    Term::Resource(true_id),
                )),
                RuleAtom::PositivePattern(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(sub_ok_pred),
                    Term::Resource(true_id),
                )),
            ],
        });
        rules.push(Rule {
            head: RuleHead::NormalHead(dgp(
                Term::Variable("n".into()),
                Term::Resource(sub_viol),
                Term::Resource(nil_id),
            )),
            body: vec![
                RuleAtom::PositivePattern(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(target_pred),
                    Term::Resource(true_id),
                )),
                RuleAtom::NotPattern(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(has_val),
                    Term::Resource(true_id),
                )),
            ],
        });
        viols.push(sub_viol);
    }
    viols
}

fn or_rules(
    shape: &ParsedShape,
    target_pred: GraphElementId,
    true_id: GraphElementId,
    nil_id: GraphElementId,
    rdf_type_id: GraphElementId,
    rules: &mut Vec<Rule>,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let si = shape.idx;

    // Each sub-shape gets an "ok" helper predicate.
    let sub_ok_preds: Vec<GraphElementId> = shape
        .or_shapes
        .iter()
        .enumerate()
        .map(|(sub_idx, sub_elem)| {
            let ok_pred = graph::intern_iri(work, &int_sub_ok(si, sub_idx));
            inline_inner_shape_rules(sub_elem, ok_pred, true_id, rdf_type_id, rules, work);
            ok_pred
        })
        .collect();

    // or-violation: target(n), NOT ok_0(n), NOT ok_1(n), …
    let viol = graph::intern_iri(work, &viol_or(si));
    let mut body = vec![RuleAtom::PositivePattern(dgp(
        Term::Variable("n".into()),
        Term::Resource(target_pred),
        Term::Resource(true_id),
    ))];
    for ok_pred in &sub_ok_preds {
        body.push(RuleAtom::NotPattern(dgp(
            Term::Variable("n".into()),
            Term::Resource(*ok_pred),
            Term::Resource(true_id),
        )));
    }
    rules.push(Rule {
        head: RuleHead::NormalHead(dgp(
            Term::Variable("n".into()),
            Term::Resource(viol),
            Term::Resource(nil_id),
        )),
        body,
    });
    vec![viol]
}

// ── Inner shape inlining ──────────────────────────────────────────────────────

/// Derive `(node, ok_pred, INT_TRUE)` for every node that satisfies the
/// constraints of the inner blank-node shape referenced by `inner`.
///
/// For Phase 1 the only inner constraint we inline is `sh:class C`:
/// `(node, ok_pred, true) :- [node, rdf:type, C]`.
///
/// More inner constraint types will be added in Phase 2.
fn inline_inner_shape_rules(
    inner: &ElemValue,
    ok_pred: GraphElementId,
    true_id: GraphElementId,
    rdf_type_id: GraphElementId,
    rules: &mut Vec<Rule>,
    work: &mut Datastore,
) {
    // For now we support only `[ sh:class C ]` as the inner shape.
    // The inner ElemValue is the blank node that IS the shape.
    // We need to resolve the sh:class value from the shapes graph at parse time.
    // Since we only have the ElemValue here (not the shapes Datastore),
    // the shapes parser must have embedded the needed info.
    //
    // For Phase 1, the only inner constraints reachable through sh:not / sh:and / sh:or
    // in our tests are sh:class. We embed the class IRI as a special ElemValue.
    //
    // Convention: when the inner shape has `sh:class C`, the ElemValue passed here is
    // `ElemValue::Iri(class_iri)` (set by the caller who resolved it).
    // Blank node inner shapes are not yet handled here.
    match inner {
        ElemValue::Iri(iri) => {
            let class_id = graph::intern_iri(work, iri);
            // ok: node is an instance of the class
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(ok_pred),
                    Term::Resource(true_id),
                )),
                body: vec![RuleAtom::PositivePattern(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(rdf_type_id),
                    Term::Resource(class_id),
                ))],
            });
        }
        ElemValue::BlankNode(_) | ElemValue::Literal { .. } => {
            log::warn!("inline_inner_shape_rules: complex inner shape not yet supported (Phase 2)");
        }
    }
}

// ── Fact / pattern helpers ────────────────────────────────────────────────────

/// A Datalog fact (empty body) asserting `(s, p, o)`.
fn fact(s: GraphElementId, p: GraphElementId, o: GraphElementId) -> Rule {
    Rule {
        head: RuleHead::NormalHead(dgp(
            Term::Resource(s),
            Term::Resource(p),
            Term::Resource(o),
        )),
        body: vec![],
    }
}

/// Shorthand: build a QuadPattern in the default graph.
fn dgp(s: Term, p: Term, o: Term) -> dag_rdf::QuadPattern {
    get_default_graph_pattern(s, p, o)
}

/// Intern an `ElemValue` into the working store and return its ID.
fn intern_elem(elem: &ElemValue, work: &mut Datastore) -> GraphElementId {
    use dag_rdf::{GraphElement, IriReference, RdfLiteral, RdfResource};
    use ingress::IriReference as IngIri;
    match elem {
        ElemValue::Iri(iri) => graph::intern_iri(work, iri),
        ElemValue::BlankNode(n) => work
            .resources
            .add_node_resource(RdfResource::AnonymousBlankNode(*n)),
        ElemValue::Literal { value, datatype, lang } => {
            let lit = if let Some(lang) = lang {
                RdfLiteral::LangLiteral {
                    lang: lang.clone(),
                    literal: value.clone(),
                }
            } else if let Some(dt) = datatype {
                RdfLiteral::TypedLiteral {
                    type_iri: IngIri(dt.clone()),
                    literal: value.clone(),
                }
            } else {
                RdfLiteral::LiteralString(value.clone())
            };
            work.resources
                .add_resource(GraphElement::GraphLiteral(lit))
        }
    }
}

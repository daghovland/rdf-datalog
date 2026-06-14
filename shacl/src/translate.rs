/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Translate parsed SHACL shapes into stratified Datalog rules.
//!
//! # Encoding
//!
//! Every derived fact goes into the working (data-clone) store as a triple with a
//! *synthetic* predicate IRI from `vocab::int_*` / `vocab::viol_*`.  No synthetic
//! IRI ever collides with real data because they all start with `urn:dagalog:shacl:`.
//!
//! After `evaluate_rules` runs, any triple whose predicate is in `viol_preds` is one
//! `ValidationResult`.
//!
//! # Cross-graph safety
//!
//! IRIs read from the **shapes** store are stored as plain `String`s in `ParsedShape`.
//! Before use in a rule body, they are re-interned into the **working** store via
//! `graph::intern_iri`.  A shapes-store `GraphElementId` is never used in a rule.
//!
//! The sole exception is `InnerShapeRef::shapes_id`, which is passed back to the
//! shapes store only (never inserted into a data-store triple or rule body directly).

use crate::graph;
use crate::shapes::{ElemValue, InnerShapeRef, ParsedShape, PropConstraint, Target};
use crate::vocab::*;
use dag_rdf::query::get_default_graph_pattern;
use dag_rdf::{Datastore, GraphElementId, QuadPattern, Term};
use datalog::types::{Rule, RuleAtom, RuleHead};
use ingress::RDF_TYPE;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Translate all parsed shapes into Datalog rules.
///
/// Returns `(rules, viol_preds)`.  Every triple `(n, p, v)` in the working store
/// after `evaluate_rules` where `p ∈ viol_preds` is one `ValidationResult`.
pub fn shapes_to_rules(
    parsed: &[ParsedShape],
    shapes: &Datastore,
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

        // Target rules
        rules.extend(target_rules(shape, target_pred, true_id, rdf_type_id, work));

        // Property shape constraints
        for prop in &shape.property_shapes {
            let path_id = graph::intern_iri(work, &prop.path);
            for (ci, constraint) in prop.constraints.iter().enumerate() {
                let key = (si, prop.idx, ci);
                let new = prop_constraint_rules(
                    constraint,
                    key,
                    path_id,
                    target_pred,
                    true_id,
                    nil_id,
                    rdf_type_id,
                    &mut rules,
                    work,
                );
                viol_preds.extend(new);
            }
        }

        // sh:closed — handled in lib.rs::pre_compute_violations (queries original data graph
        // before any Datalog materialisation, avoiding synthetic-predicate contamination).

        // sh:not
        if let Some(inner_ref) = &shape.not_inner {
            let ok_pred = graph::intern_iri(work, &int_sub_ok(si, 0));
            inner_ok_rules(
                inner_ref,
                shapes,
                ok_pred,
                true_id,
                rdf_type_id,
                &mut rules,
                work,
            );

            let viol = graph::intern_iri(work, &viol_not(si));
            // not-violation: target(n), inner_ok(n)
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Resource(nil_id),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(ok_pred),
                        Term::Resource(true_id),
                    ),
                ],
            });
            viol_preds.push(viol);
        }

        // sh:and — violation if ANY sub-shape constraint fails (inlined for arg-count)
        for (sub_idx, inner_ref) in shape.and_inners.iter().enumerate() {
            use crate::vocab::{SH_MIN_COUNT, SH_PATH, SH_PROPERTY};
            let inner_id = inner_ref.shapes_id;
            let viol = graph::intern_iri(work, &viol_and(si, sub_idx));

            for prop_node in graph::get_objects(shapes, inner_id, SH_PROPERTY) {
                if let Some(path_id) = graph::get_object(shapes, prop_node, SH_PATH)
                    && let Some(path_iri) = graph::iri_string(shapes, path_id)
                {
                    let min = graph::get_object(shapes, prop_node, SH_MIN_COUNT)
                        .and_then(|id| graph::elem_to_u64(shapes, id))
                        .unwrap_or(0);
                    if min >= 1 {
                        let path_id_work = graph::intern_iri(work, &path_iri);
                        let has_val = graph::intern_iri(
                            work,
                            &format!("urn:dagalog:shacl:andHasVal:{si}:{sub_idx}"),
                        );
                        rules.push(Rule {
                            head: RuleHead::NormalHead(dgp(
                                Term::Variable("n".into()),
                                Term::Resource(has_val),
                                Term::Resource(true_id),
                            )),
                            body: vec![
                                pos(
                                    Term::Variable("n".into()),
                                    Term::Resource(target_pred),
                                    Term::Resource(true_id),
                                ),
                                pos(
                                    Term::Variable("n".into()),
                                    Term::Resource(path_id_work),
                                    Term::Variable("v".into()),
                                ),
                            ],
                        });
                        rules.push(Rule {
                            head: RuleHead::NormalHead(dgp(
                                Term::Variable("n".into()),
                                Term::Resource(viol),
                                Term::Resource(nil_id),
                            )),
                            body: vec![
                                pos(
                                    Term::Variable("n".into()),
                                    Term::Resource(target_pred),
                                    Term::Resource(true_id),
                                ),
                                neg(
                                    Term::Variable("n".into()),
                                    Term::Resource(has_val),
                                    Term::Resource(true_id),
                                ),
                            ],
                        });
                    }
                }
            }
            viol_preds.push(viol);
        }

        // sh:or — violation if NO sub-shape conforms
        if !shape.or_inners.is_empty() {
            let ok_preds: Vec<GraphElementId> = shape
                .or_inners
                .iter()
                .enumerate()
                .map(|(sub_idx, inner_ref)| {
                    let ok_pred = graph::intern_iri(work, &int_sub_ok(si, sub_idx + 100));
                    inner_ok_rules(
                        inner_ref,
                        shapes,
                        ok_pred,
                        true_id,
                        rdf_type_id,
                        &mut rules,
                        work,
                    );
                    ok_pred
                })
                .collect();

            let viol = graph::intern_iri(work, &viol_or(si));
            let mut body = vec![pos(
                Term::Variable("n".into()),
                Term::Resource(target_pred),
                Term::Resource(true_id),
            )];
            for ok_pred in &ok_preds {
                body.push(neg(
                    Term::Variable("n".into()),
                    Term::Resource(*ok_pred),
                    Term::Resource(true_id),
                ));
            }
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Resource(nil_id),
                )),
                body,
            });
            viol_preds.push(viol);
        }

        // sh:xone — deferred to Phase 2 (requires counting conforming sub-shapes)
        if !shape.xone_inners.is_empty() {
            log::warn!("sh:xone not yet implemented (Phase 2)");
        }
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
            Target::Class(class_iri) | Target::ImplicitClass(class_iri) => {
                let class_id = graph::intern_iri(work, class_iri);
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![pos(
                        Term::Variable("n".into()),
                        Term::Resource(rdf_type_id),
                        Term::Resource(class_id),
                    )],
                });
            }
            Target::SubjectsOf(pred_iri) => {
                let pred_id = graph::intern_iri(work, pred_iri);
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![pos(
                        Term::Variable("n".into()),
                        Term::Resource(pred_id),
                        Term::Variable("o".into()),
                    )],
                });
            }
            Target::ObjectsOf(pred_iri) => {
                let pred_id = graph::intern_iri(work, pred_iri);
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![pos(
                        Term::Variable("s".into()),
                        Term::Resource(pred_id),
                        Term::Variable("n".into()),
                    )],
                });
            }
        }
    }
    rules
}

// ── Property constraint rules ─────────────────────────────────────────────────

/// Returns the violation predicate IDs produced for this constraint.
/// `key = (shape_idx, prop_idx, constraint_idx)` for unique IRI names.
#[allow(clippy::too_many_arguments)]
fn prop_constraint_rules(
    constraint: &PropConstraint,
    key: (usize, usize, usize),
    path_id: GraphElementId,
    target_pred: GraphElementId,
    true_id: GraphElementId,
    nil_id: GraphElementId,
    rdf_type_id: GraphElementId,
    rules: &mut Vec<Rule>,
    work: &mut Datastore,
) -> Vec<GraphElementId> {
    let (si, pi, ci) = key;

    match constraint {
        // §4.2.1 sh:minCount
        PropConstraint::MinCount(0) => vec![],
        PropConstraint::MinCount(1) => {
            let has_val = graph::intern_iri(work, &int_has_val(si, pi));
            // has_val(n, true) :- target(n, true), [n, path, ?v]
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(has_val),
                    Term::Resource(true_id),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v".into()),
                    ),
                ],
            });
            let viol = graph::intern_iri(work, &viol_min_count(si, pi));
            // violation: target(n), NOT has_val(n)
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Resource(nil_id),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    neg(
                        Term::Variable("n".into()),
                        Term::Resource(has_val),
                        Term::Resource(true_id),
                    ),
                ],
            });
            vec![viol]
        }
        PropConstraint::MinCount(n) => {
            log::warn!("sh:minCount {n} > 1 not yet implemented (Phase 2)");
            vec![]
        }

        // §4.2.2 sh:maxCount
        PropConstraint::MaxCount(0) => {
            let viol = graph::intern_iri(work, &viol_max_count(si, pi));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Variable("v".into()),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v".into()),
                    ),
                ],
            });
            vec![viol]
        }
        PropConstraint::MaxCount(_n) => {
            // maxCount N ≥ 1: violation if two DISTINCT values exist.
            // This correctly handles maxCount 1 exactly; for N > 1 it is conservative
            // (fires even if only 2 values exist, not N+1) — noted in SHACL_PLAN.md.
            let viol = graph::intern_iri(work, &viol_max_count(si, pi));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Resource(true_id),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v1".into()),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v2".into()),
                    ),
                    RuleAtom::NotEqualsAtom(
                        Term::Variable("v1".into()),
                        Term::Variable("v2".into()),
                    ),
                ],
            });
            vec![viol]
        }

        // §4.1.1 sh:class
        PropConstraint::Class(class_iri) => {
            let class_id = graph::intern_iri(work, class_iri);
            // helper: value IS an instance of class
            let has_class =
                graph::intern_iri(work, &format!("urn:dagalog:shacl:hasClass:{si}:{pi}:{ci}"));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("v".into()),
                    Term::Resource(has_class),
                    Term::Resource(true_id),
                )),
                body: vec![pos(
                    Term::Variable("v".into()),
                    Term::Resource(rdf_type_id),
                    Term::Resource(class_id),
                )],
            });
            let viol = graph::intern_iri(work, &viol_class(si, pi));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Variable("v".into()),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v".into()),
                    ),
                    neg(
                        Term::Variable("v".into()),
                        Term::Resource(has_class),
                        Term::Resource(true_id),
                    ),
                ],
            });
            vec![viol]
        }

        // §4.8.2 sh:hasValue
        PropConstraint::HasValue(elem) => {
            let val_id = intern_elem(elem, work);
            let has_val = graph::intern_iri(work, &int_has_val(si, pi));
            // has_val(n) :- target(n), [n, path, specific_val]
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(has_val),
                    Term::Resource(true_id),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Resource(val_id),
                    ),
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
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    neg(
                        Term::Variable("n".into()),
                        Term::Resource(has_val),
                        Term::Resource(true_id),
                    ),
                ],
            });
            vec![viol]
        }

        // §4.8.3 sh:in
        PropConstraint::In(allowed) => {
            let in_list = graph::intern_iri(work, &int_in_list(si, pi));
            for elem in allowed {
                let val_id = intern_elem(elem, work);
                rules.push(fact(val_id, in_list, true_id));
            }
            let viol = graph::intern_iri(work, &viol_in(si, pi));
            rules.push(Rule {
                head: RuleHead::NormalHead(dgp(
                    Term::Variable("n".into()),
                    Term::Resource(viol),
                    Term::Variable("v".into()),
                )),
                body: vec![
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(target_pred),
                        Term::Resource(true_id),
                    ),
                    pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id),
                        Term::Variable("v".into()),
                    ),
                    neg(
                        Term::Variable("v".into()),
                        Term::Resource(in_list),
                        Term::Resource(true_id),
                    ),
                ],
            });
            vec![viol]
        }

        // Phase 2 constraints — log and skip
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
            log::debug!("Constraint {constraint:?} at ({si},{pi},{ci}) deferred to Phase 2");
            vec![]
        }
    }
}

// ── Logical constraint helpers ────────────────────────────────────────────────

/// Generate "ok" rules: `(n, ok_pred, INT_TRUE)` when `n` satisfies the inner shape.
///
/// Supported inner shapes (Phase 1):
/// - `[ sh:class C ]` → `ok(n) :- [n, rdf:type, C]`
/// - `[ sh:property [ sh:path P ; sh:minCount 1 ] ]` → `ok(n) :- [n, P, ?v]`
fn inner_ok_rules(
    inner_ref: &InnerShapeRef,
    shapes: &Datastore,
    ok_pred: GraphElementId,
    true_id: GraphElementId,
    rdf_type_id: GraphElementId,
    rules: &mut Vec<Rule>,
    work: &mut Datastore,
) {
    use crate::vocab::{SH_CLASS, SH_PROPERTY};
    let inner_id = inner_ref.shapes_id;

    // sh:class C at node level
    if let Some(class_id) = graph::get_object(shapes, inner_id, SH_CLASS)
        && let Some(class_iri) = graph::iri_string(shapes, class_id)
    {
        let class_id_work = graph::intern_iri(work, &class_iri);
        rules.push(Rule {
            head: RuleHead::NormalHead(dgp(
                Term::Variable("n".into()),
                Term::Resource(ok_pred),
                Term::Resource(true_id),
            )),
            body: vec![pos(
                Term::Variable("n".into()),
                Term::Resource(rdf_type_id),
                Term::Resource(class_id_work),
            )],
        });
        return;
    }

    // sh:property [ sh:path P ; sh:minCount 1 ] → ok if node has at least one P value
    for prop_node in graph::get_objects(shapes, inner_id, SH_PROPERTY) {
        use crate::vocab::{SH_MIN_COUNT, SH_PATH};
        if let Some(path_id) = graph::get_object(shapes, prop_node, SH_PATH)
            && let Some(path_iri) = graph::iri_string(shapes, path_id)
        {
            // only handle minCount 1 here
            let min = graph::get_object(shapes, prop_node, SH_MIN_COUNT)
                .and_then(|id| graph::elem_to_u64(shapes, id))
                .unwrap_or(0);
            if min >= 1 {
                let path_id_work = graph::intern_iri(work, &path_iri);
                rules.push(Rule {
                    head: RuleHead::NormalHead(dgp(
                        Term::Variable("n".into()),
                        Term::Resource(ok_pred),
                        Term::Resource(true_id),
                    )),
                    body: vec![pos(
                        Term::Variable("n".into()),
                        Term::Resource(path_id_work),
                        Term::Variable("v".into()),
                    )],
                });
            }
        }
    }
}

// ── Pattern / rule constructors ───────────────────────────────────────────────

fn dgp(s: Term, p: Term, o: Term) -> QuadPattern {
    get_default_graph_pattern(s, p, o)
}

fn pos(s: Term, p: Term, o: Term) -> RuleAtom {
    RuleAtom::PositivePattern(dgp(s, p, o))
}

fn neg(s: Term, p: Term, o: Term) -> RuleAtom {
    RuleAtom::NotPattern(dgp(s, p, o))
}

fn fact(s: GraphElementId, p: GraphElementId, o: GraphElementId) -> Rule {
    Rule {
        head: RuleHead::NormalHead(dgp(Term::Resource(s), Term::Resource(p), Term::Resource(o))),
        body: vec![],
    }
}

/// Intern an `ElemValue` into the working store and return its `GraphElementId`.
pub fn intern_elem(elem: &ElemValue, work: &mut Datastore) -> GraphElementId {
    use dag_rdf::{GraphElement, RdfLiteral, RdfResource};
    use ingress::IriReference as IngIri;
    match elem {
        ElemValue::Iri(iri) => graph::intern_iri(work, iri),
        ElemValue::BlankNode(n) => work
            .resources
            .add_node_resource(RdfResource::AnonymousBlankNode(*n)),
        ElemValue::Literal {
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
                    type_iri: IngIri(dt.clone()),
                    literal: value.clone(),
                }
            } else {
                RdfLiteral::LiteralString(value.clone())
            };
            work.resources.add_resource(GraphElement::GraphLiteral(lit))
        }
    }
}

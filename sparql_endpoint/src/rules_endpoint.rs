/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `POST /{dataset}/rules` — load or replace a dataset's live Datalog ruleset at runtime.
//!
//! Parses the request body with `datalog_parser::parse` (the same parser `--rules`/
//! `Config::initial_rules` uses at startup) and **replaces** the target dataset's ruleset
//! from the caller's point of view: any rule not present in the new body stops firing,
//! and every rule in the new body ends up derived, exactly as if the whole reasoner had
//! been rebuilt from scratch. Internally, when a reasoner already exists for the dataset,
//! only the *delta* between the old and new ruleset is applied via
//! `datalog::IncrementalReasoner::apply_rule_insertions`/`apply_rule_deletions` — see
//! [`docs/plans/RULES_ENDPOINT_INCREMENTAL_568_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/RULES_ENDPOINT_INCREMENTAL_568_PLAN.md).
//! When the incremental path can't accept some part of the delta (or there is no
//! existing reasoner to diff against, or the new ruleset is empty), this falls back to
//! (or unconditionally uses, for the no-prior-reasoner/empty-ruleset cases) the original
//! full rebuild: extensional-only facts are extracted, the store is reset to just those,
//! and a fresh `IncrementalReasoner::new` is built. This fallback is never surfaced to
//! the HTTP caller as an error — from the caller's perspective, "add these rules" always
//! succeeds whenever the combined ruleset is stratifiable, whichever internal path
//! achieves it.
//!
//! This is a full-ruleset-replace, not a per-ruleset-scoped add/delete — see
//! [`docs/plans/RUNTIME_RULESET_ENDPOINT_390_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/RUNTIME_RULESET_ENDPOINT_390_PLAN.md)
//! for the scope rationale. An empty (zero-rule) body clears the dataset's ruleset
//! entirely, equivalent to "unload".
//!
//! Related: [#390](https://github.com/daghovland/rdf-datalog/issues/390),
//! [#469](https://github.com/daghovland/rdf-datalog/issues/469),
//! [#568](https://github.com/daghovland/rdf-datalog/issues/568).

use crate::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use dag_rdf::{Datastore, Quad, QuadTable};
use datalog::{IncrementalReasoner, Rule};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

/// `new_rules - old_rules` and `old_rules - new_rules`, by value equality.
/// Rules present in both sets are left out of both lists — untouched by the
/// caller's request, so the diff-driven update leaves them completely alone.
fn diff_rulesets(old_rules: &[Rule], new_rules: &[Rule]) -> (Vec<Rule>, Vec<Rule>) {
    let old_set: HashSet<&Rule> = old_rules.iter().collect();
    let new_set: HashSet<&Rule> = new_rules.iter().collect();
    let added: Vec<Rule> = new_rules
        .iter()
        .filter(|r| !old_set.contains(r))
        .cloned()
        .collect();
    let removed: Vec<Rule> = old_rules
        .iter()
        .filter(|r| !new_set.contains(r))
        .cloned()
        .collect();
    (added, removed)
}

/// Result of [`apply_ruleset_diff`]. `rebuilt` is internal/test-observability
/// only — never surfaced in the HTTP response, which has the same shape
/// regardless of which internal path ran.
#[derive(Debug, PartialEq, Eq)]
struct RulesetUpdateOutcome {
    rebuilt: bool,
}

/// Reset `store.named_graphs` to just its extensional (base) facts and build
/// a fresh `IncrementalReasoner` over `new_rules`. The full-rebuild path,
/// unconditionally used pre-#568 and now used as a fallback whenever the
/// incremental delta application can't cleanly apply.
fn full_rebuild(
    store: &mut Datastore,
    new_rules: &[Rule],
) -> Result<IncrementalReasoner, datalog::ReasoningError> {
    let base_facts: Vec<Quad> = store.named_graphs.extensional_quads().collect();
    let hint = base_facts.len() as u32;
    store.named_graphs = QuadTable::new(hint);
    for q in base_facts {
        store.named_graphs.add_quad(q);
    }
    IncrementalReasoner::new(new_rules.to_vec(), store)
}

/// Update `reasoner`/`store` so the reasoner's active ruleset matches
/// `new_rules`, via the smallest sound path available.
///
/// Diffs `new_rules` against `reasoner.active_rules()`, then tries applying
/// exactly the delta (deletions before insertions — see
/// [`docs/plans/RULES_ENDPOINT_INCREMENTAL_568_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/RULES_ENDPOINT_INCREMENTAL_568_PLAN.md)
/// §3-4) directly against the live reasoner. If either call returns any
/// `Err` — most commonly `ReasoningError::NotStratifiable` from an insertion
/// #474's design conservatively rejects, but any error is treated the same
/// way — falls back to [`full_rebuild`], which is not dependent on the
/// aborted incremental attempt's state: it only ever reads *extensional*
/// facts (untouched by `apply_rule_insertions`/`apply_rule_deletions`, which
/// only add/remove *derived* facts) and replaces `*reasoner` wholesale.
///
/// `new_rules` being identical to the reasoner's current active ruleset is a
/// no-op (`rebuilt: false`, nothing called).
fn apply_ruleset_diff(
    reasoner: &mut IncrementalReasoner,
    store: &mut Datastore,
    new_rules: &[Rule],
) -> Result<RulesetUpdateOutcome, datalog::ReasoningError> {
    let old_rules = reasoner.active_rules();
    let (added, removed) = diff_rulesets(&old_rules, new_rules);
    if added.is_empty() && removed.is_empty() {
        return Ok(RulesetUpdateOutcome { rebuilt: false });
    }

    let incremental_result = (|| -> Result<(), datalog::ReasoningError> {
        if !removed.is_empty() {
            reasoner.apply_rule_deletions(store, &removed)?;
        }
        if !added.is_empty() {
            reasoner.apply_rule_insertions(store, &added)?;
        }
        Ok(())
    })();

    match incremental_result {
        Ok(()) => Ok(RulesetUpdateOutcome { rebuilt: false }),
        Err(_) => {
            *reasoner = full_rebuild(store, new_rules)?;
            Ok(RulesetUpdateOutcome { rebuilt: true })
        }
    }
}

pub async fn dataset_rules_post(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    if state.config.read_only {
        return (StatusCode::FORBIDDEN, "Server is in read-only mode").into_response();
    }

    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !(ct.is_empty() || ct.contains("text/x-datalog") || ct.contains("text/plain")) {
        return (
            StatusCode::BAD_REQUEST,
            "Content-Type must be text/x-datalog or text/plain",
        )
            .into_response();
    }

    let Some(entry) = state.registry.read().await.get_entry(&name) else {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    };

    let body_str = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8 in rules body").into_response(),
    };

    // Hold the store write lock across parse + rebuild: `datalog_parser::parse`
    // interns new IRIs into the store as it parses, and a failed parse must
    // leave the dataset completely untouched (no partial interning visible,
    // no ruleset change) — see plan doc test
    // `test_post_rules_parse_error_leaves_dataset_untouched`.
    let mut store = entry.store.write().await;
    let rules = match datalog_parser::parse(&body_str, &mut store) {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Datalog parse error: {e}")).into_response();
        }
    };
    let rules_loaded = rules.len();

    if rules_loaded == 0 {
        // Empty ruleset: unload — no reasoner at all, matching the
        // no-`--rules` startup state. Also strips any previously-derived
        // facts back to extensional-only, mirroring the full-rebuild path
        // (there is no diff to apply against "no rules" — this is not an
        // "add these rules" request, so it always takes this unconditional
        // path, never the incremental one).
        let base_facts: Vec<Quad> = store.named_graphs.extensional_quads().collect();
        let hint = base_facts.len() as u32;
        store.named_graphs = QuadTable::new(hint);
        for q in base_facts {
            store.named_graphs.add_quad(q);
        }
        *entry.reasoner.write().await = None;
        return (StatusCode::OK, Json(serde_json::json!({"rules_loaded": 0}))).into_response();
    }

    // Snapshot whether a reasoner already exists (and grab its Arc) before
    // deciding the path: an existing reasoner is updated in place via the
    // diff/incremental-or-fallback path (`apply_ruleset_diff`); a dataset
    // with no reasoner yet has nothing to diff against, so it always takes
    // the unconditional full-rebuild path, matching pre-#568 behavior.
    let existing_reasoner = entry.reasoner.read().await.clone();
    if let Some(reasoner_arc) = existing_reasoner {
        let mut reasoner = reasoner_arc.lock().await;
        if let Err(e) = apply_ruleset_diff(&mut reasoner, &mut store, &rules) {
            return (
                StatusCode::CONFLICT,
                format!("New ruleset is contradictory over the dataset's existing data: {e}"),
            )
                .into_response();
        }
    } else {
        let new_reasoner = match full_rebuild(&mut store, &rules) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::CONFLICT,
                    format!("New ruleset is contradictory over the dataset's existing data: {e}"),
                )
                    .into_response();
            }
        };
        *entry.reasoner.write().await = Some(Arc::new(Mutex::new(new_reasoner)));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"rules_loaded": rules_loaded})),
    )
        .into_response()
}

// ── Unit tests for the diff/dispatch logic (#568) ───────────────────────────
//
// These are deliberately at the `apply_ruleset_diff`/`diff_rulesets` level,
// not HTTP-level: `rebuilt` (which internal path ran) is exactly the kind of
// implementation detail an HTTP black-box test cannot observe without adding
// test-only surface to the response, which would violate "preserve the
// existing response shape" from the issue. Existing HTTP-level tests in
// `sparql_endpoint/tests/runtime_ruleset.rs` remain the regression net for
// status codes, content negotiation, and dataset isolation.
#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::{DEFAULT_GRAPH_ELEMENT_ID, IriReference, QuadPattern, RdfResource, Term};
    use datalog::{RuleAtom, RuleHead};

    /// A store pre-loaded with distinct interned predicates
    /// `base1`/`base2`/`base3`/`blocked` and a single subject/object pair
    /// `a`/`b`, plus (graph id, a, b, base1, base2, base3, blocked).
    #[allow(clippy::type_complexity)]
    fn setup_store() -> (Datastore, u32, u32, u32, u32, u32, u32) {
        let mut ds = Datastore::new(64);
        let g = DEFAULT_GRAPH_ELEMENT_ID;
        let a = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/a".to_string(),
            )));
        let b = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/b".to_string(),
            )));
        let base1 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/base1".to_string(),
            )));
        let base2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/base2".to_string(),
            )));
        let base3 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/base3".to_string(),
            )));
        let _blocked = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/blocked".to_string(),
            )));
        ds.named_graphs.add_quad(Quad {
            triple_id: g,
            subject: a,
            predicate: base1,
            obj: b,
        });
        ds.named_graphs.add_quad(Quad {
            triple_id: g,
            subject: a,
            predicate: base2,
            obj: b,
        });
        ds.named_graphs.add_quad(Quad {
            triple_id: g,
            subject: a,
            predicate: base3,
            obj: b,
        });
        (ds, g, a, b, base1, base2, base3)
    }

    /// `{ ?x from ?y } => { ?x to ?y }`, a simple copy rule for `from`/`to`
    /// predicates supplied by the caller — used to build several disjoint
    /// derivation rules (`derive_rule(from, q1)`, `derive_rule(from, q2)`, …)
    /// that don't interact with each other.
    fn derive_rule(g: u32, from: u32, to: u32) -> Rule {
        Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(to),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(from),
                object: Term::Variable("y".to_string()),
            })],
        }
    }

    fn quad(g: u32, s: u32, p: u32, o: u32) -> Quad {
        Quad {
            triple_id: g,
            subject: s,
            predicate: p,
            obj: o,
        }
    }

    // ── diff_rulesets ────────────────────────────────────────────────────

    #[test]
    fn test_diff_rulesets_pure_addition() {
        let (_ds, g, _a, _b, base1, base2, _base3) = setup_store();
        let q1 = base1; // placeholder distinct id, not queried in this test
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q1);
        let new_rules = [a_rule.clone(), b_rule.clone()];
        let (added, removed) = diff_rulesets(std::slice::from_ref(&a_rule), &new_rules);
        assert_eq!(added, vec![b_rule]);
        assert!(removed.is_empty());
    }

    #[test]
    fn test_diff_rulesets_pure_removal() {
        let (_ds, g, _a, _b, base1, base2, _base3) = setup_store();
        let q1 = base1;
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q1);
        let (added, removed) = diff_rulesets(&[a_rule.clone(), b_rule.clone()], &[a_rule]);
        assert!(added.is_empty());
        assert_eq!(removed, vec![b_rule]);
    }

    #[test]
    fn test_diff_rulesets_mixed() {
        let (_ds, g, _a, _b, base1, base2, base3) = setup_store();
        let q1 = base1;
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q1);
        let c_rule = derive_rule(g, base3, q1);
        let (added, removed) =
            diff_rulesets(&[a_rule.clone(), b_rule.clone()], &[a_rule, c_rule.clone()]);
        assert_eq!(added, vec![c_rule]);
        assert_eq!(removed, vec![b_rule]);
    }

    #[test]
    fn test_diff_rulesets_unchanged_is_empty() {
        let (_ds, g, _a, _b, base1, base2, _base3) = setup_store();
        let q1 = base1;
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q1);
        let (added, removed) = diff_rulesets(
            &[a_rule.clone(), b_rule.clone()],
            &[a_rule.clone(), b_rule.clone()],
        );
        assert!(added.is_empty());
        assert!(removed.is_empty());
    }

    // ── apply_ruleset_diff ──────────────────────────────────────────────

    #[test]
    fn test_apply_ruleset_diff_pure_addition_is_incremental() {
        let (mut ds, g, a, b, base1, base2, _base3) = setup_store();
        let q1 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q1".to_string(),
            )));
        let q2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q2".to_string(),
            )));
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q2);
        let mut reasoner = IncrementalReasoner::new(vec![a_rule.clone()], &mut ds).unwrap();

        let outcome = apply_ruleset_diff(&mut reasoner, &mut ds, &[a_rule, b_rule]).unwrap();

        assert!(
            !outcome.rebuilt,
            "a disjoint pure addition must take the incremental path"
        );
        assert!(ds.named_graphs.contains(&quad(g, a, q1, b)));
        assert!(ds.named_graphs.contains(&quad(g, a, q2, b)));
    }

    #[test]
    fn test_apply_ruleset_diff_pure_removal_is_incremental() {
        let (mut ds, g, a, b, base1, base2, _base3) = setup_store();
        let q1 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q1".to_string(),
            )));
        let q2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q2".to_string(),
            )));
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q2);
        let mut reasoner =
            IncrementalReasoner::new(vec![a_rule.clone(), b_rule.clone()], &mut ds).unwrap();
        assert!(ds.named_graphs.contains(&quad(g, a, q2, b)));

        let outcome = apply_ruleset_diff(&mut reasoner, &mut ds, &[a_rule]).unwrap();

        assert!(
            !outcome.rebuilt,
            "a pure removal must take the incremental path"
        );
        assert!(ds.named_graphs.contains(&quad(g, a, q1, b)));
        assert!(
            !ds.named_graphs.contains(&quad(g, a, q2, b)),
            "removed rule's derived fact must be gone"
        );
    }

    #[test]
    fn test_apply_ruleset_diff_mixed_add_remove_is_incremental() {
        let (mut ds, g, a, b, base1, base2, base3) = setup_store();
        let q1 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q1".to_string(),
            )));
        let q2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q2".to_string(),
            )));
        let q3 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q3".to_string(),
            )));
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q2);
        let c_rule = derive_rule(g, base3, q3);
        let mut reasoner =
            IncrementalReasoner::new(vec![a_rule.clone(), b_rule.clone()], &mut ds).unwrap();

        let outcome = apply_ruleset_diff(&mut reasoner, &mut ds, &[a_rule, c_rule]).unwrap();

        assert!(
            !outcome.rebuilt,
            "a mixed add+remove with no interaction must take the incremental path"
        );
        assert!(ds.named_graphs.contains(&quad(g, a, q1, b)));
        assert!(
            !ds.named_graphs.contains(&quad(g, a, q2, b)),
            "removed rule's derived fact must be gone"
        );
        assert!(ds.named_graphs.contains(&quad(g, a, q3, b)));
    }

    /// An addition that #474's `apply_rule_insertions` rejects
    /// (`NotStratifiable`, because an existing rule negates the new rule's
    /// head) must still succeed from the caller's point of view via the
    /// full-rebuild fallback.
    #[test]
    fn test_apply_ruleset_diff_falls_back_on_not_stratifiable() {
        let (mut ds, g, a, b, _base1, _base2, _base3) = setup_store();
        let p = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/p".to_string(),
            )));
        let blocked = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/blocked".to_string(),
            )));
        let flag = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/flag".to_string(),
            )));
        let source = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/source".to_string(),
            )));
        ds.named_graphs.add_quad(quad(g, a, p, b));

        // Existing rule: flag(x,y) :- p(x,y), NOT blocked(x,y)
        let flag_rule = Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(flag),
                object: Term::Variable("y".to_string()),
            }),
            body: vec![
                RuleAtom::PositivePattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(p),
                    object: Term::Variable("y".to_string()),
                }),
                RuleAtom::NotPattern(QuadPattern {
                    graph: Term::Resource(g),
                    subject: Term::Variable("x".to_string()),
                    predicate: Term::Resource(blocked),
                    object: Term::Variable("y".to_string()),
                }),
            ],
        };
        let mut reasoner = IncrementalReasoner::new(vec![flag_rule.clone()], &mut ds).unwrap();
        assert!(
            ds.named_graphs.contains(&quad(g, a, flag, b)),
            "flag should be derived before the conflicting addition"
        );

        // New rule: blocked(x,y) :- source(x,y) -- an existing rule (flag)
        // negates `blocked`, so `apply_rule_insertions` alone would reject
        // this with NotStratifiable.
        let blocked_rule = derive_rule(g, source, blocked);
        ds.named_graphs.add_quad(quad(g, a, source, b));

        let outcome =
            apply_ruleset_diff(&mut reasoner, &mut ds, &[flag_rule, blocked_rule]).unwrap();

        assert!(
            outcome.rebuilt,
            "conflicting addition must fall back to a full rebuild"
        );
        assert!(
            ds.named_graphs.contains(&quad(g, a, blocked, b)),
            "blocked must be derived by the new rule after the rebuild"
        );
        assert!(
            !ds.named_graphs.contains(&quad(g, a, flag, b)),
            "flag must no longer hold: NOT blocked is now false, matching a from-scratch rebuild"
        );
    }

    #[test]
    fn test_apply_ruleset_diff_unchanged_is_noop() {
        let (mut ds, g, a, b, base1, base2, _base3) = setup_store();
        let q1 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q1".to_string(),
            )));
        let q2 = ds
            .resources
            .add_node_resource(RdfResource::Iri(IriReference(
                "http://example.org/q2".to_string(),
            )));
        let a_rule = derive_rule(g, base1, q1);
        let b_rule = derive_rule(g, base2, q2);
        let mut reasoner =
            IncrementalReasoner::new(vec![a_rule.clone(), b_rule.clone()], &mut ds).unwrap();

        let outcome = apply_ruleset_diff(&mut reasoner, &mut ds, &[a_rule, b_rule]).unwrap();

        assert!(!outcome.rebuilt, "an unchanged ruleset must be a no-op");
        assert!(ds.named_graphs.contains(&quad(g, a, q1, b)));
        assert!(ds.named_graphs.contains(&quad(g, a, q2, b)));
    }
}

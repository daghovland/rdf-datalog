/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression test for issue [#591](https://github.com/daghovland/rdf-datalog/issues/591):
//! an owl:Nothing-triggering `INSERT` sent through the **per-dataset**
//! `/{name}/update` route (`dataset_routes::dataset_update_post`) rolls
//! back the *base store* delta on the 409 it returns, but never calls
//! `apply_reasoner_delta` to also undo the derived-fact delta the
//! reasoner already wrote into the store as a side effect of
//! `apply_prepared_update_with_options`'s internal `apply_insertions`
//! call.
//!
//! The existing owl:Nothing coverage in `constraints.rs` only exercises
//! the root `/sparql` route (`query.rs::run_select_query`), which *does*
//! call `apply_reasoner_delta` on rollback — so this asymmetry between
//! the two routes had no test coverage at all. It is exactly the kind of
//! store/reasoner desync #591 was filed to find: after a rolled-back
//! update through this route, the derived closure in the store no longer
//! matches what a from-scratch rebuild would produce, and future
//! deletions touching the orphaned derived facts' support bookkeeping can
//! misbehave.

mod common;

use dag_rdf::Datastore;
use dag_rdf::{
    DEFAULT_GRAPH_ELEMENT_ID, GraphElement, IriReference, QuadPattern, RdfResource, Term,
};
use datalog::{Rule, RuleAtom, RuleHead};
use std::sync::Arc;
use tokio::sync::RwLock;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";
const EX_FORBIDDEN: &str = "http://example.org/forbidden591";
const EX_VALUE: &str = "http://example.org/value591";
const EX_BAD: &str = "http://example.org/bad591";

/// `?x a owl:Nothing :- ?x ex:forbidden591 ex:value591 .`
fn make_constraint_rule(ds: &mut Datastore) -> Rule {
    let rdf_type = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        RDF_TYPE.to_string(),
    ))));
    let owl_nothing = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        OWL_NOTHING.to_string(),
    ))));
    let ex_forbidden = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_FORBIDDEN.to_string(),
    ))));
    let ex_value = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        EX_VALUE.to_string(),
    ))));
    let g = DEFAULT_GRAPH_ELEMENT_ID;
    Rule {
        head: RuleHead::NormalHead(QuadPattern {
            graph: Term::Resource(g),
            subject: Term::Variable("x".to_string()),
            predicate: Term::Resource(rdf_type),
            object: Term::Resource(owl_nothing),
        }),
        body: vec![RuleAtom::PositivePattern(QuadPattern {
            graph: Term::Resource(g),
            subject: Term::Variable("x".to_string()),
            predicate: Term::Resource(ex_forbidden),
            object: Term::Resource(ex_value),
        })],
    }
}

/// An `INSERT DATA` through `/{dataset}/update` (not the root `/sparql`
/// route) that triggers an `owl:Nothing` constraint violation must be
/// rejected with 409 *and* leave no orphaned derived fact (`owl:Nothing`
/// itself, or anything else the reasoner derived as a side effect) behind
/// in the store — the rollback must undo the reasoner's derived-fact
/// delta, not just the base triples the client sent.
#[tokio::test]
async fn dataset_scoped_update_owl_nothing_rollback_removes_derived_fact_too() {
    let mut ds = Datastore::new(1024);
    let rule = make_constraint_rule(&mut ds);
    let store = Arc::new(RwLock::new(ds));
    let server = common::TestServer::start_with_store_and_rules(store, vec![rule], false).await;

    let update = format!("INSERT DATA {{ <{EX_BAD}> <{EX_FORBIDDEN}> <{EX_VALUE}> . }}");
    let resp = server
        .client
        .post(server.dataset_update_url("ds"))
        .header("content-type", "application/sparql-update")
        .body(update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        409,
        "INSERT through /{{dataset}}/update that triggers owl:Nothing must return 409"
    );

    // The base triple must not be present (this already passed before the fix —
    // the manual store revert in dataset_routes.rs does undo the base delta).
    let ask_base = format!("ASK {{ <{EX_BAD}> <{EX_FORBIDDEN}> <{EX_VALUE}> . }}");
    let body: serde_json::Value = server
        .client
        .get(server.sparql_query_url(&ask_base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "violating base triple must not be present after 409 rollback"
    );

    // The derived owl:Nothing fact must ALSO be gone — this is what the
    // manual store-only revert misses, since it never calls
    // apply_reasoner_delta to undo the reasoner's own derived-fact delta.
    let ask_derived = format!("ASK {{ <{EX_BAD}> <{RDF_TYPE}> <{OWL_NOTHING}> . }}");
    let body: serde_json::Value = server
        .client
        .get(server.sparql_query_url(&ask_derived))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "orphaned derived owl:Nothing fact must not survive the 409 rollback \
         (store/reasoner desync — issue #591)"
    );
}

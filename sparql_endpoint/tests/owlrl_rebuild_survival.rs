/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Companion invariant test for [#319](https://github.com/daghovland/rdf-datalog/issues/319).
//!
//! This test does NOT exercise the actual CLI-wiring bug (the split between
//! `dagalog::apply_ontologies`'s eager, untracked evaluation and
//! `IncrementalReasoner::new`'s own tracked rules) — that is covered by
//! `tests/serve_rules_unification.rs` at the repo root, which calls
//! `dagalog::collect_serve_rules` (the function `src/main.rs`'s `--serve`
//! branch actually calls) directly. This test passes even against the
//! pre-#319-fix `src/main.rs`/`src/lib.rs`, because it constructs its
//! already-merged `Vec<Rule>` by hand rather than going through the CLI's
//! rule-collection path.
//!
//! What THIS test verifies is the reasoner-level invariant the #319 fix
//! relies on: once OWL-RL axiom-derived rules and plain `.datalog`-file
//! rules DO share one `IncrementalReasoner` (one tracked `derived_from`
//! index), a contradiction-triggered `rebuild_from_base` (see
//! `sparql_endpoint::reasoner_delta::apply_reasoner_delta`) must not
//! silently discard intensional quads that came from the axiom-derived
//! rules. It builds one `Vec<Rule>` combining an OWL2RL-compiled rule (via
//! `owl2rl2datalog::owl2datalog`) with a plain hand-written contradiction
//! rule (mirroring a `.datalog`-file rule), hands both to a single
//! `IncrementalReasoner` via `Config::initial_rules`, and confirms the
//! OWL-RL-derived triple survives a genuine, unrelated contradiction
//! elsewhere in the store.

mod common;

use dag_rdf::Datastore;
use dag_rdf::{
    DEFAULT_GRAPH_ELEMENT_ID, GraphElement, IriReference, QuadPattern, RdfResource, Term,
};
use datalog::{Rule, RuleAtom, RuleHead};
use owl2rl2datalog::owl2datalog;
use rdf_owl_translator::rdf2owl;
use std::sync::Arc;
use tokio::sync::RwLock;
use turtle::parse_turtle;

const EX: &str = "http://ex/";

/// Build a "disjoint properties" style contradiction rule, unrelated to the
/// ontology: `Contradiction :- { ?x p2 ?y, ?x p3 ?y }`.
///
/// This mirrors `contradiction_http.rs`'s pattern and stands in for a plain
/// `.datalog`-file rule supplied via `--rules`.
fn make_contradiction_rule(ds: &mut Datastore) -> Rule {
    let p2 = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        format!("{EX}p2"),
    ))));
    let p3 = ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        format!("{EX}p3"),
    ))));
    let g = DEFAULT_GRAPH_ELEMENT_ID;
    Rule {
        head: RuleHead::Contradiction,
        body: vec![
            RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p2),
                object: Term::Variable("y".to_string()),
            }),
            RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: Term::Variable("x".to_string()),
                predicate: Term::Resource(p3),
                object: Term::Variable("y".to_string()),
            }),
        ],
    }
}

/// A genuine contradiction elsewhere in the store must not cause
/// `rebuild_from_base` to silently drop OWL-RL-derived quads that came from
/// axiom-compiled rules the reasoner *does* own (because #319's fix unifies
/// both rule sources into one `IncrementalReasoner`/`Config::initial_rules`
/// pass, mirroring `src/main.rs`'s `--serve` wiring).
#[tokio::test]
async fn test_owlrl_derived_quads_survive_contradiction_rebuild() {
    // TBox: ex:Dog rdfs:subClassOf ex:Animal  =>  compiles (via owl2datalog /
    // `eli::owl2datalog`, RL rule cax-sco) to a rule: type(x,Animal) :- type(x,Dog).
    // ABox: ex:Dave rdf:type ex:Dog  (base data feeding that rule)
    //       ex:Alice ex:p2 ex:Bob  (consistent on its own; feeds the separate
    //       contradiction rule only once ex:Alice ex:p3 ex:Bob is inserted)
    let turtle_data = format!(
        "@prefix ex: <{EX}> .\n\
         @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
         ex:Dog rdfs:subClassOf ex:Animal .\n\
         ex:Dave a ex:Dog .\n\
         ex:Alice ex:p2 ex:Bob .\n"
    );

    let mut ds = Datastore::new(1024);
    parse_turtle(&mut ds, turtle_data.as_bytes()).expect("fixture turtle must parse");

    // Compile the ontology's TBox into a Datalog rule, mirroring
    // `dagalog::compile_ontology_rules` (the fixed function that feeds
    // `IncrementalReasoner::new` instead of eagerly/untracked-materialising).
    let ontology_doc = rdf2owl(&mut ds);
    let mut rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
    assert!(
        !rules.is_empty(),
        "rdfs:subClassOf must compile to at least one OWL2RL rule"
    );

    // A separate, unrelated contradiction rule, mirroring a directly-supplied
    // `.datalog`-file rule (`--rules`). Both sources are merged into one
    // `Vec<Rule>` here, exactly as the fixed `src/main.rs` does for `serve_rules`.
    rules.push(make_contradiction_rule(&mut ds));

    let store = Arc::new(RwLock::new(ds));

    // `start_with_store_and_rules` hands `rules` to `Config::initial_rules`,
    // which `serve_on_listener` uses to build one `IncrementalReasoner` —
    // the real production wiring path, not an isolated unit test.
    let server = common::TestServer::start_with_store_and_rules(store, rules, false).await;

    // Sanity check: the OWL-RL-derived triple exists right after initial
    // materialisation.
    let ask_derived = format!(
        "ASK {{ <{EX}Dave> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{EX}Animal> . }}"
    );
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_derived))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "the OWL-RL-derived triple (Dave rdf:type Animal, from Dave rdf:type Dog + \
         Dog subClassOf Animal) must be present after initial materialisation"
    );

    // INSERT DATA that triggers a genuine, unrelated contradiction: Alice p3
    // Bob, combined with the existing Alice p2 Bob, satisfies the
    // contradiction rule's body. This forces `apply_reasoner_delta` down the
    // `rebuild_from_base` path (unconditionally on any Contradiction).
    let bad_update = format!("INSERT DATA {{ <{EX}Alice> <{EX}p3> <{EX}Bob> . }}");
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(bad_update)
        .send()
        .await
        .expect("POST update must return an HTTP response, not a dropped connection");
    assert!(
        resp.status().is_client_error(),
        "a genuine contradiction from client-supplied data must be reported as 4xx, got {}",
        resp.status()
    );

    // The crux of #319: after the contradiction-triggered rebuild, the
    // OWL-RL-derived triple (from a DIFFERENT rule than the one that fired
    // the contradiction) must still be present. Before the fix, it would
    // have been silently dropped because it came from an untracked,
    // separate materialisation pass the reasoner never registered.
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_derived))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "the OWL-RL-derived triple (Dave rdf:type Animal) must survive the contradiction \
         rebuild — this is the #319 regression"
    );

    // The rejected insert must not be visible.
    let ask_rejected = format!("ASK {{ <{EX}Alice> <{EX}p3> <{EX}Bob> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_rejected))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        !body["boolean"].as_bool().unwrap_or(true),
        "the rejected insert (Alice p3 Bob) must not be visible after rollback"
    );

    // The pre-existing base fact must have survived the rejected transaction.
    let ask_before = format!("ASK {{ <{EX}Alice> <{EX}p2> <{EX}Bob> . }}");
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_before))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "the pre-existing fact (Alice p2 Bob) must survive the rejected transaction"
    );

    // The server (and its reasoner) must still be usable: a subsequent,
    // non-contradictory insert that itself feeds the OWL-RL rule must still
    // derive correctly.
    let good_update = format!(
        "INSERT DATA {{ <{EX}Frank> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{EX}Dog> . }}"
    );
    let resp = server
        .client
        .post(server.sparql_url())
        .header("content-type", "application/sparql-update")
        .body(good_update)
        .send()
        .await
        .expect("POST update failed");
    assert_eq!(
        resp.status(),
        204,
        "server must remain usable after rejecting a contradiction"
    );

    let ask_new_derived = format!(
        "ASK {{ <{EX}Frank> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{EX}Animal> . }}"
    );
    let resp = server
        .client
        .get(server.sparql_query_url(&ask_new_derived))
        .header("accept", "application/sparql-results+json")
        .send()
        .await
        .expect("GET ASK failed");
    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["boolean"].as_bool().unwrap_or(false),
        "the OWL-RL rule must still fire correctly for new data after the rebuild"
    );
}

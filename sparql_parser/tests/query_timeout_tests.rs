/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Cooperative query-timeout cancellation (issue
//! [#372](https://github.com/daghovland/rdf-datalog/issues/372)).
//!
//! Unit tests for `Deadline::check` itself live next to the type, in
//! `sparql_parser::deadline`. This file covers the integration surface:
//! `execute_with_base`'s `timeout` parameter actually aborts a runaway
//! evaluation, an ordinary query with a generous/no timeout is unaffected,
//! and the whole existing suite (`cargo test --workspace`) still passes with
//! no timeout configured anywhere — the primary correctness bar for this
//! change, since it touches the evaluator's hottest, most heavily-tested
//! code paths.

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfResource};
use sparql_parser::{execute_with_base, parse_query, NetworkPolicy, ParserContext, QueryResult};
use std::collections::HashMap;
use std::time::Duration;

fn iri_node(iri: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
}

fn add_quad(ds: &mut Datastore, subject: &str, predicate: &str, object: &str) {
    let s = ds.add_resource(iri_node(subject));
    let p = ds.add_resource(iri_node(predicate));
    let o = ds.add_resource(iri_node(object));
    ds.add_quad(Quad {
        triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
        subject: s,
        predicate: p,
        obj: o,
    });
}

/// A long chain `ex:n0 ex:p ex:n1 ex:p ex:n2 ... ex:p ex:n{len}`, sized
/// generously so that a transitive-closure property path over it (`ex:n0
/// ex:p* ?x`) does real, measurable BFS work — large enough that, if the
/// deadline were never actually checked (a regression this test exists to
/// catch), the query would complete and this test's `is_err()` assertion
/// would visibly fail rather than the test silently passing for the wrong
/// reason.
fn chain_store(len: usize) -> Datastore {
    let mut ds = Datastore::new(4 * len as u32 + 16);
    for k in 0..len {
        add_quad(
            &mut ds,
            &format!("http://example.org/n{k}"),
            "http://example.org/p",
            &format!("http://example.org/n{}", k + 1),
        );
    }
    ds
}

fn parse(query: &str) -> sparql_parser::ast::Query {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (rest, parsed) = parse_query(query, &mut ctx).expect("query should parse");
    assert!(
        rest.trim().is_empty(),
        "parser left input unconsumed: {rest:?} (query: {query:?})"
    );
    parsed
}

/// A transitive-closure property path (`+`/`*`) over a large synthetic graph,
/// run with a 1ms configured timeout, must return `Err` rather than hang or
/// complete. A 1ms budget is effectively "already elapsed" by the time
/// `transitive_closure`'s BFS loop starts checking it (parsing and dataset
/// construction alone dwarf 1ms), so this is not a timing race — it
/// deterministically exercises the "the deadline is respected" path, not "the
/// deadline is respected under specific timing conditions".
#[test]
fn transitive_closure_path_pattern_respects_timeout() {
    let ds = chain_store(50_000);
    let query = parse(
        "PREFIX ex: <http://example.org/> \
         SELECT ?x WHERE { ex:n0 ex:p+ ?x }",
    );

    let result = execute_with_base(
        &query,
        &ds,
        NetworkPolicy::Deny,
        None,
        Some(Duration::from_millis(1)),
    );

    let err = match result {
        Ok(_) => panic!("expected a timeout error, got Ok(..)"),
        Err(e) => e,
    };
    assert!(
        err.contains("timeout"),
        "expected a timeout-shaped error message, got: {err:?}"
    );
}

/// A large Cartesian-product BGP (two independent unbound-triple patterns
/// joined with no shared variable) run with a 1ms timeout must also abort —
/// this exercises the BGP/join chain (`eval_bgp`/`eval_triple_pattern_core`)
/// independently of the property-path/`transitive_closure` chain above.
#[test]
fn cartesian_bgp_respects_timeout() {
    let mut ds = Datastore::new(1024);
    for k in 0..2000 {
        add_quad(
            &mut ds,
            &format!("http://example.org/a{k}"),
            "http://example.org/p",
            &format!("http://example.org/oa{k}"),
        );
        add_quad(
            &mut ds,
            &format!("http://example.org/b{k}"),
            "http://example.org/q",
            &format!("http://example.org/ob{k}"),
        );
    }
    let query = parse(
        "PREFIX ex: <http://example.org/> \
         SELECT ?x ?y WHERE { ?x ex:p ?xo . ?y ex:q ?yo }",
    );

    let result = execute_with_base(
        &query,
        &ds,
        NetworkPolicy::Deny,
        None,
        Some(Duration::from_millis(1)),
    );

    assert!(result.is_err(), "expected a timeout error, got Ok(..)");
}

/// An ordinary query with a generous timeout must produce byte-identical
/// results to the same query with no timeout configured at all — threading
/// the extra `Deadline` parameter through the evaluator must not change any
/// query's actual result.
#[test]
fn generous_timeout_does_not_change_results() {
    let ds = chain_store(20);
    let query = parse(
        "PREFIX ex: <http://example.org/> \
         SELECT ?x WHERE { ex:n0 ex:p+ ?x } ORDER BY ?x",
    );

    let no_timeout = match execute_with_base(&query, &ds, NetworkPolicy::Deny, None, None)
        .expect("query should execute with no timeout")
    {
        QueryResult::Select(r) => r.rows,
        _ => panic!("expected SELECT result"),
    };

    let generous_timeout = match execute_with_base(
        &query,
        &ds,
        NetworkPolicy::Deny,
        None,
        Some(Duration::from_secs(60)),
    )
    .expect("query should execute within a generous timeout")
    {
        QueryResult::Select(r) => r.rows,
        _ => panic!("expected SELECT result"),
    };

    assert_eq!(no_timeout, generous_timeout);
    assert_eq!(
        no_timeout.len(),
        20,
        "sanity: chain of 20 should reach 20 nodes"
    );
}

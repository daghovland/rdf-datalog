/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Pure/sync tests for the `SERVICE` pre-resolution + executor-wiring pieces
//! (issue [#744](https://github.com/daghovland/rdf-datalog/issues/744)):
//! `collect_service_calls`, `service_key`'s key stability, and the
//! `QueryComponent::Service` evaluation arm's join against a pre-resolved
//! map installed via `ServiceResultsGuard`. The async-fetch half (real HTTP,
//! SSRF hardening reuse) lives in `sparql_endpoint`'s own integration test
//! against a mock server — this file only exercises the parts that don't
//! need a network at all.

use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use ingress::NetworkPolicy;
use sparql_parser::{
    collect_service_calls, execute, parse_query, where_clause_of, ParserContext, QueryResult,
    ServiceKey, ServiceResultsGuard, SolutionRow,
};
use std::collections::HashMap;

fn empty_store() -> Datastore {
    Datastore::new(64)
}

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
        base: None,
    }
}

fn iri(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

fn row(pairs: &[(&str, GraphElement)]) -> SolutionRow {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

// ── collect_service_calls ──────────────────────────────────────────────────

#[test]
fn collect_service_calls_finds_constant_endpoint_non_silent_service() {
    let q = "SELECT * WHERE { SERVICE <http://example.org/svc> { ?s ?p ?o } }";
    let (_, query) = parse_query(q, &mut ctx()).expect("should parse");
    let calls = collect_service_calls(where_clause_of(&query));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "http://example.org/svc");
    assert!(calls[0].1.contains("SELECT"));
}

#[test]
fn collect_service_calls_skips_silent_service() {
    let q = "SELECT * WHERE { SERVICE SILENT <http://example.org/svc> { ?s ?p ?o } }";
    let (_, query) = parse_query(q, &mut ctx()).expect("should parse");
    let calls = collect_service_calls(where_clause_of(&query));
    assert!(calls.is_empty(), "SILENT SERVICE must not be collected");
}

#[test]
fn collect_service_calls_skips_variable_endpoint() {
    let q = "SELECT * WHERE { SERVICE ?ep { ?s ?p ?o } }";
    let (_, query) = parse_query(q, &mut ctx()).expect("should parse");
    let calls = collect_service_calls(where_clause_of(&query));
    assert!(
        calls.is_empty(),
        "a variable SERVICE endpoint cannot be statically pre-resolved (#744 out of scope)"
    );
}

#[test]
fn collect_service_calls_recurses_into_optional_union_and_subquery() {
    let q = r#"
        SELECT * WHERE {
          { SELECT * WHERE { SERVICE <http://example.org/sub> { ?s ?p ?o } } }
          OPTIONAL { SERVICE <http://example.org/opt> { ?a ?b ?c } }
          { SERVICE <http://example.org/u1> { ?x ?y ?z } }
          UNION
          { SERVICE <http://example.org/u2> { ?x2 ?y2 ?z2 } }
        }
    "#;
    let (_, query) = parse_query(q, &mut ctx()).expect("should parse");
    let calls = collect_service_calls(where_clause_of(&query));
    let endpoints: Vec<&str> = calls.iter().map(|(e, _)| e.as_str()).collect();
    assert!(
        endpoints.contains(&"http://example.org/sub"),
        "{endpoints:?}"
    );
    assert!(
        endpoints.contains(&"http://example.org/opt"),
        "{endpoints:?}"
    );
    assert!(
        endpoints.contains(&"http://example.org/u1"),
        "{endpoints:?}"
    );
    assert!(
        endpoints.contains(&"http://example.org/u2"),
        "{endpoints:?}"
    );
    assert_eq!(calls.len(), 4);
}

#[test]
fn collect_service_calls_does_not_descend_into_nested_service_body() {
    // The outer SERVICE's body is forwarded as text to the remote endpoint;
    // a SERVICE nested inside it is resolved *there*, not by this walk.
    let q = r#"
        SELECT * WHERE {
          SERVICE <http://example.org/outer> {
            SERVICE <http://example.org/inner> { ?s ?p ?o }
          }
        }
    "#;
    let (_, query) = parse_query(q, &mut ctx()).expect("should parse");
    let calls = collect_service_calls(where_clause_of(&query));
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "http://example.org/outer");
    assert!(
        calls[0].1.contains("http://example.org/inner"),
        "the inner SERVICE must survive as forwarded text: {}",
        calls[0].1
    );
}

// ── End-to-end: ServiceResultsGuard + QueryComponent::Service join ────────

#[test]
fn service_allow_without_pre_resolved_results_still_errors() {
    // Regression guard: an Allow-policy SERVICE call with nothing installed
    // in the thread-local (the pre-#744 behavior, and what every unwired
    // caller still gets) must still error, not silently return empty rows.
    let q = "SELECT * WHERE { SERVICE <http://example.org/svc> { ?s ?p ?o } }";
    let (_, query) = parse_query(q, &mut ctx()).expect("should parse");
    let err = match execute(&query, &empty_store(), NetworkPolicy::Allow) {
        Err(e) => e,
        Ok(_) => panic!("Allow with no pre-resolved map must error"),
    };
    assert!(matches!(
        err,
        sparql_parser::ExecError::ServiceNotImplemented
    ));
}

#[test]
fn service_allow_with_pre_resolved_results_joins_on_shared_variable() {
    // Outer BGP binds ?x to two local resources; SERVICE's pre-resolved
    // results bind ?x/?o for three rows, only two of which share an ?x
    // value with the outer solutions — the natural join must keep exactly
    // those two, dropping the unrelated third pre-resolved row.
    let mut ds = empty_store();
    let local_pred = ds.resources.add_resource(iri("http://example.org/local"));
    let a = ds.resources.add_resource(iri("http://example.org/a"));
    let b = ds.resources.add_resource(iri("http://example.org/b"));
    let x1 = ds.resources.add_resource(iri("http://example.org/x1"));
    let x2 = ds.resources.add_resource(iri("http://example.org/x2"));
    ds.add_quad(dag_rdf::ingress::Quad {
        triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
        subject: a,
        predicate: local_pred,
        obj: x1,
    });
    ds.add_quad(dag_rdf::ingress::Quad {
        triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
        subject: b,
        predicate: local_pred,
        obj: x2,
    });

    let q = r#"
        SELECT ?s ?o WHERE {
          ?s <http://example.org/local> ?x .
          SERVICE <http://example.org/svc> { ?x <http://example.org/remote> ?o }
        }
    "#;
    let (_, query) = parse_query(q, &mut ctx()).expect("should parse");

    let calls = collect_service_calls(where_clause_of(&query));
    assert_eq!(calls.len(), 1);
    let (endpoint, body) = calls[0].clone();

    let pre_resolved_rows = vec![
        row(&[
            ("x", iri("http://example.org/x1")),
            ("o", iri("http://example.org/o1")),
        ]),
        row(&[
            ("x", iri("http://example.org/x2")),
            ("o", iri("http://example.org/o2")),
        ]),
        row(&[
            // Unrelated: no outer solution binds ?x to x3.
            ("x", iri("http://example.org/x3")),
            ("o", iri("http://example.org/o3")),
        ]),
    ];
    let mut map = HashMap::new();
    map.insert(ServiceKey { endpoint, body }, pre_resolved_rows);
    let _guard = ServiceResultsGuard::install(map);

    let result =
        execute(&query, &ds, NetworkPolicy::Allow).expect("pre-resolved SERVICE must succeed");
    let QueryResult::Select(sel) = result else {
        panic!("expected SELECT result");
    };

    let mut pairs: Vec<(String, String)> = sel
        .rows
        .iter()
        .map(|r| {
            (
                r.get("s").unwrap().to_string(),
                r.get("o").unwrap().to_string(),
            )
        })
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            (
                "<http://example.org/a>".to_string(),
                "<http://example.org/o1>".to_string()
            ),
            (
                "<http://example.org/b>".to_string(),
                "<http://example.org/o2>".to_string()
            ),
        ]
    );
}

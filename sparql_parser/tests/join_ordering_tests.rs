/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Locks in the invariant that BGP join reordering (Phase A,
//! `docs/plans/JOIN_REORDERING_PLAN.md`) must rely on: a BGP's result is
//! independent of the order its triple patterns are written in. This must
//! hold *before* reordering is implemented (it's already true of a plain
//! left-to-right nested-loop join) and must keep holding once `eval_bgp`
//! reorders patterns internally.

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfResource};
use sparql_parser::{execute, parse_query, ParserContext, QueryResult, SolutionRow};
use std::collections::HashMap;

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

fn run_query(ds: &Datastore, query: &str) -> Vec<SolutionRow> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, parsed) = parse_query(query, &mut ctx).expect("query should parse");
    match execute(&parsed, ds).expect("query should execute") {
        QueryResult::Select(r) => r.rows,
        QueryResult::Ask(_) | QueryResult::Construct(_) => panic!("expected SELECT result"),
    }
}

/// Order-independent comparison: a row is a set of (var, value) pairs, and
/// the result is a multiset of rows. Row order is unspecified by SPARQL
/// without ORDER BY, so we compare as sorted bags of rows.
fn rows_as_sorted_bag(rows: &[SolutionRow]) -> Vec<Vec<(String, GraphElement)>> {
    let mut bag: Vec<Vec<(String, GraphElement)>> = rows
        .iter()
        .map(|row| {
            let mut entries: Vec<(String, GraphElement)> =
                row.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            entries.sort();
            entries
        })
        .collect();
    bag.sort();
    bag
}

#[test]
fn bgp_result_is_independent_of_triple_pattern_order() {
    let mut ds = Datastore::new(1_000);

    // A small star: several people, each with a name and an age, plus one
    // extra "decoy" predicate so a naive bad join order has real work to do.
    for i in 0..20 {
        let person = format!("http://example.org/person{i}");
        add_quad(
            &mut ds,
            &person,
            "http://example.org/name",
            &format!("\"Name{i}\""),
        );
        add_quad(
            &mut ds,
            &person,
            "http://example.org/age",
            &format!("\"{}\"", 20 + i),
        );
        add_quad(
            &mut ds,
            &person,
            "http://example.org/type",
            "http://example.org/Person",
        );
    }

    // Well-ordered: most selective pattern (rare predicate, here all equally
    // common, but constant object) first.
    let well_ordered = r#"
        SELECT ?p ?name ?age WHERE {
          ?p <http://example.org/type> <http://example.org/Person> .
          ?p <http://example.org/name> ?name .
          ?p <http://example.org/age> ?age .
        }
    "#;

    // Deliberately bad order: same patterns, different textual order.
    let bad_order = r#"
        SELECT ?p ?name ?age WHERE {
          ?p <http://example.org/age> ?age .
          ?p <http://example.org/name> ?name .
          ?p <http://example.org/type> <http://example.org/Person> .
        }
    "#;

    let well_ordered_rows = run_query(&ds, well_ordered);
    let bad_order_rows = run_query(&ds, bad_order);

    assert_eq!(well_ordered_rows.len(), 20);
    assert_eq!(
        rows_as_sorted_bag(&well_ordered_rows),
        rows_as_sorted_bag(&bad_order_rows),
        "reordering triple patterns within a BGP must not change the result set"
    );
}

#[test]
fn bgp_result_independent_of_order_with_disconnected_pattern() {
    // A BGP containing a pattern that shares no variable with the rest
    // (forces a cartesian product) — order must still not matter.
    let mut ds = Datastore::new(1_000);
    add_quad(
        &mut ds,
        "http://example.org/a1",
        "http://example.org/p",
        "http://example.org/o1",
    );
    add_quad(
        &mut ds,
        "http://example.org/a2",
        "http://example.org/p",
        "http://example.org/o2",
    );
    add_quad(
        &mut ds,
        "http://example.org/b1",
        "http://example.org/q",
        "http://example.org/o3",
    );

    let order1 = r#"
        SELECT ?x ?y WHERE {
          ?x <http://example.org/p> ?o .
          ?y <http://example.org/q> ?o2 .
        }
    "#;
    let order2 = r#"
        SELECT ?x ?y WHERE {
          ?y <http://example.org/q> ?o2 .
          ?x <http://example.org/p> ?o .
        }
    "#;

    let rows1 = run_query(&ds, order1);
    let rows2 = run_query(&ds, order2);

    assert_eq!(
        rows1.len(),
        2,
        "cartesian product of 2 ?x matches x 1 ?y match"
    );
    assert_eq!(
        rows_as_sorted_bag(&rows1),
        rows_as_sorted_bag(&rows2),
        "reordering must not change results even for a disconnected (cartesian) BGP"
    );
}

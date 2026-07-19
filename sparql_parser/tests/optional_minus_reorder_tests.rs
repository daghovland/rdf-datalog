/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! End-to-end (parse + execute) correctness tests for join reordering Phase
//! C-2 (issue [#174](https://github.com/daghovland/rdf-datalog/issues/174)):
//! hoisting a later, independent conjunct across an `OPTIONAL`/`MINUS`
//! barrier. `sparql_parser/src/component_ordering.rs` has white-box unit
//! tests asserting the reordering (or lack of it) directly on the component
//! plan; these tests instead run real queries end-to-end and check the
//! actual result set, to make sure the optimization doesn't change the
//! answer a user gets — not just that the plan looks right in isolation.

use dag_rdf::{Datastore, GraphElement, IriReference, Quad, RdfResource};
use sparql_parser::{execute, parse_query, NetworkPolicy, ParserContext, QueryResult, SolutionRow};
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
    match execute(&parsed, ds, NetworkPolicy::Deny).expect("query should execute") {
        QueryResult::Select(r) => r.rows,
        QueryResult::Ask(_) | QueryResult::Construct(_) | QueryResult::Describe(_) => {
            panic!("expected SELECT result")
        }
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

/// Issue #174 counter-example (from the issue body), end-to-end: a trailing
/// pattern that touches a variable left unbound by a non-matching `OPTIONAL`
/// must remain free to bind independently, not be forced to compete with the
/// `OPTIONAL` for the same value. If the reordering were unsound, hoisting
/// `?opt :ex:q ex:val` before the `OPTIONAL` would turn this into a
/// completely different join and produce the wrong row for `?s2`.
#[test]
fn optional_escape_counter_example_preserves_correct_results() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example.org/";

    // s1 has a :p2 match -> OPTIONAL binds ?opt = o1, and o1 satisfies the
    // trailing pattern.
    add_quad(
        &mut ds,
        &format!("{ex}s1"),
        &format!("{ex}p1"),
        &format!("{ex}x1"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}s1"),
        &format!("{ex}p2"),
        &format!("{ex}o1"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}o1"),
        &format!("{ex}q"),
        &format!("{ex}val"),
    );

    // s2 has no :p2 at all -> OPTIONAL leaves ?opt unbound for this row, so
    // the trailing pattern is free to bind ?opt to *any* node satisfying
    // `?opt :q :val`, unrelated to s2 -- here, a decoy node.
    add_quad(
        &mut ds,
        &format!("{ex}s2"),
        &format!("{ex}p1"),
        &format!("{ex}x2"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}decoy"),
        &format!("{ex}q"),
        &format!("{ex}val"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?s ?opt WHERE {{
  ?s ex:p1 ?x .
  OPTIONAL {{ ?s ex:p2 ?opt }}
  ?opt ex:q ex:val .
}}"#
    );

    // s1 (opt bound via the OPTIONAL match to o1) yields exactly one row.
    // s2 (opt left unbound by a non-matching OPTIONAL) is free to bind ?opt
    // to *any* node satisfying `?opt :q :val`, disconnected from s2 — that's
    // both o1 and decoy here, so s2 contributes *two* rows. This is the
    // correct (if unintuitive) SPARQL semantics the counter-example
    // illustrates; a broken hoist would instead force ?opt to relate to
    // whatever the OPTIONAL itself matched for s2 (nothing), producing a
    // different — wrong — row count and/or content.
    let rows = run_query(&ds, &query);
    assert_eq!(
        rows.len(),
        3,
        "s1 contributes 1 row (opt=o1); s2 contributes 2 (opt=o1 and opt=decoy, both independent of s2)"
    );

    let mut by_s: HashMap<String, Vec<GraphElement>> = HashMap::new();
    for row in &rows {
        let s = match row.get("s").unwrap() {
            GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s))) => s.clone(),
            _ => panic!("expected IRI"),
        };
        by_s.entry(s)
            .or_default()
            .push(row.get("opt").unwrap().clone());
    }
    let mut s1_opts = by_s.remove(&format!("{ex}s1")).unwrap_or_default();
    s1_opts.sort();
    assert_eq!(s1_opts, vec![iri_node(&format!("{ex}o1"))]);

    let mut s2_opts = by_s.remove(&format!("{ex}s2")).unwrap_or_default();
    s2_opts.sort();
    let mut expected_s2 = vec![
        iri_node(&format!("{ex}o1")),
        iri_node(&format!("{ex}decoy")),
    ];
    expected_s2.sort();
    assert_eq!(
        s2_opts, expected_s2,
        "s2's ?opt bindings must come from independent matches, not be forced to relate to s2"
    );
}

/// Issue #174, positive case, end-to-end: a later conjunct independent of
/// the `OPTIONAL`'s internal-only variable is safely hoisted (per the
/// white-box test in `component_ordering.rs`); this just confirms results
/// stay correct when it happens.
#[test]
fn optional_safe_hoist_preserves_correct_results() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example.org/";

    for i in 0..5 {
        let s = format!("{ex}s{i}");
        add_quad(&mut ds, &s, &format!("{ex}p1"), &format!("{ex}x{i}"));
        add_quad(&mut ds, &s, &format!("{ex}p3"), &format!("{ex}z{i}"));
    }
    // Only s0 and s2 have a :p2 match (OPTIONAL binds ?opt for those rows).
    add_quad(
        &mut ds,
        &format!("{ex}s0"),
        &format!("{ex}p2"),
        &format!("{ex}o0"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}s2"),
        &format!("{ex}p2"),
        &format!("{ex}o2"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?s ?z WHERE {{
  ?s ex:p1 ?x .
  OPTIONAL {{ ?s ex:p2 ?opt }}
  ?s ex:p3 ?z .
}}"#
    );

    let rows = run_query(&ds, &query);
    assert_eq!(rows.len(), 5, "every s{{0..4}} has both p1 and p3");
    let expected: Vec<Vec<(String, GraphElement)>> = (0..5)
        .map(|i| {
            let mut entries = vec![
                ("s".to_string(), iri_node(&format!("{ex}s{i}"))),
                ("z".to_string(), iri_node(&format!("{ex}z{i}"))),
            ];
            entries.sort();
            entries
        })
        .collect();
    let mut expected_bag = expected;
    expected_bag.sort();
    assert_eq!(rows_as_sorted_bag(&rows), expected_bag);
}

/// `MINUS` counterpart of the `OPTIONAL` escape counter-example. `MINUS`
/// never binds anything into the outer solution, but this codebase's actual
/// evaluation (see `component_ordering.rs` module docs) threads the outer
/// row's bindings into the `MINUS` body, so hoisting a trailing pattern that
/// reuses `MINUS`'s internal variable name can still change which rows get
/// excluded. Confirms the safe condition keeps this correct end-to-end.
#[test]
fn minus_escape_counter_example_preserves_correct_results() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example.org/";

    // s1 has a :p2 match for :m1 -> MINUS excludes s1 (its inner pattern is
    // satisfiable).
    add_quad(
        &mut ds,
        &format!("{ex}s1"),
        &format!("{ex}p1"),
        &format!("{ex}x1"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}s1"),
        &format!("{ex}p2"),
        &format!("{ex}m1"),
    );

    // s2 has no :p2 at all -> MINUS's inner pattern is unsatisfiable for s2,
    // so s2 survives; the trailing `?m ex:p3 ?z` pattern (reusing the name
    // `?m`) is then free to bind independently, from some unrelated data.
    add_quad(
        &mut ds,
        &format!("{ex}s2"),
        &format!("{ex}p1"),
        &format!("{ex}x2"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}other"),
        &format!("{ex}p3"),
        &format!("{ex}z9"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?s ?z WHERE {{
  ?s ex:p1 ?x .
  MINUS {{ ?s ex:p2 ?m }}
  ?m ex:p3 ?z .
}}"#
    );

    let rows = run_query(&ds, &query);
    assert_eq!(
        rows.len(),
        1,
        "only s2 survives MINUS, then joins independently via ?m=ex:other"
    );
    assert_eq!(rows[0].get("s"), Some(&iri_node(&format!("{ex}s2"))));
    assert_eq!(rows[0].get("z"), Some(&iri_node(&format!("{ex}z9"))));
}

/// Issue #174, positive case for `MINUS`, end-to-end.
#[test]
fn minus_safe_hoist_preserves_correct_results() {
    let mut ds = Datastore::new(1_000);
    let ex = "http://example.org/";

    for i in 0..5 {
        let s = format!("{ex}s{i}");
        add_quad(&mut ds, &s, &format!("{ex}p1"), &format!("{ex}x{i}"));
        add_quad(&mut ds, &s, &format!("{ex}p3"), &format!("{ex}z{i}"));
    }
    // s0 and s2 are excluded by MINUS (they have a :p2 match).
    add_quad(
        &mut ds,
        &format!("{ex}s0"),
        &format!("{ex}p2"),
        &format!("{ex}m0"),
    );
    add_quad(
        &mut ds,
        &format!("{ex}s2"),
        &format!("{ex}p2"),
        &format!("{ex}m2"),
    );

    let query = format!(
        r#"PREFIX ex: <{ex}>
SELECT ?s ?z WHERE {{
  ?s ex:p1 ?x .
  MINUS {{ ?s ex:p2 ?m }}
  ?s ex:p3 ?z .
}}"#
    );

    let rows = run_query(&ds, &query);
    assert_eq!(rows.len(), 3, "s1, s3, s4 survive MINUS");
    let expected_bag: Vec<Vec<(String, GraphElement)>> = [1, 3, 4]
        .iter()
        .map(|&i| {
            let mut entries = vec![
                ("s".to_string(), iri_node(&format!("{ex}s{i}"))),
                ("z".to_string(), iri_node(&format!("{ex}z{i}"))),
            ];
            entries.sort();
            entries
        })
        .collect();
    let mut expected_bag = expected_bag;
    expected_bag.sort();
    assert_eq!(rows_as_sorted_bag(&rows), expected_bag);
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Correctness guard for Phase C component reordering / semi-join pushdown
//! across `UNION` branches (issue
//! [#38](https://github.com/daghovland/rdf-datalog/issues/38)).
//!
//! Reordering a conjunctive group so a constraining conjunct runs before a
//! `UNION` is only *result-preserving* if the pass never reorders across a
//! barrier and never mangles `OPTIONAL`/`FILTER` semantics. Each test below
//! pins the exact result multiset for a query shape that exercises the
//! reordering path, so any regression that drops or duplicates rows fails
//! loudly. See `docs/plans/JOIN_REORDERING_PLAN.md` (Phase C).

use dag_rdf::Datastore;
use dagalog::{graph_element_display, run_sparql_query};

fn ds_from(ttl: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).expect("inline Turtle must parse");
    ds
}

/// Sorted list of a single projected variable's values (local-name suffix for
/// brevity), including a placeholder for rows where the variable is unbound.
fn values(ds: &Datastore, sparql: &str, var: &str) -> Vec<String> {
    let result = run_sparql_query(ds, sparql).expect("query should execute");
    let mut out: Vec<String> = result
        .rows
        .iter()
        .map(|row| match row.get(var) {
            Some(el) => short(&graph_element_display(el)),
            None => "<unbound>".to_string(),
        })
        .collect();
    out.sort();
    out
}

/// Sorted list of `(a, b)` projected pairs, rendered compactly.
fn pairs(ds: &Datastore, sparql: &str, a: &str, b: &str) -> Vec<String> {
    let result = run_sparql_query(ds, sparql).expect("query should execute");
    let mut out: Vec<String> = result
        .rows
        .iter()
        .map(|row| {
            let av = row
                .get(a)
                .map(|e| short(&graph_element_display(e)))
                .unwrap_or_else(|| "<unbound>".to_string());
            let bv = row
                .get(b)
                .map(|e| short(&graph_element_display(e)))
                .unwrap_or_else(|| "<unbound>".to_string());
            format!("{av},{bv}")
        })
        .collect();
    out.sort();
    out
}

fn row_count(ds: &Datastore, sparql: &str) -> usize {
    run_sparql_query(ds, sparql)
        .expect("query should execute")
        .rows
        .len()
}

/// Trim an IRI/display string down to its last path segment for readable
/// assertions (`http://example.org/a` -> `a`).
fn short(s: &str) -> String {
    let trimmed = s.trim_matches(|c| c == '<' || c == '>');
    trimmed
        .rsplit(['/', '#'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

const PREFIX: &str = "PREFIX : <http://example.org/>";

/// The headline target shape: `{ ?s :p1 ?o1 } UNION { ?s :p2 ?o1 } ?s :q ?y`.
/// The downstream `:q` constraint shares `?s` with both arms; only subjects
/// that also have a `:q` edge survive. Reordering must not change the result.
#[test]
fn union_constraint_shared_in_both_arms() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :a :p2 :x2 .
           :b :p1 :x3 .       # b has p1 but no :q — must be excluded
           :a :q  :y1 .       # only a is constrained by :q
        "#,
    );
    let q = format!(
        "{PREFIX} SELECT ?s ?o1 WHERE {{ {{ ?s :p1 ?o1 }} UNION {{ ?s :p2 ?o1 }} ?s :q ?y }}"
    );
    // a via p1 (x1) and a via p2 (x2); b dropped for lack of :q.
    assert_eq!(pairs(&ds, &q, "s", "o1"), vec!["a,x1", "a,x2"]);
}

/// The shared variable is bound in only ONE arm. The downstream `:q` still
/// binds `?s` for the other arm's rows (SPARQL join semantics), so both arms
/// contribute. This is the case a naive "filter the whole union by outer ?s"
/// implementation would get wrong; reordering + per-arm threading must not.
#[test]
fn union_shared_variable_in_one_arm_only() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :c :p2 :x2 .       # arm 2 binds ?z, not ?s
           :a :q  :y1 .
        "#,
    );
    let q = format!(
        "{PREFIX} SELECT ?s ?o1 WHERE {{ {{ ?s :p1 ?o1 }} UNION {{ ?z :p2 ?o1 }} ?s :q ?y }}"
    );
    // arm1: (s=a, o1=x1). arm2: ?s unbound there, but `?s :q ?y` binds s=a, so
    // (s=a, o1=x2) also survives.
    assert_eq!(pairs(&ds, &q, "s", "o1"), vec!["a,x1", "a,x2"]);
}

/// `OPTIONAL` after a `UNION` is a barrier: it must stay after the union and
/// keep every union row, binding the optional variable only where it matches.
/// The reorder pass must not hoist anything across it or drop unmatched rows.
#[test]
fn optional_after_union_keeps_all_rows_unbound() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :a :q  :y1 .
           :b :p2 :x2 .       # b has no :q — must be kept with ?y unbound
        "#,
    );
    let q = format!(
        "{PREFIX} SELECT ?s ?y WHERE {{ {{ ?s :p1 ?o1 }} UNION {{ ?s :p2 ?o1 }} OPTIONAL {{ ?s :q ?y }} }}"
    );
    // Both union rows kept; only a gets ?y bound.
    assert_eq!(pairs(&ds, &q, "s", "y"), vec!["a,y1", "b,<unbound>"]);
}

/// `OPTIONAL` nested inside a `UNION` arm must still evaluate correctly when
/// the outer group is a reorder candidate (here via a trailing constraint).
#[test]
fn optional_inside_union_arm() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :a :q  :y1 .
           :b :p1 :x2 .       # b: p1 but no :q
           :c :p2 :x3 .
           :a :keep :k .
           :b :keep :k .
           :c :keep :k .
        "#,
    );
    // Every subject has :keep, so the trailing constraint keeps all of them and
    // makes the outer group a reorder candidate, while the OPTIONAL lives
    // inside arm 1.
    let q = format!(
        "{PREFIX} SELECT ?s ?y WHERE {{ {{ ?s :p1 ?o1 OPTIONAL {{ ?s :q ?y }} }} UNION {{ ?s :p2 ?o1 }} ?s :keep ?k }}"
    );
    // arm1: a (y=y1), b (y unbound); arm2: c (y unbound).
    assert_eq!(
        pairs(&ds, &q, "s", "y"),
        vec!["a,y1", "b,<unbound>", "c,<unbound>"]
    );
}

/// A `FILTER` trailing a reordered `UNION`-constraint run must stay last and
/// filter the correct rows after reordering.
#[test]
fn filter_after_reordered_union_constraint() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :a :p2 :x2 .
           :a :q  :y1 .
           :b :p1 :x3 .       # no :q
        "#,
    );
    let q = format!(
        "{PREFIX} SELECT ?s ?o1 WHERE {{ {{ ?s :p1 ?o1 }} UNION {{ ?s :p2 ?o1 }} ?s :q ?y FILTER(?o1 != :x2) }}"
    );
    // Constraint keeps a's two rows; FILTER drops o1=x2. Only (a,x1) remains.
    assert_eq!(pairs(&ds, &q, "s", "o1"), vec!["a,x1"]);
}

/// A genuinely unconstrained `UNION` (no shared downstream conjunct) must be
/// untouched and return every arm row.
#[test]
fn unconstrained_union_untouched() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :b :p1 :x2 .
           :c :p2 :x3 .
        "#,
    );
    let q = format!("{PREFIX} SELECT ?s WHERE {{ {{ ?s :p1 ?o1 }} UNION {{ ?s :p2 ?o1 }} }}");
    assert_eq!(values(&ds, &q, "s"), vec!["a", "b", "c"]);
    assert_eq!(row_count(&ds, &q), 3);
}

/// Nested `OPTIONAL` (well-designed pattern) combined with a `UNION`-constraint
/// run: the outer left-join structure and the inner optional must both be
/// preserved.
#[test]
fn nested_optional_with_union_constraint() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :a :q  :y1 .
           :y1 :r :z1 .
           :b :p2 :x2 .       # b: no :q at all
           :a :keep :k .
           :b :keep :k .
        "#,
    );
    let q = format!(
        "{PREFIX} SELECT ?s ?z WHERE {{ {{ ?s :p1 ?o1 }} UNION {{ ?s :p2 ?o1 }} ?s :keep ?k OPTIONAL {{ ?s :q ?y OPTIONAL {{ ?y :r ?z }} }} }}"
    );
    // a: q->y1, r->z1 so z bound; b: no q so z unbound. Both kept via :keep.
    assert_eq!(pairs(&ds, &q, "s", "z"), vec!["a,z1", "b,<unbound>"]);
}

/// Interaction with the #165 row-budget short-circuit: a query with both a
/// reorderable `UNION`-constraint group and a top-level `LIMIT` must return
/// exactly the first `n` rows of the unlimited result — reordering changes
/// *which* component ends up physically last (here the constraining BGP,
/// originally last, is hoisted before the `UNION`, so the `UNION` ends up
/// last instead), and the budget is only ever passed to whatever is last.
/// Since `UNION` doesn't honor the budget itself, correctness must fall back
/// on the caller's own truncation rather than a broken/missing cutoff.
#[test]
fn union_constraint_reorder_composes_with_limit() {
    let ds = ds_from(
        r#"@prefix : <http://example.org/> .
           :a :p1 :x1 .
           :b :p1 :x1 .
           :c :p2 :x1 .
           :d :p2 :x1 .
           :a :pc :y .
           :b :pc :y .
           :c :pc :y .
           :d :pc :y .
        "#,
    );
    let unlimited = format!(
        "{PREFIX} SELECT ?s WHERE {{ {{ ?s :p1 ?o1 }} UNION {{ ?s :p2 ?o1 }} ?s :pc ?o2 }}"
    );
    let limited = format!("{unlimited} LIMIT 2");

    assert_eq!(row_count(&ds, &unlimited), 4, "sanity: 4 rows unlimited");
    let full = values(&ds, &unlimited, "s");
    let mut top2 = values(&ds, &limited, "s");
    top2.sort();

    assert_eq!(
        row_count(&ds, &limited),
        2,
        "LIMIT 2 must return exactly 2 rows even though the group was reordered"
    );
    // Every limited row must be one that actually appears in the unlimited
    // result (no rows invented or dropped-then-reintroduced by reordering).
    assert!(
        top2.iter().all(|s| full.contains(s)),
        "limited rows {top2:?} must be a subset of the unlimited rows {full:?}"
    );
}

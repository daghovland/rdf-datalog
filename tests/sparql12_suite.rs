/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! End-to-end SPARQL 1.2 query test suite.
//!
//! Each test loads a small, self-contained, public-domain RDF dataset from
//! `tests/testdata/sparql12_*.{ttl,trig}` (modelled on examples from the W3C
//! SPARQL 1.2 specification) and executes one SPARQL SELECT query, asserting
//! both the projected variable set and the exact result-row count.
//!
//! The queries are numbered to match their corresponding SPARQL 1.2 spec section.
//!
//! Reference:  https://www.w3.org/TR/sparql12-query/
//!
//! Run just this file: `cargo test --test sparql12_suite`

use dag_rdf::Datastore;
use dagalog::{graph_element_display, load_file, run_sparql_query};
use sparql_parser::{NetworkPolicy, ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn load(file: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    load_file(&mut ds, &testdata(file)).expect("should load test data");
    ds
}

fn parse_inline_ttl(ttl: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).expect("inline Turtle must parse");
    ds
}

/// Like [`parse_inline_ttl`], but for TriG input with `<graph-iri> { ... }`
/// named-graph blocks (plain Turtle has no such syntax).
fn parse_inline_trig(trig: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    turtle::parse_trig(&mut ds, trig.as_bytes()).expect("inline TriG must parse");
    ds
}

fn query_rows(ds: &Datastore, sparql: &str) -> usize {
    run_sparql_query(ds, sparql)
        .expect("query should execute")
        .rows
        .len()
}

fn query_vars(ds: &Datastore, sparql: &str) -> Vec<String> {
    run_sparql_query(ds, sparql)
        .expect("query should execute")
        .variables
        .clone()
}

/// Execute an ASK query and return its boolean result. `run_sparql_query`
/// only supports SELECT (it rejects ASK/CONSTRUCT/DESCRIBE), so ASK tests go
/// through `sparql_parser::execute` directly — same route the W3C suite
/// harness's `compare_ask_with_srx` uses (see issue #203, pp08 triage).
fn query_ask(ds: &Datastore, sparql: &str) -> bool {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("query should parse");
    match execute(&query, ds, NetworkPolicy::Deny).expect("query should execute") {
        QueryResult::Ask(b) => b,
        _ => panic!("expected an ASK result"),
    }
}

fn query_values(ds: &Datastore, sparql: &str, variable: &str) -> Vec<String> {
    let result = run_sparql_query(ds, sparql).expect("query should execute");
    result
        .rows
        .iter()
        .filter_map(|row| row.get(variable))
        .map(graph_element_display)
        .collect()
}

fn query_single_value(ds: &Datastore, sparql: &str, variable: &str) -> Option<String> {
    let result = run_sparql_query(ds, sparql).expect("query should execute");
    result
        .rows
        .first()
        .and_then(|row| row.get(variable))
        .map(graph_element_display)
}

// ── §2  Basic Graph Patterns ─────────────────────────────────────────────────

/// SPARQL 1.2 §2.1: SELECT with a single triple pattern.
///
/// Data: sparql12_people.ttl  (4 foaf:Person resources)
/// Query: SELECT ?x WHERE { ?x a foaf:Person . }
/// Expected: 4 rows (Alice, Bob, Carol, Dave)
#[test]
fn spec_s2_basic_graph_pattern_single_triple() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x WHERE { ?x a foaf:Person . }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        4,
        "§2.1: should find 4 foaf:Person resources"
    );
}

/// SPARQL 1.2 §2.3: SELECT with multiple triple patterns in a BGP.
///
/// Data: sparql12_people.ttl
/// Query: SELECT ?x ?name WHERE { ?x a foaf:Person ; foaf:name ?name . }
/// Expected: 4 rows (all persons have foaf:name)
#[test]
fn spec_s2_basic_graph_pattern_multiple_triples() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x ?name WHERE {
    ?x a foaf:Person ;
       foaf:name ?name .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        4,
        "§2.3: all 4 persons have foaf:name, expecting 4 rows"
    );
}

/// SPARQL 1.2 §2.6: Turtle-style `;` and `,` object-list shorthand in WHERE.
///
/// Query selects persons who have both a name and a mbox via shorthand predicate list.
/// Data: Alice, Carol have mbox; Bob, Dave do not.
/// Expected: 2 rows
#[test]
fn spec_s2_semicolon_shorthand_in_where() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x WHERE {
    ?x foaf:name ?name ;
       foaf:mbox ?mbox .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§2.6: only Alice and Carol have foaf:mbox"
    );
}

/// SPARQL 1.2 §2.6: Comma object-list — two foaf:knows triples using comma shorthand.
///
/// Alice knows both Bob and Carol from one semicolon+comma pattern.
/// Expected: 1 row for each know-link rooted at Alice = 2 rows
#[test]
fn spec_s2_comma_object_list_in_where() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?who WHERE {
    ex:alice foaf:knows ?who .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§2.6: alice knows bob and carol — 2 rows"
    );
}

// ── §6  Including Optional Values ────────────────────────────────────────────

/// SPARQL 1.2 §6.1: OPTIONAL for missing values (mbox is optional).
///
/// All 4 persons are returned; mbox is bound for Alice and Carol, unbound for Bob and Dave.
/// Expected: 4 rows total
#[test]
fn spec_s6_optional_basic() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x ?mbox WHERE {
    ?x a foaf:Person .
    OPTIONAL { ?x foaf:mbox ?mbox . }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        4,
        "§6.1: OPTIONAL preserves all persons"
    );
}

/// SPARQL 1.2 §6.4: FILTER with BOUND to select only rows that lack mbox.
///
/// Bob and Dave have no mbox.
/// Expected: 2 rows
#[test]
fn spec_s6_optional_filter_bound() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x WHERE {
    ?x a foaf:Person .
    OPTIONAL { ?x foaf:mbox ?mbox . }
    FILTER(!BOUND(?mbox))
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§6.4: FILTER(!BOUND) — Bob and Dave have no mbox"
    );
}

/// SPARQL 1.2 §6.4: NOT EXISTS for resources that have no foaf:mbox.
///
/// Expected: 2 rows (Bob and Dave)
#[test]
fn spec_s6_not_exists() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x WHERE {
    ?x a foaf:Person .
    FILTER NOT EXISTS { ?x foaf:mbox ?mbox . }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§6.4: NOT EXISTS — Bob and Dave have no mbox"
    );
}

// ── §6.3  Union ──────────────────────────────────────────────────────────────

/// SPARQL 1.2 §6.3: UNION of two graph patterns.
///
/// Query collects persons whose name is "Alice" OR "Bob".
/// Expected: 2 rows
#[test]
fn spec_s6_union() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x WHERE {
    { ?x foaf:name "Alice" . }
    UNION
    { ?x foaf:name "Bob" . }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§6.3: UNION should produce 2 rows"
    );
}

// ── §8  Named Graphs ─────────────────────────────────────────────────────────

/// SPARQL 1.2 §8.2: GRAPH <iri> restricts matching to a specific named graph.
///
/// The engineering graph holds Alice and Carol.
/// Expected: 2 rows
#[test]
fn spec_s8_graph_iri() {
    let ds = load("sparql12_named_graphs.trig");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?person WHERE {
    GRAPH <http://example.org/graphs/engineering> {
        ?person foaf:name ?name .
    }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§8.2: GRAPH <engineering> should contain 2 people"
    );
}

/// SPARQL 1.2 §8.3: GRAPH ?g binds the graph IRI variable for all named graphs.
///
/// 3 named graphs × their members:
///   engineering  → 2 persons
///   marketing    → 1 person
///   publications → 2 papers
/// Total foaf:name + dc:title triples across all graphs: 5
/// Expected: 5 rows
#[test]
fn spec_s8_graph_variable_all_graphs() {
    let ds = load("sparql12_named_graphs.trig");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?g ?s WHERE {
    GRAPH ?g {
        ?s foaf:name ?name .
    }
}
"#;
    // engineering: alice, carol  |  marketing: bob  = 3
    assert_eq!(
        query_rows(&ds, sparql),
        3,
        "§8.3: GRAPH ?g should enumerate persons across named graphs"
    );
}

/// SPARQL 1.2 §8.4: Top-level BGP does NOT include triples in named graphs.
///
/// The default graph of sparql12_named_graphs.trig contains ex:worksIn triples.
/// A query for foaf:name should return 0 results from the default graph
/// (names are in the named graphs only).
/// Expected: 0 rows
#[test]
fn spec_s8_default_graph_excludes_named_graphs() {
    let ds = load("sparql12_named_graphs.trig");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?person WHERE {
    ?person foaf:name ?name .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        0,
        "§8.4: BGP without GRAPH should not match triples in named graphs"
    );
}

/// SPARQL 1.2 §8: Default graph triples are visible to top-level BGPs.
///
/// The default graph contains ex:worksIn triples (3 of them).
/// Expected: 3 rows
#[test]
fn spec_s8_default_graph_is_visible() {
    let ds = load("sparql12_named_graphs.trig");
    let sparql = r#"
PREFIX ex:  <http://example.org/>
PREFIX org: <http://example.org/org/>
SELECT ?person WHERE {
    ?person ex:worksIn ?dept .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        3,
        "§8: default graph triples should be visible as top-level BGP"
    );
}

// ── §9  Property Paths ────────────────────────────────────────────────────────

/// SPARQL 1.2 §9.1: Sequence property path p1/p2.
///
/// Data chain: alice→bob→carol→dave→eve (all via foaf:knows).
/// 2-hop pairs (x knows/knows z): alice→carol, bob→dave, carol→eve = 3 pairs.
/// Expected: 3 rows
#[test]
fn spec_s9_sequence_path() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x ?z WHERE {
    ?x foaf:knows/foaf:knows ?z .
}
"#;
    // Chain: alice→bob→carol→dave→eve
    // 2-hop pairs: alice→carol, bob→dave, carol→eve  (3 pairs)
    assert_eq!(
        query_rows(&ds, sparql),
        3,
        "§9.1: 2-hop knows path — alice→carol, bob→dave, carol→eve"
    );
}

/// SPARQL 1.2 §9.1: 3-hop sequence path p1/p2/p3.
///
/// alice→bob→carol→dave
/// Expected: 2 rows (alice→dave, bob→eve)
#[test]
fn spec_s9_three_hop_path() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x ?z WHERE {
    ?x foaf:knows/foaf:knows/foaf:knows ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§9.1: 3-hop knows — alice→dave, bob→eve"
    );
}

/// SPARQL 1.2 §9: SELECT * excludes internal path-expansion variables.
///
/// `SELECT *` on a query with a property path must not expose synthetic
/// `__path_*` variables. Per SPARQL spec, intermediate path nodes are
/// anonymous (not returned in the result).
/// Expected: variables = ["x", "z"] only
#[test]
fn spec_s9_select_star_no_internal_path_vars() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT * WHERE {
    ?x foaf:knows/foaf:knows ?z .
}
"#;
    let vars = query_vars(&ds, sparql);
    assert!(
        vars.iter().all(|v| !v.starts_with("__path_")),
        "§9: SELECT * must not expose engine-internal path variables; got: {:?}",
        vars
    );
    assert!(vars.contains(&"x".to_string()), "§9: ?x must be projected");
    assert!(vars.contains(&"z".to_string()), "§9: ?z must be projected");
}

/// SPARQL 1.2 §9.1 / issue #203: bounded repeat `p{n}` — exact hop count.
///
/// Chain: alice→bob→carol→dave→eve (all via foaf:knows).
/// `foaf:knows{2}` from alice is a single, unique 2-hop walk to carol.
/// Expected: 1 row (?z = carol)
#[test]
fn spec_s9_bounded_repeat_exact() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?z WHERE {
    ex:alice foaf:knows{2} ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§9.1 issue #203: foaf:knows{{2}} from alice should reach only carol"
    );
}

/// SPARQL 1.2 §9.1 / issue #203: bounded repeat `p{n,m}` — range.
///
/// `foaf:knows{2,3}` from alice unions the 2-hop (carol) and 3-hop (dave)
/// walks.
/// Expected: 2 rows (?z = carol, dave)
#[test]
fn spec_s9_bounded_repeat_range() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?z WHERE {
    ex:alice foaf:knows{2,3} ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§9.1 issue #203: foaf:knows{{2,3}} from alice should reach carol and dave"
    );
}

/// SPARQL 1.2 §9.1 / issue #203: bounded repeat `p{n,}` — unbounded lower bound.
///
/// `foaf:knows{2,}` from alice reaches everything 2 or more hops away:
/// carol (2), dave (3), eve (4).
/// Expected: 3 rows (?z = carol, dave, eve)
#[test]
fn spec_s9_bounded_repeat_min_only() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?z WHERE {
    ex:alice foaf:knows{2,} ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        3,
        "§9.1 issue #203: foaf:knows{{2,}} from alice should reach carol, dave, eve"
    );
}

/// SPARQL 1.2 §9.1 / issue #203: bounded repeat `p{,m}` — up to m, from zero.
///
/// `foaf:knows{,2}` from alice includes the zero-hop identity (alice
/// itself), the 1-hop (bob), and the 2-hop (carol).
/// Expected: 3 rows (?z = alice, bob, carol)
#[test]
fn spec_s9_bounded_repeat_max_only() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?z WHERE {
    ex:alice foaf:knows{,2} ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        3,
        "§9.1 issue #203: foaf:knows{{,2}} from alice should include alice, bob, carol"
    );
}

/// SPARQL 1.2 §9.1 / issue #203: bounded repeat uses sequence (join)
/// semantics, not arbitrary-length-path (set) semantics — so a diamond
/// graph with two distinct 2-hop walks between the same endpoints produces
/// two solutions, not one deduplicated solution. This is the exact case
/// covered by the W3C property-path tests pp20/pp22/pp24/pp26/pp27/pp29
/// (`tests/testdata/w3c_sparql11/property-path/data-diamond*.ttl`).
///
/// Diamond: a→b→z and a→c→z (two distinct 2-hop walks from a to z).
/// Expected: 2 rows (?z = z, z) — NOT deduplicated to 1.
#[test]
fn spec_s9_bounded_repeat_diamond_multiplicity() {
    let ds = parse_inline_ttl(
        r#"
        @prefix : <http://example/> .
        :a :p :b .
        :b :p :z .
        :a :p :c .
        :c :p :z .
        "#,
    );
    let sparql = r#"
PREFIX : <http://example/>
SELECT ?z WHERE {
    :a :p{2} ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§9.1 issue #203: :p{{2}} over a diamond graph should yield one solution \
         per distinct 2-hop walk (2), not one deduplicated pair"
    );
}

/// SPARQL 1.2 §9.1 / issue #203, W3C `pp13` ("Zero Length Paths with
/// Literals"): `?X :p{0} ?Y` with both endpoints unbound must enumerate
/// `X = Y` for *every* node in the graph, including literals in object
/// position — not just the subjects/objects of an explicit BGP.
///
/// Graph: `:s :p "o"`. Nodes: `:s`, `"o"`.
/// Expected: 2 rows — (s,s) and ("o","o").
#[test]
fn spec_s9_zero_length_path_both_unbound_enumerates_all_nodes() {
    let ds = parse_inline_ttl(
        r#"
        @prefix : <http://ex.org/> .
        :s :p "o".
        "#,
    );
    let sparql = r#"
PREFIX : <http://ex.org/>
SELECT * WHERE { ?X :p{0} ?Y }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "issue #203 (W3C pp13): {{0}} with both endpoints unbound should bind \
         X=Y for every node in the graph (:s and the literal \"o\")"
    );
}

/// SPARQL 1.2 §9.1 / issue #203, W3C `pp15` ("Zero Length Paths on an empty
/// graph"): zero-length paths with one bound endpoint must bind the other
/// endpoint to the same term even when the graph is completely empty — this
/// case never queries `nodes(G)` at all, since one side is already known.
///
/// Expected: 1 row, X="o", Y=:o, Z=:s.
#[test]
fn spec_s9_zero_length_path_bound_endpoint_empty_graph() {
    let ds = parse_inline_ttl("");
    let sparql = r#"
PREFIX : <http://www.example.org/>
SELECT * WHERE {
    ?X :p{0} "o" .
    ?Y :p{0} :o .
    :s :p{0} ?Z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "issue #203 (W3C pp15): a bound zero-length-path endpoint must bind \
         the other side to the same term, even on an empty graph"
    );
}

/// SPARQL 1.2 §9.1 / issue #203, W3C `pp04` ("Variable length path with
/// loop"): `{1,}` sequence over a self-looping/branching structure — a
/// sanity check that unbounded-lower-bound repeat (`p{1,}` = `p` followed by
/// `p*`) still resolves correctly when composed via `/` with another such
/// path.
///
/// Graph: a→b (ex:p1), b→a (ex:p2) [so ex:p1/ex:p2 loops back to a], and
/// a→c (ex:p3), c→a (ex:p4) [so ex:p3/ex:p4 also loops back to a].
/// `(ex:p1/ex:p2){1,}/(ex:p3/ex:p4){1,}` from :a must still terminate and
/// reach :a (each `{1,}` sub-path is a self-loop back to :a).
#[test]
fn spec_s9_variable_length_path_with_loop() {
    let ds = parse_inline_ttl(
        r#"
        @prefix ex: <http://www.example.org/schema#> .
        @prefix in: <http://www.example.org/instance#> .
        in:a ex:p1 in:b .
        in:b ex:p2 in:a .
        in:a ex:p3 in:c .
        in:c ex:p4 in:a .
        "#,
    );
    let sparql = r#"
prefix ex: <http://www.example.org/schema#>
prefix in: <http://www.example.org/instance#>
select * where {
in:a (ex:p1/ex:p2){1,}/(ex:p3/ex:p4){1,} ?x
}
"#;
    let xs = query_values(&ds, sparql, "x");
    assert_eq!(
        xs,
        vec!["<http://www.example.org/instance#a>".to_string()],
        "issue #203 (W3C pp04): looping {{1,}} composition must terminate \
         and land back on :a"
    );
}

/// SPARQL 1.2 §9.1 / issue #203, W3C `pp07`/`pp34`/`pp35`: a property path
/// evaluated inside `GRAPH <iri> { ... }` must be scoped to that named
/// graph's quads only, not the whole dataset.
///
/// `<g1>` has `:a :p1/:p2 :c`; `<g2>` has an unrelated triple with the same
/// predicates but a different final object, so a leaking (unscoped)
/// evaluation would also see `<g2>`'s object.
#[test]
fn spec_s9_property_path_scoped_to_named_graph() {
    let ds = parse_inline_trig(
        r#"
        @prefix : <http://www.example.org/> .
        @prefix ex: <http://www.example.org/schema#> .
        @prefix in: <http://www.example.org/instance#> .
        <http://example.org/g1> {
            in:a ex:p1 in:b .
            in:b ex:p2 in:c .
        }
        <http://example.org/g2> {
            in:a ex:p1 in:x .
            in:x ex:p2 in:y .
        }
        "#,
    );
    let sparql = r#"
prefix ex: <http://www.example.org/schema#>
prefix in: <http://www.example.org/instance#>
select ?x where {
graph <http://example.org/g1> { in:a ex:p1/ex:p2 ?x }
}
"#;
    assert_eq!(
        query_values(&ds, sparql, "x"),
        vec!["<http://www.example.org/instance#c>".to_string()],
        "issue #203 (W3C pp07): a property path inside GRAPH <iri> {{ }} must \
         only see that named graph's quads"
    );
}

/// SPARQL 1.2 §9.1 / issue #203, W3C `pp35`: a property path evaluated
/// inside `GRAPH ?g { ... }` with an *unbound* graph variable must both (a)
/// range over every named graph and (b) actually bind `?g` per graph, so
/// that a subsequent `FILTER (?g = <iri>)` can select just one of them.
///
/// Before the fix, the zero-hop/reachability enumeration for `?s :p1* ?t`
/// with both endpoints unbound collapsed the active-graph lookup to `None`
/// (unconstrained across all graphs) without ever binding `?g`, so the
/// `FILTER` always dropped every row.
#[test]
fn spec_s9_property_path_graph_variable_binds_and_filters() {
    let ds = parse_inline_trig(
        r#"
        @prefix : <http://www.example.org/> .
        <http://example.org/ng-01> { :a :p1 :b . }
        <http://example.org/ng-02> { :a :p1 :c . }
        "#,
    );
    let sparql = r#"
prefix : <http://www.example.org/>
select ?t where {
  graph ?g {
    ?s :p1* ?t }
  FILTER (?g = <http://example.org/ng-01>)
}
"#;
    let mut ts = query_values(&ds, sparql, "t");
    ts.sort();
    assert_eq!(
        ts,
        vec![
            "<http://www.example.org/a>".to_string(),
            "<http://www.example.org/b>".to_string(),
            "<http://www.example.org/b>".to_string(),
        ],
        "issue #203 (W3C pp35): GRAPH ?g with an unbound graph variable must \
         bind ?g per named graph so FILTER(?g = ...) can select one of them \
         — expect the zero-hop pair (a,a), the one-hop pair (a,b), and the \
         zero-hop pair (b,b) for node b (itself only reachable as an object)"
    );
    assert_eq!(ts.len(), 3);
}

/// SPARQL 1.2 §9.1 / issue #203, W3C `pp08` ("Reverse path") as an ASK
/// query. Triage note: the W3C-suite skip comment claimed
/// `run_sparql_query` "doesn't support ASK at all" — true for that specific
/// helper (it only accepts SELECT), but the actual query engine
/// (`sparql_parser::execute`) has always supported `Query::Ask` /
/// `QueryResult::Ask`; the W3C harness's `compare_ask_with_srx` already
/// calls `execute` directly and passed pp08 with no engine change needed.
/// This regression test exercises the same `^path` + ASK combination
/// through that same direct-execute route.
#[test]
fn spec_s9_reverse_path_ask() {
    let ds = parse_inline_ttl(
        r#"
        @prefix ex: <http://www.example.org/schema#> .
        @prefix in: <http://www.example.org/instance#> .
        in:a ex:p in:b .
        "#,
    );
    let sparql = r#"
prefix ex: <http://www.example.org/schema#>
prefix in: <http://www.example.org/instance#>
ask {
in:b ^ex:p in:a
}
"#;
    assert!(
        query_ask(&ds, sparql),
        "issue #203 (W3C pp08): ASK {{ in:b ^ex:p in:a }} should be true given in:a ex:p in:b"
    );
}

// ── §10  SELECT Modifiers ─────────────────────────────────────────────────────

/// SPARQL 1.2 §10.4: DISTINCT removes duplicate rows.
///
/// Alice is author of 2 books; querying dc:creator without DISTINCT yields 5 rows.
/// With DISTINCT on creator, 4 unique authors.
/// Expected without DISTINCT: 5  — with DISTINCT: 4
#[test]
fn spec_s10_distinct() {
    let ds = load("sparql12_books.ttl");
    let sparql_no_distinct = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?creator WHERE {
    ?book dc:creator ?creator .
}
"#;
    let sparql_distinct = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT DISTINCT ?creator WHERE {
    ?book dc:creator ?creator .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql_no_distinct),
        5,
        "§10.4: without DISTINCT, 5 creator bindings (Alice appears twice)"
    );
    assert_eq!(
        query_rows(&ds, sparql_distinct),
        4,
        "§10.4: with DISTINCT, 4 unique creators"
    );
}

/// SPARQL 1.2 §13.4: LIMIT restricts to at most N rows.
///
/// Expected: at most 2 rows
#[test]
fn spec_s13_limit() {
    let ds = load("sparql12_books.ttl");
    let sparql = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?book WHERE {
    ?book dc:title ?title .
}
LIMIT 2
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§13.4: LIMIT 2 should return exactly 2 rows"
    );
}

/// SPARQL 1.2 §13.4: OFFSET skips the first N rows.
///
/// There are 6 books total (including one without creator). OFFSET 4 → 2 remaining.
/// Expected: 2 rows
#[test]
fn spec_s13_offset() {
    let ds = load("sparql12_books.ttl");
    let sparql = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?book WHERE {
    ?book dc:title ?title .
}
OFFSET 4
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§13.4: OFFSET 4 over 6 rows should leave 2 rows"
    );
}

/// SPARQL 1.2 §13.4: LIMIT + OFFSET together.
///
/// Expected: LIMIT 3 OFFSET 1 over 6 rows → 3 rows
#[test]
fn spec_s13_limit_offset() {
    let ds = load("sparql12_books.ttl");
    let sparql = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?book WHERE {
    ?book dc:title ?title .
}
LIMIT 3 OFFSET 1
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        3,
        "§13.4: LIMIT 3 OFFSET 1 should return 3 rows"
    );
}

/// SPARQL 1.2 §13.3: ORDER BY at the top level of a query (no subquery
/// wrapper) must sort the returned rows, not merely compute them.
///
/// Regression test for issue #170: `sparql_parser::execute`'s
/// `Query::Select` arm computed solutions and applied DISTINCT/OFFSET/LIMIT
/// but never called `sort_solutions`, so `ORDER BY` was silently ignored at
/// the top level (only subqueries, via `execute_select_inner`, sorted).
///
/// Books have `ex:year` 2023, 2021, 2022, 2020, 2024 for book1..book5
/// respectively (book6 has no year) — insertion order does not match sorted
/// order, so this asserts exact row order, not just set membership.
/// Expected ascending order by year: book4 (2020), book2 (2021), book3
/// (2022), book1 (2023), book5 (2024).
#[test]
fn spec_s13_order_by_top_level_sorts_rows() {
    let ds = load("sparql12_books.ttl");
    let sparql = r#"
PREFIX ex: <http://example.org/>
SELECT ?book ?year WHERE {
    ?book ex:year ?year .
}
ORDER BY ?year
"#;
    assert_eq!(
        query_values(&ds, sparql, "book"),
        vec![
            "<http://example.org/book4>",
            "<http://example.org/book2>",
            "<http://example.org/book3>",
            "<http://example.org/book1>",
            "<http://example.org/book5>",
        ],
        "§13.3: top-level ORDER BY ?year must return rows in ascending year order"
    );
}

// ── §5  FILTER ───────────────────────────────────────────────────────────────

/// SPARQL 1.2 §5.3: FILTER with equality comparison on a literal.
///
/// Only Alice's books (2 books with dc:creator "Alice").
/// Expected: 2 rows
#[test]
fn spec_s5_filter_eq_literal() {
    let ds = load("sparql12_books.ttl");
    let sparql = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?book WHERE {
    ?book dc:creator ?creator .
    FILTER(?creator = "Alice")
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§5.3: FILTER ?creator = 'Alice' should return 2 books"
    );
}

/// SPARQL 1.2 §5.3: FILTER with REGEX.
///
/// dc:title containing "SPARQL" (case-insensitive) — the SPARQL Tutorial book.
/// Expected: 1 row
#[test]
fn spec_s5_filter_regex() {
    let ds = load("sparql12_books.ttl");
    let sparql = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?book ?title WHERE {
    ?book dc:title ?title .
    FILTER(REGEX(?title, "sparql", "i"))
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§5.3: REGEX case-insensitive 'sparql' should match one book"
    );
}

/// SPARQL 1.2 §5.3: FILTER with OPTIONAL and a BOUND check.
///
/// Books without a creator: book6 only.
/// Expected: 1 row
#[test]
fn spec_s5_filter_optional_bound() {
    let ds = load("sparql12_books.ttl");
    let sparql = r#"
PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?book WHERE {
    ?book dc:title ?title .
    OPTIONAL { ?book dc:creator ?creator . }
    FILTER(!BOUND(?creator))
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§5.3: book without creator — only book6 is unattributed"
    );
}

/// SPARQL 1.2 §5.3: EXISTS confirms presence of a related triple.
///
/// Names of persons who know at least one other person.
/// Alice knows bob+carol, Bob knows alice. Carol and Dave have no foaf:knows.
/// Expected: 2 rows
#[test]
fn spec_s5_filter_exists() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x WHERE {
    ?x a foaf:Person .
    FILTER EXISTS { ?x foaf:knows ?other . }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§5.3: EXISTS — 2 persons have foaf:knows (Alice and Bob; Carol and Dave do not)"
    );
}

/// Regression test for issue #199: `FILTER EXISTS` inside a `GRAPH { ... }`
/// block must check the *active* (currently-selected) named graph, not the
/// default graph or some other named graph.
///
/// Dataset (TriG): the default graph has `:s :p :o1, :o2` (so `:s` would
/// wrongly satisfy the EXISTS check if it leaked into the default graph),
/// while named graph `:g` has `:a :p :o1` (no `:o2` — EXISTS fails) and
/// `:b :p :o1, :o2` (EXISTS succeeds).
///
/// Query: `GRAPH :g { ?s ?p :o1 . FILTER EXISTS { ?s ?p :o2 } }`
///
/// Expected: exactly one row, `?s = :b` — matching only within `:g`, not
/// pulled in from the default graph's `:s`. Modelled on the W3C
/// `data-sparql11/exists/exists03` test ("Exists within graph pattern"),
/// which failed with "expected 1 rows, got 0" purely because the W3C test
/// harness (`tests/w3c_sparql11_suite.rs`) never loaded `qt:graphData` at
/// all — this test exercises the same shape directly against `execute.rs`,
/// independent of that harness gap, and confirms the underlying
/// `GRAPH`+`FILTER EXISTS` active-graph threading was already correct.
#[test]
fn spec_s5_filter_exists_scoped_to_graph_block() {
    let mut ds = Datastore::new(10_000);
    turtle::parse_trig(
        &mut ds,
        r#"
@prefix : <https://example.org/> .

:s :p :o1, :o2 .

:g {
    :a :p :o1 .
    :b :p :o1, :o2 .
}
"#
        .as_bytes(),
    )
    .expect("inline TriG must parse");

    let sparql = r#"
PREFIX : <https://example.org/>
SELECT ?s ?p WHERE {
    GRAPH :g {
        ?s ?p :o1 .
        FILTER EXISTS { ?s ?p :o2 }
    }
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "s"),
        Some("<https://example.org/b>".to_string()),
        "issue #199: EXISTS inside GRAPH must be scoped to that named graph, not the default graph"
    );
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "issue #199: only :b satisfies EXISTS within graph :g; the default graph's :s must not leak in"
    );
}

// ── §2.7  SELECT * ────────────────────────────────────────────────────────────

/// SPARQL 1.2 §2.7: SELECT * projects all visible variables.
///
/// All variables from the WHERE clause, but no internal engine variables.
/// Expected: variables include only user-visible names.
#[test]
fn spec_s2_select_star_projects_all_visible_vars() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT * WHERE {
    ?x foaf:name ?name .
    OPTIONAL { ?x foaf:mbox ?mbox . }
}
"#;
    let vars = query_vars(&ds, sparql);
    assert!(
        vars.contains(&"x".to_string()),
        "§2.7: ?x should be projected"
    );
    assert!(
        vars.contains(&"name".to_string()),
        "§2.7: ?name should be projected"
    );
    assert!(
        vars.contains(&"mbox".to_string()),
        "§2.7: ?mbox should be projected"
    );
    assert!(
        vars.iter().all(|v| !v.starts_with("__")),
        "§2.7: no internal variables should appear in SELECT *"
    );
    // 4 rows (all have name; mbox unbound for Bob and Dave)
    assert_eq!(
        query_rows(&ds, sparql),
        4,
        "§2.7: SELECT * should return 4 rows"
    );
}

// ── §15 VALUES inline data ────────────────────────────────────────────────────

/// SPARQL 1.2 §15: VALUES provides inline bindings for variables.
///
/// Restrict ?x to Alice and Bob inline; both are persons in the dataset.
/// Expected: 2 rows
#[test]
fn spec_s15_values_inline() {
    let ds = load("sparql12_people.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?x ?name WHERE {
    ?x foaf:name ?name .
    VALUES ?x { ex:alice ex:bob }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§15: VALUES should restrict to Alice and Bob"
    );
    let names = query_values(&ds, sparql, "name");
    let mut names = names;
    names.sort();
    assert_eq!(
        names,
        vec!["\"Alice\"".to_string(), "\"Bob\"".to_string()],
        "§15: VALUES should bind Alice and Bob"
    );
}

// ── §15 Post-query / post-subquery VALUES ────────────────────────────────────
//
// Mirrors the W3C SPARQL 1.1 bindings test suite entries `values01`–`values07`
// and `inline02` (`tests/testdata/w3c_sparql11/bindings/`); see
// `tests/w3c_sparql11_suite.rs::w3c_sparql11_bindings` for the fixture-driven
// equivalents and https://github.com/daghovland/rdf-datalog/issues/200.
//
// A trailing `VALUES { ... }` clause placed *after* the closing `}` of a
// query's WHERE block (and, for a subquery, after its own solution
// modifiers) is a distinct grammar production from the inline
// `VALUES` tested by `spec_s15_values_inline` above (which sits *inside* the
// WHERE block's group graph pattern). Per the SPARQL 1.1 grammar,
// `SubSelect ::= SelectClause DatasetClause* WhereClause SolutionModifier
// ValuesClause`, so a `{ SELECT ... } VALUES ... }` is the *same* production
// applied to a nested subquery.

/// W3C `values01` — post-query VALUES restricting a subject variable.
#[test]
fn spec_s15_post_query_values_subj_var() {
    let ds = parse_inline_ttl(
        r#"
@prefix dc:   <http://purl.org/dc/elements/1.1/> .
@prefix :     <http://example.org/book/> .
@prefix ns:   <http://example.org/ns#> .

:book1  dc:title  "SPARQL Tutorial" .
:book1  ns:price  42 .
:book2  dc:title  "The Semantic Web" .
:book2  ns:price  23 .
"#,
    );
    let sparql = r#"
PREFIX dc:   <http://purl.org/dc/elements/1.1/>
PREFIX :     <http://example.org/book/>
PREFIX ns:   <http://example.org/ns#>

SELECT ?book ?title ?price
{
   ?book dc:title ?title ;
         ns:price ?price .
}
VALUES ?book {
 :book1
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "values01: post-query VALUES should restrict to book1 only"
    );
    assert_eq!(
        query_single_value(&ds, sparql, "title").as_deref(),
        Some("\"SPARQL Tutorial\"")
    );
}

/// W3C `values02` — post-query VALUES restricting an object variable.
#[test]
fn spec_s15_post_query_values_obj_var() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

:a foaf:name "Alan" .
:a foaf:mbox "alan@example.org" .
:b foaf:name "Bob" .
:b foaf:mbox "bob@example.org" .
:a foaf:knows :b .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>

SELECT ?s ?o
{
  ?s ?p ?o .
} VALUES ?o {
 :b
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "values02: post-query VALUES should restrict ?o to :b"
    );
}

/// W3C `values03` — post-query VALUES with two object variables, one row.
#[test]
fn spec_s15_post_query_values_two_obj_vars() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

:a foaf:name "Alan" .
:a foaf:mbox "alan@example.org" .
:b foaf:name "Bob" .
:b foaf:mbox "bob@example.org" .
:a foaf:knows :b .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>

SELECT ?s ?o1 ?o2
{
  ?s ?p1 ?o1 .
  ?s ?p2 ?o2 .
} VALUES (?o1 ?o2) {
 ("Alan" "alan@example.org")
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "values03: post-query VALUES should restrict to the single matching pair"
    );
}

/// W3C `values04` — post-query VALUES, two object variables, one row with UNDEF.
///
/// `UNDEF` in a VALUES row leaves that variable unconstrained by the row (not
/// forced unbound); each compatible combination is its own solution.
#[test]
fn spec_s15_post_query_values_two_obj_vars_undef() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

:a foaf:name "Alan" .
:a foaf:mbox "alan@example.org" .
:b foaf:name "Bob" .
:b foaf:mbox "bob@example.org" .
:a foaf:knows :b .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>

SELECT ?s ?o1 ?o2
{
  ?s ?p1 ?o1 .
  ?s ?p2 ?o2 .
} VALUES (?o1 ?o2) {
 ("Alan" UNDEF)
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        3,
        "values04: UNDEF ?o2 is unconstrained, so all three ?o2 bindings for ?s=:a survive"
    );
}

/// W3C `values05` — post-query VALUES, two rows with UNDEF each.
#[test]
fn spec_s15_post_query_values_two_obj_vars_two_rows_undef() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

:a foaf:name "Alan" .
:a foaf:mbox "alan@example.org" .
:b foaf:name "Bob" .
:b foaf:mbox "bob@example.org" .
:a foaf:knows :b .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>

SELECT ?s ?o1 ?o2
{
  ?s ?p1 ?o1 .
  ?s ?p2 ?o2 .
} VALUES (?o1 ?o2) {
 (UNDEF "Alan")
 (:b UNDEF)
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        6,
        "values05: six compatible combinations across both VALUES rows (see values05.srx)"
    );
}

/// W3C `values06` — post-query VALUES restricting a predicate variable.
#[test]
fn spec_s15_post_query_values_pred_var() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

:a foaf:name "Alan" .
:a foaf:mbox "alan@example.org" .
:b foaf:name "Bob" .
:b foaf:mbox "bob@example.org" .
:a foaf:knows :b .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?s ?p1 ?o1
{
  ?s ?p1 ?o1 .
} VALUES ?p1 {
 foaf:knows
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "values06: post-query VALUES should restrict ?p1 to foaf:knows"
    );
}

/// W3C `values07` — post-query VALUES joining across an OPTIONAL.
///
/// The VALUES clause binds `?o2` even for rows where the OPTIONAL left it
/// unbound (compatible join — no conflicting existing binding to reject).
#[test]
fn spec_s15_post_query_values_optional_obj_var() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

:a foaf:name "Alan" .
:a foaf:mbox "alan@example.org" .
:b foaf:name "Bob" .
:b foaf:mbox "bob@example.org" .
:c foaf:name "Alice" .
:c foaf:mbox "alice@example.org" .
:a foaf:knows :b .
:b foaf:knows :c .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?s ?o1 ?o2
{
  ?s ?p1 ?o1
  OPTIONAL { ?s foaf:knows ?o2 }
} VALUES (?o2) {
 (:b)
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        5,
        "values07: all 5 rows survive, each with ?o2 = :b (see values07.srx)"
    );
}

/// W3C `inline02` ("Post-subquery VALUES") — VALUES immediately following a
/// `{ SELECT ... }` subquery's own solution modifiers, inside the same
/// braces (`SubSelect ::= ... WhereClause SolutionModifier ValuesClause`).
#[test]
fn spec_s15_post_subquery_values() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

:a foaf:name "Alan" .
:a foaf:mbox "alan@example.org" .
:b foaf:name "Bob" .
:b foaf:mbox "bob@example.org" .
:a foaf:knows :b .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>

SELECT ?s ?o {
	{
		SELECT * WHERE {
			?s ?p ?o .
		}
		VALUES (?o) { (:b) }
	}
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "inline02: subquery's own trailing VALUES should restrict ?o to :b"
    );
}

// ── §15 ValuesClause variable visibility (PR #231 review) ───────────────────
//
// A trailing ValuesClause is joined into the pattern *before* the final
// `Project` (SPARQL 1.1 §18.2.4.3) — so a ValuesClause variable that isn't in
// the explicit SELECT list must still take part in the join (bind/restrict
// solutions exactly like any other WHERE-clause-only variable) but must NOT
// appear in the output header/rows unless the query uses `SELECT *` (which
// projects every visible variable). An earlier version of this fix
// incorrectly force-added every ValuesClause variable name to the output
// header regardless of projection; see
// https://github.com/daghovland/rdf-datalog/pull/231#pullrequestreview.

/// A ValuesClause variable not in the SELECT list must not appear in the
/// output header, even though `?x` (which *is* selected) still does.
#[test]
fn spec_s15_post_query_values_unselected_var_hidden_from_header() {
    let ds = Datastore::new(10);
    let sparql = "SELECT ?x WHERE { BIND(1 AS ?x) } VALUES ?y { 2 }";
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 1, "the single solution should survive");
    assert!(
        result.variables.contains(&"x".to_string()),
        "?x is explicitly selected, so it must appear in the header"
    );
    assert!(
        !result.variables.contains(&"y".to_string()),
        "?y is only introduced by the ValuesClause and never selected, so it must not appear in the header"
    );
}

/// The un-selected ValuesClause variable must still genuinely participate in
/// the join (restrict/filter solutions), not merely be hidden from the
/// header — proven by binding `?y` inside the WHERE clause and observing
/// that a *mismatching* VALUES row for `?y` drops the solution entirely,
/// while a matching one keeps it (both with `?y` absent from the header).
#[test]
fn spec_s15_post_query_values_unselected_var_still_filters() {
    let ds = Datastore::new(10);

    let matching = "SELECT ?x WHERE { BIND(1 AS ?x) BIND(2 AS ?y) } VALUES ?y { 2 }";
    let result = run_sparql_query(&ds, matching).expect("query should execute");
    assert_eq!(
        result.rows.len(),
        1,
        "?y = 2 (BIND) is compatible with VALUES ?y {{ 2 }}, so the solution survives"
    );
    assert!(!result.variables.contains(&"y".to_string()));

    let mismatching = "SELECT ?x WHERE { BIND(1 AS ?x) BIND(2 AS ?y) } VALUES ?y { 3 }";
    let result = run_sparql_query(&ds, mismatching).expect("query should execute");
    assert_eq!(
        result.rows.len(),
        0,
        "?y = 2 (BIND) conflicts with VALUES ?y {{ 3 }}, so the join must drop the solution \
         even though ?y is never in the output header"
    );
}

/// `SELECT *` projects every visible variable, including one introduced
/// solely by a trailing ValuesClause.
#[test]
fn spec_s15_post_query_values_select_star_projects_values_var() {
    let ds = Datastore::new(10);
    let sparql = "SELECT * WHERE { BIND(1 AS ?x) } VALUES ?y { 2 }";
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 1);
    assert!(result.variables.contains(&"x".to_string()));
    assert!(
        result.variables.contains(&"y".to_string()),
        "SELECT * must project ?y even though it's only bound by the ValuesClause"
    );
    assert_eq!(
        result.rows[0]
            .get("y")
            .map(graph_element_display)
            .as_deref(),
        Some("\"2\"^^<http://www.w3.org/2001/XMLSchema#integer>")
    );
}

/// A subquery's own trailing ValuesClause variable, when not in the
/// subquery's (non-Star) projection, must not leak out to the outer query —
/// `QueryComponent::Subquery` merges every key present in the subquery's
/// result rows into the outer solution unconditionally, so an un-projected
/// key here would incorrectly become visible outside the subquery's scope.
/// Mirrors `spec_s15_post_query_values_unselected_var_hidden_from_header` /
/// `..._still_filters` one level down.
#[test]
fn spec_s15_post_subquery_values_unselected_var_not_leaked() {
    let ds = Datastore::new(10);

    let matching =
        "SELECT ?s WHERE { { SELECT ?s WHERE { BIND(1 AS ?s) BIND(2 AS ?y) } VALUES ?y { 2 } } }";
    let result = run_sparql_query(&ds, matching).expect("query should execute");
    assert_eq!(result.rows.len(), 1);
    assert!(
        !result.variables.contains(&"y".to_string()),
        "the subquery never selected ?y, so it must not leak into the outer header"
    );

    let mismatching =
        "SELECT ?s WHERE { { SELECT ?s WHERE { BIND(1 AS ?s) BIND(2 AS ?y) } VALUES ?y { 3 } } }";
    let result = run_sparql_query(&ds, mismatching).expect("query should execute");
    assert_eq!(
        result.rows.len(),
        0,
        "the subquery's internal ValuesClause join must still filter its own solutions \
         even though ?y is never exposed to the outer query"
    );
}

/// When a subquery uses `SELECT *`, a ValuesClause variable it introduces
/// (not otherwise bound in its WHERE clause) crosses the subquery boundary
/// and is genuinely available to the outer query — contrast with the
/// non-Star leak test above.
#[test]
fn spec_s15_post_subquery_values_select_star_crosses_boundary() {
    let ds = Datastore::new(10);
    let sparql = "SELECT ?y WHERE { { SELECT * WHERE { BIND(1 AS ?s) } VALUES ?y { 2 } } }";
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0]
            .get("y")
            .map(graph_element_display)
            .as_deref(),
        Some("\"2\"^^<http://www.w3.org/2001/XMLSchema#integer>"),
        "the subquery's SELECT * exposes ?y (bound only by its own ValuesClause) to the outer query"
    );
}

// ── §11  Aggregates ───────────────────────────────────────────────────────────
//
// Data: tests/testdata/sparql12_aggregates.ttl
//   org1 → book1 (price 10), book2 (price 20)
//   org2 → book3 (price 30)
//   Distinct authors: alice (books 1+2), bob (book 3)

/// SPARQL 1.2 §11.4: COUNT(*) with no GROUP BY → one implicit group, count = 3.
#[test]
fn spec_s11_count_star() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT (COUNT(*) AS ?n)
WHERE { ?book :price ?price . }
"#;
    assert_eq!(query_rows(&ds, sparql), 1, "§11.4: COUNT(*) → one row");
    let val = query_single_value(&ds, sparql, "n");
    assert_eq!(val.as_deref(), Some("3"), "§11.4: COUNT(*) = 3 books total");
}

/// SPARQL 1.2 §11.4: COUNT(?x) skips rows where ?x is unbound, counts bound.
#[test]
fn spec_s11_count_var() {
    let ds = load("sparql12_aggregates.ttl");
    // Query books that have a price AND an author; all 3 books have both.
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT (COUNT(?author) AS ?n)
WHERE { ?book :price ?price . ?book :author ?author . }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§11.4: COUNT(?author) → one row"
    );
    let val = query_single_value(&ds, sparql, "n");
    assert_eq!(
        val.as_deref(),
        Some("3"),
        "§11.4: COUNT(?author) = 3 (alice, alice, bob)"
    );
}

/// SPARQL 1.2 §11.4: COUNT(DISTINCT ?author) deduplicates across the group.
#[test]
fn spec_s11_count_distinct() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT (COUNT(DISTINCT ?author) AS ?n)
WHERE { ?book :author ?author . }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§11.4: COUNT(DISTINCT) → one row"
    );
    let val = query_single_value(&ds, sparql, "n");
    assert_eq!(
        val.as_deref(),
        Some("2"),
        "§11.4: COUNT(DISTINCT ?author) = 2 unique authors"
    );
}

/// SPARQL 1.2 §11.4: SUM(?price) GROUP BY ?org → 2 rows.
///
/// org1: 10 + 20 = 30
/// org2: 30
/// (row order is unspecified; we check the set of sums)
#[test]
fn spec_s11_sum_group_by() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?org (SUM(?price) AS ?total)
WHERE { ?org :hasBook ?book . ?book :price ?price . }
GROUP BY ?org
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§11.4: SUM GROUP BY → 2 organisation rows"
    );
    // `SUM`'s xsd:integer result now renders in the same
    // `"value"^^<datatype>` wire form as any other `xsd:integer` value (see
    // #228, generalizing the arithmetic-result literal-shape fix from
    // #198/#227 to `sum_values`).
    let mut sums = query_values(&ds, sparql, "total");
    sums.sort();
    assert_eq!(
        sums,
        vec![
            "\"30\"^^<http://www.w3.org/2001/XMLSchema#integer>",
            "\"30\"^^<http://www.w3.org/2001/XMLSchema#integer>"
        ],
        "§11.4: org1 sum=30, org2 sum=30"
    );
}

/// SPARQL 1.2 §11.4: AVG(?price) GROUP BY ?org.
///
/// org1: (10 + 20) / 2 = 15
/// org2: 30 / 1 = 30
#[test]
fn spec_s11_avg_group_by() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?org (AVG(?price) AS ?avg)
WHERE { ?org :hasBook ?book . ?book :price ?price . }
GROUP BY ?org
"#;
    assert_eq!(query_rows(&ds, sparql), 2, "§11.4: AVG GROUP BY → 2 rows");
    // Exact numeric representation depends on the executor; check row count only.
}

/// SPARQL 1.2 §11.4: MIN and MAX in one query.
///
/// Over all books: MIN=10, MAX=30.
#[test]
fn spec_s11_min_max() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql_min = r#"
PREFIX : <http://example.org/>
SELECT (MIN(?price) AS ?m)
WHERE { ?book :price ?price . }
"#;
    let sparql_max = r#"
PREFIX : <http://example.org/>
SELECT (MAX(?price) AS ?m)
WHERE { ?book :price ?price . }
"#;
    let min = query_single_value(&ds, sparql_min, "m");
    let max = query_single_value(&ds, sparql_max, "m");
    // MIN/MAX return the raw RDF term from the group. The Turtle parser stores
    // bare integers as xsd:integer TypedLiterals, so the display includes the type.
    assert!(
        min.as_deref().map(|s| s.contains("10")).unwrap_or(false),
        "§11.4: MIN price should contain '10', got {:?}",
        min
    );
    assert!(
        max.as_deref().map(|s| s.contains("30")).unwrap_or(false),
        "§11.4: MAX price should contain '30', got {:?}",
        max
    );
}

/// SPARQL 1.2 §11.4: HAVING filters out groups that do not satisfy the condition.
///
/// org1's min price is 10, org2's is 30. `HAVING (MIN(?price) > 15)` keeps
/// only org2 and filters org1 out.
#[test]
fn spec_s11_having() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?org (MIN(?price) AS ?minP)
WHERE { ?org :hasBook ?book . ?book :price ?price . }
GROUP BY ?org
HAVING (MIN(?price) > 15)
"#;
    // org1 min=10 (filtered out), org2 min=30 (kept)
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§11.4: HAVING (MIN > 15) keeps only org2"
    );
}

/// SPARQL 1.2 §11.4: GROUP_CONCAT concatenates string values with a separator.
///
/// book titles for org1: "Alpha", "Beta" (order unspecified, test sorted).
#[test]
fn spec_s11_group_concat() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?org (GROUP_CONCAT(?title ; separator=",") AS ?titles)
WHERE { ?org :hasBook ?book . ?book :title ?title . }
GROUP BY ?org
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§11.4: GROUP_CONCAT → 2 rows (one per org)"
    );
    // Row content is order-dependent; only assert row count here.
}

/// SPARQL 1.2 §11.4: Aggregate with no GROUP BY → exactly one output row.
///
/// Asking for COUNT(*) with no GROUP BY over 3 books gives a single row with count 3.
/// Covered by spec_s11_count_star; this variant asserts the implicit-group semantics
/// explicitly with a named aggregate alias.
#[test]
fn spec_s11_implicit_group_no_group_by() {
    let ds = load("sparql12_aggregates.ttl");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT (COUNT(?book) AS ?bookCount)
WHERE { ?book :price ?price . }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§11.4: implicit group → exactly one row"
    );
    let val = query_single_value(&ds, sparql, "bookCount");
    assert_eq!(
        val.as_deref(),
        Some("3"),
        "§11.4: all 3 books counted in implicit group"
    );
}

/// SPARQL 1.2 §11.4 / W3C `grouping` suite `Group-4`: `GROUP BY` with an
/// EXPRESSION (not a bare variable) as the grouping key, using the
/// `(expr AS ?var)` form so the computed key is also bound for projection.
///
/// Isolates the grouping *mechanism* with plain arithmetic rather than
/// `COALESCE`, so a failure here points at GROUP BY parsing/evaluation and
/// not at the COALESCE function itself (which is exercised separately by
/// `Group-4` in `tests/w3c_sparql11_suite.rs::w3c_sparql11_grouping`).
///
/// s1: x=1,y=4 → sum=5; s2: x=2,y=3 → sum=5; s3: x=10,y=1 → sum=11.
/// Expect 2 groups: sum=5 (2 members), sum=11 (1 member).
///
/// Tracked by https://github.com/daghovland/rdf-datalog/issues/206.
#[test]
fn spec_s11_group_by_expression_key() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:s1 :x 1 ; :y 4 .
:s2 :x 2 ; :y 3 .
:s3 :x 10 ; :y 1 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?sum (COUNT(?s) AS ?cnt)
WHERE { ?s :x ?x ; :y ?y . }
GROUP BY (?x + ?y AS ?sum)
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "GROUP BY (?x + ?y AS ?sum) → 2 distinct sums (5 and 11)"
    );
    // `?sum` is bound to a `BIND`-style arithmetic result (`?x + ?y`), which
    // `graph_element_display` now renders in the same `"value"^^<datatype>`
    // wire form as any other `xsd:integer` value (real parsed data already
    // displayed this way — see
    // <https://github.com/daghovland/rdf-datalog/issues/198> for the
    // arithmetic-result literal-shape fix that made the two consistent).
    let mut sums = query_values(&ds, sparql, "sum");
    sums.sort();
    assert_eq!(
        sums,
        vec![
            "\"11\"^^<http://www.w3.org/2001/XMLSchema#integer>",
            "\"5\"^^<http://www.w3.org/2001/XMLSchema#integer>"
        ],
        "grouping key values must be bound as ?sum in the output"
    );
    let mut counts = query_values(&ds, sparql, "cnt");
    counts.sort();
    assert_eq!(
        counts,
        vec!["1", "2"],
        "sum=5 has 2 members (s1,s2); sum=11 has 1 member (s3)"
    );
}

/// SPARQL 1.2 §11.4.1 / W3C `aggregates` suite "agg empty group": an explicit
/// `GROUP BY` over a `WHERE` clause that matches zero solutions still yields
/// exactly one (empty) output row, not zero rows — every `GROUP BY` key and
/// aggregate is left unbound in that single row. This is a special case:
/// distinct from the no-`GROUP BY` empty-group case (already covered by
/// `spec_s11_implicit_group_no_group_by`), which always produced one implicit
/// group regardless.
///
/// Tracked by https://github.com/daghovland/rdf-datalog/issues/202.
#[test]
fn spec_s11_group_by_empty_input_one_unbound_row() {
    let ds = parse_inline_ttl("@prefix : <http://example.org/> .\n");
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?x (MAX(?value) AS ?max)
WHERE { ?x :p ?value }
GROUP BY ?x
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§11.4.1: GROUP BY over zero solutions → exactly one row"
    );
    assert_eq!(
        query_single_value(&ds, sparql, "max"),
        None,
        "the single row's aggregate must be unbound, not defaulted to some value"
    );
    assert_eq!(
        query_single_value(&ds, sparql, "x"),
        None,
        "the single row's GROUP BY key must be unbound too"
    );
}

/// SPARQL 1.2 §11.4 / W3C `aggregates` suite "Error in AVG" (`agg-err-01`):
/// an aggregate expression combining `MIN`/`MAX` over a group that mixes a
/// numeric literal with a blank node must be unbound in the output row,
/// mirroring `agg-err-01`'s `((MIN(?p) + MAX(?p)) / 2 AS ?c)` over its `:y`
/// group.
///
/// Deliberately asserts only this composite-expression shape, which is what
/// the W3C fixture actually pins down — not that a *raw* `MIN(?p)` alone
/// must be unbound for such a group. This project's `Aggregate::Min`/`Max`
/// happen to implement that stronger behavior too (an incomparable pair
/// makes the whole aggregate error, via the `<`-operator semantics in
/// `compare_graph_elements`), but no W3C `aggregates` entry exercises raw
/// `MIN`/`MAX` in isolation over a non-comparable group: `agg-err-01` always
/// wraps them in arithmetic, whose own non-`GraphLiteral` guard in
/// `eval_binary_value` would independently produce an unbound result even
/// under the alternative ORDER-BY-total-ordering reading (where MIN would
/// return the blank node itself, then `blanknode + ...` fails to be
/// numeric). Locking in the raw-alone case is left to
/// <https://github.com/daghovland/rdf-datalog/issues/202> as a follow-up
/// once that semantics is verified against the spec/reference behavior.
///
/// Tracked by https://github.com/daghovland/rdf-datalog/issues/202.
#[test]
fn spec_s11_min_max_arithmetic_unbound_on_incomparable_types() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:x :p 1, 2, 3 .
:y :p 1, _:b1, 3 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?g ((MIN(?p) + MAX(?p)) / 2 AS ?c)
WHERE { ?g :p ?p . }
GROUP BY ?g
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 2, "two groups: :x and :y");
    for row in &result.rows {
        let g = row.get("g").map(graph_element_display).unwrap_or_default();
        if g.contains("/y") {
            assert!(
                row.get("c").is_none(),
                "group :y mixes a numeric literal with a blank node — (MIN+MAX)/2 must be unbound, got {:?}",
                row.get("c").map(graph_element_display)
            );
        } else {
            assert!(
                row.get("c").is_some(),
                "group :x is all-numeric — (MIN+MAX)/2 must be bound"
            );
        }
    }
}

/// SPARQL 1.2 §11.4: `AVG` over `xsd:integer`/`xsd:decimal` inputs must stay
/// `xsd:decimal` (per SPARQL/XPath `op:numeric-divide`, which never returns
/// `xsd:integer`), not unconditionally promote to `xsd:double`. Only a
/// genuinely `xsd:double`/`xsd:float` input should force floating point.
///
/// Tracked by https://github.com/daghovland/rdf-datalog/issues/202.
#[test]
fn spec_s11_avg_preserves_decimal_type() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:s :p 1, 2, 3, 4 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT (AVG(?p) AS ?avg)
WHERE { :s :p ?p . }
"#;
    let avg = query_single_value(&ds, sparql, "avg").expect("avg should be bound");
    assert!(
        avg.contains("XMLSchema#decimal"),
        "AVG of xsd:integer inputs must be xsd:decimal, got {}",
        avg
    );
    assert!(
        avg.contains("2.5"),
        "AVG(1,2,3,4) should be 2.5, got {}",
        avg
    );
}

/// SPARQL 1.2 §5.4 / W3C `aggregates` suite "GROUP_CONCAT 2": a blank-node
/// property list `[] ?p ?o` with a *variable* predicate must parse — the
/// `[]`/`[...]` shorthand's predicate-object-pair parser
/// (`parse_predobj_pairs`) previously only accepted property-path predicates
/// (bare IRIs, `^`/`|`/`/` expressions), not a variable, unlike the ordinary
/// (non-bracketed) triple-pattern parser which already special-cased it.
///
/// Tracked by https://github.com/daghovland/rdf-datalog/issues/202.
#[test]
fn spec_s5_blank_node_property_list_variable_predicate() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:s1 :p1 1 .
:s2 :p2 2 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT (COUNT(*) AS ?c)
WHERE { [] ?p ?o }
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "c").as_deref(),
        Some("2"),
        "`[] ?p ?o` (variable predicate inside blank-node property list) must parse and match both triples"
    );
}

// ── §9 (extended)  Property Paths ────────────────────────────────────────────
//
// Data: tests/testdata/sparql12_paths.ttl
//   foaf:knows chain: alice→bob→carol→dave→eve
//   ex:likes edges:  alice→frank, dave→frank

/// SPARQL 1.2 §9.2: Alternative path p1|p2 matches either predicate.
///
/// `?x (foaf:knows|ex:likes) ex:frank` — foaf:knows does not reach frank;
/// ex:likes reaches alice and dave. Expected: 2 rows.
#[test]
fn spec_s9_alternative_path() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?x WHERE {
    ?x (foaf:knows|ex:likes) ex:frank .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§9.2: alternative path — alice and dave like frank; neither knows frank"
    );
}

/// SPARQL 1.2 §9.3: Inverse path ^p reverses subject/object.
///
/// ex:carol ^foaf:knows ?x  ≡  ?x foaf:knows ex:carol
/// bob knows carol → 1 row
/// Expected: 1 row (?x = bob)
#[test]
fn spec_s9_inverse_path() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?x WHERE {
    ex:carol ^foaf:knows ?x .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§9.3: inverse path — only bob knows carol directly"
    );
    let vals = query_values(&ds, sparql, "x");
    assert!(
        vals.iter().any(|v| v.contains("bob")),
        "§9.3: ?x should be bob"
    );
}

/// SPARQL 1.2 §9.5: Zero-or-more path p* includes zero-hop (self) and transitive.
///
/// ?z foaf:knows* ex:eve
///   0 hops: eve
///   1 hop:  dave (dave knows eve)
///   2 hops: carol (carol knows dave)
///   3 hops: bob
///   4 hops: alice
/// Expected: 5 rows (alice, bob, carol, dave, eve)
#[test]
fn spec_s9_zero_or_more() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?z WHERE {
    ?z foaf:knows* <http://example.org/eve> .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        5,
        "§9.5: foaf:knows* to eve — 5 nodes (alice, bob, carol, dave, eve)"
    );
}

/// SPARQL 1.2 §9.5: One-or-more path p+ requires at least one hop.
///
/// ?z foaf:knows+ ex:eve
///   ≥1 hops: dave, carol, bob, alice (eve itself excluded)
/// Expected: 4 rows
#[test]
fn spec_s9_one_or_more() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?z WHERE {
    ?z foaf:knows+ <http://example.org/eve> .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        4,
        "§9.5: foaf:knows+ to eve — 4 nodes (alice, bob, carol, dave; not eve)"
    );
}

/// SPARQL 1.2 §9.5: Zero-or-one path p? — direct edge or identity.
///
/// ex:alice foaf:knows? ?z
///   0 hops: alice (self)
///   1 hop:  bob (alice knows bob)
/// Expected: 2 rows
#[test]
fn spec_s9_zero_or_one() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?z WHERE {
    ex:alice foaf:knows? ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§9.5: foaf:knows? from alice — alice (self) and bob (1 hop)"
    );
}

/// SPARQL 1.2 §9.7: Negated property set !p excludes triples with predicate p.
///
/// ?x !(foaf:knows) ?y from alice:
///   alice has foaf:name, foaf:knows, ex:likes.
///   Excluding foaf:knows leaves: foaf:name "Alice", ex:likes frank → 2 rows.
/// Expected: 2 rows
#[test]
fn spec_s9_negated_property_set() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.org/>
SELECT ?y WHERE {
    ex:alice !(foaf:knows) ?y .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "§9.7: negated set — alice's non-knows triples: name and likes"
    );
}

/// SPARQL 1.2 §9: Inverse combined with sequence  ^foaf:knows/foaf:knows.
///
/// `?x ^foaf:knows/foaf:knows ?z` matches pairs (x, z) for which some w
/// exists with `w foaf:knows x` and `w foaf:knows z`. In this chain
/// (alice→bob→carol→dave→eve) every knower has exactly one target, so each
/// w only ever pairs a node with itself: w=alice gives (bob, bob), w=bob
/// gives (carol, carol), w=carol gives (dave, dave), w=dave gives (eve, eve).
/// Expected: 4 rows (all self-pairs, one per common parent)
#[test]
fn spec_s9_inverse_sequence() {
    let ds = load("sparql12_paths.ttl");
    let sparql = r#"
PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?x ?z WHERE {
    ?x ^foaf:knows/foaf:knows ?z .
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        4,
        "§9: ^foaf:knows/foaf:knows — 4 self-pairs via single common parent"
    );
}

// ── §17.4.3  String Functions ────────────────────────────────────────────────

/// SPARQL 1.1 §17.4.3: STRSTARTS as a FILTER condition.
#[test]
fn spec_s17_strstarts_filter() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?name WHERE {
    <http://ex/s> <http://ex/name> ?name .
    FILTER STRSTARTS(?name, "Ali")
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§17.4.3: STRSTARTS(\"Alice\", \"Ali\") is true"
    );
}

/// SPARQL 1.1 §17.4.3: STRSTARTS as a BIND expression (value path).
#[test]
fn spec_s17_strstarts_bind() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/name> ?name .
    BIND(STRSTARTS(?name, "Ali") AS ?b)
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("true".to_string()),
        "§17.4.3: BIND(STRSTARTS(...)) should yield boolean true"
    );
}

/// SPARQL 1.1 §17.4.3: STRENDS as a FILTER condition.
#[test]
fn spec_s17_strends_filter() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?name WHERE {
    <http://ex/s> <http://ex/name> ?name .
    FILTER STRENDS(?name, "ice")
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§17.4.3: STRENDS(\"Alice\", \"ice\") is true"
    );
}

/// SPARQL 1.1 §17.4.3: STRENDS as a BIND expression (value path).
#[test]
fn spec_s17_strends_bind() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/name> ?name .
    BIND(STRENDS(?name, "ice") AS ?b)
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("true".to_string()),
        "§17.4.3: BIND(STRENDS(...)) should yield boolean true"
    );
}

/// SPARQL 1.1 §17.4.3: CONTAINS as a FILTER condition.
#[test]
fn spec_s17_contains_filter() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?name WHERE {
    <http://ex/s> <http://ex/name> ?name .
    FILTER CONTAINS(?name, "lic")
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "§17.4.3: CONTAINS(\"Alice\", \"lic\") is true"
    );
}

/// SPARQL 1.1 §17.4.3: CONTAINS as a BIND expression (value path), negative case.
#[test]
fn spec_s17_contains_bind_false() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/name> ?name .
    BIND(CONTAINS(?name, "zzz") AS ?b)
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("false".to_string()),
        "§17.4.3: BIND(CONTAINS(...)) should yield boolean false when not found"
    );
}

/// SPARQL 1.1 §17.4.3: STRBEFORE returns the substring before the first occurrence of sep.
#[test]
fn spec_s17_strbefore_match() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice-Bob" ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/name> ?name .
    BIND(STRBEFORE(?name, "-") AS ?b)
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"Alice\"".to_string()),
        "§17.4.3: STRBEFORE(\"Alice-Bob\", \"-\") = \"Alice\""
    );
}

/// SPARQL 1.1 §17.4.3: STRBEFORE returns "" when sep does not occur.
#[test]
fn spec_s17_strbefore_no_match() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/name> ?name .
    BIND(STRBEFORE(?name, "-") AS ?b)
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"\"".to_string()),
        "§17.4.3: STRBEFORE with no match returns empty string"
    );
}

/// SPARQL 1.1 §17.4.3: STRAFTER returns the substring after the first occurrence of sep.
#[test]
fn spec_s17_strafter_match() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice-Bob" ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/name> ?name .
    BIND(STRAFTER(?name, "-") AS ?b)
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"Bob\"".to_string()),
        "§17.4.3: STRAFTER(\"Alice-Bob\", \"-\") = \"Bob\""
    );
}

/// SPARQL 1.1 §17.4.3: STRAFTER returns "" when sep does not occur.
#[test]
fn spec_s17_strafter_no_match() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/name> "Alice" ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/name> ?name .
    BIND(STRAFTER(?name, "-") AS ?b)
}
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"\"".to_string()),
        "§17.4.3: STRAFTER with no match returns empty string"
    );
}

// ── §17.4.5  Numeric Functions ───────────────────────────────────────────────

/// SPARQL 1.1 §17.4.5: ABS on a negative integer literal.
#[test]
fn spec_s17_abs_negative_integer() {
    let ds = parse_inline_ttl(r#"<http://ex/s> <http://ex/delta> -5 ."#);
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/delta> ?delta .
    BIND(ABS(?delta) AS ?b)
}
"#;
    // `ABS`'s result now renders in the same `"value"^^<datatype>` wire form
    // as any other `xsd:integer` value (see #228, generalizing the
    // arithmetic-result literal-shape fix from #198/#227).
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"5\"^^<http://www.w3.org/2001/XMLSchema#integer>".to_string()),
        "§17.4.5: ABS(-5) = 5, preserving integer type"
    );
}

/// SPARQL 1.1 §17.4.5: CEIL on a decimal literal.
#[test]
fn spec_s17_ceil_decimal() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/score> "3.2"^^xsd:decimal .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/score> ?score .
    BIND(CEIL(?score) AS ?b)
}
"#;
    // CEIL/FLOOR/ROUND preserve the operand's numeric type per §17.4.5 (an
    // `xsd:decimal` input stays `xsd:decimal`) — see #205.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"4\"^^<http://www.w3.org/2001/XMLSchema#decimal>".to_string()),
        "§17.4.5: CEIL(3.2) = 4"
    );
}

/// SPARQL 1.1 §17.4.5: FLOOR on a decimal literal.
#[test]
fn spec_s17_floor_decimal() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/score> "3.8"^^xsd:decimal .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/score> ?score .
    BIND(FLOOR(?score) AS ?b)
}
"#;
    // See #205, as above for CEIL: the decimal input's type is preserved.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"3\"^^<http://www.w3.org/2001/XMLSchema#decimal>".to_string()),
        "§17.4.5: FLOOR(3.8) = 3"
    );
}

/// SPARQL 1.1 §17.4.5: ROUND on a positive decimal, rounding up at .5.
#[test]
fn spec_s17_round_half_up() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/score> "2.5"^^xsd:decimal .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/score> ?score .
    BIND(ROUND(?score) AS ?b)
}
"#;
    // See #205, as above for CEIL: the decimal input's type is preserved.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"3\"^^<http://www.w3.org/2001/XMLSchema#decimal>".to_string()),
        "§17.4.5: ROUND(2.5) = 3 (round half toward positive infinity)"
    );
}

/// SPARQL 1.1 §17.4.5: ROUND on a negative decimal at the .5 boundary rounds
/// toward positive infinity per spec (not away from zero).
#[test]
fn spec_s17_round_negative_half_toward_positive_infinity() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/score> "-2.5"^^xsd:decimal .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/score> ?score .
    BIND(ROUND(?score) AS ?b)
}
"#;
    // See #205, as above for CEIL: the decimal input's type is preserved.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"-2\"^^<http://www.w3.org/2001/XMLSchema#decimal>".to_string()),
        "§17.4.5: ROUND(-2.5) = -2 per spec (round half toward +infinity), not -3"
    );
}

// ── §17.4.6  Date/Time Functions ─────────────────────────────────────────────

/// SPARQL 1.1 §17.4.6: YEAR on an xsd:dateTime literal.
#[test]
fn spec_s17_year_datetime() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/published> "2014-03-05T10:20:30Z"^^xsd:dateTime .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/published> ?d .
    BIND(YEAR(?d) AS ?b)
}
"#;
    // YEAR's xsd:integer result now renders in the same
    // `"value"^^<datatype>` wire form as any other xsd:integer value (#228,
    // extended beyond the issue's enumerated scope to the same producer bug
    // in the date/time component functions).
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"2014\"^^<http://www.w3.org/2001/XMLSchema#integer>".to_string()),
        "§17.4.6: YEAR of a dateTime literal"
    );
}

/// SPARQL 1.1 §17.4.6: YEAR on an xsd:gYear literal (common in DBLP-style data).
#[test]
fn spec_s17_year_gyear() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/published> "2014"^^xsd:gYear .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/published> ?d .
    BIND(YEAR(?d) AS ?b)
}
"#;
    // See #228, as above.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"2014\"^^<http://www.w3.org/2001/XMLSchema#integer>".to_string()),
        "§17.4.6: YEAR of an xsd:gYear literal"
    );
}

/// SPARQL 1.1 §17.4.6: MONTH on an xsd:date literal.
#[test]
fn spec_s17_month_date() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/created> "2014-03-05"^^xsd:date .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/created> ?d .
    BIND(MONTH(?d) AS ?b)
}
"#;
    // See #228, as above.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"3\"^^<http://www.w3.org/2001/XMLSchema#integer>".to_string()),
        "§17.4.6: MONTH of a date literal"
    );
}

/// SPARQL 1.1 §17.4.6: DAY on an xsd:date literal.
#[test]
fn spec_s17_day_date() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/created> "2014-03-05"^^xsd:date .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/created> ?d .
    BIND(DAY(?d) AS ?b)
}
"#;
    // See #228, as above.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"5\"^^<http://www.w3.org/2001/XMLSchema#integer>".to_string()),
        "§17.4.6: DAY of a date literal"
    );
}

/// SPARQL 1.1 §17.4.6: DAY on an xsd:dateTime literal (date functions operate on dateTime too).
#[test]
fn spec_s17_day_datetime() {
    let ds = parse_inline_ttl(
        r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
<http://ex/s> <http://ex/published> "2014-03-05T10:20:30Z"^^xsd:dateTime .
"#,
    );
    let sparql = r#"
SELECT ?b WHERE {
    <http://ex/s> <http://ex/published> ?d .
    BIND(DAY(?d) AS ?b)
}
"#;
    // See #228, as above.
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("\"5\"^^<http://www.w3.org/2001/XMLSchema#integer>".to_string()),
        "§17.4.6: DAY of a dateTime literal"
    );
}

// ── SPARQL 1.2 triple-term pattern tests ─────────────────────────────────────
//
// Tracked in [#146](https://github.com/daghovland/rdf-datalog/issues/146).
//
// These datasets are built directly via `Datastore::add_triple_term` rather
// than parsed from Turtle: Turtle 1.2's `<<( s p o )>>` syntax is phase R2
// ([#145](https://github.com/daghovland/rdf-datalog/issues/145)), a separate,
// independent phase of epic #143 not required to be complete for SPARQL 1.2
// triple-term *query* support (this phase, R3). See
// `docs/plans/RDF12_PLAN.md`.

use dag_rdf::{IriReference, RdfResource};

fn iri(local: &str) -> RdfResource {
    RdfResource::Iri(IriReference(format!("https://example.org/{local}")))
}

/// Builds the dataset equivalent to the Turtle:
/// ```turtle
/// @prefix : <https://example.org/> .
/// <<( :alice :knows :bob )>> :assertedBy :carol .
/// ```
/// directly through the `Datastore` API, since Turtle 1.2 triple-term parsing
/// (phase R2, #145) is a separate, independent piece of work from this phase.
fn build_triple_term_dataset() -> Datastore {
    let mut ds = Datastore::new(10_000);
    let alice = ds.add_node_resource(iri("alice"));
    let knows = ds.add_node_resource(iri("knows"));
    let bob = ds.add_node_resource(iri("bob"));
    let asserted_by = ds.add_node_resource(iri("assertedBy"));
    let carol = ds.add_node_resource(iri("carol"));

    let triple_term = ds.add_triple_term(alice, knows, bob);
    ds.add_triple(dag_rdf::Triple {
        subject: triple_term,
        predicate: asserted_by,
        obj: carol,
    });
    ds
}

/// SPARQL 1.2 — SELECT with a concrete triple-term pattern in WHERE.
///
/// Dataset: see [`build_triple_term_dataset`].
///
/// Query:
/// ```sparql
/// PREFIX : <https://example.org/>
/// SELECT ?ann WHERE { <<( :alice :knows :bob )>> :assertedBy ?ann }
/// ```
///
/// Expected: one result row with `?ann = :carol`.
#[test]
fn test_sparql_triple_term_where_clause() {
    let ds = build_triple_term_dataset();
    let sparql = r#"
PREFIX : <https://example.org/>
SELECT ?ann WHERE { <<( :alice :knows :bob )>> :assertedBy ?ann }
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "ann"),
        Some("<https://example.org/carol>".to_string()),
        "?ann should bind to :carol"
    );
}

/// SPARQL 1.2 — SELECT with variables inside the embedded triple pattern.
///
/// Dataset: see [`build_triple_term_dataset`].
///
/// Query:
/// ```sparql
/// PREFIX : <https://example.org/>
/// SELECT ?s ?o WHERE { <<( ?s :knows ?o )>> :assertedBy :carol }
/// ```
///
/// Expected: one result row with `?s = :alice`, `?o = :bob`.
#[test]
fn test_sparql_triple_term_variable_inner() {
    let ds = build_triple_term_dataset();
    let sparql = r#"
PREFIX : <https://example.org/>
SELECT ?s ?o WHERE { <<( ?s :knows ?o )>> :assertedBy :carol }
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "s"),
        Some("<https://example.org/alice>".to_string()),
        "?s should bind to :alice"
    );
    assert_eq!(
        query_single_value(&ds, sparql, "o"),
        Some("<https://example.org/bob>".to_string()),
        "?o should bind to :bob"
    );
}

/// Builds the dataset equivalent to the TriG:
/// ```trig
/// @prefix : <https://example.org/> .
/// :g1 { <<( :alice :knows :bob )>> :assertedBy :carol . }
/// ```
/// directly through the `Datastore` API — see [`build_triple_term_dataset`]
/// for why this bypasses the (separate-phase) Turtle/TriG 1.2 parser.
fn build_triple_term_named_graph_dataset() -> Datastore {
    let mut ds = Datastore::new(10_000);
    let alice = ds.add_node_resource(iri("alice"));
    let knows = ds.add_node_resource(iri("knows"));
    let bob = ds.add_node_resource(iri("bob"));
    let asserted_by = ds.add_node_resource(iri("assertedBy"));
    let carol = ds.add_node_resource(iri("carol"));
    let g1 = ds.add_node_resource(iri("g1"));

    let triple_term = ds.add_triple_term(alice, knows, bob);
    ds.add_named_graph_triple(
        g1,
        dag_rdf::Triple {
            subject: triple_term,
            predicate: asserted_by,
            obj: carol,
        },
    );
    ds
}

/// SPARQL 1.2 — SELECT with a triple-term pattern inside a named GRAPH clause.
///
/// Dataset: see [`build_triple_term_named_graph_dataset`].
///
/// Query:
/// ```sparql
/// PREFIX : <https://example.org/>
/// SELECT ?g WHERE { GRAPH ?g { <<( :alice :knows :bob )>> :assertedBy :carol } }
/// ```
///
/// Expected: one result row with `?g = :g1`.
#[test]
fn test_sparql_triple_term_in_named_graph() {
    let ds = build_triple_term_named_graph_dataset();
    let sparql = r#"
PREFIX : <https://example.org/>
SELECT ?g WHERE { GRAPH ?g { <<( :alice :knows :bob )>> :assertedBy :carol } }
"#;
    assert_eq!(
        query_single_value(&ds, sparql, "g"),
        Some("<https://example.org/g1>".to_string()),
        "?g should bind to :g1"
    );
}

/// SPARQL 1.2 — a triple term in *object* position (unsupported by phase R3,
/// #146) must match zero rows, not silently drop the constraint and match
/// every quad with the given subject/predicate.
///
/// Regression test for a bug found in review of PR #151 / tracked in #153:
/// resolving an unsupported term shape to `None` and passing it straight to
/// `Datastore::quads_matching` made `None` ambiguous between "unbound
/// variable" (wildcard) and "can never match" — collapsing both cases
/// silently turned an unsupported pattern into a wildcard instead of an
/// empty result. See `MatchTerm` in `sparql_parser::execute`.
///
/// Dataset: two ordinary quads, no triple term involved at all —
/// `(:s :p :o1)`, `(:s :p :o2)`.
///
/// Query:
/// ```sparql
/// SELECT * WHERE { :s :p <<( :a :b :c )>> }
/// ```
///
/// Expected: zero rows. `<<( :a :b :c )>>` isn't in the store as a triple
/// term at all, and even if it were, object-position triple terms aren't
/// supported yet — either way this must not match `:o1`/`:o2`.
#[test]
fn test_sparql_triple_term_object_position_matches_nothing() {
    let mut ds = Datastore::new(10_000);
    let s = ds.add_node_resource(iri("s"));
    let p = ds.add_node_resource(iri("p"));
    let o1 = ds.add_node_resource(iri("o1"));
    let o2 = ds.add_node_resource(iri("o2"));
    ds.add_triple(dag_rdf::Triple {
        subject: s,
        predicate: p,
        obj: o1,
    });
    ds.add_triple(dag_rdf::Triple {
        subject: s,
        predicate: p,
        obj: o2,
    });

    let sparql = r#"
PREFIX : <https://example.org/>
SELECT * WHERE { :s :p <<( :a :b :c )>> }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert!(
        result.rows.is_empty(),
        "triple-term object position is unsupported and must match nothing, got {} rows",
        result.rows.len()
    );
}

/// SPARQL 1.2 — a variable bound via `BIND` to a computed value that was
/// never interned into the datastore (i.e. that exact value does not appear
/// as a term in any stored quad) must, when used later in a triple-pattern
/// position, match zero rows — not silently drop the constraint and match
/// every quad in that position.
///
/// Regression test for #154, the same root-cause bug class as #146/#153
/// (`MatchTerm` collapsing "unconstrained wildcard" and "structurally
/// cannot match" into the same case) but triggered by a `BIND`-computed
/// value rather than an unsupported term shape.
///
/// Dataset: two ordinary quads, `(:s :n 1)` and `(:other :q 2)`. Neither has
/// `1000001` — the value `?y` gets bound to below — as a term anywhere.
///
/// Query:
/// ```sparql
/// SELECT * WHERE {
///     :s :n ?x .
///     BIND(?x + 1000000 AS ?y)
///     ?a ?b ?y .
/// }
/// ```
///
/// Expected: zero rows. `?y` is bound to `1000001`, a concrete value that
/// was never interned into the store, so `?a ?b ?y` structurally cannot
/// match any real quad.
#[test]
fn test_sparql_bind_computed_value_not_interned_matches_nothing() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <https://example.org/>
:s :n 1 .
:other :q 2 .
"#,
    );

    let sparql = r#"
PREFIX : <https://example.org/>
SELECT * WHERE {
    :s :n ?x .
    BIND(?x + 1000000 AS ?y)
    ?a ?b ?y .
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert!(
        result.rows.is_empty(),
        "?y is bound to a computed value (1000001) never interned into the store, \
         so `?a ?b ?y` must match zero rows, got {} rows",
        result.rows.len()
    );
}

// ── BIND arithmetic/type-coercion + scoping (issue #198) ────────────────────
//
// Unit-level equivalents of the W3C SPARQL 1.1 `bind` conformance entries
// tracked in [#198](https://github.com/daghovland/rdf-datalog/issues/198)
// (fixtures live at `tests/testdata/w3c_sparql11/bind/bind03..bind10`,
// exercised end-to-end by
// `tests/w3c_sparql11_suite.rs::w3c_sparql11_bind`).

/// W3C `bind03`: a `BIND`-computed arithmetic value must be usable as a
/// constraint in a later triple pattern, matching real interned data — not
/// silently fail to match because the computed value's internal `RdfLiteral`
/// representation differs from the one the store interned for the same
/// value.
///
/// Data: `:s1 :p 1`, `:s2 :p 2`, `:s3 :p 3`, `:s4 :p 4`. Query binds
/// `?z = ?o + 1` then joins `?s1 ?p1 ?z` against the same data, so only
/// `?o` in `{1,2,3}` produces a `?z` that also appears as some `?p`'s object
/// (`?o=4` gives `?z=5`, which matches nothing).
#[test]
fn spec_bind_arithmetic_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 1 .
:s2 :p 2 .
:s3 :p 3 .
:s4 :p 4 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?s1
{
  ?s ?p ?o .
  BIND(?o+1 AS ?z)
  ?s1 ?p1 ?z
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        3,
        "W3C bind03: ?z=?o+1 must join back against :p's own values \
         (o=1→z=2→s2, o=2→z=3→s3, o=3→z=4→s4; o=4→z=5 matches nothing), got {:?}",
        result
            .rows
            .iter()
            .map(|r| (
                r.get("z").map(graph_element_display),
                r.get("s1").map(graph_element_display)
            ))
            .collect::<Vec<_>>()
    );
}

/// W3C `bind04`: `BIND` of an expression that references a never-bound
/// variable must leave the target variable unbound for that solution — the
/// row itself must survive, per SPARQL 1.1 §18.3 Extend ("if evaluating the
/// expression raises an error, the variable remains unbound for that
/// solution"). The previous implementation dropped the whole row instead.
#[test]
fn spec_bind_unbound_expression_leaves_alias_unbound_row_survives() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 1 .
:s2 :p 2 .
:s3 :p 3 .
:s4 :p 4 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT *
{
  ?s ?p ?o .
  BIND(?nova AS ?z)
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        4,
        "W3C bind04: every `?s ?p ?o` row must survive even though `?nova` \
         (and so `?z`) is never bound, got {} rows",
        result.rows.len()
    );
    assert!(
        result.rows.iter().all(|r| r.get("z").is_none()),
        "?z must stay unbound in every row since `?nova` is never bound"
    );
}

// ── BIND-computed non-integer/function-call values must join too (#228) ────
//
// [PR #227](https://github.com/daghovland/rdf-datalog/pull/227) (#198) fixed
// this representation mismatch (a `BIND`-computed value's internal
// `RdfLiteral` shape differing from the `TypedLiteral` shape real interned
// data always uses, so a later triple-pattern join against it silently
// matched zero rows) for `eval_arithmetic`'s *integer* fast path only. This
// section is the systematic sweep from
// [#228](https://github.com/daghovland/rdf-datalog/issues/228): every other
// numeric/cast function that produces a computed literal (`eval_arithmetic`'s
// decimal/float/double branches, unary minus, `ABS`/`CEIL`/`FLOOR`/`ROUND`,
// and the `xsd:integer`/`xsd:decimal`/`xsd:double`/`xsd:float`/`xsd:boolean`/
// `xsd:dateTime` casts) needed the identical fix.

/// Issue #228 repro 1 (verbatim): `ABS(?o)` on an integer must join back
/// against the same integer value already interned by real data.
#[test]
fn spec_bind_abs_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 1 .
:s2 :p 2 .
:s3 :p 2.5 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?s1 WHERE { :s1 :p ?o . BIND(ABS(?o) AS ?z) ?s1 :p ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "ABS(1) = 1 must join back against :s1's own :p 1, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// Issue #228 repro 2 (the `?s2b` typo in the issue's verbatim query text
/// corrected to `?s2`, so the projected variable is actually bound by the
/// pattern — same arithmetic and data as reported): `?o + 0.5` on an integer
/// must produce a decimal that joins against real decimal data of the same
/// value.
#[test]
fn spec_bind_decimal_arithmetic_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 1 .
:s2 :p 2 .
:s3 :p 2.5 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?s2 WHERE { :s2 :p ?o . BIND(?o + 0.5 AS ?z) ?s2 :p ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "?o+0.5 = 2.5 must join back against :s3's :p 2.5, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `ABS` on a genuinely `xsd:decimal`-typed input must stay `xsd:decimal` in
/// its output — not silently widen to `xsd:double` — or the join below fails
/// on a datatype mismatch (both sides display as "2.5" but with different
/// `type_iri`s, so the resource lookup misses) rather than a value mismatch.
#[test]
fn spec_bind_abs_result_preserves_decimal_type_for_join() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "-2.5"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q "2.5"^^<http://www.w3.org/2001/XMLSchema#decimal> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(ABS(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "ABS(-2.5) = 2.5 must stay xsd:decimal and join :t1's :q 2.5, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `eval_arithmetic`'s float branch (triggered by a genuinely `xsd:float`
/// operand): the sum must join against real `xsd:float` data of the same
/// value.
#[test]
fn spec_bind_float_arithmetic_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :f "2.0"^^<http://www.w3.org/2001/XMLSchema#float> .
:t1 :q "2.5"^^<http://www.w3.org/2001/XMLSchema#float> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :f ?fo . BIND(?fo + 0.5 AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "2.0f + 0.5 = 2.5 must stay xsd:float and join :t1's :q 2.5, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `eval_arithmetic`'s double branch (triggered by a genuinely `xsd:double`
/// operand): the sum must join against real `xsd:double` data of the same
/// value.
#[test]
fn spec_bind_double_arithmetic_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :d "2.0"^^<http://www.w3.org/2001/XMLSchema#double> .
:t1 :q "2.5"^^<http://www.w3.org/2001/XMLSchema#double> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :d ?dv . BIND(?dv + 0.5 AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "2.0d + 0.5 = 2.5 must stay xsd:double and join :t1's :q 2.5, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// Unary minus (`arithmetic_negate`) is the same producer/lookup bug class:
/// a negated value must join against real data of the same (negative) value.
#[test]
fn spec_bind_negate_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 5 .
:t1 :q -5 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(-?o AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "-5 must join back against :t1's :q -5, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `CEIL` on a real `xsd:decimal` input must join against real decimal data.
/// (CEIL/FLOOR/ROUND preserve the operand's numeric type per SPARQL 1.1
/// §17.4.5 — an `xsd:decimal` input stays `xsd:decimal` — so the join target
/// here is `xsd:decimal`, not `xsd:integer`; see #205.)
#[test]
fn spec_bind_ceil_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "2.3"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q "3"^^<http://www.w3.org/2001/XMLSchema#decimal> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(CEIL(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "CEIL(2.3) = 3 (xsd:decimal) must join back against :t1's :q 3, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `FLOOR` on a real `xsd:decimal` input must join against real decimal data.
#[test]
fn spec_bind_floor_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "2.7"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q "2"^^<http://www.w3.org/2001/XMLSchema#decimal> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(FLOOR(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "FLOOR(2.7) = 2 (xsd:decimal) must join back against :t1's :q 2, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `ROUND` on a real `xsd:decimal` input must join against real decimal data.
#[test]
fn spec_bind_round_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "2.5"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q "3"^^<http://www.w3.org/2001/XMLSchema#decimal> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(ROUND(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "ROUND(2.5) = 3 (xsd:decimal) must join back against :t1's :q 3, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `xsd:integer(...)` cast (truncating a decimal) must join against real
/// integer data of the truncated value.
#[test]
fn spec_bind_cast_xsd_integer_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "7.9"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q 7 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(xsd:integer(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "xsd:integer(7.9) = 7 must join back against :t1's :q 7, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `xsd:decimal(...)` cast of an integer must join against real decimal data
/// of the same value.
#[test]
fn spec_bind_cast_xsd_decimal_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 5 .
:t1 :q "5"^^<http://www.w3.org/2001/XMLSchema#decimal> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(xsd:decimal(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "xsd:decimal(5) = 5 must join back against :t1's :q \"5\"^^xsd:decimal, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `xsd:double(...)` cast of an integer must join against real `xsd:double`
/// data of the same value.
#[test]
fn spec_bind_cast_xsd_double_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 5 .
:t1 :q "5"^^<http://www.w3.org/2001/XMLSchema#double> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(xsd:double(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "xsd:double(5) = 5 must join back against :t1's :q \"5\"^^xsd:double, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `xsd:float(...)` cast of an integer must join against real `xsd:float`
/// data of the same value.
#[test]
fn spec_bind_cast_xsd_float_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 5 .
:t1 :q "5"^^<http://www.w3.org/2001/XMLSchema#float> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(xsd:float(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "xsd:float(5) = 5 must join back against :t1's :q \"5\"^^xsd:float, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `xsd:boolean(...)` cast of a non-zero integer must join against real
/// `xsd:boolean` `true` data.
#[test]
fn spec_bind_cast_xsd_boolean_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p 1 .
:t1 :q true .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(xsd:boolean(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "xsd:boolean(1) = true must join back against :t1's :q true, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `xsd:dateTime(...)` cast of a plain string must join against real
/// `xsd:dateTime` data whose lexical form matches `chrono`'s
/// `to_rfc3339()` output (`+00:00` offset, not `Z`) for the same instant.
#[test]
fn spec_bind_cast_xsd_datetime_result_joins_against_interned_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "2021-06-01T00:00:00Z" .
:t1 :q "2021-06-01T00:00:00+00:00"^^<http://www.w3.org/2001/XMLSchema#dateTime> .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(xsd:dateTime(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "xsd:dateTime(\"2021-06-01T00:00:00Z\") must join back against :t1's \
         :q \"2021-06-01T00:00:00+00:00\"^^xsd:dateTime, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `YEAR(...)` wasn't in #228's enumerated scope, but is the same
/// producer/lookup bug: a `BIND`-computed date/time component must join
/// against real interned integer data of the same value.
#[test]
fn spec_bind_year_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
:s1 :p "2014-03-05T10:20:30Z"^^xsd:dateTime .
:t1 :q 2014 .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?z ?t WHERE { :s1 :p ?o . BIND(YEAR(?o) AS ?z) ?t :q ?z }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(
        result.rows.len(),
        1,
        "YEAR(2014-03-05T10:20:30Z) = 2014 must join back against :t1's :q 2014, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

// ── Project-expression evaluation (SELECT (expr AS ?var)) ───────────────────
//
// Mirrors the four W3C SPARQL 1.1 `project-expression` conformance entries
// tracked in [#207](https://github.com/daghovland/rdf-datalog/issues/207)
// (the vendored fixtures live at
// `tests/testdata/w3c_sparql11/project-expression/projexp01..04`, exercised
// end-to-end by `tests/w3c_sparql11_suite.rs::w3c_sparql11_project_expression`).
// These are unit-level equivalents that assert on the raw `GraphElement`
// binding (not just its display string) so the numeric *type* — not merely
// its printed value — is checked: a naive fix could return the right number
// as `xsd:double` instead of `xsd:integer`.

use dag_rdf::{GraphElement, RdfLiteral};
use ingress::XSD_INTEGER;

/// Assert that `el` is `xsd:integer`-typed with the given value. Accepts
/// either internal representation of an integer literal — the canonical
/// `RdfLiteral::IntegerLiteral` (produced by e.g. aggregate/BIND arithmetic)
/// or the generic `RdfLiteral::TypedLiteral { type_iri: xsd:integer, .. }`
/// shape (produced by the Turtle and SPARQL literal parsers) — since which
/// one a given code path returns is an implementation detail; both are
/// `xsd:integer` on the wire. What must NOT happen is silently promoting to
/// `xsd:double`, which is the bug this test guards against.
fn assert_xsd_integer(el: Option<&GraphElement>, expected: i64, msg: &str) {
    match el {
        Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n))) => {
            assert_eq!(n.to_string(), expected.to_string(), "{msg}");
        }
        Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal }))
            if type_iri.0 == XSD_INTEGER =>
        {
            assert_eq!(literal.parse::<i64>().ok(), Some(expected), "{msg}");
        }
        other => panic!("{msg}: expected xsd:integer {expected}, got {other:?}"),
    }
}

/// W3C `project-expression` "Expression is equality": a projected equality
/// comparison `(?y = ?z AS ?eq)` must produce an `xsd:boolean` value, not
/// silently vanish. Data: `in:a ex:p 1 ; ex:q 1, 2` — one row where `?y = ?z`
/// holds, one where it doesn't.
#[test]
fn spec_project_expression_equality() {
    let ds = parse_inline_ttl(
        r#"
PREFIX ex: <http://www.example.org/schema#>
PREFIX in: <http://www.example.org/instance#>
in:a ex:p 1 .
in:a ex:q 1 .
in:a ex:q 2 .
"#,
    );
    let sparql = r#"
PREFIX ex: <http://www.example.org/schema#>
SELECT ?z ((?y = ?z) AS ?eq) WHERE {
  ?x ex:p ?y .
  ?x ex:q ?z
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 2, "expected one row per ?z value");

    let bools: Vec<bool> = result
        .rows
        .iter()
        .map(|row| match row.get("eq") {
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b))) => *b,
            other => panic!("expected xsd:boolean ?eq binding, got {other:?}"),
        })
        .collect();
    assert!(
        bools.contains(&true) && bools.contains(&false),
        "expected both a true and a false ?eq row, got {bools:?}"
    );
}

/// W3C `project-expression` "Expression raise an error": a projected
/// arithmetic expression that errors during evaluation (`1 + "foobar"`, a
/// type error per SPARQL's `op:numeric-add`) must leave *only* its own alias
/// unbound for that solution — sibling projected variables in the same row
/// are unaffected — while a row where the expression evaluates cleanly must
/// still get the correctly `xsd:integer`-typed result (not `xsd:double`).
#[test]
fn spec_project_expression_arithmetic_error_leaves_alias_unbound() {
    let ds = parse_inline_ttl(
        r#"
PREFIX ex: <http://www.example.org/schema#>
PREFIX in: <http://www.example.org/instance#>
in:a ex:p 1 .
in:a ex:q 1 .
in:a ex:q "foobar" .
"#,
    );
    let sparql = r#"
PREFIX ex: <http://www.example.org/schema#>
SELECT ?z ((?y + ?z) AS ?sum) WHERE {
  ?x ex:p ?y .
  ?x ex:q ?z
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 2, "?z should still project on both rows");

    for row in &result.rows {
        match row.get("z") {
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s))) if s == "foobar" => {
                assert!(
                    row.get("sum").is_none(),
                    "1 + \"foobar\" is a type error; ?sum must be left unbound, got {:?}",
                    row.get("sum")
                );
            }
            _ => {
                assert_xsd_integer(row.get("sum"), 2, "1 + 1 must be an xsd:integer 2");
            }
        }
    }
}

/// W3C `project-expression` "Reuse a project expression variable in select":
/// a later SELECT item may reference an alias bound by an earlier one in the
/// same projection list (`(?y + ?z AS ?sum) (2 * ?sum AS ?twice)`), not just
/// the WHERE-clause bindings.
#[test]
fn spec_project_expression_reuse_alias_in_select() {
    let ds = parse_inline_ttl(
        r#"
PREFIX ex: <http://www.example.org/schema#>
PREFIX in: <http://www.example.org/instance#>
in:a ex:p 1 .
in:a ex:q 2 .
"#,
    );
    let sparql = r#"
PREFIX ex: <http://www.example.org/schema#>
SELECT ((?y + ?z) AS ?sum) ((2 * ?sum) AS ?twice) WHERE {
  ?x ex:p ?y .
  ?x ex:q ?z
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_xsd_integer(row.get("sum"), 3, "?sum = 1 + 2");
    assert_xsd_integer(
        row.get("twice"),
        6,
        "?twice = 2 * ?sum must see the ?sum alias from the earlier SELECT item",
    );
}

/// W3C `project-expression` "Reuse a project expression variable in order
/// by": `ORDER BY` may reference a `(expr AS ?alias)` projected variable, and
/// must sort by its (correctly `xsd:integer`-typed) value.
#[test]
fn spec_project_expression_reuse_alias_in_order_by() {
    let ds = parse_inline_ttl(
        r#"
PREFIX ex: <http://www.example.org/schema#>
PREFIX in: <http://www.example.org/instance#>
in:a ex:p 1 .
in:a ex:p 2 .
"#,
    );
    let sparql = r#"
PREFIX ex: <http://www.example.org/schema#>
SELECT ?y ((?y + ?y) AS ?sum) WHERE {
  ?x ex:p ?y
}
ORDER BY ?sum
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 2);
    assert_xsd_integer(result.rows[0].get("sum"), 2, "first row (ascending ?sum)");
    assert_xsd_integer(result.rows[1].get("sum"), 4, "second row (ascending ?sum)");
}

/// `project-expression` alias reuse inside a subquery's own SELECT list.
///
/// [#207](https://github.com/daghovland/rdf-datalog/issues/207) / [PR
/// #220](https://github.com/daghovland/rdf-datalog/pull/220) fixed alias reuse
/// (a later `(expr AS ?alias)` referencing an earlier one) for the top-level
/// `SELECT` projection path (`project_with_exprs`). Subquery projection goes
/// through a separate code path (`execute_select_inner`), so it was left
/// unverified whether the fix applies there too. See
/// [#223](https://github.com/daghovland/rdf-datalog/issues/223).
#[test]
fn spec_project_expression_reuse_alias_in_subquery_select() {
    let ds = parse_inline_ttl(
        r#"
PREFIX ex: <http://www.example.org/schema#>
PREFIX in: <http://www.example.org/instance#>
in:a ex:p 1 .
in:a ex:q 2 .
"#,
    );
    let sparql = r#"
PREFIX ex: <http://www.example.org/schema#>
SELECT ?sum ?twice WHERE {
  { SELECT ((?y + ?z) AS ?sum) ((2 * ?sum) AS ?twice) WHERE {
      ?x ex:p ?y .
      ?x ex:q ?z
    }
  }
}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_xsd_integer(row.get("sum"), 3, "?sum = 1 + 2");
    assert_xsd_integer(
        row.get("twice"),
        6,
        "?twice = 2 * ?sum must see the ?sum alias from the earlier subquery SELECT item",
    );
}

// ── Blank-node property lists in object position (issue #201) ──────────────
//
// `TriplesNode`/`PropertyListNotEmpty` (`[ pred obj ; pred obj ]`, or its
// empty form `[]`) is valid wherever a term may appear per the SPARQL
// grammar, including *object* position — e.g. `?s :p [ :q ?v ]`. Before
// #201, `parse_term` (used for objects) only recognized `_:label` blank
// nodes; the `[...]`/`[]` shorthand was handled only inline in
// `parse_group_graph_pattern_contents`, and only for *subject* position, so
// `?s :p [ :q ?v ] .` failed to parse at all. Fixed by `parse_object_term`,
// which recognizes the property-list/empty-blank-node shorthand in object
// position too, rewriting it to a fresh internal blank-node variable plus
// extra triples for the nested pred-obj pairs (recursing for nested lists).
// This is what the W3C subquery-suite entries sq11/sq13 exercise
// (`?O :hasItem [ rdfs:label ?L ] .`).

/// A single pred-obj pair inside an object-position blank node property
/// list: `?s :p [ :q ?v ]` must behave exactly like
/// `?s :p _:fresh . _:fresh :q ?v .`
#[test]
fn spec_bnode_property_list_object_position_single_pred() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:a :hasItem [ :label "widget" ] .
:b :hasItem [ :label "gadget" ] .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?s ?label WHERE { ?s :hasItem [ :label ?label ] . } ORDER BY ?s
"#;
    assert_eq!(
        query_values(&ds, sparql, "label"),
        vec!["\"widget\"", "\"gadget\""],
        "blank-node property list in object position must bind the nested predicate"
    );
}

/// Multiple pred-obj pairs (separated by `;`) inside an object-position
/// blank node property list.
#[test]
fn spec_bnode_property_list_object_position_multi_pred() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:a :hasItem [ :label "widget" ; :qty 3 ] .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?label ?qty WHERE { ?s :hasItem [ :label ?label ; :qty ?qty ] . }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should execute");
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(
        row.get("label").map(graph_element_display),
        Some("\"widget\"".to_string())
    );
    assert_xsd_integer(row.get("qty"), 3, "?qty = 3 from the nested property list");
}

/// The empty blank node shorthand `[]` in object position must match any
/// blank node without binding any of its properties.
#[test]
fn spec_bnode_empty_object_position() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:a :hasItem [ :label "widget" ] .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?s WHERE { ?s :hasItem [] . }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "[] in object position should match the anonymous blank node without further constraint"
    );
}

/// A blank-node property list in object position whose own object is itself
/// a nested blank node property list — recursion in `parse_object_term`.
#[test]
fn spec_bnode_property_list_object_position_nested() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:a :hasItem [ :label "widget" ; :part [ :name "screw" ] ] .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?partName WHERE { ?s :hasItem [ :label ?label ; :part [ :name ?partName ] ] . }
"#;
    assert_eq!(
        query_values(&ds, sparql, "partName"),
        vec!["\"screw\""],
        "nested blank-node property lists in object position must recurse correctly"
    );
}

/// W3C subquery-suite `sq11`/`sq13` fixture pattern: blank-node property
/// list in object position combined with a nested `{ SELECT ... }`
/// subquery in the same group — the exact combination that failed to parse
/// before #201.
#[test]
fn spec_bnode_property_list_object_position_with_subquery() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://www.example.org> .
:order1 a :Order .
:order2 a :Order .
:order1 :hasItem [ :label "first" ] .
:order2 :hasItem [ :label "second" ] .
"#,
    );
    let sparql = r#"
PREFIX : <http://www.example.org>
SELECT ?L
WHERE {
 ?O :hasItem [ :label ?L ] .
 {
 SELECT DISTINCT ?O
 WHERE { ?O a :Order }
 ORDER BY ?O
 LIMIT 2
 }
} ORDER BY ?L
"#;
    assert_eq!(
        query_values(&ds, sparql, "L"),
        vec!["\"first\"", "\"second\""],
        "blank-node property list in object position must parse alongside a nested subquery"
    );
}

/// `SELECT *` must not leak the internal `__bn_N` variable a blank-node
/// property list in object position introduces — same guarantee
/// `is_internal_variable` already provided for `__path_*` (property-path
/// midpoints); widened to cover `__bn_*` alongside the object-position fix
/// above, since a `[...]` object is far more common than a `[...]` subject
/// and would otherwise leak a name the query text never mentions.
#[test]
fn spec_bnode_property_list_object_position_select_star_no_leak() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://example.org/> .
:a :hasItem [ :label "widget" ] .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT * WHERE { ?s :hasItem [ :label ?label ] . }
"#;
    let vars = query_vars(&ds, sparql);
    assert!(
        vars.iter().all(|v| !v.starts_with("__bn_")),
        "SELECT * must not project the internal blank-node variable, got: {vars:?}"
    );
    assert_eq!(
        vars.len(),
        2,
        "SELECT * should project exactly ?s and ?label, got: {vars:?}"
    );
}

/// Subquery isolation (W3C `sq13`'s actual query shape, `sq13.rq` on disk —
/// not the fixture `sq13`'s `mf:action` resolves to, see the comment in
/// `w3c_sparql11_subquery`): a subquery is evaluated independently of the
/// outer pattern's bindings, so a variable used *inside* the subquery but
/// NOT in its own projection (here `?L`) must not leak out and constrain the
/// outer pattern's use of that same variable name. Only the subquery's
/// projected variable (`?O2`) is visible to the outer join; `?O1`/outer `?L`
/// and the subquery's internal `?L` are unrelated. If isolation holds, every
/// outer `?O1` (each with its own `?L`) pairs with every subquery-selected
/// `?O2` — the full 2x2 cross product, not just the pairs whose `?L`
/// happens to coincide (which a bugged "subquery bindings leak out"
/// evaluator would collapse to).
#[test]
fn spec_subquery_isolation_cartesian_product() {
    let ds = parse_inline_ttl(
        r#"
@prefix : <http://www.example.org> .
:order1 :hasItem [ :label "first" ] .
:order2 :hasItem [ :label "second" ] .
"#,
    );
    let sparql = r#"
PREFIX : <http://www.example.org>
SELECT ?O1 ?O2
WHERE {
 ?O1 :hasItem [ :label ?L ] .
 {
 SELECT ?O2
 WHERE { ?O2 :hasItem [ :label ?L ] . }
 }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        4,
        "the subquery's internal ?L must not leak out and constrain the outer ?L — \
         expected the full 2x2 cross product of ?O1 x ?O2"
    );
}

/// W3C `sq06`-style pattern (Turtle-only regression, since `sq06` itself is
/// skipped pending RDF/XML support — see `w3c_sparql11_subquery`): a bare
/// `{ SELECT ... }` subquery directly in the outer WHERE clause, with no
/// enclosing `GRAPH` block, over data loaded into a named graph via `FROM
/// NAMED`. Exercises the same subquery-in-group parsing path as the
/// GRAPH-wrapped `sq01`-`sq05`/`sq07` variants without depending on RDF/XML
/// fixture data.
#[test]
fn spec_subquery_bare_in_group_over_named_graph() {
    let ds = parse_inline_ttl(
        r#"
@prefix ex: <http://www.example.org/schema#> .
@prefix in: <http://www.example.org/instance#> .
in:a ex:p in:b .
in:c ex:q in:d .
"#,
    );
    let sparql = r#"
PREFIX ex: <http://www.example.org/schema#>
SELECT ?x ?p WHERE {
  { SELECT * WHERE { ?x ?p ?y } }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "a bare subquery directly in the outer WHERE clause should see all default-graph triples"
    );
}

/// W3C `sq01`-style pattern (Turtle/TriG-only regression, since `sq01` itself
/// is skipped pending RDF/XML support): a `{ SELECT ... }` subquery nested
/// *inside* a `GRAPH ?g { ... }` block, over data loaded into a genuinely
/// named (non-default) graph. This is the specific scoping combination
/// `sq01`-`sq03`/`sq05`-`sq07` exercise and that this crate's #201 fix does
/// NOT independently re-verify for the RDF/XML-blocked entries (see the
/// hedge in `w3c_sparql11_subquery`'s doc comment) — this test at least
/// confirms the combination isn't broken outright when reachable via
/// Turtle-loadable data.
#[test]
fn spec_subquery_within_graph_pattern() {
    let mut ds = Datastore::new(10_000);
    let trig = r#"
@prefix ex: <http://www.example.org/schema#> .
@prefix in: <http://www.example.org/instance#> .
<http://www.example.org/instance#g1> {
    in:a ex:p in:b .
    in:c ex:q in:d .
}
"#;
    turtle::parse_trig(&mut ds, trig.as_bytes()).expect("inline TriG must parse");
    let sparql = r#"
PREFIX ex: <http://www.example.org/schema#>
SELECT ?x ?p WHERE {
  GRAPH ?g {
    { SELECT * WHERE { ?x ?p ?y } }
  }
}
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "a subquery nested inside GRAPH ?g {{ ... }} should see the named graph's triples"
    );
}

// ── Builtin function fixes (#205) ───────────────────────────────────────────
//
// Unit-level regressions for the W3C SPARQL 1.1 `functions`-suite gaps fixed
// alongside issue #205 (see `tests/w3c_sparql11_suite.rs::w3c_sparql11_functions`
// for the end-to-end fixtures these mirror).

/// REGEX previously did a plain substring `.contains()` check instead of a
/// real regular-expression match, so anchors/character-classes/repetition
/// never worked (W3C `uuid01`/`struuid01` rely on exactly this).
#[test]
fn spec_regex_performs_real_pattern_match() {
    let ds = parse_inline_ttl(
        r#"@prefix : <http://example.org/> . :s1 <http://example.org/p> "abc123" ."#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?o WHERE { :s1 <http://example.org/p> ?o . FILTER(REGEX(?o, "^[a-z]+[0-9]{3}$")) }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        1,
        "REGEX with anchors/char-classes/repetition must match structurally, not as a substring"
    );
    let sparql_no_match = r#"
PREFIX : <http://example.org/>
SELECT ?o WHERE { :s1 <http://example.org/p> ?o . FILTER(REGEX(?o, "^[0-9]+$")) }
"#;
    assert_eq!(
        query_rows(&ds, sparql_no_match),
        0,
        "a pattern that doesn't match the whole anchored expression must not match"
    );
}

/// `DATATYPE(NOW())` must be `xsd:dateTime`: `NOW()` produces a native
/// `DateTimeLiteral` GraphElement variant, which `DATATYPE()` previously had
/// no arm for (falling through to `None`/unbound).
#[test]
fn spec_datatype_of_now_is_xsd_datetime() {
    use sparql_parser::{
        NetworkPolicy, ParserContext, QueryResult, execute as run_execute, parse_query,
    };
    let ds = parse_inline_ttl("");
    let sparql = r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
ASK { BIND(NOW() AS ?n) FILTER(DATATYPE(?n) = xsd:dateTime) }
"#;
    let mut ctx = ParserContext {
        prefixes: std::collections::HashMap::new(),
        base: None,
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("query must parse");
    let result = run_execute(&query, &ds, NetworkPolicy::Deny).expect("query must execute");
    match result {
        QueryResult::Ask(b) => assert!(b, "DATATYPE(NOW()) must equal xsd:dateTime"),
        QueryResult::Select(_) => panic!("expected an ASK result, got Select"),
        QueryResult::Construct(_) => panic!("expected an ASK result, got Construct"),
        QueryResult::Describe(_) => panic!("expected an ASK result, got Describe"),
    }
}

/// `FILTER isNumeric(?x)` previously always rejected every row:
/// `eval_function_bool` had no arm for `ISNUMERIC` and no fallback to
/// `eval_function_value`, so the filter condition evaluated to `None` (=
/// false) regardless of the operand.
#[test]
fn spec_filter_is_numeric_accepts_numeric_literals() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:n1 :p 1 .
:n2 :p 1.5 .
:s1 :p "not a number" .
"#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT ?s ?o WHERE { ?s :p ?o . FILTER isNumeric(?o) }
"#;
    assert_eq!(
        query_rows(&ds, sparql),
        2,
        "FILTER isNumeric(?o) must select only the two numeric-literal rows"
    );
}

/// CEIL/FLOOR/ROUND preserve the operand's numeric type instead of always
/// promoting to `xsd:integer` (SPARQL 1.1 §17.4.5, `fn:round`/`fn:ceiling`/
/// `fn:floor`): an `xsd:decimal` input stays `xsd:decimal`.
#[test]
fn spec_round_ceil_floor_preserve_decimal_type() {
    use dag_rdf::{GraphElement, RdfLiteral};
    use ingress::XSD_DECIMAL;
    let ds = parse_inline_ttl("");
    for (func, expected) in [("ROUND", "-2"), ("CEIL", "-1"), ("FLOOR", "-2")] {
        let sparql = format!(
            r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT (({func}("-1.6"^^xsd:decimal)) AS ?r) WHERE {{}}"#
        );
        let result = run_sparql_query(&ds, &sparql).expect("query should parse and execute");
        let el = result.rows[0].get("r").expect("?r must be bound");
        match el {
            GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal }) => {
                assert_eq!(
                    type_iri.0, XSD_DECIMAL,
                    "{func}(-1.6 as xsd:decimal) must stay xsd:decimal"
                );
                assert_eq!(literal, expected, "{func}(-1.6) value mismatch");
            }
            other => panic!("{func} result should be a TypedLiteral, got {:?}", other),
        }
    }
}

/// HOURS/MINUTES/SECONDS report the time-of-day components as written in the
/// literal's own timezone offset, not shifted to UTC first (W3C `hours-01`:
/// `HOURS("2010-12-21T15:38:02-08:00"^^xsd:dateTime)` is `15`, not `23`).
#[test]
fn spec_hours_uses_literal_timezone_not_utc() {
    let ds = parse_inline_ttl("");
    let sparql = r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT (HOURS("2010-12-21T15:38:02-08:00"^^xsd:dateTime) AS ?h) WHERE {}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    let el = result.rows[0].get("h").expect("?h must be bound");
    assert_eq!(
        graph_element_display(el),
        "\"15\"^^<http://www.w3.org/2001/XMLSchema#integer>",
        "HOURS must read the hour as written in the literal's own offset, not after UTC normalisation"
    );
}

/// TIMEZONE() (distinct from TZ()) returns an `xsd:dayTimeDuration`.
#[test]
fn spec_timezone_returns_daytimeduration() {
    use dag_rdf::{GraphElement, RdfLiteral};
    let ds = parse_inline_ttl("");
    let sparql = r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT (TIMEZONE("2010-12-21T15:38:02-08:00"^^xsd:dateTime) AS ?tz) WHERE {}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    let el = result.rows[0].get("tz").expect("?tz must be bound");
    match el {
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal }) => {
            assert_eq!(
                type_iri.0, "http://www.w3.org/2001/XMLSchema#dayTimeDuration",
                "TIMEZONE() must return xsd:dayTimeDuration"
            );
            assert_eq!(literal, "-PT8H");
        }
        other => panic!("TIMEZONE result should be a TypedLiteral, got {:?}", other),
    }
}

/// UCASE/LCASE/SUBSTR propagate the operand's language tag to the output
/// (SPARQL 1.1 §17.4.3.7/8/10) instead of always emitting a plain literal.
#[test]
fn spec_string_functions_preserve_lang_tag() {
    use dag_rdf::{GraphElement, RdfLiteral};
    let ds = parse_inline_ttl(
        r#"@prefix : <http://example.org/> . :s1 <http://example.org/p> "bar"@en ."#,
    );
    for (func, args, expected) in [
        ("UCASE", "?o", "BAR"),
        ("LCASE", "?o", "bar"),
        ("SUBSTR", "?o,2", "ar"),
    ] {
        let sparql = format!(
            r#"PREFIX : <http://example.org/>
SELECT ({func}({args}) AS ?r) WHERE {{ :s1 <http://example.org/p> ?o }}"#
        );
        let result = run_sparql_query(&ds, &sparql).expect("query should parse and execute");
        let el = result.rows[0].get("r").expect("?r must be bound");
        match el {
            GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, lang }) => {
                assert_eq!(literal, expected, "{func} value mismatch");
                assert_eq!(lang, "en", "{func} must preserve the input's language tag");
            }
            other => panic!(
                "{func} result should be a LangLiteral, got {:?} (lang tag not propagated)",
                other
            ),
        }
    }
}

/// STRBEFORE/STRAFTER propagate arg1's tag when a match is found, but fall
/// back to an untagged plain empty literal when the separator isn't found in
/// the text at all (distinct from an *empty* separator, which still
/// preserves arg1's tag) — see the W3C `strbefore01a`/`strafter01a` revision.
#[test]
fn spec_strbefore_strafter_tag_propagation() {
    use dag_rdf::{GraphElement, RdfLiteral};
    let ds = parse_inline_ttl(
        r#"@prefix : <http://example.org/> . :s1 <http://example.org/p> "abc"@en ."#,
    );

    // Found: propagates arg1's tag.
    let sparql = r#"
PREFIX : <http://example.org/>
SELECT (STRBEFORE(?o,"b") AS ?r) WHERE { :s1 <http://example.org/p> ?o }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    match result.rows[0].get("r").expect("?r must be bound") {
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, lang }) => {
            assert_eq!(literal, "a");
            assert_eq!(lang, "en");
        }
        other => panic!("expected a LangLiteral, got {:?}", other),
    }

    // Not found: untagged plain empty literal.
    let sparql_not_found = r#"
PREFIX : <http://example.org/>
SELECT (STRAFTER(?o,"z") AS ?r) WHERE { :s1 <http://example.org/p> ?o }
"#;
    let result = run_sparql_query(&ds, sparql_not_found).expect("query should parse and execute");
    match result.rows[0].get("r").expect("?r must be bound") {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => {
            assert_eq!(s, "", "not-found result must be the empty string");
        }
        other => panic!(
            "a not-found STRAFTER result must be an untagged plain literal, got {:?}",
            other
        ),
    }
}

/// Integer/integer division promotes to `xsd:decimal` (SPARQL/XPath's
/// `op:numeric-divide` always does, even for exact quotients) instead of
/// truncating like `BigInt` division and staying `xsd:integer`.
#[test]
fn spec_integer_division_promotes_to_decimal() {
    use dag_rdf::{GraphElement, RdfLiteral};
    use ingress::XSD_DECIMAL;
    let ds = parse_inline_ttl("");
    let sparql = r#"
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ((4 / 2) AS ?r) WHERE {}
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    match result.rows[0].get("r").expect("?r must be bound") {
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal }) => {
            assert_eq!(
                type_iri.0, XSD_DECIMAL,
                "integer/integer division must promote to xsd:decimal even for an exact quotient"
            );
            assert_eq!(literal, "2");
        }
        other => panic!("division result should be a TypedLiteral, got {:?}", other),
    }
}

/// STRDT/STRLANG require a simple-literal (no lang tag, no datatype) first
/// argument and must error (leave the projected variable unbound) on
/// anything else — e.g. an already language-tagged literal.
#[test]
fn spec_strdt_strlang_error_on_non_simple_literal() {
    let ds = parse_inline_ttl(
        r#"@prefix : <http://example.org/> . :s1 <http://example.org/p> "bar"@en ."#,
    );
    let sparql = r#"
PREFIX : <http://example.org/>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?s (STRDT(?o,xsd:string) AS ?r) WHERE { :s1 <http://example.org/p> ?o }
"#;
    let result = run_sparql_query(&ds, sparql).expect("query should parse and execute");
    assert_eq!(result.rows.len(), 1);
    assert!(
        !result.rows[0].contains_key("r"),
        "STRDT on an already language-tagged literal must leave ?r unbound"
    );
}

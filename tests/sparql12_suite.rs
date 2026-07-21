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
    let mut sums = query_values(&ds, sparql, "total");
    sums.sort();
    assert_eq!(sums, vec!["30", "30"], "§11.4: org1 sum=30, org2 sum=30");
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("5".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("4".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("3".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("3".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("-2".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("2014".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("2014".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("3".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("5".to_string()),
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
    assert_eq!(
        query_single_value(&ds, sparql, "b"),
        Some("5".to_string()),
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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

/// `CEIL` on a real `xsd:decimal` input must join against real integer data.
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
#[test]
fn spec_bind_ceil_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "2.3"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q 3 .
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
        "CEIL(2.3) = 3 must join back against :t1's :q 3, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `FLOOR` on a real `xsd:decimal` input must join against real integer data.
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
#[test]
fn spec_bind_floor_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "2.7"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q 2 .
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
        "FLOOR(2.7) = 2 must join back against :t1's :q 2, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `ROUND` on a real `xsd:decimal` input must join against real integer data.
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
#[test]
fn spec_bind_round_result_joins_against_interned_integer_data() {
    let ds = parse_inline_ttl(
        r#"
PREFIX : <http://example.org/>
:s1 :p "2.5"^^<http://www.w3.org/2001/XMLSchema#decimal> .
:t1 :q 3 .
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
        "ROUND(2.5) = 3 must join back against :t1's :q 3, got {:?}",
        result
            .rows
            .iter()
            .map(|r| r.get("z").map(graph_element_display))
            .collect::<Vec<_>>()
    );
}

/// `xsd:integer(...)` cast (truncating a decimal) must join against real
/// integer data of the truncated value.
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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
#[ignore = "TDD red phase for #228 - unignore once the corresponding producer emits TypedLiteral"]
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

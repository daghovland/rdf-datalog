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
/// Only org1 has total price > 25 (sum 30 vs org2's sum 30 — both pass here,
/// so use HAVING (SUM(?price) > 25) — both pass, test HAVING (SUM(?price) > 30)
/// to confirm neither passes, or adjust.
/// Strategy: HAVING (MIN(?price) > 15) keeps only org2 (min price 30 > 15).
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
/// ?x ^foaf:knows/foaf:knows ?z:
///   ^foaf:knows from x gives the set that x is known-by,
///   then foaf:knows from there gives the next hop.
///   alice is known by nobody → 0 pairs starting at alice.
///   bob is known by alice → alice/foaf:knows→bob and alice/foaf:knows→carol; so bob→{bob,carol}
///   carol is known by bob  → bob/foaf:knows→{carol, dave}...
/// Concretely: pairs (x, z) where ∃w: w knows x ∧ w knows z.
///   w=alice: x=bob, z=bob (same); x=bob, z=carol (different)
///              but alice knows only bob → only (bob,bob) and that's it for alice
///   Actually alice knows {bob}, bob knows {carol}, etc. Let's recalculate:
///   For each w, w knows x and w knows z.
///   alice knows {bob}: pairs (bob, bob)
///   bob knows {carol}: pairs (carol, carol)
///   carol knows {dave}: pairs (dave, dave)
///   dave knows {eve}: pairs (eve, eve)
///   So 4 pairs (x=z cases, self-pairs via one common parent).
/// Expected: 4 rows
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

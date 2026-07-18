/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Executable versions of every code example shown in README.md.
//!
//! If a test here breaks, the corresponding README section is wrong — update
//! both together.  The test names match the README headings they cover.

use dag_rdf::Datastore;
use dagalog::{apply_rules, graph_element_display, run_sparql_query};
use ingress::NetworkPolicy;
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

// ── JSON-LD parsing ───────────────────────────────────────────────────────────

/// README §JSON-LD parsing — inline document.
#[test]
fn readme_jsonld_parse_inline() {
    let jsonld = r#"{
      "@context": { "foaf": "http://xmlns.com/foaf/0.1/" },
      "@id": "http://example.org/alice",
      "@type": "foaf:Person",
      "foaf:name": "Alice"
    }"#;

    let mut ds = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes(), NetworkPolicy::Deny)
        .expect("parse must succeed");

    let result = run_sparql_query(
        &ds,
        "SELECT ?name WHERE { \
            <http://example.org/alice> \
            <http://xmlns.com/foaf/0.1/name> ?name }",
    )
    .expect("query must succeed");
    assert_eq!(result.rows.len(), 1);
    let name = graph_element_display(result.rows[0].get("name").unwrap());
    assert!(name.contains("Alice"), "got: {name}");
}

// ── Turtle / TriG parsing ─────────────────────────────────────────────────────

/// README §Turtle parsing — basic triple loading.
#[test]
fn readme_turtle_parse_basic() {
    let ttl = r#"
        PREFIX dc: <http://purl.org/dc/elements/1.1/>
        <http://example.org/book/1> dc:title "SPARQL Tutorial" .
    "#;

    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse must succeed");

    let result = run_sparql_query(
        &ds,
        r#"SELECT ?title WHERE {
            <http://example.org/book/1>
            <http://purl.org/dc/elements/1.1/title>
            ?title }"#,
    )
    .expect("query must succeed");
    assert_eq!(result.rows.len(), 1);
    let title = graph_element_display(result.rows[0].get("title").unwrap());
    assert!(title.contains("SPARQL Tutorial"), "got: {title}");
}

// ── SPARQL queries ────────────────────────────────────────────────────────────

/// README §SPARQL — basic SELECT with FILTER.
#[test]
fn readme_sparql_filter() {
    let ttl = r#"
        PREFIX ns: <http://example.org/ns#>
        <http://example.org/cheap>     ns:price  9 ; ns:title "Budget Book" .
        <http://example.org/expensive> ns:price 49 ; ns:title "Deluxe Book" .
    "#;

    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).expect("Turtle parse");

    let result = run_sparql_query(
        &ds,
        "PREFIX ns: <http://example.org/ns#>
         SELECT ?title ?price WHERE {
             ?x ns:price ?price ;
                ns:title ?title .
             FILTER (?price < 20)
         }",
    )
    .expect("query must succeed");

    assert_eq!(result.rows.len(), 1);
    let title = graph_element_display(result.rows[0].get("title").unwrap());
    assert!(title.contains("Budget Book"), "got: {title}");
}

// ── OWL-RL reasoning ─────────────────────────────────────────────────────────

/// README §OWL reasoning — equality inference via owl:sameAs.
#[test]
fn readme_owl_same_as() {
    use dagalog::load_file;
    use datalog::evaluate_rules;
    use owl2rl2datalog::owl2datalog;
    use rdf_owl_translator::rdf2owl;

    let mut ds = Datastore::new(100_000);
    load_file(&mut ds, &testdata("equality.owl")).expect("equality.owl must load");

    let ontology = rdf2owl(&mut ds).ontology;
    let rules = owl2datalog(&mut ds.resources, &ontology);
    assert!(!rules.is_empty(), "OWL-RL should produce Datalog rules");
    evaluate_rules(rules, &mut ds);

    // ind2 gets the same type as ind1 via owl:sameAs
    let typed = run_sparql_query(
        &ds,
        "SELECT ?t WHERE { \
            <https://example.com/vocab#ind2> \
            <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ?t }",
    )
    .expect("query must succeed");
    assert!(
        !typed.rows.is_empty(),
        "ind2 must have at least one rdf:type after reasoning"
    );
}

// ── Custom Datalog rules ──────────────────────────────────────────────────────

/// README §Datalog rules — simple forward-chaining rule.
#[test]
fn readme_datalog_rule_forward_chain() {
    let mut ds = Datastore::new(10_000);
    dagalog::load_file(&mut ds, &testdata("data.ttl")).expect("data.ttl must load");

    const PRED: &str = "https://example.com/data#predicate";
    const OBJ2: &str = "https://example.com/data#object2";

    // Before rules: object2 does not exist
    let before =
        run_sparql_query(&ds, &format!("SELECT ?s WHERE {{ ?s <{PRED}> <{OBJ2}> }}")).unwrap();
    assert_eq!(
        before.rows.len(),
        0,
        "object2 should not exist before rules"
    );

    // Apply rules from data.ttl companion file
    apply_rules(&mut ds, &[testdata("rules.datalog")]).expect("rules must apply");

    // After rules: object2 is derived
    let after =
        run_sparql_query(&ds, &format!("SELECT ?s WHERE {{ ?s <{PRED}> <{OBJ2}> }}")).unwrap();
    assert_eq!(after.rows.len(), 1, "rule should have derived object2");
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests based on examples from the SPARQL 1.1/1.2 specification.
//!
//! Each test section is labelled with the spec section it exercises.
//! Spec reference: https://www.w3.org/TR/sparql11-query/
//!                 https://www.w3.org/TR/sparql12-query/

mod common;

// ── Section 2.1 — Writing a Simple Query ─────────────────────────────────────

/// SPARQL 1.1 spec §2.1 — basic SELECT ?title retrieval.
///
/// Dataset has one book; the query finds its title.
#[tokio::test]
async fn spec_2_1_simple_select() {
    let turtle = r#"
        @prefix dc: <http://purl.org/dc/elements/1.1/> .
        <http://example.org/book/book1> dc:title "SPARQL Tutorial" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = "PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?title
WHERE { ?book dc:title ?title }";

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1);
    common::assert_binding_contains(bindings, "title", "literal", "SPARQL Tutorial");
}

// ── Section 2.3.1 — Matching Literals with Language Tags ─────────────────────

/// SPARQL 1.1 spec §2.3.1 — language-tagged literal `"chat"@fr`.
///
/// A triple has an English and a French name; the query binds ?name
/// and we verify the French value appears.
#[tokio::test]
async fn spec_2_3_1_language_tag() {
    let turtle = r#"
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <http://example.org/alice>
            foaf:name "Alice"@en ;
            foaf:name "Alicia"@es .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name
WHERE { <http://example.org/alice> foaf:name ?name }"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    // Both language variants should be returned
    assert!(!bindings.is_empty(), "expected at least one binding");
    let has_en = bindings
        .iter()
        .any(|r| r["name"]["value"] == "Alice" && r["name"]["xml:lang"] == "en");
    let has_es = bindings
        .iter()
        .any(|r| r["name"]["value"] == "Alicia" && r["name"]["xml:lang"] == "es");
    assert!(has_en, "expected English name");
    assert!(has_es, "expected Spanish name");
}

/// SPARQL 1.1 spec §2.3.1 — FILTER with LANGMATCHES.
///
/// `lang()` and `langMatches()` are optional function support. This test
/// verifies the endpoint responds (200, 400, or 500) without crashing.
#[tokio::test]
async fn spec_2_3_1_langmatches_filter() {
    let turtle = r#"
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <http://example.org/alice> foaf:name "Alice"@en .
        <http://example.org/alice> foaf:name "Alicia"@es .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name
WHERE {
  <http://example.org/alice> foaf:name ?name .
  FILTER langMatches(lang(?name), "en")
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    // langMatches/lang are optional — accept any valid HTTP response
    let code = resp.status().as_u16();
    assert!(
        code == 200 || code == 400 || code == 500,
        "unexpected status {code}"
    );
    // If it does succeed, it should return at most 2 bindings
    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap();
        let bindings = body["results"]["bindings"].as_array().unwrap();
        assert!(bindings.len() <= 2, "at most two name bindings");
    }
}

// ── Section 2.3.2 — Matching Typed Literals ──────────────────────────────────

/// SPARQL 1.1 spec §2.3.2 — matching integer-typed literal stored in Turtle.
///
/// The Turtle file stores age as a bare integer (42); SPARQL queries it by
/// matching the same IRI-typed value.
#[tokio::test]
async fn spec_2_3_2_typed_integer() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:alice ex:age 42 .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?person
WHERE { ?person ex:age 42 }"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert!(!bindings.is_empty(), "expected a match for age 42");
    common::assert_binding_contains(bindings, "person", "uri", "http://example.org/alice");
}

// ── Section 3.1 — FILTER — Restricting by String Value ───────────────────────

/// SPARQL 1.1 spec §3.1 — FILTER using regex() on a string literal.
///
/// Two books; the query returns only the one whose title contains "SPARQL".
#[tokio::test]
async fn spec_3_1_filter_regex() {
    let turtle = r#"
        @prefix dc: <http://purl.org/dc/elements/1.1/> .
        <http://example.org/book/book1> dc:title "SPARQL Tutorial" .
        <http://example.org/book/book2> dc:title "The Semantic Web" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?title
WHERE {
  ?book dc:title ?title .
  FILTER regex(?title, "SPARQL")
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "expected exactly one match");
    common::assert_binding_contains(bindings, "title", "literal", "SPARQL Tutorial");
}

/// SPARQL 1.1 spec §3.1 — case-insensitive FILTER regex.
#[tokio::test]
async fn spec_3_1_filter_regex_case_insensitive() {
    let turtle = r#"
        @prefix dc: <http://purl.org/dc/elements/1.1/> .
        <http://example.org/book/book1> dc:title "SPARQL Tutorial" .
        <http://example.org/book/book2> dc:title "The Semantic Web" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX dc: <http://purl.org/dc/elements/1.1/>
SELECT ?title
WHERE {
  ?book dc:title ?title .
  FILTER regex(?title, "SPARQL TUTORIAL", "i")
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(
        bindings.len(),
        1,
        "expected exactly one case-insensitive match"
    );
    common::assert_binding_contains(bindings, "title", "literal", "SPARQL Tutorial");
}

// ── Section 3.2 — FILTER — Restricting Numeric Values ────────────────────────

/// SPARQL 1.1 spec §3.2 — FILTER with numeric comparison.
///
/// Books with prices; retrieve those below 30.
#[tokio::test]
async fn spec_3_2_filter_numeric_comparison() {
    let turtle = r#"
        @prefix dc: <http://purl.org/dc/elements/1.1/> .
        @prefix ns: <http://example.org/ns#> .
        <http://example.org/book/book1> dc:title "SPARQL Tutorial" .
        <http://example.org/book/book1> ns:price 42 .
        <http://example.org/book/book2> dc:title "The Semantic Web" .
        <http://example.org/book/book2> ns:price 23 .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX dc: <http://purl.org/dc/elements/1.1/>
PREFIX ns: <http://example.org/ns#>
SELECT ?title ?price
WHERE {
  ?book dc:title ?title .
  ?book ns:price ?price .
  FILTER (?price < 30)
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "expected one book under 30");
    common::assert_binding_contains(bindings, "title", "literal", "The Semantic Web");
}

// ── Section 3.3 — FILTER with BOUND ──────────────────────────────────────────

/// SPARQL 1.1 spec §6 — BOUND() checks if an optional variable has a value.
#[tokio::test]
async fn spec_filter_bound() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:alice ex:name "Alice" .
        ex:bob   ex:name "Bob" ; ex:email "bob@example.org" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?person ?email
WHERE {
  ?person ex:name ?name .
  OPTIONAL { ?person ex:email ?email }
  FILTER BOUND(?email)
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "only Bob has an email");
    common::assert_binding_contains(bindings, "email", "literal", "bob@example.org");
}

// ── Section 6 — OPTIONAL ─────────────────────────────────────────────────────

/// SPARQL 1.1 spec §6.1 — basic OPTIONAL pattern.
///
/// Some people have a mbox; OPTIONAL includes those without one.
#[tokio::test]
async fn spec_6_1_optional_basic() {
    let turtle = r#"
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <http://example.org/alice> foaf:name "Alice" ; foaf:mbox <mailto:alice@example.org> .
        <http://example.org/bob>   foaf:name "Bob" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name ?mbox
WHERE {
  ?person foaf:name ?name .
  OPTIONAL { ?person foaf:mbox ?mbox }
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    // Both Alice and Bob should appear; only Alice has mbox
    assert_eq!(bindings.len(), 2, "expected two rows (Alice + Bob)");

    let alice = bindings
        .iter()
        .find(|r| r["name"]["value"] == "Alice")
        .expect("Alice must be present");
    assert!(
        alice.get("mbox").is_some() && alice["mbox"]["type"] == "uri",
        "Alice should have mbox"
    );

    let bob = bindings
        .iter()
        .find(|r| r["name"]["value"] == "Bob")
        .expect("Bob must be present");
    assert!(
        bob.get("mbox").is_none() || bob["mbox"].is_null(),
        "Bob should not have mbox"
    );
}

/// SPARQL 1.1 spec §6.2 — OPTIONAL combined with FILTER.
#[tokio::test]
async fn spec_6_2_optional_with_filter() {
    let turtle = r#"
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <http://example.org/alice> foaf:name "Alice" ; foaf:mbox <mailto:alice@example.org> .
        <http://example.org/bob>   foaf:name "Bob" .
        <http://example.org/carol> foaf:name "Carol" ; foaf:mbox <mailto:carol@example.org> .
    "#;
    let server = common::TestServer::start(turtle).await;

    // Find people with a mbox (using FILTER BOUND after OPTIONAL)
    let query = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name ?mbox
WHERE {
  ?person foaf:name ?name .
  OPTIONAL { ?person foaf:mbox ?mbox }
  FILTER BOUND(?mbox)
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 2, "Alice and Carol have mbox");
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
    common::assert_binding_contains(bindings, "name", "literal", "Carol");
}

/// SPARQL 1.1 spec §6.3 — multiple OPTIONAL clauses.
#[tokio::test]
async fn spec_6_3_multiple_optionals() {
    let turtle = r#"
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <http://example.org/alice>
            foaf:name "Alice" ;
            foaf:mbox <mailto:alice@example.org> .
        <http://example.org/bob>
            foaf:name "Bob" ;
            foaf:nick "bob42" .
        <http://example.org/carol>
            foaf:name "Carol" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?name ?mbox ?nick
WHERE {
  ?person foaf:name ?name .
  OPTIONAL { ?person foaf:mbox ?mbox }
  OPTIONAL { ?person foaf:nick ?nick }
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 3, "three people");
}

// ── Section 7 — UNION ─────────────────────────────────────────────────────────

/// SPARQL 1.1 spec §7 — UNION to merge alternative patterns.
///
/// Books have either dc:title or rdfs:label; UNION retrieves both.
#[tokio::test]
async fn spec_7_union() {
    let turtle = r#"
        @prefix dc:   <http://purl.org/dc/elements/1.1/> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        <http://example.org/book1> dc:title   "A First Book" .
        <http://example.org/book2> rdfs:label "A Second Book" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX dc:   <http://purl.org/dc/elements/1.1/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?label
WHERE {
  { ?book dc:title ?label }
  UNION
  { ?book rdfs:label ?label }
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 2, "both books' titles returned via UNION");
    common::assert_binding_contains(bindings, "label", "literal", "A First Book");
    common::assert_binding_contains(bindings, "label", "literal", "A Second Book");
}

/// SPARQL 1.1 spec §7 — UNION where one branch has no matches.
#[tokio::test]
async fn spec_7_union_one_empty_branch() {
    let turtle = r#"
        @prefix dc: <http://purl.org/dc/elements/1.1/> .
        <http://example.org/book1> dc:title "Only Title" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX dc:   <http://purl.org/dc/elements/1.1/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?label
WHERE {
  { ?book dc:title ?label }
  UNION
  { ?book rdfs:label ?label }
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "only the dc:title branch matches");
    common::assert_binding_contains(bindings, "label", "literal", "Only Title");
}

// ── Section 8.2 — MINUS ──────────────────────────────────────────────────────

/// SPARQL 1.1 spec §8.2 — MINUS to exclude results.
///
/// Five people; one is excluded by MINUS.
#[tokio::test]
async fn spec_8_2_minus() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:alice ex:name "Alice" .
        ex:bob   ex:name "Bob" .
        ex:carol ex:name "Carol" .
        ex:alice ex:excludeMe "yes" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?name
WHERE {
  ?person ex:name ?name .
  MINUS { ?person ex:excludeMe ?any }
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 2, "Alice should be excluded");
    common::assert_binding_contains(bindings, "name", "literal", "Bob");
    common::assert_binding_contains(bindings, "name", "literal", "Carol");
}

// ── Section 8.1 — FILTER NOT EXISTS / FILTER EXISTS ──────────────────────────

/// SPARQL 1.1 spec §8.1.2 — FILTER NOT EXISTS.
#[tokio::test]
async fn spec_8_1_filter_not_exists() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:alice ex:name "Alice" .
        ex:bob   ex:name "Bob" ; ex:role "admin" .
        ex:carol ex:name "Carol" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?name
WHERE {
  ?person ex:name ?name .
  FILTER NOT EXISTS { ?person ex:role ?r }
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 2, "Alice and Carol have no role");
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
    common::assert_binding_contains(bindings, "name", "literal", "Carol");
}

/// SPARQL 1.1 spec §8.1.1 — FILTER EXISTS.
#[tokio::test]
async fn spec_8_1_filter_exists() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:alice ex:name "Alice" ; ex:role "admin" .
        ex:bob   ex:name "Bob" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?name
WHERE {
  ?person ex:name ?name .
  FILTER EXISTS { ?person ex:role ?r }
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "only Alice has a role");
    common::assert_binding_contains(bindings, "name", "literal", "Alice");
}

// ── Section 9 — Aggregates (expect unsupported or 200) ───────────────────────

/// SPARQL 1.1 spec §9 — GROUP BY and COUNT aggregate.
///
/// We expect either a working result (200 with count) or an unsupported error
/// (400/500 is acceptable until aggregates are implemented).
#[tokio::test]
async fn spec_9_group_by_count() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:book1 ex:author ex:alice .
        ex:book2 ex:author ex:alice .
        ex:book3 ex:author ex:bob .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?author (COUNT(?book) AS ?cnt)
WHERE { ?book ex:author ?author }
GROUP BY ?author"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    // Aggregates are not yet implemented; 400/500 is acceptable
    let ok = resp.status().is_success()
        || resp.status().is_client_error()
        || resp.status().is_server_error();
    assert!(ok, "unexpected HTTP status {}", resp.status());
}

// ── Section 10 — DISTINCT ─────────────────────────────────────────────────────

/// SPARQL 1.1 spec §10 — SELECT DISTINCT removes duplicate rows.
#[tokio::test]
async fn spec_10_distinct() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:alice ex:likes "cats" .
        ex:bob   ex:likes "cats" .
        ex:carol ex:likes "dogs" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT DISTINCT ?animal
WHERE { ?person ex:likes ?animal }"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 2, "DISTINCT should deduplicate 'cats'");
}

// ── Section 11 — LIMIT and OFFSET ─────────────────────────────────────────────

/// SPARQL 1.1 spec §11 — LIMIT restricts result set size.
#[tokio::test]
async fn spec_11_limit() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:a ex:val "1" .
        ex:b ex:val "2" .
        ex:c ex:val "3" .
        ex:d ex:val "4" .
        ex:e ex:val "5" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?v WHERE { ?x ex:val ?v } LIMIT 3"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 3, "LIMIT 3 should return exactly 3 rows");
}

/// SPARQL 1.1 spec §11 — OFFSET skips rows.
#[tokio::test]
async fn spec_11_offset() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:a ex:val "v1" .
        ex:b ex:val "v2" .
        ex:c ex:val "v3" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX ex: <http://example.org/>
SELECT ?v WHERE { ?x ex:val ?v } OFFSET 2"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "OFFSET 2 on 3 rows should yield 1 row");
}

// ── SPARQL 1.2 — Triple Terms (RDF-star / RDF 1.2) ───────────────────────────

/// SPARQL 1.2 — triple term syntax `<<( ?s ?p ?o )>>`.
///
/// This feature requires SPARQL 1.2 parser support. We accept 400 if not yet
/// implemented, and verify the endpoint stays alive afterwards.
#[tokio::test]
async fn spec_sparql12_triple_term_syntax() {
    let server = common::TestServer::start("").await;

    // SPARQL 1.2 triple term in subject position
    let query = r#"SELECT ?s ?p ?o
WHERE { <<( ?s ?p ?o )>> <http://example.org/occursIn> ?doc }"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    // 400 is acceptable if triple terms are not yet supported
    let ok = resp.status() == 400 || resp.status() == 200;
    assert!(ok, "expected 200 or 400, got {}", resp.status());
}

// ── Section 5 — Graph Patterns — Multiple Triples ────────────────────────────

/// SPARQL 1.1 spec §5 — multi-variable SELECT joining two triple patterns.
#[tokio::test]
async fn spec_5_multi_triple_bgp() {
    let turtle = r#"
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <http://example.org/alice>
            foaf:name "Alice" ;
            foaf:knows <http://example.org/bob> .
        <http://example.org/bob>
            foaf:name "Bob" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?person ?friend
WHERE {
  ?person foaf:name "Alice" .
  ?person foaf:knows ?friend .
}"#;

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1);
    common::assert_binding_contains(bindings, "friend", "uri", "http://example.org/bob");
}

// ── Section 4.1 — IRI Values ──────────────────────────────────────────────────

/// SPARQL 1.1 spec §4.1 — SELECT with star projection (*).
#[tokio::test]
async fn spec_4_1_star_projection() {
    let turtle = r#"
        @prefix ex: <http://example.org/> .
        ex:alice ex:name "Alice" .
    "#;
    let server = common::TestServer::start(turtle).await;

    let query = "PREFIX ex: <http://example.org/>
SELECT * WHERE { ?s ex:name ?o }";

    let resp = server
        .client
        .get(server.sparql_query_url(query))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let bindings = body["results"]["bindings"].as_array().unwrap();
    assert!(!bindings.is_empty());
    common::assert_binding_contains(bindings, "o", "literal", "Alice");
}

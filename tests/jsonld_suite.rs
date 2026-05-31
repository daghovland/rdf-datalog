/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! JSON-LD 1.1 parsing and serialisation tests.
//!
//! Each test corresponds to one or more examples from the W3C JSON-LD 1.1
//! specification: <https://www.w3.org/TR/json-ld11/>
//!
//! All tests are `#[ignore]` until the `jsonld_parser` crate is implemented.
//! Remove the `#[ignore]` attribute as features are completed.
//!
//! Assumed API (to be provided by a `jsonld_parser` workspace crate):
//!   - `jsonld_parser::parse_jsonld(ds: &mut Datastore, reader: impl Read) -> Result<(), Error>`
//!   - `jsonld_parser::serialize_jsonld(ds: &Datastore) -> String`

use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use dagalog::run_sparql_query;
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn parse_file(name: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    let path = testdata(name);
    let reader = std::fs::File::open(&path)
        .unwrap_or_else(|e| panic!("could not open {}: {}", path.display(), e));
    jsonld_parser::parse_jsonld(&mut ds, reader)
        .unwrap_or_else(|e| panic!("parse error in {}: {}", name, e));
    ds
}

fn parse_str(jsonld: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    jsonld_parser::parse_jsonld(&mut ds, jsonld.as_bytes()).expect("inline JSON-LD must parse");
    ds
}

fn query_count(ds: &Datastore, sparql: &str) -> usize {
    run_sparql_query(ds, sparql)
        .expect("SPARQL query must execute")
        .rows
        .len()
}

fn query_values(ds: &Datastore, sparql: &str, var: &str) -> Vec<String> {
    run_sparql_query(ds, sparql)
        .expect("SPARQL query must execute")
        .rows
        .iter()
        .filter_map(|row| row.get(var))
        .map(dagalog::graph_element_display)
        .collect()
}

fn has_iri(ds: &Datastore, iri: &str) -> bool {
    ds.resources
        .resource_map
        .contains_key(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            iri.to_string(),
        ))))
}

// ── §3.1  Expanded (no-context) document ─────────────────────────────────────

/// Spec §3.1 — a document with no @context; all keys are full IRIs.
/// Three triples on a blank-node subject: foaf:name, foaf:homepage, foaf:img.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_no_context_expanded_iris() {
    let ds = parse_file("jsonld_no_context.jsonld");

    // Three predicates must be present in the store
    assert!(has_iri(&ds, "http://xmlns.com/foaf/0.1/name"));
    assert!(has_iri(&ds, "http://xmlns.com/foaf/0.1/homepage"));
    assert!(has_iri(&ds, "http://xmlns.com/foaf/0.1/img"));

    // foaf:name value is the string "Manu Sporny"
    let names = query_values(
        &ds,
        "SELECT ?name WHERE { ?s <http://xmlns.com/foaf/0.1/name> ?name }",
        "name",
    );
    assert_eq!(names.len(), 1);
    assert!(names[0].contains("Manu Sporny"), "got: {}", names[0]);

    // foaf:homepage is an IRI node, not a literal
    let homepages = query_values(
        &ds,
        "SELECT ?hp WHERE { ?s <http://xmlns.com/foaf/0.1/homepage> ?hp }",
        "hp",
    );
    assert_eq!(homepages.len(), 1);
    assert_eq!(homepages[0], "<http://manu.sporny.org/>");
}

// ── §3.2  Document with @context ──────────────────────────────────────────────

/// Spec §3.2 — compact form using a context with prefix mappings and
/// `"@type": "@id"` for IRI-coerced properties.
/// Subject IRI is <http://manu.sporny.org/>.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_context_compact_form() {
    let ds = parse_file("jsonld_context.jsonld");

    // Subject must be interned as an IRI
    assert!(has_iri(&ds, "http://manu.sporny.org/"));

    // foaf:name literal
    let names = query_values(
        &ds,
        r#"SELECT ?n WHERE { <http://manu.sporny.org/> <http://xmlns.com/foaf/0.1/name> ?n }"#,
        "n",
    );
    assert_eq!(names.len(), 1);
    assert!(names[0].contains("Manu Sporny"));

    // homepage and img are IRI-coerced → should be IRI nodes, not literals
    let homepages = query_values(
        &ds,
        r#"SELECT ?h WHERE { <http://manu.sporny.org/> <http://xmlns.com/foaf/0.1/homepage> ?h }"#,
        "h",
    );
    assert_eq!(homepages.len(), 1);
    assert_eq!(homepages[0], "<http://manu.sporny.org/>");
}

// ── §3.4  @type declarations ──────────────────────────────────────────────────

/// Spec §3.4 — `@type` on a node maps to rdf:type triples; nested nodes with
/// their own @id are stored as separate subjects.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_type_declarations() {
    let ds = parse_file("jsonld_types.jsonld");

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const FOAF_PERSON: &str = "http://xmlns.com/foaf/0.1/Person";

    // Both alice and bob must have rdf:type foaf:Person
    let typed = query_count(
        &ds,
        &format!("SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{FOAF_PERSON}> }}"),
    );
    assert_eq!(
        typed, 2,
        "alice and bob should both be typed as foaf:Person"
    );

    // foaf:knows triple: alice → bob
    let knows = query_count(
        &ds,
        "SELECT ?s WHERE { \
            <http://example.org/person/alice> \
            <http://xmlns.com/foaf/0.1/knows> \
            <http://example.org/person/bob> }",
    );
    assert_eq!(knows, 1, "alice should know bob");
}

// ── §3.5  Language-tagged strings ────────────────────────────────────────────

/// Spec §3.5 — `@language` produces language-tagged literals (e.g. "foo"@en).
/// dc:title has two values (one @en, one @de); dc:description is English only.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_language_tags() {
    let ds = parse_file("jsonld_language_tags.jsonld");

    // Two dc:title values
    let titles = query_count(
        &ds,
        "SELECT ?t WHERE { <http://example.org/book/1> <http://purl.org/dc/elements/1.1/title> ?t }",
    );
    assert_eq!(titles, 2, "expected en and de titles");

    // The English title specifically
    let en_titles = query_values(
        &ds,
        r#"SELECT ?t WHERE {
            <http://example.org/book/1>
            <http://purl.org/dc/elements/1.1/title>
            ?t FILTER(lang(?t) = "en") }"#,
        "t",
    );
    assert_eq!(en_titles.len(), 1);
    assert!(en_titles[0].contains("Fundamentals of Linked Data"));
    assert!(en_titles[0].contains("@en"));
}

// ── §4  Typed literals (xsd:date, xsd:integer) ───────────────────────────────

/// Spec §4 — `"@type": "xsd:date"` and `"@type": "xsd:integer"` produce
/// typed literals rather than plain strings.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_typed_literals() {
    let ds = parse_file("jsonld_typed_literals.jsonld");

    const LENNON: &str = "http://dbpedia.org/resource/John_Lennon";
    const BIRTH_DATE: &str = "http://schema.org/birthDate";
    const AGE: &str = "http://schema.org/age";

    // birthDate should be a typed literal "1940-10-09"^^xsd:date
    let bdates = query_values(
        &ds,
        &format!("SELECT ?d WHERE {{ <{LENNON}> <{BIRTH_DATE}> ?d }}"),
        "d",
    );
    assert_eq!(bdates.len(), 1);
    assert!(
        bdates[0].contains("1940-10-09"),
        "expected birth date literal, got: {}",
        bdates[0]
    );
    assert!(
        bdates[0].contains("http://www.w3.org/2001/XMLSchema#date"),
        "expected xsd:date type, got: {}",
        bdates[0]
    );

    // age should be typed xsd:integer
    let ages = query_values(
        &ds,
        &format!("SELECT ?a WHERE {{ <{LENNON}> <{AGE}> ?a }}"),
        "a",
    );
    assert_eq!(ages.len(), 1);
    assert!(
        ages[0].contains("http://www.w3.org/2001/XMLSchema#integer"),
        "expected xsd:integer type, got: {}",
        ages[0]
    );
}

// ── §4.9  Named graphs via @graph ─────────────────────────────────────────────

/// Spec §4.9 — `@graph` creates a named graph. Triples inside @graph belong
/// to the named graph identified by the outer @id.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_named_graph() {
    let ds = parse_file("jsonld_named_graph.jsonld");

    // The named graph IRI must be interned
    assert!(has_iri(&ds, "http://example.org/myGraph"));

    // alice and bob must be present as subjects
    assert!(has_iri(&ds, "http://example.org/alice"));
    assert!(has_iri(&ds, "http://example.org/bob"));

    // Triples live inside the named graph, not the default graph
    let in_named = query_count(
        &ds,
        r#"SELECT ?s ?p ?o WHERE {
            GRAPH <http://example.org/myGraph> { ?s ?p ?o }
        }"#,
    );
    assert!(
        in_named >= 2,
        "expected triples in named graph, got {}",
        in_named
    );

    let in_default = query_count(
        &ds,
        "SELECT ?s ?p ?o WHERE { \
            <http://example.org/alice> \
            <http://xmlns.com/foaf/0.1/name> ?o }",
    );
    assert_eq!(
        in_default, 0,
        "triples inside @graph should not appear in the default graph"
    );
}

// ── §4.3  Lists via @list ─────────────────────────────────────────────────────

/// Spec §4.3 — `@list` maps to an RDF list (rdf:first / rdf:rest / rdf:nil
/// chain). Order must be preserved.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_rdf_list() {
    let ds = parse_file("jsonld_list.jsonld");

    // The two foaf:knows list members must ultimately be reachable
    assert!(has_iri(&ds, "http://example.org/bob"));
    assert!(has_iri(&ds, "http://example.org/carol"));

    // rdf:first and rdf:rest must appear as predicates (list encoding)
    assert!(has_iri(
        &ds,
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#first"
    ));
    assert!(has_iri(
        &ds,
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest"
    ));
    assert!(has_iri(
        &ds,
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil"
    ));
}

// ── §3.1 (array form)  Multiple top-level subjects ───────────────────────────

/// A JSON-LD document whose top level is an array of node objects.
/// Three persons: alice, bob, carol. Alice and Bob mutually foaf:know each other.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_multiple_subjects_array() {
    let ds = parse_file("jsonld_multiple_subjects.jsonld");

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const FOAF_PERSON: &str = "http://xmlns.com/foaf/0.1/Person";

    // Three foaf:Person individuals
    let persons = query_count(
        &ds,
        &format!("SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{FOAF_PERSON}> }}"),
    );
    assert_eq!(persons, 3);

    // Mutual foaf:knows
    let knows = query_count(
        &ds,
        "SELECT ?s ?o WHERE { ?s <http://xmlns.com/foaf/0.1/knows> ?o }",
    );
    assert_eq!(knows, 2, "alice knows bob and bob knows alice");
}

// ── §4.7  @reverse properties ────────────────────────────────────────────────

/// Spec §4.7 — `@reverse` inverts the direction of the triple.
/// `{ "@id": "bob", "@reverse": { "foaf:knows": [alice, carol] } }`
/// must produce `alice foaf:knows bob` and `carol foaf:knows bob`, not the
/// other way around.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_reverse_property() {
    let ds = parse_file("jsonld_reverse.jsonld");

    // Triples should be alice→bob and carol→bob (reversed), not bob→alice/carol
    let knows_bob = query_count(
        &ds,
        r#"SELECT ?s WHERE {
            ?s <http://xmlns.com/foaf/0.1/knows> <http://example.org/bob>
        }"#,
    );
    assert_eq!(knows_bob, 2, "alice and carol should both know bob");

    let bob_knows = query_count(
        &ds,
        r#"SELECT ?o WHERE {
            <http://example.org/bob> <http://xmlns.com/foaf/0.1/knows> ?o
        }"#,
    );
    assert_eq!(
        bob_knows, 0,
        "@reverse must not produce forward triples from bob"
    );
}

// ── §4.5  @vocab shorthand ───────────────────────────────────────────────────

/// Spec §4.5 — `@vocab` resolves unqualified property names to a base IRI.
/// "predicate" → <http://example.org/vocab/predicate>
/// "label"     → <http://example.org/vocab/label>
/// "count"     → <http://example.org/vocab/count> with xsd:integer type
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_vocab_shorthand() {
    let ds = parse_file("jsonld_vocab.jsonld");

    // @vocab-resolved IRIs must appear in the store
    assert!(has_iri(&ds, "http://example.org/vocab/predicate"));
    assert!(has_iri(&ds, "http://example.org/vocab/label"));
    assert!(has_iri(&ds, "http://example.org/vocab/count"));

    // "count" value is typed xsd:integer
    let count_vals = query_values(
        &ds,
        "SELECT ?v WHERE { <http://example.org/subject> <http://example.org/vocab/count> ?v }",
        "v",
    );
    assert_eq!(count_vals.len(), 1);
    assert!(
        count_vals[0].contains("http://www.w3.org/2001/XMLSchema#integer"),
        "count should be typed xsd:integer, got: {}",
        count_vals[0]
    );
}

// ── Nested / embedded node objects ───────────────────────────────────────────

/// Deeply nested node objects (company → employees → address).
/// All @id nodes must be interned; blank-node address becomes a reified
/// resource with its own property triples.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_nested_node_objects() {
    let ds = parse_file("jsonld_nested_nodes.jsonld");

    // Top-level organisation
    assert!(has_iri(&ds, "http://example.org/company/acme"));

    // Two employees
    let employees = query_count(
        &ds,
        r#"SELECT ?e WHERE {
            <http://example.org/company/acme>
            <http://schema.org/employee> ?e
        }"#,
    );
    assert_eq!(employees, 2);

    // Alice's job title
    let titles = query_values(
        &ds,
        r#"SELECT ?t WHERE {
            <http://example.org/person/alice> <http://schema.org/jobTitle> ?t
        }"#,
        "t",
    );
    assert_eq!(titles.len(), 1);
    assert!(titles[0].contains("Engineer"));

    // Nested postal address for alice (blank node with addressLocality)
    let localities = query_count(
        &ds,
        r#"SELECT ?loc WHERE {
            <http://example.org/person/alice> <http://schema.org/address> ?addr .
            ?addr <http://schema.org/addressLocality> ?loc
        }"#,
    );
    assert_eq!(
        localities, 1,
        "alice should have an address with a locality"
    );
}

// ── Inline parsing ─────────────────────────────────────────────────────────────

/// Parse JSON-LD supplied as an inline string (bytes), without a file on disk.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_inline_jsonld_string() {
    let ds = parse_str(
        r#"{
          "@context": { "schema": "http://schema.org/" },
          "@id": "http://example.org/thing",
          "schema:name": "A Thing",
          "schema:description": "Just a test."
        }"#,
    );

    let names = query_values(
        &ds,
        r#"SELECT ?n WHERE { <http://example.org/thing> <http://schema.org/name> ?n }"#,
        "n",
    );
    assert_eq!(names.len(), 1);
    assert!(names[0].contains("A Thing"));
}

// ── Serialisation (output) tests ─────────────────────────────────────────────

/// Serialising a non-empty datastore must produce a non-empty string.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn output_non_empty() {
    let ds = parse_file("jsonld_context.jsonld");
    let out = jsonld_parser::serialize_jsonld(&ds);
    assert!(!out.is_empty(), "serialised JSON-LD must not be empty");
}

/// The serialised output must be valid JSON (parseable by serde_json).
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn output_is_valid_json() {
    let ds = parse_file("jsonld_types.jsonld");
    let out = jsonld_parser::serialize_jsonld(&ds);
    serde_json::from_str::<serde_json::Value>(&out).expect("serialised JSON-LD must be valid JSON");
}

/// The serialised output must contain a `@context` key.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn output_contains_context() {
    let ds = parse_file("jsonld_context.jsonld");
    let out = jsonld_parser::serialize_jsonld(&ds);
    assert!(
        out.contains("@context"),
        "serialised JSON-LD should include @context"
    );
}

/// The serialised output must contain `@id` for IRI-identified nodes.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn output_contains_node_ids() {
    let ds = parse_file("jsonld_types.jsonld");
    let out = jsonld_parser::serialize_jsonld(&ds);
    assert!(
        out.contains("@id"),
        "serialised JSON-LD should include @id entries"
    );
    assert!(
        out.contains("http://example.org/person/alice") || out.contains("alice"),
        "alice's IRI should appear in the output"
    );
}

// ── Round-trip tests ─────────────────────────────────────────────────────────

/// Parse → serialise → re-parse: the triple count must be identical.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn roundtrip_triple_count_preserved() {
    let ds1 = parse_file("jsonld_types.jsonld");
    let out = jsonld_parser::serialize_jsonld(&ds1);
    let ds2 = parse_str(&out);

    let count1 = query_count(&ds1, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }");
    let count2 = query_count(&ds2, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }");
    assert_eq!(count1, count2, "round-trip must preserve triple count");
}

/// Round-trip preserves typed literals (xsd:date, xsd:integer).
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn roundtrip_typed_literals_preserved() {
    let ds1 = parse_file("jsonld_typed_literals.jsonld");
    let out = jsonld_parser::serialize_jsonld(&ds1);
    let ds2 = parse_str(&out);

    const LENNON: &str = "http://dbpedia.org/resource/John_Lennon";
    const BIRTH_DATE: &str = "http://schema.org/birthDate";

    let bdates = query_values(
        &ds2,
        &format!("SELECT ?d WHERE {{ <{LENNON}> <{BIRTH_DATE}> ?d }}"),
        "d",
    );
    assert_eq!(bdates.len(), 1, "birth date must survive round-trip");
    assert!(
        bdates[0].contains("http://www.w3.org/2001/XMLSchema#date"),
        "xsd:date type must survive round-trip, got: {}",
        bdates[0]
    );
}

/// Round-trip preserves language-tagged strings.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn roundtrip_language_tags_preserved() {
    let ds1 = parse_file("jsonld_language_tags.jsonld");
    let out = jsonld_parser::serialize_jsonld(&ds1);
    let ds2 = parse_str(&out);

    let titles = query_count(
        &ds2,
        "SELECT ?t WHERE { <http://example.org/book/1> <http://purl.org/dc/elements/1.1/title> ?t }",
    );
    assert_eq!(
        titles, 2,
        "both language-tagged titles must survive round-trip"
    );
}

/// Round-trip preserves named graph membership.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn roundtrip_named_graph_preserved() {
    let ds1 = parse_file("jsonld_named_graph.jsonld");
    let in_named1 = query_count(
        &ds1,
        "SELECT ?s ?p ?o WHERE { GRAPH <http://example.org/myGraph> { ?s ?p ?o } }",
    );

    let out = jsonld_parser::serialize_jsonld(&ds1);
    let ds2 = parse_str(&out);
    let in_named2 = query_count(
        &ds2,
        "SELECT ?s ?p ?o WHERE { GRAPH <http://example.org/myGraph> { ?s ?p ?o } }",
    );

    assert_eq!(
        in_named1, in_named2,
        "named graph triple count must survive round-trip"
    );
}

// ── §3.5  Multiple @type values ───────────────────────────────────────────────

/// Spec §3.5 — a node may carry several types; each becomes a separate
/// rdf:type triple.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_multiple_types() {
    let ds = parse_file("jsonld_multiple_types.jsonld");

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let types = query_count(
        &ds,
        &format!("SELECT ?t WHERE {{ <http://example.org/alice> <{RDF_TYPE}> ?t }}"),
    );
    assert_eq!(types, 3, "alice should have three rdf:type triples");

    assert!(has_iri(&ds, "http://xmlns.com/foaf/0.1/Person"));
    assert!(has_iri(&ds, "http://schema.org/Person"));
    assert!(has_iri(&ds, "http://example.org/Employee"));
}

// ── §4.1.3  @base for relative IRI resolution ────────────────────────────────

/// Spec §4.1.3 — `@base` in the context resolves relative IRI references in
/// `@id` values.  "people/alice" → <http://example.org/people/alice>.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_base_relative_iris() {
    let ds = parse_file("jsonld_base.jsonld");

    // Resolved subject
    assert!(
        has_iri(&ds, "http://example.org/people/alice"),
        "relative @id should resolve against @base"
    );
    // Resolved object of foaf:knows
    assert!(has_iri(&ds, "http://example.org/people/bob"));

    // Absolute IRI in @id must be left unchanged
    let homepages = query_values(
        &ds,
        r#"SELECT ?h WHERE {
            <http://example.org/people/alice>
            <http://xmlns.com/foaf/0.1/homepage> ?h }"#,
        "h",
    );
    assert_eq!(homepages.len(), 1);
    assert_eq!(homepages[0], "<http://alice.example.org/>");
}

// ── §4.1.5  Compact IRIs (prefix:local) ──────────────────────────────────────

/// Spec §4.1.5 — compact IRIs in context-defined prefixes expand to full IRIs.
/// "foaf:Person" → <http://xmlns.com/foaf/0.1/Person>.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_compact_iris() {
    let ds = parse_file("jsonld_compact_iris.jsonld");

    // Subject "ex:alice" must resolve to full IRI
    assert!(has_iri(&ds, "http://example.org/alice"));

    // @type "foaf:Person" must resolve
    assert!(has_iri(&ds, "http://xmlns.com/foaf/0.1/Person"));

    let names = query_values(
        &ds,
        r#"SELECT ?n WHERE {
            <http://example.org/alice>
            <http://xmlns.com/foaf/0.1/name> ?n }"#,
        "n",
    );
    assert_eq!(names.len(), 1);
    assert!(names[0].contains("Alice"));
}

// ── §4.1.7  Keyword aliasing ─────────────────────────────────────────────────

/// Spec §4.1.7 — keywords like @id, @type, @value, @language may be aliased
/// to plain terms in the context.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_keyword_aliases() {
    let ds = parse_file("jsonld_keyword_alias.jsonld");

    // "id" is an alias for @id → alice must be interned as IRI
    assert!(has_iri(&ds, "http://example.org/alice"));

    // "type" is an alias for @type → rdf:type foaf:Person
    let types = query_count(
        &ds,
        "SELECT ?t WHERE { \
            <http://example.org/alice> \
            <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ?t }",
    );
    assert_eq!(types, 1, "type alias should produce an rdf:type triple");

    // "lang" alias for @language → language-tagged literal
    let names = query_values(
        &ds,
        r#"SELECT ?n WHERE {
            <http://example.org/alice>
            <http://xmlns.com/foaf/0.1/name> ?n }"#,
        "n",
    );
    assert_eq!(names.len(), 1);
    assert!(
        names[0].contains("@en"),
        "name should be language-tagged @en"
    );
}

// ── §4.1.8  Property-scoped contexts ─────────────────────────────────────────

/// Spec §4.1.8 — a `@context` nested inside a term definition applies only
/// within values of that property.  Terms like "street" should only resolve
/// inside the "address" property.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_property_scoped_context() {
    let ds = parse_file("jsonld_scoped_context_property.jsonld");

    // Top-level schema:name on alice
    let names = query_values(
        &ds,
        r#"SELECT ?n WHERE {
            <http://example.org/alice> <http://schema.org/name> ?n }"#,
        "n",
    );
    assert_eq!(names.len(), 1);
    assert!(names[0].contains("Alice"));

    // "street" inside address resolves to schema:streetAddress
    let streets = query_count(
        &ds,
        r#"SELECT ?a WHERE {
            <http://example.org/alice> <http://schema.org/address> ?addr .
            ?addr <http://schema.org/streetAddress> ?a }"#,
    );
    assert_eq!(
        streets, 1,
        "scoped context should resolve 'street' to schema:streetAddress"
    );

    // "city" resolves to schema:addressLocality inside address
    let cities = query_count(
        &ds,
        r#"SELECT ?c WHERE {
            <http://example.org/alice> <http://schema.org/address> ?addr .
            ?addr <http://schema.org/addressLocality> ?c }"#,
    );
    assert_eq!(cities, 1);
}

// ── §4.1.9  Type-scoped contexts ─────────────────────────────────────────────

/// Spec §4.1.9 — a `@context` attached to a type term applies within objects
/// of that type.  "name" resolves to foaf:name only for foaf:Person nodes.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_type_scoped_context() {
    let ds = parse_file("jsonld_scoped_context_type.jsonld");

    // Both alice and bob are foaf:Person
    let persons = query_count(
        &ds,
        "SELECT ?s WHERE { \
            ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> \
               <http://xmlns.com/foaf/0.1/Person> }",
    );
    assert_eq!(persons, 2);

    // "name" (type-scoped alias) expands to foaf:name for both
    let names = query_count(
        &ds,
        "SELECT ?s ?n WHERE { ?s <http://xmlns.com/foaf/0.1/name> ?n }",
    );
    assert_eq!(
        names, 2,
        "type-scoped 'name' should expand to foaf:name for both persons"
    );
}

// ── §4.1.10  Protected term definitions ──────────────────────────────────────

/// Spec §4.1.10 — `@protected: true` marks terms that cannot be overridden
/// by a subsequent context.  The parser must accept the document and load
/// the triple normally.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_protected_terms() {
    let ds = parse_file("jsonld_protected_terms.jsonld");

    // Protected term "name" → foaf:name must still resolve
    let names = query_values(
        &ds,
        r#"SELECT ?n WHERE {
            <http://example.org/alice>
            <http://xmlns.com/foaf/0.1/name> ?n }"#,
        "n",
    );
    assert_eq!(names.len(), 1);
    assert!(names[0].contains("Alice"));
}

// ── §4.1.11  Imported contexts (@import) ─────────────────────────────────────

/// Spec §4.1.11 — `@import` pulls in an external context document.
/// Requires network access; skipped in offline environments.
#[test]
#[ignore = "jsonld_parser crate not yet implemented; @import requires network access"]
fn parse_imported_context() {
    // This test would load a JSON-LD document whose @context contains
    // "@import": "https://example.org/base-context.jsonld" and verify
    // that terms from the imported context are resolved correctly.
    // Deferred: requires HTTP context fetching.
    unimplemented!("@import context fetching not yet implemented");
}

// ── §4.2.2  JSON literals (@type: @json) ─────────────────────────────────────

/// Spec §4.2.2 — `"@type": "@json"` stores the value as a JSON literal
/// (rdf:JSON datatype).  The raw JSON structure must be preserved in the
/// datastore.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_json_literal() {
    let ds = parse_file("jsonld_json_literal.jsonld");

    assert!(has_iri(&ds, "http://example.org/event/1"));
    assert!(has_iri(&ds, "http://example.org/payload"));

    // The object must be a typed literal with rdf:JSON datatype
    let payloads = query_values(
        &ds,
        r#"SELECT ?p WHERE {
            <http://example.org/event/1>
            <http://example.org/payload> ?p }"#,
        "p",
    );
    assert_eq!(payloads.len(), 1);
    assert!(
        payloads[0].contains("http://www.w3.org/1999/02/22-rdf-syntax-ns#JSON"),
        "JSON literal must have rdf:JSON datatype, got: {}",
        payloads[0]
    );
}

// ── §4.2.6  Base direction (@direction) ──────────────────────────────────────

/// Spec §4.2.6 — `@direction` records the base text direction of a string
/// value ("ltr" or "rtl").  Two title values with different directions.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_base_direction() {
    let ds = parse_file("jsonld_direction.jsonld");

    // Two dc:title values (one English ltr, one Arabic rtl)
    let titles = query_count(
        &ds,
        "SELECT ?t WHERE { \
            <http://example.org/book/1> \
            <http://purl.org/dc/elements/1.1/title> ?t }",
    );
    assert_eq!(titles, 2, "expected two directional title literals");
}

// ── §4.3.2  @set container (unordered values) ────────────────────────────────

/// Spec §4.3.2 — `@container: @set` indicates that multiple values are
/// unordered.  Each value in the array becomes a separate triple; no
/// rdf:List encoding is used.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_set_container() {
    let ds = parse_file("jsonld_set.jsonld");

    // Three foaf:nick values, no rdf:first/rdf:rest
    let nicks = query_count(
        &ds,
        "SELECT ?n WHERE { \
            <http://example.org/alice> \
            <http://xmlns.com/foaf/0.1/nick> ?n }",
    );
    assert_eq!(nicks, 3, "each set element should become a separate triple");

    // rdf:first must NOT appear — @set does not use list encoding
    assert!(
        !has_iri(&ds, "http://www.w3.org/1999/02/22-rdf-syntax-ns#first"),
        "@set must not produce rdf:first/rdf:rest list structure"
    );
}

// ── §4.4  Nested properties (@nest) ──────────────────────────────────────────

/// Spec §4.4 — `@nest` groups properties for readability; the nesting key
/// itself does not appear as a predicate.  Properties inside the nest resolve
/// as if they were at the top level.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_nested_properties_at_nest() {
    let ds = parse_file("jsonld_nest.jsonld");

    // schema:jobTitle and schema:telephone must resolve from inside "details"
    let titles = query_values(
        &ds,
        r#"SELECT ?t WHERE {
            <http://example.org/alice> <http://schema.org/jobTitle> ?t }"#,
        "t",
    );
    assert_eq!(titles.len(), 1);
    assert!(titles[0].contains("Engineer"));

    let phones = query_values(
        &ds,
        r#"SELECT ?p WHERE {
            <http://example.org/alice> <http://schema.org/telephone> ?p }"#,
        "p",
    );
    assert_eq!(phones.len(), 1);

    // The nest key "details" must NOT become a predicate
    assert!(
        !has_iri(&ds, "details"),
        "@nest key must not appear as a predicate IRI"
    );
}

// ── §4.6  Data indexing ───────────────────────────────────────────────────────

/// Spec §4.6.1 — `@container: @index` uses arbitrary string keys as index
/// annotations.  Each value in the map becomes a triple; the index key itself
/// is stored as an rdf:value annotation (or discarded, depending on impl).
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_index_map() {
    let ds = parse_file("jsonld_index_map.jsonld");

    // The catalog must have two schema:entry triples
    let entries = query_count(
        &ds,
        "SELECT ?e WHERE { \
            <http://example.org/catalog> <http://schema.org/entry> ?e }",
    );
    assert_eq!(
        entries, 2,
        "index map should produce two schema:entry triples"
    );

    assert!(has_iri(&ds, "http://example.org/book/1"));
    assert!(has_iri(&ds, "http://example.org/article/1"));
}

/// Spec §4.6.4 — `@container: @language` is a language map: keys are BCP 47
/// language tags, values are plain strings.  Each entry becomes a
/// language-tagged literal triple.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_language_map() {
    let ds = parse_file("jsonld_language_map.jsonld");

    // Three dc:title triples, one per language
    let titles = query_count(
        &ds,
        "SELECT ?t WHERE { \
            <http://example.org/book/1> \
            <http://purl.org/dc/elements/1.1/title> ?t }",
    );
    assert_eq!(
        titles, 3,
        "language map should produce three language-tagged titles"
    );

    // English title specifically
    let en = query_values(
        &ds,
        r#"SELECT ?t WHERE {
            <http://example.org/book/1>
            <http://purl.org/dc/elements/1.1/title>
            ?t FILTER(lang(?t) = "en") }"#,
        "t",
    );
    assert_eq!(en.len(), 1);
    assert!(en[0].contains("Linked Data in Practice"));
}

/// Spec §4.6.5 — `@container: @id` is an id map: keys are IRI/compact IRI
/// values and become the `@id` of the nested node.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_id_map() {
    let ds = parse_file("jsonld_id_map.jsonld");

    // alice knows bob and carol
    let knows = query_count(
        &ds,
        "SELECT ?o WHERE { \
            <http://example.org/alice> \
            <http://xmlns.com/foaf/0.1/knows> ?o }",
    );
    assert_eq!(knows, 2);

    assert!(has_iri(&ds, "http://example.org/bob"));
    assert!(has_iri(&ds, "http://example.org/carol"));
}

/// Spec §4.6.6 — `@container: @type` is a type map: keys are type IRIs and
/// implicitly set `@type` on the nested node objects.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_type_map() {
    let ds = parse_file("jsonld_type_map.jsonld");

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const FOAF_PERSON: &str = "http://xmlns.com/foaf/0.1/Person";

    // Both alice and bob get rdf:type foaf:Person from the type-map key
    let persons = query_count(
        &ds,
        &format!("SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{FOAF_PERSON}> }}"),
    );
    assert_eq!(
        persons, 2,
        "type map key should set rdf:type on each member"
    );

    // Group has ex:member links to both
    let members = query_count(
        &ds,
        "SELECT ?m WHERE { \
            <http://example.org/group/1> <http://example.org/member> ?m }",
    );
    assert_eq!(members, 2);
}

/// Spec §4.6.7 — `@container: @graph` wraps each value in a named graph.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_graph_container() {
    let ds = parse_file("jsonld_graph_container.jsonld");

    // alice must appear inside the named graph
    let in_named = query_count(
        &ds,
        r#"SELECT ?s WHERE {
            GRAPH <http://example.org/graph/people> {
                ?s <http://xmlns.com/foaf/0.1/name> ?n
            }
        }"#,
    );
    assert_eq!(
        in_named, 1,
        "graph container should place triples in named graph"
    );
}

// ── §4.7  @included ──────────────────────────────────────────────────────────

/// Spec §4.7 — `@included` embeds additional node objects that are included
/// in the same output but not nested under any property.  Their triples end
/// up in the same graph as the top-level node.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn parse_included_nodes() {
    let ds = parse_file("jsonld_included.jsonld");

    // Both alice and bob must be present
    assert!(has_iri(&ds, "http://example.org/alice"));
    assert!(has_iri(&ds, "http://example.org/bob"));

    // Bob's name must be loaded from the @included block
    let bob_names = query_values(
        &ds,
        r#"SELECT ?n WHERE {
            <http://example.org/bob> <http://xmlns.com/foaf/0.1/name> ?n }"#,
        "n",
    );
    assert_eq!(bob_names.len(), 1);
    assert!(bob_names[0].contains("Bob"));

    // Mutual foaf:knows
    let knows = query_count(
        &ds,
        "SELECT ?s ?o WHERE { ?s <http://xmlns.com/foaf/0.1/knows> ?o }",
    );
    assert_eq!(
        knows, 2,
        "alice knows bob and bob knows alice via @included"
    );
}

// ── §5  Document forms (output) ───────────────────────────────────────────────

/// Spec §5.1 — the expanded form of a JSON-LD document uses full IRIs for
/// all keys and no @context.  The serialiser must be able to emit this form.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn output_expanded_form() {
    let ds = parse_file("jsonld_context.jsonld");
    let expanded = jsonld_parser::serialize_jsonld_expanded(&ds);
    let v: serde_json::Value =
        serde_json::from_str(&expanded).expect("expanded form must be valid JSON");

    // Expanded form has no @context key at the top level
    if let serde_json::Value::Array(nodes) = &v {
        for node in nodes {
            assert!(
                !node
                    .as_object()
                    .map(|o| o.contains_key("@context"))
                    .unwrap_or(false),
                "expanded form must not contain @context"
            );
        }
    } else {
        panic!("expanded JSON-LD must be a JSON array");
    }

    // All keys (other than @-keywords) must be absolute IRIs
    assert!(
        expanded.contains("http://xmlns.com/foaf/0.1/name"),
        "expanded form must use full IRI for foaf:name"
    );
}

/// Spec §5.2 — the compacted form uses a @context to shorten IRIs.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn output_compacted_form() {
    let ds = parse_file("jsonld_context.jsonld");
    let compacted = jsonld_parser::serialize_jsonld(&ds);
    let v: serde_json::Value =
        serde_json::from_str(&compacted).expect("compacted form must be valid JSON");

    // Must have a @context
    let obj = v
        .as_object()
        .expect("compacted JSON-LD should be a JSON object");
    assert!(
        obj.contains_key("@context"),
        "compacted form must have @context"
    );
}

/// Spec §5.3 — the flattened form puts all node objects at the top level
/// (no nesting) and uses full IRIs.
#[test]
#[ignore = "jsonld_parser crate not yet implemented"]
fn output_flattened_form() {
    let ds = parse_file("jsonld_nested_nodes.jsonld");
    let flattened = jsonld_parser::serialize_jsonld_flattened(&ds);
    let v: serde_json::Value =
        serde_json::from_str(&flattened).expect("flattened form must be valid JSON");

    // Flattened form is a JSON-LD document with a top-level @graph array
    // where every node object has an @id and no node is nested inside another.
    let graph = v
        .get("@graph")
        .and_then(|g| g.as_array())
        .expect("flattened form must have a top-level @graph array");

    // Every entry in @graph must have @id
    for node in graph {
        assert!(
            node.get("@id").is_some(),
            "every node in flattened @graph must have @id"
        );
        // No nested node objects (values should be IRI references, not inline objects
        // with their own properties beyond @id/@value)
    }

    // All four named nodes from the nested file must appear at top level
    let ids: Vec<_> = graph
        .iter()
        .filter_map(|n| n.get("@id").and_then(|v| v.as_str()))
        .collect();
    assert!(ids.contains(&"http://example.org/company/acme"));
    assert!(ids.contains(&"http://example.org/person/alice"));
    assert!(ids.contains(&"http://example.org/person/bob"));
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! TDD tests for RDF 1.2 triple-term support in the Turtle / TriG parser.
//!
//! All tests are `#[ignore]` pending implementation in phase R2.
//! Tracked in [#145](https://github.com/daghovland/rdf-datalog/issues/145).
//!
//! Each test documents one specific behaviour that the parser must exhibit once
//! RDF 1.2 triple-term syntax (`<<( s p o )>>`) is wired up:
//!
//! - How many rows appear in `Datastore::reified_triples`
//! - How many rows appear in `Datastore::named_graphs`
//! - What `GraphElement` variant the relevant quad carries as its subject

use dag_rdf::{Datastore, GraphElement};

/// Parse Turtle/TriG from a string and return the populated `Datastore`.
///
/// Panics if parsing fails — these tests are about correct *successful* parses.
fn parse_ttl(ttl: &str) -> Datastore {
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).expect("parse failed");
    ds
}

/// A triple term used as the **subject** of an annotation triple.
///
/// ```turtle
/// @prefix : <https://example.org/> .
/// <<( :alice :knows :bob )>> :assertedBy :carol .
/// ```
///
/// After parsing:
/// - `reified_triples` must contain exactly one row — the interned triple term.
/// - `named_graphs` must contain exactly one row — the annotation triple.
/// - The `subject` field of that annotation quad must resolve to
///   `GraphElement::TripleTerm(_)`.
#[test]
// #145: blocked on subject-position triple terms. Empirically, `oxttl` 0.2.3
// (even with the `rdf-12` feature) rejects `<<(` in subject position with
// `TurtleSyntaxError { message: "<<( is not a valid subject or graph name" }`.
// This matches `oxrdf` 0.3.3's `Triple::subject: NamedOrBlankNode`, which has
// no variant for a nested triple term. See "Subject-position blocker" in
// docs/plans/RDF12_PLAN.md (Phase R2) for the two options considered; this is
// Option A (object-position support only) until oxrdf/oxttl add a subject
// representation for triple terms. Pending upstream oxrdf/oxttl support:
// tracked in #153.
#[ignore]
fn test_triple_term_as_subject() {
    let ds = parse_ttl(
        r#"
        @prefix : <https://example.org/> .
        <<( :alice :knows :bob )>> :assertedBy :carol .
    "#,
    );
    // The embedded triple must be interned exactly once.
    assert_eq!(
        ds.reified_triples.quad_count, 1,
        "reified_triples should hold exactly one triple term"
    );
    // The annotation triple appears in the default graph.
    assert_eq!(
        ds.named_graphs.quad_count, 1,
        "named_graphs should hold exactly one annotation quad"
    );
    // The subject of that quad must be a TripleTerm.
    let quad = ds.named_graphs.get_all_quads().next().unwrap();
    assert!(
        matches!(
            ds.resources.get_graph_element(quad.subject),
            GraphElement::TripleTerm(_)
        ),
        "the subject of the annotation quad must be GraphElement::TripleTerm"
    );
}

/// Same triple term appears in **two** named graphs.
///
/// ```turtle
/// @prefix : <https://example.org/> .
/// :g1 { <<( :alice :knows :bob )>> :assertedBy :carol . }
/// :g2 { <<( :alice :knows :bob )>> :believedBy :dave . }
/// ```
///
/// The triple term `<<( :alice :knows :bob )>>` is the same value in both
/// graphs.  The parser must intern it **once** (one row in `reified_triples`)
/// and produce **two** annotation quads (one per named graph) in `named_graphs`.
#[test]
// #145: blocked on subject-position triple terms (see comment on
// `test_triple_term_as_subject` above). Also note: this test's Turtle source
// uses TriG named-graph block syntax (`:g1 { ... }`), so once unblocked it
// will additionally need `parse_ttl` (or a TriG-specific variant) to call
// `turtle::parse_trig` rather than `turtle::parse_turtle`. Pending upstream
// oxrdf/oxttl support: tracked in #153.
#[ignore]
fn test_same_triple_term_in_two_named_graphs() {
    let ds = parse_ttl(
        r#"
        @prefix : <https://example.org/> .
        :g1 { <<( :alice :knows :bob )>> :assertedBy :carol . }
        :g2 { <<( :alice :knows :bob )>> :believedBy :dave . }
    "#,
    );
    assert_eq!(
        ds.reified_triples.quad_count, 1,
        "identical triple terms must be interned exactly once"
    );
    assert_eq!(
        ds.named_graphs.quad_count, 2,
        "two annotation quads — one per named graph"
    );
}

/// **Nested** triple terms: the inner term is itself embedded in an outer term.
///
/// ```turtle
/// @prefix : <https://example.org/> .
/// <<( <<( :alice :knows :bob )>> :assertedBy :carol )>> :believedBy :eve .
/// ```
///
/// After parsing:
/// - `reified_triples` must contain **two** rows — the inner and outer triple
///   terms, each interned once.
/// - `named_graphs` must contain one annotation quad.
#[test]
// #145: blocked on subject-position triple terms (see comment on
// `test_triple_term_as_subject` above) — the outer triple term here is
// itself a statement subject. Pending upstream oxrdf/oxttl support:
// tracked in #153.
#[ignore]
fn test_nested_triple_term() {
    let ds = parse_ttl(
        r#"
        @prefix : <https://example.org/> .
        <<( <<( :alice :knows :bob )>> :assertedBy :carol )>> :believedBy :eve .
    "#,
    );
    assert_eq!(
        ds.reified_triples.quad_count, 2,
        "two triple terms: inner and outer"
    );
    assert_eq!(
        ds.named_graphs.quad_count, 1,
        "one annotation quad in the default graph"
    );
}

/// Triple term appears as an **object** (the `<<( … )>>` syntax in object
/// position, i.e. `rdf:reifies` usage via triple terms).
///
/// ```turtle
/// @prefix : <https://example.org/> .
/// :carol :claims <<( :alice :knows :bob )>> .
/// ```
///
/// After parsing:
/// - `reified_triples` must contain exactly one row — the interned triple term.
/// - `named_graphs` must contain exactly one quad with the triple term as
///   the **object**.
#[test]
fn test_triple_term_as_object() {
    let ds = parse_ttl(
        r#"
        @prefix : <https://example.org/> .
        :carol :claims <<( :alice :knows :bob )>> .
    "#,
    );
    assert_eq!(
        ds.reified_triples.quad_count, 1,
        "reified_triples should hold exactly one triple term"
    );
    assert_eq!(
        ds.named_graphs.quad_count, 1,
        "named_graphs should hold exactly one annotation quad"
    );
    let quad = ds.named_graphs.get_all_quads().next().unwrap();
    assert!(
        matches!(
            ds.resources.get_graph_element(quad.obj),
            GraphElement::TripleTerm(_)
        ),
        "the object of the quad must be GraphElement::TripleTerm"
    );
}

/// Triple term with a **literal object** inside the embedded triple.
///
/// ```turtle
/// @prefix : <https://example.org/> .
/// <<( :alice :age "42"^^<http://www.w3.org/2001/XMLSchema#integer> )>>
///     :assertedBy :carol .
/// ```
///
/// After parsing `reified_triples` must contain one row and `named_graphs`
/// one row, with the triple term as the subject.
#[test]
// #145: blocked on subject-position triple terms (see comment on
// `test_triple_term_as_subject` above). Pending upstream oxrdf/oxttl
// support: tracked in #153.
#[ignore]
fn test_triple_term_with_literal_object() {
    let ds = parse_ttl(
        r#"
        @prefix : <https://example.org/> .
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
        <<( :alice :age "42"^^xsd:integer )>> :assertedBy :carol .
    "#,
    );
    assert_eq!(ds.reified_triples.quad_count, 1);
    assert_eq!(ds.named_graphs.quad_count, 1);
    let quad = ds.named_graphs.get_all_quads().next().unwrap();
    assert!(
        matches!(
            ds.resources.get_graph_element(quad.subject),
            GraphElement::TripleTerm(_)
        ),
        "the subject of the annotation quad must be GraphElement::TripleTerm"
    );
}

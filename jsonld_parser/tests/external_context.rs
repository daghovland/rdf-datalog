/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Tests for external `@context` URL handling in the JSON-LD parser.
//!
//! Covers issue [#82](https://github.com/daghovland/rdf-datalog/issues/82):
//! - Error on unknown external context URL (no loader / Deny policy)
//! - Static-cache loader resolves well-known vocabulary contexts (schema.org)
//! - Custom mock loader resolves hand-written context terms

use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use ingress::NetworkPolicy;
use jsonld_parser::{StaticDocumentLoader, parse_jsonld, parse_jsonld_with_loader};
use std::sync::Arc;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Look up the first literal object of `(subject_iri, pred_iri, ?o)` in the
/// default graph.  Returns `None` if no such triple exists.
fn first_literal(ds: &Datastore, subject_iri: &str, pred_iri: &str) -> Option<String> {
    let subj_id = ds
        .resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            subject_iri.to_string(),
        ))))?;
    let pred_id = ds
        .resources
        .resource_map
        .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            pred_iri.to_string(),
        ))))?;
    ds.get_triples_with_subject_predicate(*subj_id, *pred_id)
        .next()
        .map(|t| ds.resources.get_graph_element(t.obj).to_string())
}

// ── Test 1: Deny policy returns a descriptive error ───────────────────────────

/// Parsing a JSON-LD document whose `@context` is an external URL must return
/// an `Err` when `NetworkPolicy::Deny` is in effect (the safe default), not
/// silently produce empty triples.
///
/// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
#[test]
fn test_external_context_url_returns_error_without_loader() {
    let mut ds = Datastore::new(1_000);
    let json = r#"{
        "@context": "https://example.org/context",
        "@id": "https://example.org/foo",
        "name": "Foo"
    }"#;
    let result = parse_jsonld(&mut ds, json.as_bytes(), NetworkPolicy::Deny);
    assert!(
        result.is_err(),
        "NetworkPolicy::Deny must return Err for external @context URLs; got Ok(())"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("https://example.org/context"),
        "error message should include the offending URL; got: {msg}"
    );
    // The error must reference either the issue tracker or network access instructions.
    assert!(
        msg.contains("#82") || msg.contains("network") || msg.contains("disabled"),
        "error message should explain why the fetch was blocked; got: {msg}"
    );
}

// ── Test 2: StaticDocumentLoader resolves schema.org context ─────────────────

/// Parsing with a `StaticDocumentLoader` that includes a schema.org context
/// stub must correctly resolve `name` (a schema.org term) to
/// `https://schema.org/name`.
///
/// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
#[test]
fn test_external_context_static_cache_schema_org() {
    let loader = StaticDocumentLoader::with_schema_org();
    let mut ds = Datastore::new(1_000);
    let json = r#"{
        "@context": "https://schema.org/",
        "@id": "https://example.org/thing",
        "name": "A Test Thing"
    }"#;
    parse_jsonld_with_loader(&mut ds, json.as_bytes(), Arc::new(loader))
        .expect("parse with static schema.org loader should succeed");

    let name_val = first_literal(&ds, "https://example.org/thing", "https://schema.org/name");
    assert!(
        name_val.is_some(),
        "schema:name triple must be present after resolving https://schema.org/ context; \
         check that @vocab or term expansion in the static schema.org stub maps 'name' → schema:name"
    );
    let name_str = name_val.unwrap();
    assert!(
        name_str.contains("A Test Thing"),
        "schema:name value should be 'A Test Thing'; got: {name_str}"
    );
}

// ── Test 3: Custom StaticDocumentLoader resolves hand-written context ─────────

/// A `StaticDocumentLoader` populated with a custom context JSON must resolve
/// the vocabulary terms defined in that context when parsing.
///
/// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
#[test]
fn test_external_context_mock_loader() {
    // The mock context maps `label` to `https://example.org/label`
    // and declares `ex` as a prefix for `https://example.org/`.
    let context_json = r#"{
        "@context": {
            "ex":    "https://example.org/",
            "label": { "@id": "ex:label" }
        }
    }"#;
    let loader = StaticDocumentLoader::new([(
        "https://example.org/vocab/context".to_string(),
        context_json.to_string(),
    )]);

    let mut ds = Datastore::new(1_000);
    let json = r#"{
        "@context": "https://example.org/vocab/context",
        "@id": "https://example.org/thing",
        "label": "My Mock Thing"
    }"#;
    parse_jsonld_with_loader(&mut ds, json.as_bytes(), Arc::new(loader))
        .expect("parse with mock loader should succeed");

    let label_val = first_literal(
        &ds,
        "https://example.org/thing",
        "https://example.org/label",
    );
    assert!(
        label_val.is_some(),
        "ex:label triple must be present after resolving mock context; \
         check that the loader returned the context JSON and terms were expanded"
    );
    let label_str = label_val.unwrap();
    assert!(
        label_str.contains("My Mock Thing"),
        "ex:label value should be 'My Mock Thing'; got: {label_str}"
    );
}

// ── Test 4: null in @context array preserves loader ───────────────────────────

/// A `null` entry in a `@context` array resets terms/prefixes but must preserve
/// the active document loader and network policy.  If it drops the loader, any
/// subsequent URL string in the same array will fail with "requires a
/// DocumentLoader" instead of being resolved.
///
/// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
#[test]
fn test_null_in_context_array_preserves_loader() {
    // The context array is [null, "https://example.org/vocab/context"].
    // null should reset any previously accumulated terms, but the loader must
    // survive so the subsequent URL can be resolved.
    let context_json = r#"{
        "@context": {
            "ex":    "https://example.org/",
            "label": { "@id": "ex:label" }
        }
    }"#;
    let loader = StaticDocumentLoader::new([(
        "https://example.org/vocab/context".to_string(),
        context_json.to_string(),
    )]);

    let mut ds = Datastore::new(1_000);
    let json = r#"{
        "@context": [null, "https://example.org/vocab/context"],
        "@id": "https://example.org/thing",
        "label": "Preserved"
    }"#;
    parse_jsonld_with_loader(&mut ds, json.as_bytes(), Arc::new(loader))
        .expect("null in @context array must not drop the loader");

    let label_val = first_literal(
        &ds,
        "https://example.org/thing",
        "https://example.org/label",
    );
    assert!(
        label_val.is_some(),
        "ex:label triple must be present after [null, url] context array"
    );
}

// ── Test 5: cycle detection does not stack-overflow ───────────────────────────

/// When two context URLs each reference the other (A → B → A …), the parser
/// must detect the cycle and return a result rather than stack-overflowing.
///
/// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
#[test]
fn test_cycle_detection_does_not_stack_overflow() {
    // ctx_a references ctx_b and ctx_b references ctx_a.
    let ctx_a = r#"{"@context": ["https://example.org/ctx_b", {"ex": "https://example.org/"}]}"#;
    let ctx_b = r#"{"@context": "https://example.org/ctx_a"}"#;

    let loader = StaticDocumentLoader::new([
        ("https://example.org/ctx_a".to_string(), ctx_a.to_string()),
        ("https://example.org/ctx_b".to_string(), ctx_b.to_string()),
    ]);

    let mut ds = Datastore::new(1_000);
    let json = r#"{
        "@context": "https://example.org/ctx_a",
        "@id": "https://example.org/thing",
        "ex:name": "Cycle Test"
    }"#;
    // Must not panic/overflow. May succeed or return an error, but not hang.
    let _ = parse_jsonld_with_loader(&mut ds, json.as_bytes(), Arc::new(loader));
}

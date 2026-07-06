/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for `NetworkPolicy` (issue #118).
//!
//! Verifies that `Deny`, `Ignore`, and `Allow` modes behave correctly for
//! SPARQL SERVICE federation, SPARQL LOAD, and JSON-LD external @context URLs.
//!
//! See: <https://github.com/daghovland/rdf-datalog/issues/118>

use dag_rdf::Datastore;
use ingress::NetworkPolicy;
use jsonld_parser::parse_jsonld;
use sparql_endpoint::sparql_update::{apply_prepared_update, parse_update, prepare_update};
use sparql_parser::{ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;

fn empty_store() -> Datastore {
    Datastore::new(64)
}

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
    }
}

// ── SPARQL SERVICE ────────────────────────────────────────────────────────────

const SERVICE_QUERY: &str = r#"
    SELECT * WHERE {
      SERVICE <https://query.wikidata.org/sparql> { ?s ?p ?o }
    } LIMIT 5
"#;

const SERVICE_SILENT_QUERY: &str = r#"
    SELECT * WHERE {
      SERVICE SILENT <https://query.wikidata.org/sparql> { ?s ?p ?o }
    }
"#;

#[test]
fn service_deny_non_silent_returns_error() {
    let ds = empty_store();
    let (_, query) = parse_query(SERVICE_QUERY, &mut ctx()).expect("should parse");
    let err = match execute(&query, &ds, NetworkPolicy::Deny) {
        Err(e) => e,
        Ok(_) => panic!("Deny mode must reject non-SILENT SERVICE"),
    };
    assert!(
        err.contains("rejected") || err.contains("SERVICE"),
        "unexpected error message: {err}"
    );
    assert!(
        err.contains("--network") || err.contains("#118") || err.contains("network"),
        "error should mention how to enable network: {err}"
    );
}

#[test]
fn service_deny_silent_returns_empty() {
    let ds = empty_store();
    let (_, query) = parse_query(SERVICE_SILENT_QUERY, &mut ctx()).expect("should parse");
    // SILENT SERVICE: even under Deny the query should succeed with empty rows
    let result = execute(&query, &ds, NetworkPolicy::Deny)
        .expect("SILENT SERVICE under Deny must not error");
    let QueryResult::Select(r) = result else {
        panic!("expected SELECT result");
    };
    assert!(
        r.rows.is_empty(),
        "SILENT SERVICE must return empty result set"
    );
}

#[test]
fn service_ignore_returns_empty() {
    let ds = empty_store();
    let (_, query) = parse_query(SERVICE_QUERY, &mut ctx()).expect("should parse");
    let result =
        execute(&query, &ds, NetworkPolicy::Ignore).expect("Ignore mode must not error on SERVICE");
    let QueryResult::Select(r) = result else {
        panic!("expected SELECT result");
    };
    assert!(
        r.rows.is_empty(),
        "Ignore mode must return empty result set"
    );
}

#[test]
fn service_allow_returns_not_implemented_error() {
    let ds = empty_store();
    let (_, query) = parse_query(SERVICE_QUERY, &mut ctx()).expect("should parse");
    let err = match execute(&query, &ds, NetworkPolicy::Allow) {
        Err(e) => e,
        Ok(_) => panic!("Allow mode must return not-implemented error for SERVICE"),
    };
    assert!(
        err.contains("not yet implemented") || err.contains("not implemented"),
        "unexpected error message: {err}"
    );
}

// ── JSON-LD external @context ─────────────────────────────────────────────────

const JSONLD_EXTERNAL_CONTEXT: &str = r#"{
    "@context": "https://schema.org/",
    "@id": "http://example.org/person1",
    "name": "Alice"
}"#;

#[test]
fn jsonld_deny_external_context_returns_error() {
    let mut ds = empty_store();
    let err = parse_jsonld(
        &mut ds,
        JSONLD_EXTERNAL_CONTEXT.as_bytes(),
        NetworkPolicy::Deny,
    )
    .expect_err("Deny mode must reject external @context URL");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("not fetched") || msg.contains("rejected") || msg.contains("network"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn jsonld_ignore_external_context_succeeds_with_empty() {
    let mut ds = empty_store();
    // Ignore mode silently skips the external @context: no error.
    parse_jsonld(
        &mut ds,
        JSONLD_EXTERNAL_CONTEXT.as_bytes(),
        NetworkPolicy::Ignore,
    )
    .expect("Ignore mode must not error on external @context URL");
}

// ── SPARQL LOAD ───────────────────────────────────────────────────────────────

fn run_load(store: &mut Datastore, sparql: &str, network: NetworkPolicy) -> Result<(), String> {
    let ops = parse_update(sparql).map_err(|e| format!("parse error: {e:?}"))?;
    let (prepared, _log) = prepare_update(store, ops)?;
    apply_prepared_update(store, prepared, None, network).map(|_| ())
}

#[test]
fn load_deny_returns_error() {
    let mut ds = empty_store();
    let err = run_load(
        &mut ds,
        "LOAD <https://example.org/data.ttl>",
        NetworkPolicy::Deny,
    )
    .expect_err("Deny mode must reject LOAD");
    assert!(
        err.contains("rejected") || err.contains("LOAD") || err.contains("network"),
        "unexpected error message: {err}"
    );
}

#[test]
fn load_deny_silent_returns_ok() {
    let mut ds = empty_store();
    // LOAD SILENT: even under Deny the operation should succeed silently.
    run_load(
        &mut ds,
        "LOAD SILENT <https://example.org/data.ttl>",
        NetworkPolicy::Deny,
    )
    .expect("LOAD SILENT under Deny must not error");
    // No triples should have been loaded.
    assert_eq!(ds.named_graphs.quad_count, 0);
}

#[test]
fn load_ignore_returns_ok() {
    let mut ds = empty_store();
    run_load(
        &mut ds,
        "LOAD <https://example.org/data.ttl>",
        NetworkPolicy::Ignore,
    )
    .expect("Ignore mode must not error on LOAD");
    assert_eq!(ds.named_graphs.quad_count, 0);
}

/// Regression: `NetworkPolicy::Allow` no longer returns a "not implemented"
/// error — it now actually attempts the network fetch.
///
/// This test verifies that calling LOAD with Allow mode produces a network
/// error (connection refused or similar), not the old "not yet implemented"
/// placeholder message.
///
/// The full "it actually loads" behaviour is tested at the HTTP level in
/// `sparql_endpoint/tests/load_network.rs`.
///
/// Related: <https://github.com/daghovland/rdf-datalog/issues/119>
#[test]
fn load_allow_no_longer_not_implemented() {
    let mut ds = empty_store();
    // Port 1 is virtually never listening; we expect a connection-refused
    // network error rather than a "not yet implemented" message.
    let result = run_load(
        &mut ds,
        "LOAD <http://127.0.0.1:1/data.ttl>",
        NetworkPolicy::Allow,
    );
    match result {
        Err(e) => {
            assert!(
                !e.contains("not yet implemented") && !e.contains("not implemented"),
                "Allow mode must no longer return a not-implemented placeholder error; got: {e}"
            );
            // The error should describe an actual network failure.
            assert!(
                e.contains("HTTP") || e.contains("request") || e.contains("connect")
                    || e.contains("failed") || e.contains("LOAD"),
                "error should describe a network or parse failure: {e}"
            );
        }
        Ok(()) => {
            // Extremely unlikely, but if something is listening on port 1 and
            // serves valid Turtle, that's also an acceptable (passing) outcome.
        }
    }
}

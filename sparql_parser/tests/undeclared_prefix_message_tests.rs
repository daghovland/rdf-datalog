/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Unit coverage for `sparql_parser::describe_parse_error` and its helpers
//! (issue [#747](https://github.com/daghovland/rdf-datalog/issues/747)):
//! an undeclared-prefix parse failure must produce a message that actually
//! names the prefix, not just an opaque `nom::ErrorKind` dump. See also
//! `sparql_endpoint/tests/undeclared_prefix_http_tests.rs` for the
//! HTTP-level end-to-end coverage.

use sparql_parser::{describe_parse_error, parse_query, ParseFailureInfo, ParserContext};
use std::collections::HashMap;

fn ctx() -> ParserContext {
    ParserContext {
        prefixes: HashMap::new(),
        base: None,
    }
}

fn ctx_with(pairs: &[(&str, &str)]) -> ParserContext {
    ParserContext {
        prefixes: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        base: None,
    }
}

/// Builds the message exactly the way `sparql_endpoint`'s call sites do,
/// for a given query string and context.
fn message_for(query: &str, ctx: &mut ParserContext) -> String {
    match parse_query(query, ctx) {
        Ok(_) => panic!("expected a parse error for: {query}"),
        Err(e) => {
            let failure = ParseFailureInfo::from_nom_err(&e).expect("not Incomplete");
            describe_parse_error(query, &failure, &ctx.prefixes)
        }
    }
}

#[test]
fn test_message_names_the_undeclared_prefix() {
    let msg = message_for(
        "SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }",
        &mut ctx(),
    );
    assert!(
        msg.contains("totallyundeclaredprefix"),
        "message must name the prefix, got: {msg}"
    );
    assert!(
        !msg.contains("Verify"),
        "message must not just be the raw nom ErrorKind, got: {msg}"
    );
}

#[test]
fn test_message_lists_declared_prefixes_when_present() {
    let msg = message_for(
        "SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }",
        &mut ctx_with(&[("ex", "http://example.org/")]),
    );
    assert!(
        msg.contains("ex"),
        "message should list the declared prefix 'ex', got: {msg}"
    );
}

#[test]
fn test_message_notes_no_declared_prefixes_when_absent() {
    let msg = message_for(
        "SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }",
        &mut ctx(),
    );
    assert!(
        msg.to_ascii_lowercase().contains("no prefixes"),
        "message should say no prefixes are declared, got: {msg}"
    );
}

/// Covers `describe_parse_error`'s line/column computation (and, per issue
/// #747's UTF-8 safety concern, confirms neither it nor
/// `undeclared_prefix_at` panics) when the query text contains multi-byte
/// UTF-8 characters *before* the offending prefix -- the byte offset into
/// the source must still land on a valid char boundary.
#[test]
fn test_message_handles_non_ascii_text_before_the_prefix_without_panicking() {
    let query = r#"
        SELECT * WHERE {
            ?s <http://example.org/label> "héllo wörld –日本語" .
            ?s totallyundeclaredprefix:foo ?o
        }
    "#;
    let msg = message_for(query, &mut ctx());
    assert!(
        msg.contains("totallyundeclaredprefix"),
        "message must still name the prefix after non-ASCII text, got: {msg}"
    );
    assert!(
        msg.contains("line 4"),
        "expected the error to be reported on line 4, got: {msg}"
    );
}

/// Same non-ASCII safety check for `DESCRIBE`'s resource list, which uses
/// a separate code path (`parse_describe_resources`) than the main BGP loop.
#[test]
fn test_describe_handles_non_ascii_resource_list_without_panicking() {
    let query = r#"DESCRIBE <http://example.org/日本語> totallyundeclaredprefix:foo"#;
    let msg = message_for(query, &mut ctx());
    assert!(
        msg.contains("totallyundeclaredprefix"),
        "message must still name the prefix, got: {msg}"
    );
}

/// Non-regression: a declared prefix must still parse with no error at all.
#[test]
fn test_declared_prefix_parses_cleanly() {
    let sparql = r#"
        PREFIX ex: <http://example.org/>
        SELECT * WHERE { ?s ex:foo ?o }
    "#;
    parse_query(sparql, &mut ctx())
        .unwrap_or_else(|e| panic!("parse failed for: {sparql}\nerror: {e:?}"));
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Minimal SPARQL 1.1 Update parser and executor.
//!
//! Supported operations: INSERT DATA, DELETE DATA, CLEAR, DROP, CREATE.
//! Each operation may appear in a `;`-separated sequence.
//!
//! Spec: <https://www.w3.org/TR/sparql11-update/>

use crate::persistence::{LogEntry, to_repr};
use dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource, ingress};
use std::collections::HashSet;

// ── AST ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum UpdateOp {
    InsertData { content: String },
    DeleteData { content: String },
    ClearDefault,
    ClearNamed,
    ClearAll,
    ClearGraph(String),
    DropDefault,
    DropNamed,
    DropAll,
    DropGraph(String),
    CreateGraph(String),
}

// ── Parser ────────────────────────────────────────────────────────────────────

fn skip_ws(s: &str) -> &str {
    s.trim_start()
}

/// Try to consume a case-insensitive keyword at the start of `s`.
/// Returns the remainder if successful, `None` otherwise.
/// Requires a word boundary after the keyword (whitespace, `{`, or end of string).
fn kw<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let s = skip_ws(s);
    let upper: String = s
        .chars()
        .take(word.len())
        .collect::<String>()
        .to_ascii_uppercase();
    if upper != word {
        return None;
    }
    let rest = &s[word.len()..];
    // Require word boundary
    match rest.chars().next() {
        None | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('{') | Some(';') => {
            Some(rest)
        }
        _ => None,
    }
}

/// Parse an IRI `<...>` at the start of `s`.
fn take_iri(s: &str) -> Option<(String, &str)> {
    let s = skip_ws(s);
    let s = s.strip_prefix('<')?;
    let end = s.find('>')?;
    Some((s[..end].to_string(), &s[end + 1..]))
}

/// Extract the content between matching `{` and `}`.
fn take_braced(s: &str) -> Option<(String, &str)> {
    let s = skip_ws(s);
    let s = s.strip_prefix('{')?;
    let mut depth = 1usize;
    let mut end = None;
    let mut in_string = false;
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        if in_string {
            if c == '"' && prev != '\\' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    let end = end?;
    Some((s[..end].to_string(), &s[end + 1..]))
}

fn parse_one(s: &str) -> Result<(UpdateOp, &str), String> {
    let s = skip_ws(s);
    if s.is_empty() {
        return Err("empty input".to_string());
    }

    // INSERT DATA { ... }
    if let Some(rest) = kw(s, "INSERT") {
        let rest = kw(rest, "DATA").ok_or("expected DATA after INSERT")?;
        let (content, rest) = take_braced(rest).ok_or("expected { } after INSERT DATA")?;
        return Ok((UpdateOp::InsertData { content }, rest));
    }

    // DELETE DATA { ... }
    if let Some(rest) = kw(s, "DELETE") {
        let rest = kw(rest, "DATA").ok_or("expected DATA after DELETE")?;
        let (content, rest) = take_braced(rest).ok_or("expected { } after DELETE DATA")?;
        return Ok((UpdateOp::DeleteData { content }, rest));
    }

    // CLEAR [SILENT] (DEFAULT | NAMED | ALL | GRAPH <iri>)
    if let Some(rest) = kw(s, "CLEAR") {
        let rest = kw(rest, "SILENT").unwrap_or(rest);
        if let Some(rest) = kw(rest, "DEFAULT") {
            return Ok((UpdateOp::ClearDefault, rest));
        }
        if let Some(rest) = kw(rest, "NAMED") {
            return Ok((UpdateOp::ClearNamed, rest));
        }
        if let Some(rest) = kw(rest, "ALL") {
            return Ok((UpdateOp::ClearAll, rest));
        }
        if let Some(rest) = kw(rest, "GRAPH") {
            let (iri, rest) = take_iri(rest).ok_or("expected IRI after CLEAR GRAPH")?;
            return Ok((UpdateOp::ClearGraph(iri), rest));
        }
        return Err("expected DEFAULT, NAMED, ALL, or GRAPH after CLEAR".to_string());
    }

    // DROP [SILENT] (DEFAULT | NAMED | ALL | GRAPH <iri>)
    if let Some(rest) = kw(s, "DROP") {
        let rest = kw(rest, "SILENT").unwrap_or(rest);
        if let Some(rest) = kw(rest, "DEFAULT") {
            return Ok((UpdateOp::DropDefault, rest));
        }
        if let Some(rest) = kw(rest, "NAMED") {
            return Ok((UpdateOp::DropNamed, rest));
        }
        if let Some(rest) = kw(rest, "ALL") {
            return Ok((UpdateOp::DropAll, rest));
        }
        if let Some(rest) = kw(rest, "GRAPH") {
            let (iri, rest) = take_iri(rest).ok_or("expected IRI after DROP GRAPH")?;
            return Ok((UpdateOp::DropGraph(iri), rest));
        }
        return Err("expected DEFAULT, NAMED, ALL, or GRAPH after DROP".to_string());
    }

    // CREATE [SILENT] GRAPH <iri>
    if let Some(rest) = kw(s, "CREATE") {
        let rest = kw(rest, "SILENT").unwrap_or(rest);
        let rest = kw(rest, "GRAPH").ok_or("expected GRAPH after CREATE")?;
        let (iri, rest) = take_iri(rest).ok_or("expected IRI after CREATE GRAPH")?;
        return Ok((UpdateOp::CreateGraph(iri), rest));
    }

    Err(format!(
        "unrecognised update operation at: {}",
        &s[..s.len().min(40)]
    ))
}

pub fn parse_update(input: &str) -> Result<Vec<UpdateOp>, String> {
    let mut ops = Vec::new();
    let mut rest = input;
    loop {
        rest = skip_ws(rest);
        if rest.is_empty() {
            break;
        }
        let (op, tail) = parse_one(rest)?;
        ops.push(op);
        rest = skip_ws(tail);
        if let Some(tail) = rest.strip_prefix(';') {
            rest = tail;
        } else if rest.is_empty() {
            break;
        } else {
            return Err(format!(
                "expected ';' between operations, found: {}",
                &rest[..rest.len().min(40)]
            ));
        }
    }
    Ok(ops)
}

// ── Executor ──────────────────────────────────────────────────────────────────

// ── Prepared operations ───────────────────────────────────────────────────────

/// An `UpdateOp` with its Turtle content already parsed (for Insert/Delete).
///
/// Produced by `prepare_update`; consumed by `apply_prepared_update`.
/// The Turtle parse happens exactly once and the result is shared between
/// log-entry generation and in-memory application.
pub enum PreparedOp {
    InsertData(Datastore),
    DeleteData(Datastore),
    ClearDefault,
    ClearNamed,
    ClearAll,
    ClearGraph(String),
    DropDefault,
    DropNamed,
    DropAll,
    DropGraph(String),
    CreateGraph(String),
}

/// Parse `ops`, build WAL entries, and return prepared ops ready for apply.
///
/// This is the first half of the update path.  Call it while holding the store
/// read lock so that ClearNamed/ClearAll entries enumerate the correct graphs.
/// Then write the returned `LogEntry` values to the changelog, and finally call
/// `apply_prepared_update` to mutate the in-memory store.
pub fn prepare_update(
    store: &Datastore,
    ops: Vec<UpdateOp>,
) -> Result<(Vec<PreparedOp>, Vec<LogEntry>), String> {
    let mut prepared = Vec::with_capacity(ops.len());
    let mut entries = Vec::new();

    for op in ops {
        match op {
            UpdateOp::InsertData { content } => {
                let tmp = parse_turtle_content(&content)?;
                for q in tmp
                    .named_graphs
                    .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
                    .collect::<Vec<_>>()
                {
                    entries.push(LogEntry::InsertQuad {
                        graph: None,
                        s: to_repr(tmp.resources.get_graph_element(q.subject)),
                        p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                        o: to_repr(tmp.resources.get_graph_element(q.obj)),
                    });
                }
                prepared.push(PreparedOp::InsertData(tmp));
            }
            UpdateOp::DeleteData { content } => {
                let tmp = parse_turtle_content(&content)?;
                for q in tmp
                    .named_graphs
                    .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
                    .collect::<Vec<_>>()
                {
                    entries.push(LogEntry::DeleteQuad {
                        graph: None,
                        s: to_repr(tmp.resources.get_graph_element(q.subject)),
                        p: to_repr(tmp.resources.get_graph_element(q.predicate)),
                        o: to_repr(tmp.resources.get_graph_element(q.obj)),
                    });
                }
                prepared.push(PreparedOp::DeleteData(tmp));
            }
            UpdateOp::ClearDefault => {
                entries.push(LogEntry::ClearGraph { graph: None });
                prepared.push(PreparedOp::ClearDefault);
            }
            UpdateOp::DropDefault => {
                entries.push(LogEntry::ClearGraph { graph: None });
                prepared.push(PreparedOp::DropDefault);
            }
            UpdateOp::ClearGraph(ref iri) => {
                entries.push(LogEntry::ClearGraph {
                    graph: Some(iri.clone()),
                });
                prepared.push(PreparedOp::ClearGraph(iri.clone()));
            }
            UpdateOp::DropGraph(ref iri) => {
                entries.push(LogEntry::ClearGraph {
                    graph: Some(iri.clone()),
                });
                prepared.push(PreparedOp::DropGraph(iri.clone()));
            }
            UpdateOp::ClearNamed => {
                collect_named_graph_entries(store, &mut entries);
                prepared.push(PreparedOp::ClearNamed);
            }
            UpdateOp::DropNamed => {
                collect_named_graph_entries(store, &mut entries);
                prepared.push(PreparedOp::DropNamed);
            }
            UpdateOp::ClearAll => {
                entries.push(LogEntry::ClearGraph { graph: None });
                collect_named_graph_entries(store, &mut entries);
                prepared.push(PreparedOp::ClearAll);
            }
            UpdateOp::DropAll => {
                entries.push(LogEntry::ClearGraph { graph: None });
                collect_named_graph_entries(store, &mut entries);
                prepared.push(PreparedOp::DropAll);
            }
            UpdateOp::CreateGraph(iri) => {
                prepared.push(PreparedOp::CreateGraph(iri));
                // No quads added; nothing to log.
            }
        }
    }

    Ok((prepared, entries))
}

fn collect_named_graph_entries(store: &Datastore, entries: &mut Vec<LogEntry>) {
    let ids: Vec<_> = store
        .named_graphs
        .triple_id_index
        .keys()
        .copied()
        .filter(|&id| id != DEFAULT_GRAPH_ELEMENT_ID)
        .collect();
    for id in ids {
        if let Some(iri_ref) = store.resources.get_named_resource(id) {
            entries.push(LogEntry::ClearGraph {
                graph: Some(iri_ref.0.clone()),
            });
        }
    }
}

/// Apply pre-parsed ops to the store.  No Turtle re-parsing.
pub fn apply_prepared_update(store: &mut Datastore, ops: Vec<PreparedOp>) -> Result<(), String> {
    for op in ops {
        match op {
            PreparedOp::InsertData(tmp) => apply_insert(store, tmp),
            PreparedOp::DeleteData(tmp) => apply_delete(store, tmp),
            PreparedOp::ClearDefault | PreparedOp::DropDefault => {
                store.remove_graph(DEFAULT_GRAPH_ELEMENT_ID);
            }
            PreparedOp::ClearAll | PreparedOp::DropAll => {
                let ids: Vec<_> = store.named_graphs.triple_id_index.keys().copied().collect();
                for id in ids {
                    store.remove_graph(id);
                }
            }
            PreparedOp::ClearNamed | PreparedOp::DropNamed => {
                let ids: Vec<_> = store
                    .named_graphs
                    .triple_id_index
                    .keys()
                    .copied()
                    .filter(|&id| id != DEFAULT_GRAPH_ELEMENT_ID)
                    .collect();
                for id in ids {
                    store.remove_graph(id);
                }
            }
            PreparedOp::ClearGraph(iri) | PreparedOp::DropGraph(iri) => {
                if let Some(id) = store.lookup_named_graph_id(&iri) {
                    store.remove_graph(id);
                }
            }
            PreparedOp::CreateGraph(iri) => {
                let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri)));
                store.resources.add_resource(elem);
            }
        }
    }
    Ok(())
}

/// Convenience wrapper: parse, discard log entries, apply.
/// Use only when persistence is not configured.
pub fn execute_update(store: &mut Datastore, ops: Vec<UpdateOp>) -> Result<(), String> {
    let (prepared, _) = prepare_update(store, ops)?;
    apply_prepared_update(store, prepared)
}

fn ensure_trailing_dot(content: &str) -> String {
    let t = content.trim_end();
    if t.ends_with('.') {
        content.to_string()
    } else {
        format!("{t} .")
    }
}

fn parse_turtle_content(content: &str) -> Result<Datastore, String> {
    let mut tmp = Datastore::new(64);
    let body = ensure_trailing_dot(content);
    turtle::parse_turtle(&mut tmp, body.as_bytes())
        .map(|_| tmp)
        .map_err(|e| format!("parse error: {e}"))
}

fn apply_insert(store: &mut Datastore, tmp: Datastore) {
    let quads: Vec<_> = tmp
        .named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .collect();
    for q in quads {
        let s = store
            .resources
            .add_resource(tmp.resources.get_graph_element(q.subject).clone());
        let p = store
            .resources
            .add_resource(tmp.resources.get_graph_element(q.predicate).clone());
        let o = store
            .resources
            .add_resource(tmp.resources.get_graph_element(q.obj).clone());
        store.named_graphs.add_quad(ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s,
            predicate: p,
            obj: o,
        });
    }
}

fn apply_delete(store: &mut Datastore, tmp: Datastore) {
    // Build the set of quads to remove using IDs from the MAIN store.
    // add_resource de-duplicates, so existing elements return their stored ID;
    // new elements (not in main store) get a fresh ID that matches no quad.
    let to_remove: HashSet<ingress::Quad> = tmp
        .named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .filter_map(|q| {
            let s = store
                .resources
                .add_resource(tmp.resources.get_graph_element(q.subject).clone());
            let p = store
                .resources
                .add_resource(tmp.resources.get_graph_element(q.predicate).clone());
            let o = store
                .resources
                .add_resource(tmp.resources.get_graph_element(q.obj).clone());
            let quad = ingress::Quad {
                triple_id: DEFAULT_GRAPH_ELEMENT_ID,
                subject: s,
                predicate: p,
                obj: o,
            };
            store.named_graphs.contains(&quad).then_some(quad)
        })
        .collect();

    for quad in to_remove {
        store.named_graphs.remove_quad(quad);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_insert_data() {
        let ops =
            parse_update(r#"INSERT DATA { <http://example.org/s> <http://example.org/p> "o" . }"#)
                .unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], UpdateOp::InsertData { .. }));
    }

    #[test]
    fn parse_clear_default() {
        let ops = parse_update("CLEAR DEFAULT").unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], UpdateOp::ClearDefault));
    }

    #[test]
    fn parse_drop_graph() {
        let ops = parse_update("DROP GRAPH <http://example.org/g>").unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], UpdateOp::DropGraph(_)));
    }

    #[test]
    fn parse_multi_op() {
        let ops = parse_update(r#"INSERT DATA { <s> <p> <o> . } ; CLEAR DEFAULT"#).unwrap();
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn parse_malformed_returns_err() {
        assert!(parse_update("MANGLE DATA { }").is_err());
    }

    /// Regression: log entries and applied quads must describe the same triples.
    ///
    /// Previously `ops_to_log_entries` and `execute_update` each parsed the same
    /// Turtle content independently. `prepare_update` now parses once and derives
    /// both the WAL entries and the in-memory apply from the single result.
    #[test]
    fn insert_log_entries_match_applied_quads() {
        let content = r#"<http://example.org/s> <http://example.org/p> <http://example.org/o> ."#;
        let ops = parse_update(&format!("INSERT DATA {{ {content} }}")).unwrap();

        let mut store = Datastore::new(64);
        let (prepared, log_entries) = prepare_update(&store, ops).unwrap();

        assert_eq!(log_entries.len(), 1, "one triple → one log entry");

        apply_prepared_update(&mut store, prepared).unwrap();

        // The single quad in the store must match the single log entry.
        let quads: Vec<_> = store
            .named_graphs
            .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
            .collect();
        assert_eq!(quads.len(), 1, "one quad in store");

        if let LogEntry::InsertQuad { s, p, o, .. } = &log_entries[0] {
            let q = &quads[0];
            let actual_s = to_repr(store.resources.get_graph_element(q.subject));
            let actual_p = to_repr(store.resources.get_graph_element(q.predicate));
            let actual_o = to_repr(store.resources.get_graph_element(q.obj));
            assert_eq!(s, &actual_s, "subject must match");
            assert_eq!(p, &actual_p, "predicate must match");
            assert_eq!(o, &actual_o, "object must match");
        } else {
            panic!("expected InsertQuad log entry, got {:?}", log_entries[0]);
        }
    }

    #[test]
    fn delete_log_entries_match_removed_quads() {
        let content = r#"<http://example.org/s> <http://example.org/p> <http://example.org/o> ."#;

        // Seed the store with the triple.
        let mut store = Datastore::new(64);
        let insert_ops = parse_update(&format!("INSERT DATA {{ {content} }}")).unwrap();
        let (prepared, _) = prepare_update(&store, insert_ops).unwrap();
        apply_prepared_update(&mut store, prepared).unwrap();
        assert_eq!(
            store
                .named_graphs
                .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
                .count(),
            1
        );

        // Now delete it.
        let delete_ops = parse_update(&format!("DELETE DATA {{ {content} }}")).unwrap();
        let (prepared, log_entries) = prepare_update(&store, delete_ops).unwrap();
        assert_eq!(log_entries.len(), 1, "one log entry for the deletion");
        assert!(
            matches!(log_entries[0], LogEntry::DeleteQuad { .. }),
            "log entry should be DeleteQuad"
        );

        apply_prepared_update(&mut store, prepared).unwrap();
        assert_eq!(
            store
                .named_graphs
                .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
                .count(),
            0,
            "store should be empty after delete"
        );
    }
}

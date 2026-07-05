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
use datalog::IncrementalReasoner;
use sparql_parser::ast::{Query, QueryComponent, Term, TriplePattern};
use sparql_parser::{NetworkPolicy, ParserContext, QueryResult, SolutionRow, execute, parse_query};
use std::collections::HashMap;
use std::collections::HashSet;

// ── AST ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum UpdateOp {
    InsertData {
        content: String,
    },
    DeleteData {
        content: String,
    },
    ClearDefault,
    ClearNamed,
    ClearAll,
    ClearGraph(String),
    DropDefault,
    DropNamed,
    DropAll,
    DropGraph(String),
    CreateGraph(String),
    /// `LOAD [SILENT] <url> [INTO GRAPH <graph>]`.
    ///
    /// Parsed for syntax conformance; execution (actual HTTP fetch) is not
    /// implemented.
    LoadGraph {
        source: String,
        into: Option<String>,
        /// Whether `LOAD SILENT` was specified (errors are suppressed when true).
        silent: bool,
    },
    /// `INSERT { template } WHERE { pattern }`.
    ///
    /// Not yet logged for persistence.
    InsertWhere {
        template: String,
        pattern: String,
    },
    /// `DELETE { template } WHERE { pattern }`.
    ///
    /// Not yet logged for persistence.
    DeleteWhere {
        template: String,
        pattern: String,
    },
    /// `DELETE { delete_template } INSERT { insert_template } WHERE { pattern }`.
    ///
    /// Not yet logged for persistence.
    DeleteInsertWhere {
        delete_template: String,
        insert_template: String,
        pattern: String,
    },
}

// ── Parser ────────────────────────────────────────────────────────────────────

fn skip_ws(s: &str) -> &str {
    let mut s = s.trim_start();
    // Also skip `# comment` lines (SPARQL Update comment syntax)
    while let Some(rest) = s.strip_prefix('#') {
        let nl = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        s = rest[nl..].trim_start();
    }
    s
}

/// Skip SPARQL Update prologue declarations (BASE and PREFIX) that may appear
/// before the first operation or between `;`-separated operations.
fn skip_prologue(s: &str) -> &str {
    let mut rest = skip_ws(s);
    loop {
        if let Some(r) = kw(rest, "PREFIX") {
            // Skip: PREFIX prefix: <iri>
            let r = skip_ws(r);
            if let Some(gt) = r.find('>') {
                rest = skip_ws(&r[gt + 1..]);
            } else {
                break;
            }
        } else if let Some(r) = kw(rest, "BASE") {
            // Skip: BASE <iri>
            let r = skip_ws(r);
            if let Some(gt) = r.find('>') {
                rest = skip_ws(&r[gt + 1..]);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    rest
}

/// Parse a graph reference: either `<iri>` or a prefixed name (`prefix:local`).
///
/// Returns `(graph_ref_string, remainder)`.
fn take_iri_or_prefixed(s: &str) -> Option<(String, &str)> {
    let s = skip_ws(s);
    if s.starts_with('<') {
        return take_iri(s);
    }
    // Prefixed name: scan to first whitespace or structural char
    let end = s
        .find(|c: char| c.is_whitespace() || matches!(c, '{' | '}' | ';' | '(' | ')' | ',' | '.'))
        .unwrap_or(s.len());
    if end > 0 && s[..end].contains(':') {
        Some((s[..end].to_string(), &s[end..]))
    } else {
        None
    }
}

/// Returns `true` if `s` contains a SPARQL variable marker (`?` or `$` followed
/// by a letter or `_`) outside of quoted string literals.
///
/// Used to reject variables inside INSERT DATA / DELETE DATA blocks.
fn content_has_variable(s: &str) -> bool {
    let mut in_str = false;
    let mut str_ch = '"';
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if in_str {
            if c == str_ch && (i == 0 || chars[i - 1] != '\\') {
                in_str = false;
            }
        } else {
            match c {
                '#' => {
                    // Skip rest of line
                    while i < n && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                '"' | '\'' => {
                    in_str = true;
                    str_ch = c;
                }
                '?' | '$' if i + 1 < n && (chars[i + 1].is_alphabetic() || chars[i + 1] == '_') => {
                    return true;
                }
                _ => {}
            }
        }
        i += 1;
    }
    false
}

/// Returns `true` if `s` contains a blank node label (`_:name`) outside of
/// quoted string literals.
///
/// Used to detect blank nodes in DELETE DATA, DELETE WHERE, and DELETE templates.
fn content_has_bnode_label(s: &str) -> bool {
    let mut in_str = false;
    let mut str_ch = '"';
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if in_str {
            if c == str_ch && (i == 0 || chars[i - 1] != '\\') {
                in_str = false;
            }
        } else {
            match c {
                '#' => {
                    while i < n && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                '"' | '\'' => {
                    in_str = true;
                    str_ch = c;
                }
                '_' if i + 1 < n && chars[i + 1] == ':' => return true,
                _ => {}
            }
        }
        i += 1;
    }
    false
}

/// Returns `true` if `s` contains an anonymous blank node (`[`) outside of
/// quoted string literals.
///
/// Used to reject anonymous blank nodes in DELETE templates.
fn content_has_anon_bnode(s: &str) -> bool {
    let mut in_str = false;
    let mut str_ch = '"';
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if in_str {
            if c == str_ch && (i == 0 || chars[i - 1] != '\\') {
                in_str = false;
            }
        } else {
            match c {
                '#' => {
                    while i < n && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                '"' | '\'' => {
                    in_str = true;
                    str_ch = c;
                }
                '[' => return true,
                _ => {}
            }
        }
        i += 1;
    }
    false
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

    // LOAD [SILENT] <url> [INTO GRAPH <graph>]
    if let Some(rest) = kw(s, "LOAD") {
        let (rest, silent) = if let Some(r) = kw(rest, "SILENT") {
            (r, true)
        } else {
            (rest, false)
        };
        let (source, rest) = take_iri(rest).ok_or("expected IRI after LOAD")?;
        let (into, rest) = if let Some(into_rest) = kw(rest, "INTO") {
            let into_rest = kw(into_rest, "GRAPH").ok_or("expected GRAPH after INTO")?;
            let (iri, rest) = take_iri(into_rest).ok_or("expected IRI after INTO GRAPH")?;
            (Some(iri), rest)
        } else {
            (None, rest)
        };
        return Ok((
            UpdateOp::LoadGraph {
                source,
                into,
                silent,
            },
            rest,
        ));
    }

    // INSERT DATA { ... }  |  INSERT { template } WHERE { pattern }
    if let Some(rest) = kw(s, "INSERT") {
        if let Some(data_rest) = kw(rest, "DATA") {
            let (content, rest) = take_braced(data_rest).ok_or("expected { } after INSERT DATA")?;
            if content_has_variable(&content) {
                return Err("variables are not allowed in INSERT DATA".to_string());
            }
            return Ok((UpdateOp::InsertData { content }, rest));
        }
        let (template, rest) = take_braced(rest).ok_or("expected { } after INSERT")?;
        let rest = kw(rest, "WHERE").ok_or("expected WHERE after INSERT { ... }")?;
        let (pattern, rest) = take_braced(rest).ok_or("expected { } after WHERE")?;
        return Ok((UpdateOp::InsertWhere { template, pattern }, rest));
    }

    // DELETE DATA { ... }  |  DELETE WHERE { pattern }  (short form)
    //   |  DELETE { template } WHERE { pattern }
    //   |  DELETE { d } INSERT { i } WHERE { pattern }
    if let Some(rest) = kw(s, "DELETE") {
        if let Some(data_rest) = kw(rest, "DATA") {
            let (content, rest) = take_braced(data_rest).ok_or("expected { } after DELETE DATA")?;
            if content_has_variable(&content) {
                return Err("variables are not allowed in DELETE DATA".to_string());
            }
            if content_has_bnode_label(&content) {
                return Err("blank nodes are not allowed in DELETE DATA".to_string());
            }
            return Ok((UpdateOp::DeleteData { content }, rest));
        }
        // DELETE WHERE { ... } — short form with no explicit template
        if let Some(where_rest) = kw(rest, "WHERE") {
            let (pattern, rest) =
                take_braced(where_rest).ok_or("expected { } after DELETE WHERE")?;
            if content_has_bnode_label(&pattern) {
                return Err("blank nodes are not allowed in DELETE WHERE pattern".to_string());
            }
            return Ok((
                UpdateOp::DeleteWhere {
                    template: pattern.clone(),
                    pattern,
                },
                rest,
            ));
        }
        let (delete_template, rest) = take_braced(rest).ok_or("expected { } after DELETE")?;
        if content_has_bnode_label(&delete_template) || content_has_anon_bnode(&delete_template) {
            return Err("blank nodes are not allowed in DELETE template".to_string());
        }
        if let Some(insert_rest) = kw(rest, "INSERT") {
            let (insert_template, rest) =
                take_braced(insert_rest).ok_or("expected { } after INSERT")?;
            let rest = kw(rest, "WHERE").ok_or("expected WHERE after INSERT { ... }")?;
            let (pattern, rest) = take_braced(rest).ok_or("expected { } after WHERE")?;
            return Ok((
                UpdateOp::DeleteInsertWhere {
                    delete_template,
                    insert_template,
                    pattern,
                },
                rest,
            ));
        }
        let rest = kw(rest, "WHERE").ok_or("expected WHERE after DELETE { ... }")?;
        let (pattern, rest) = take_braced(rest).ok_or("expected { } after WHERE")?;
        return Ok((
            UpdateOp::DeleteWhere {
                template: delete_template,
                pattern,
            },
            rest,
        ));
    }

    // WITH <graph> (DELETE { ... })? (INSERT { ... })? (USING ...)* WHERE { ... }
    //
    // The WITH clause specifies the default graph context for the update.
    // The graph IRI is parsed but currently ignored during execution.
    if let Some(rest) = kw(s, "WITH") {
        let (_graph_iri, rest) = take_iri_or_prefixed(rest).ok_or("expected IRI after WITH")?;
        // Optional DELETE clause
        let (delete_template, rest) = if let Some(r) = kw(rest, "DELETE") {
            let (t, r) = take_braced(r).ok_or("expected { } after DELETE")?;
            if content_has_bnode_label(&t) || content_has_anon_bnode(&t) {
                return Err("blank nodes are not allowed in DELETE template".to_string());
            }
            (Some(t), r)
        } else {
            (None, rest)
        };
        // Optional INSERT clause
        let (insert_template, rest) = if let Some(r) = kw(rest, "INSERT") {
            let (t, r) = take_braced(r).ok_or("expected { } after INSERT")?;
            (Some(t), r)
        } else {
            (None, rest)
        };
        // Zero or more USING clauses
        let mut rest = rest;
        while let Some(r) = kw(rest, "USING") {
            let r = kw(r, "NAMED").unwrap_or(r);
            let (_, r) = take_iri_or_prefixed(r).ok_or("expected IRI after USING [NAMED]")?;
            rest = r;
        }
        let rest = kw(rest, "WHERE").ok_or("expected WHERE in WITH...DELETE/INSERT")?;
        let (pattern, rest) = take_braced(rest).ok_or("expected { } after WHERE")?;
        let op = match (delete_template, insert_template) {
            (Some(d), Some(i)) => UpdateOp::DeleteInsertWhere {
                delete_template: d,
                insert_template: i,
                pattern,
            },
            (Some(d), None) => UpdateOp::DeleteWhere {
                template: d,
                pattern,
            },
            (None, Some(i)) => UpdateOp::InsertWhere {
                template: i,
                pattern,
            },
            (None, None) => {
                return Err("expected DELETE or INSERT clause after WITH <iri>".to_string());
            }
        };
        return Ok((op, rest));
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
    // Skip optional prologue (PREFIX / BASE declarations) before first operation.
    let mut rest = skip_prologue(input);
    loop {
        if rest.is_empty() {
            break;
        }
        let (op, tail) = parse_one(rest)?;
        ops.push(op);
        rest = skip_ws(tail);
        if let Some(tail) = rest.strip_prefix(';') {
            // After `;`, skip any prologue before the next operation (or trailing `;`).
            rest = skip_prologue(tail);
            if rest.is_empty() {
                break;
            }
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
    /// WHERE-form update, executed against the live store at apply time.
    ///
    /// Unlike the other variants, the WHERE clause is evaluated lazily in
    /// `apply_prepared_update` rather than at `prepare_update` time, because
    /// solutions depend on the state of the store *after* any preceding ops
    /// in the same request have already been applied. These updates are not
    /// yet written to the changelog.
    PatternUpdate {
        delete_template: Option<String>,
        insert_template: Option<String>,
        pattern: String,
    },
    /// `LOAD [SILENT] <url> [INTO GRAPH <graph>]` — remote fetch required.
    ///
    /// Whether it's an error, a no-op, or a live HTTP fetch depends on the
    /// [`NetworkPolicy`] passed to `apply_prepared_update`.
    ///
    /// Related: [#119](https://github.com/daghovland/rdf-datalog/issues/119)
    LoadGraph {
        source: String,
        into: Option<String>,
        silent: bool,
    },
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
            UpdateOp::LoadGraph {
                source,
                into,
                silent,
            } => {
                // Defer network-policy enforcement to apply_prepared_update so the
                // policy is evaluated at execution time rather than parse time.
                prepared.push(PreparedOp::LoadGraph {
                    source,
                    into,
                    silent,
                });
            }
            UpdateOp::InsertWhere { template, pattern } => {
                // Not yet logged for persistence.
                prepared.push(PreparedOp::PatternUpdate {
                    delete_template: None,
                    insert_template: Some(template),
                    pattern,
                });
            }
            UpdateOp::DeleteWhere { template, pattern } => {
                // Not yet logged for persistence.
                prepared.push(PreparedOp::PatternUpdate {
                    delete_template: Some(template),
                    insert_template: None,
                    pattern,
                });
            }
            UpdateOp::DeleteInsertWhere {
                delete_template,
                insert_template,
                pattern,
            } => {
                // Not yet logged for persistence.
                prepared.push(PreparedOp::PatternUpdate {
                    delete_template: Some(delete_template),
                    insert_template: Some(insert_template),
                    pattern,
                });
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

/// Translate quads from a temporary datastore's default graph into the IDs of
/// the main `store`, interning any new resources.  The quads are NOT inserted
/// into `store` — the caller decides what to do with them.
fn translate_to_main_ids(store: &mut Datastore, tmp: &Datastore) -> Vec<ingress::Quad> {
    tmp.named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .map(|q| {
            let s = store
                .resources
                .add_resource(tmp.resources.get_graph_element(q.subject).clone());
            let p = store
                .resources
                .add_resource(tmp.resources.get_graph_element(q.predicate).clone());
            let o = store
                .resources
                .add_resource(tmp.resources.get_graph_element(q.obj).clone());
            ingress::Quad {
                triple_id: DEFAULT_GRAPH_ELEMENT_ID,
                subject: s,
                predicate: p,
                obj: o,
            }
        })
        .collect()
}

/// Apply pre-parsed ops to the store.  No Turtle re-parsing.
///
/// Uses a **collect-then-apply** strategy for atomicity: `InsertData`,
/// `DeleteData`, and `PatternUpdate` mutations are buffered into
/// `pending_inserts` / `pending_deletes` and never touch the live store
/// until every operation in the sequence has been validated.  If any
/// operation returns an error (e.g. a rejected `LOAD`), the function returns
/// immediately and the live store is unmodified — the earlier inserts are
/// effectively rolled back.
///
/// `PatternUpdate` WHERE clauses evaluate against a clone of the live store
/// with the pending delta applied so they see inserts/deletes from earlier
/// ops in the same request (SPARQL 1.1 Update §3.1.3).
///
/// `CLEAR`, `DROP`, and `CREATE` are applied eagerly because they cannot
/// fail, so they never compromise the atomicity guarantee.
///
/// When `reasoner` is `Some`, the reasoner is called **once** at the end
/// with the net delta to prevent transient wrong inferences.
///
/// Other operation types (CLEAR, DROP, etc.) bypass the reasoner for now —
/// a full re-materialisation would be needed for those, which is tracked in
/// [#110](https://github.com/daghovland/rdf-datalog/issues/110).
///
/// Fixes: [#114](https://github.com/daghovland/rdf-datalog/issues/114),
///        [#126](https://github.com/daghovland/rdf-datalog/issues/126)
///
/// `network` controls how `LOAD` operations are handled.
pub fn apply_prepared_update(
    store: &mut Datastore,
    ops: Vec<PreparedOp>,
    reasoner: Option<&mut IncrementalReasoner>,
    network: NetworkPolicy,
) -> Result<(), String> {
    // Pending delta: buffered across all INSERT DATA / DELETE DATA /
    // PatternUpdate ops.  Nothing touches the live store for these
    // operations until all ops succeed, ensuring that a later failure
    // (e.g. a rejected LOAD) leaves the live store unmodified.
    let mut pending_inserts: Vec<ingress::Quad> = Vec::new();
    let mut pending_deletes: Vec<ingress::Quad> = Vec::new();

    for op in ops {
        match op {
            PreparedOp::InsertData(tmp) => {
                // Intern resources into live store (so IDs are valid there)
                // but buffer the quads rather than writing to named_graphs.
                let quads = translate_to_main_ids(store, &tmp);
                pending_inserts.extend(quads);
            }
            PreparedOp::DeleteData(tmp) => {
                let quads = translate_to_main_ids(store, &tmp);
                // Keep only quads that exist in the live store OR are already
                // in pending_inserts so that an insert-then-delete in the same
                // request works correctly.
                let existing: Vec<_> = quads
                    .into_iter()
                    .filter(|q| store.named_graphs.contains(q) || pending_inserts.contains(q))
                    .collect();
                pending_deletes.extend(existing);
            }
            PreparedOp::ClearDefault | PreparedOp::DropDefault => {
                // CLEAR/DROP cannot fail — apply eagerly.
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
            PreparedOp::PatternUpdate {
                delete_template,
                insert_template,
                pattern,
            } => {
                // Build a view of the store with the pending delta applied so
                // that the WHERE clause sees inserts/deletes from earlier ops
                // in this request (SPARQL 1.1 Update §3.1.3).
                let mut view = store.clone();
                for &q in &pending_inserts {
                    view.named_graphs.add_quad(q);
                }
                for &q in &pending_deletes {
                    view.named_graphs.remove_quad(q);
                }

                // Evaluate WHERE against the view to get solution bindings.
                let rows = eval_where_pattern(&view, &pattern)?;

                // Materialise DELETE and INSERT templates from the bindings.
                let to_delete = match delete_template.as_deref() {
                    Some(template) => {
                        let triples = parse_template(template)?;
                        materialise_template(&triples, &rows)
                    }
                    None => Vec::new(),
                };
                let to_insert = match insert_template.as_deref() {
                    Some(template) => {
                        let triples = parse_template(template)?;
                        materialise_template(&triples, &rows)
                    }
                    None => Vec::new(),
                };

                // Intern resources into the live store (not the view) so that
                // the returned IDs are valid in the live store, then buffer.
                for (s, p, o) in to_delete {
                    let quad = ground_quad(store, s, p, o);
                    pending_deletes.push(quad);
                }
                for (s, p, o) in to_insert {
                    let quad = ground_quad(store, s, p, o);
                    pending_inserts.push(quad);
                }
            }
            PreparedOp::LoadGraph { source, silent, .. } => match network {
                NetworkPolicy::Deny => {
                    if !silent {
                        // Since inserts/deletes are buffered, returning Err here
                        // leaves the live store unmodified.
                        return Err(format!(
                            "LOAD <{source}> was rejected: remote network access is disabled. \
                             Start the server with --network=allow to enable remote loading. \
                             See https://github.com/daghovland/rdf-datalog/issues/119"
                        ));
                    }
                    // LOAD SILENT: fail silently per the SPARQL Update spec.
                }
                NetworkPolicy::Ignore => {
                    // Silent no-op regardless of SILENT flag.
                }
                NetworkPolicy::Allow => {
                    return Err(format!(
                        "LOAD <{source}>: NetworkPolicy::Allow is not yet implemented. \
                         Track progress at https://github.com/daghovland/rdf-datalog/issues/119"
                    ));
                }
            },
        }
    }

    // Apply the pending delta atomically to the live store using set semantics:
    // a quad that is both inserted and deleted in the same request is a net
    // no-op.  Deletions are applied before insertions so that the reasoner's
    // re-derivation step in apply_deletions sees the correct base state.
    let delete_set: HashSet<ingress::Quad> = pending_deletes.iter().copied().collect();

    // Only delete quads that actually exist in the live store.
    let net_deletes: Vec<_> = delete_set
        .iter()
        .copied()
        .filter(|q| store.named_graphs.contains(q))
        .collect();
    for &q in &net_deletes {
        store.remove_quad(q);
    }

    // Only insert quads not cancelled by a matching delete.
    let net_inserts: Vec<_> = pending_inserts
        .iter()
        .copied()
        .filter(|q| !delete_set.contains(q))
        .collect();
    for &q in &net_inserts {
        store.add_quad(q);
    }

    // Reason once over the net delta accumulated across the entire request.
    if let Some(r) = reasoner {
        if !net_deletes.is_empty() {
            r.apply_deletions(store, &net_deletes);
        }
        if !net_inserts.is_empty() {
            r.apply_insertions(store, &net_inserts);
        }
    }

    Ok(())
}

// ── WHERE-form pattern updates ────────────────────────────────────────────────
//
// `INSERT { ... } WHERE { ... }`, `DELETE { ... } WHERE { ... }`, and the
// combined `DELETE { ... } INSERT { ... } WHERE { ... }` form.
//
// These are evaluated by wrapping the WHERE clause text in a synthetic
// `SELECT * WHERE { ... }` query and reusing the `sparql_parser` query
// executor to obtain solution bindings, then materialising the DELETE/INSERT
// templates (themselves parsed as a bare BGP) once per solution row.
//
// Not yet logged to the changelog for persistence.

/// Parse `pattern` as the WHERE clause of a `SELECT * WHERE { pattern }`
/// query and execute it against `store`, returning the solution rows.
fn eval_where_pattern(store: &Datastore, pattern: &str) -> Result<Vec<SolutionRow>, String> {
    let query_text = format!("SELECT * WHERE {{ {pattern} }}");
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(&query_text, &mut ctx)
        .map_err(|e| format!("WHERE clause parse error: {e:?}"))?;
    match execute(&query, store, NetworkPolicy::Deny)
        .map_err(|e| format!("WHERE clause execution error: {e}"))?
    {
        QueryResult::Select(select_result) => Ok(select_result.rows),
        other => Err(format!(
            "WHERE clause did not evaluate to a solution sequence: {:?}",
            std::mem::discriminant(&other)
        )),
    }
}

/// Parse a DELETE/INSERT template as a bare Basic Graph Pattern and return
/// its triple patterns, by wrapping it the same way as a WHERE clause.
fn parse_template(template: &str) -> Result<Vec<TriplePattern>, String> {
    let query_text = format!("SELECT * WHERE {{ {template} }}");
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) =
        parse_query(&query_text, &mut ctx).map_err(|e| format!("template parse error: {e:?}"))?;
    let where_clause = match query {
        Query::Select { where_clause, .. } => where_clause,
        _ => return Err("template did not parse as a graph pattern".to_string()),
    };
    let mut patterns = Vec::new();
    for component in where_clause {
        match component {
            QueryComponent::BGP(triples) => patterns.extend(triples),
            other => {
                return Err(format!(
                    "unsupported construct in DELETE/INSERT template: {:?}",
                    std::mem::discriminant(&other)
                ));
            }
        }
    }
    Ok(patterns)
}

/// Resolve a template `Term` against a solution row, returning `None` if the
/// term is an unbound variable (in which case the ground triple is skipped).
fn resolve_term(term: &Term, row: &SolutionRow) -> Option<GraphElement> {
    match term {
        Term::Constant(elem) => Some(elem.clone()),
        Term::Variable(name) => row.get(name).cloned(),
    }
}

/// Materialise `triples` against every row in `rows`, producing ground
/// `(subject, predicate, object)` `GraphElement` triples. Rows that leave a
/// template variable unbound are skipped for that triple pattern.
fn materialise_template(
    triples: &[TriplePattern],
    rows: &[SolutionRow],
) -> Vec<(GraphElement, GraphElement, GraphElement)> {
    let mut out = Vec::new();
    for row in rows {
        for pattern in triples {
            let s = resolve_term(&pattern.subject, row);
            let p = resolve_term(&pattern.predicate, row);
            let o = resolve_term(&pattern.object, row);
            if let (Some(s), Some(p), Some(o)) = (s, p, o) {
                out.push((s, p, o));
            }
        }
    }
    out
}

fn ground_quad(
    store: &mut Datastore,
    s: GraphElement,
    p: GraphElement,
    o: GraphElement,
) -> ingress::Quad {
    ingress::Quad {
        triple_id: DEFAULT_GRAPH_ELEMENT_ID,
        subject: store.add_resource(s),
        predicate: store.add_resource(p),
        obj: store.add_resource(o),
    }
}

/// Convenience wrapper: parse, discard log entries, apply.
/// Use only when persistence is not configured and no incremental reasoner is active.
pub fn execute_update(store: &mut Datastore, ops: Vec<UpdateOp>) -> Result<(), String> {
    let (prepared, _) = prepare_update(store, ops)?;
    apply_prepared_update(store, prepared, None, NetworkPolicy::Deny)
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

        apply_prepared_update(&mut store, prepared, None, NetworkPolicy::Deny).unwrap();

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
        apply_prepared_update(&mut store, prepared, None, NetworkPolicy::Deny).unwrap();
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

        apply_prepared_update(&mut store, prepared, None, NetworkPolicy::Deny).unwrap();
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

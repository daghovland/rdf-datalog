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
use sparql_parser::ast::{Query, QueryComponent, Term, TriplePattern};
use sparql_parser::{ParserContext, QueryResult, SolutionRow, execute, parse_query};
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
    /// `INSERT { template } WHERE { pattern }`.
    ///
    /// Not yet logged for persistence — see
    /// <https://github.com/daghovland/rdf-datalog/issues/53>.
    InsertWhere {
        template: String,
        pattern: String,
    },
    /// `DELETE { template } WHERE { pattern }`.
    ///
    /// Not yet logged for persistence — see
    /// <https://github.com/daghovland/rdf-datalog/issues/53>.
    DeleteWhere {
        template: String,
        pattern: String,
    },
    /// `DELETE { delete_template } INSERT { insert_template } WHERE { pattern }`.
    ///
    /// Not yet logged for persistence — see
    /// <https://github.com/daghovland/rdf-datalog/issues/53>.
    DeleteInsertWhere {
        delete_template: String,
        insert_template: String,
        pattern: String,
    },
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

    // INSERT DATA { ... }  |  INSERT { template } WHERE { pattern }
    if let Some(rest) = kw(s, "INSERT") {
        if let Some(data_rest) = kw(rest, "DATA") {
            let (content, rest) = take_braced(data_rest).ok_or("expected { } after INSERT DATA")?;
            return Ok((UpdateOp::InsertData { content }, rest));
        }
        let (template, rest) = take_braced(rest).ok_or("expected { } after INSERT")?;
        let rest = kw(rest, "WHERE").ok_or("expected WHERE after INSERT { ... }")?;
        let (pattern, rest) = take_braced(rest).ok_or("expected { } after WHERE")?;
        return Ok((UpdateOp::InsertWhere { template, pattern }, rest));
    }

    // DELETE DATA { ... }  |  DELETE { template } WHERE { pattern }
    //   |  DELETE { d } INSERT { i } WHERE { pattern }
    if let Some(rest) = kw(s, "DELETE") {
        if let Some(data_rest) = kw(rest, "DATA") {
            let (content, rest) = take_braced(data_rest).ok_or("expected { } after DELETE DATA")?;
            return Ok((UpdateOp::DeleteData { content }, rest));
        }
        let (delete_template, rest) = take_braced(rest).ok_or("expected { } after DELETE")?;
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
    /// WHERE-form update, executed against the live store at apply time.
    ///
    /// Unlike the other variants, the WHERE clause is evaluated lazily in
    /// `apply_prepared_update` rather than at `prepare_update` time, because
    /// solutions depend on the state of the store *after* any preceding ops
    /// in the same request have already been applied. These updates are not
    /// yet written to the changelog — see
    /// <https://github.com/daghovland/rdf-datalog/issues/53>.
    PatternUpdate {
        delete_template: Option<String>,
        insert_template: Option<String>,
        pattern: String,
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
            UpdateOp::InsertWhere { template, pattern } => {
                // Not yet logged for persistence; see issue #53.
                prepared.push(PreparedOp::PatternUpdate {
                    delete_template: None,
                    insert_template: Some(template),
                    pattern,
                });
            }
            UpdateOp::DeleteWhere { template, pattern } => {
                // Not yet logged for persistence; see issue #53.
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
                // Not yet logged for persistence; see issue #53.
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
            PreparedOp::PatternUpdate {
                delete_template,
                insert_template,
                pattern,
            } => {
                apply_pattern_update(
                    store,
                    delete_template.as_deref(),
                    insert_template.as_deref(),
                    &pattern,
                )?;
            }
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
// Not yet logged to the changelog for persistence — see
// <https://github.com/daghovland/rdf-datalog/issues/53>.

/// Parse `pattern` as the WHERE clause of a `SELECT * WHERE { pattern }`
/// query and execute it against `store`, returning the solution rows.
fn eval_where_pattern(store: &Datastore, pattern: &str) -> Result<Vec<SolutionRow>, String> {
    let query_text = format!("SELECT * WHERE {{ {pattern} }}");
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(&query_text, &mut ctx)
        .map_err(|e| format!("WHERE clause parse error: {e:?}"))?;
    match execute(&query, store).map_err(|e| format!("WHERE clause execution error: {e}"))? {
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

fn apply_pattern_update(
    store: &mut Datastore,
    delete_template: Option<&str>,
    insert_template: Option<&str>,
    pattern: &str,
) -> Result<(), String> {
    let rows = eval_where_pattern(store, pattern)?;

    // Materialise DELETE first (against the pre-update store), matching the
    // SPARQL 1.1 Update semantics that DELETE and INSERT templates are both
    // evaluated against the bindings produced by the single WHERE solution
    // set, before any of the deletions/insertions are applied.
    let to_delete = match delete_template {
        Some(template) => {
            let triples = parse_template(template)?;
            materialise_template(&triples, &rows)
        }
        None => Vec::new(),
    };
    let to_insert = match insert_template {
        Some(template) => {
            let triples = parse_template(template)?;
            materialise_template(&triples, &rows)
        }
        None => Vec::new(),
    };

    for (s, p, o) in to_delete {
        let quad = ground_quad(store, s, p, o);
        if store.named_graphs.contains(&quad) {
            store.remove_quad(quad);
        }
    }
    for (s, p, o) in to_insert {
        let quad = ground_quad(store, s, p, o);
        store.add_quad(quad);
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
        store.add_quad(ingress::Quad {
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
        store.remove_quad(quad);
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

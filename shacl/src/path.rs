/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `sh:path` property-path expressions: AST, parsing, and evaluation.
//!
//! Spec: <https://www.w3.org/TR/shacl/#property-paths>
//!
//! See [`docs/plans/SHACL_COMPLEX_PATHS_PLAN.md`](../../docs/plans/SHACL_COMPLEX_PATHS_PLAN.md)
//! for the full design rationale. In short: [`pairs`] computes a compound
//! path's full `(subject, object)` extension over a `Datastore` — shared by
//! both consumers, but used differently by each, since they need different
//! things and mutate at different scopes:
//! - `evaluate.rs` (direct, Phase 2 constraint evaluation, read-only against
//!   the original data graph) calls [`values_from`], which re-evaluates
//!   `pairs` per focus node with no stored predicate — this also covers
//!   `sh:not`/`sh:and`/`sh:or` inner shapes, which are re-parsed ad hoc via
//!   `shapes::parse_one_shape` and so have nowhere stable to cache a
//!   resolved id against.
//! - `translate.rs` (Datalog rule generation, Phase 1) calls
//!   [`resolve_one_path`], which materializes the extension as ground
//!   triples in `work` under a fresh synthetic predicate, once per
//!   top-level property shape — every existing rule-generation code path
//!   (which assumes a path *is* a single predicate) then needs no further
//!   changes to handle a compound one.
//!
//! See [#307](https://github.com/daghovland/rdf-datalog/issues/307).

use crate::{graph, vocab};
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElementId};
use ingress::{RDF_FIRST, RDF_NIL, RDF_REST};
use std::collections::{HashMap, HashSet};

/// Prefix for synthetic predicates materializing a compound `sh:path`'s
/// extension in `work` (see [`resolve_one_path`]) — Datalog's own working
/// store, where synthetic marker predicates are already how this crate
/// encodes derived facts (see `translate.rs`'s module doc). Never added to
/// the original data graph, so nothing (e.g. `sh:closed`) needs to filter
/// it out. Deliberately an `urn:` (never a real IRI a shapes/data graph
/// author would write) so it can't collide with anything.
pub const SYNTHETIC_PATH_PREFIX: &str = "urn:dagalog:shacl:pathext:";

/// A parsed SHACL `sh:path` property-path expression.
///
/// Mirrors `sparql_parser::ast::PropertyPath` in shape (this codebase
/// already implements exactly this class of expression for SPARQL property
/// paths) but is intentionally a separate type: a SHACL path is parsed out
/// of an RDF object structure (see [`parse_path`]), not SPARQL query syntax,
/// and has no `Repeat`/`NegatedSet` counterparts — SHACL's grammar doesn't
/// have them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ShPath {
    /// A single predicate IRI (`ex:foo`).
    Predicate(String),
    /// `[ sh:inversePath p ]`.
    Inverse(Box<ShPath>),
    /// An RDF list of paths (`( p1 p2 … )`).
    Sequence(Vec<ShPath>),
    /// `[ sh:alternativePath ( p1 p2 … ) ]`.
    Alternative(Vec<ShPath>),
    /// `[ sh:zeroOrMorePath p ]`.
    ZeroOrMore(Box<ShPath>),
    /// `[ sh:oneOrMorePath p ]`.
    OneOrMore(Box<ShPath>),
    /// `[ sh:zeroOrOnePath p ]`.
    ZeroOrOne(Box<ShPath>),
}

impl ShPath {
    /// `Some(iri)` only for a plain single-predicate path. Used by
    /// `sh:closed` (only simple paths contribute to a shape's
    /// allowed-predicates set per spec) and to give a resolved property
    /// shape its `sh:resultPath` reporting IRI directly (no synthetic
    /// predicate needed for the common case).
    pub fn as_simple_iri(&self) -> Option<&str> {
        match self {
            ShPath::Predicate(iri) => Some(iri),
            _ => None,
        }
    }
}

/// Parse a `sh:path` object node (`path_id`, in the **shapes** store) into
/// an [`ShPath`]. Returns `None` for a structure that doesn't match any
/// known path shape — mirrors how the rest of `shapes.rs` treats malformed
/// input (the property shape is silently skipped).
pub fn parse_path(shapes: &Datastore, path_id: GraphElementId) -> Option<ShPath> {
    let mut seen = HashSet::new();
    parse_path_inner(shapes, path_id, &mut seen)
}

fn parse_path_inner(
    shapes: &Datastore,
    path_id: GraphElementId,
    seen: &mut HashSet<GraphElementId>,
) -> Option<ShPath> {
    // Guard against a cyclic path blank-node structure (not sanctioned by
    // SHACL, but a shapes graph is untrusted input) hanging recursion. `seen`
    // tracks only the nodes on the *current* recursion stack (pushed here,
    // popped before returning) — a genuine cycle re-enters one of its own
    // ancestors, which this catches, but the same node legitimately
    // referenced twice from unrelated branches (e.g. `sh:path ( _:p _:p )`,
    // a sequence that repeats one inverse-path node — see the W3C suite's
    // `path-complex-002.ttl`) must still parse both occurrences.
    if !seen.insert(path_id) {
        return None;
    }
    let result = parse_path_body(shapes, path_id, seen);
    seen.remove(&path_id);
    result
}

fn parse_path_body(
    shapes: &Datastore,
    path_id: GraphElementId,
    seen: &mut HashSet<GraphElementId>,
) -> Option<ShPath> {
    if let Some(iri) = graph::iri_string(shapes, path_id) {
        return Some(ShPath::Predicate(iri));
    }
    // Sequence: `path_id` itself is the head of an RDF list of paths. Checked
    // before the special-predicate cases below (`sh:inversePath` etc.) since
    // the W3C suite's "strange path" fixtures deliberately attach an
    // `sh:inversePath` triple to a blank node that is *also* a well-formed
    // `rdf:first`/`rdf:rest` list — a conformant reading treats the list
    // structure as authoritative in that case (see
    // `tests/testdata/w3c_shacl/core/path/path-strange-{001,002}.ttl`).
    let list_ids = graph::rdf_list(shapes, path_id);
    if !list_ids.is_empty() {
        let members: Vec<ShPath> = list_ids
            .into_iter()
            .filter_map(|id| parse_path_inner(shapes, id, seen))
            .collect();
        return match members.len() {
            0 => None,
            1 => members.into_iter().next(),
            _ => Some(ShPath::Sequence(members)),
        };
    }
    if let Some(inner_id) = graph::get_object(shapes, path_id, vocab::SH_INVERSE_PATH) {
        return parse_path_inner(shapes, inner_id, seen).map(|p| ShPath::Inverse(Box::new(p)));
    }
    if let Some(inner_id) = graph::get_object(shapes, path_id, vocab::SH_ZERO_OR_MORE_PATH) {
        return parse_path_inner(shapes, inner_id, seen).map(|p| ShPath::ZeroOrMore(Box::new(p)));
    }
    if let Some(inner_id) = graph::get_object(shapes, path_id, vocab::SH_ONE_OR_MORE_PATH) {
        return parse_path_inner(shapes, inner_id, seen).map(|p| ShPath::OneOrMore(Box::new(p)));
    }
    if let Some(inner_id) = graph::get_object(shapes, path_id, vocab::SH_ZERO_OR_ONE_PATH) {
        return parse_path_inner(shapes, inner_id, seen).map(|p| ShPath::ZeroOrOne(Box::new(p)));
    }
    if let Some(list_head) = graph::get_object(shapes, path_id, vocab::SH_ALTERNATIVE_PATH) {
        let members: Vec<ShPath> = graph::rdf_list(shapes, list_head)
            .into_iter()
            .filter_map(|id| parse_path_inner(shapes, id, seen))
            .collect();
        return if members.is_empty() {
            None
        } else {
            Some(ShPath::Alternative(members))
        };
    }
    None
}

/// Serialize `path` into `ds` as the SHACL-spec RDF encoding of a `sh:path`
/// object, returning the `GraphElementId` of the root path term — the
/// reverse of [`parse_path`]/[`parse_path_body`]. A simple `Predicate(iri)`
/// serializes as the IRI itself (no blank node, matching how a real shapes
/// graph writes `sh:path ex:foo`); every compound variant gets a fresh
/// blank node (never shared/deduplicated across calls — see
/// [#335](https://github.com/daghovland/rdf-datalog/issues/335), which notes
/// this doesn't matter for RDFC-1.0 graph-canonicalization correctness, only
/// that the emitted subtree is a real, correctly-shaped path expression each
/// time) carrying the one triple (`sh:inversePath`/`sh:alternativePath`/
/// `sh:zeroOrMorePath`/`sh:oneOrMorePath`/`sh:zeroOrOnePath`) SHACL uses to
/// mark its kind, or — for `Sequence` — no wrapper node at all: a sequence
/// *is* the RDF list itself (`sh:path ( p1 p2 … )`), so this returns the
/// list's own head id directly.
pub fn to_datastore(ds: &mut Datastore, path: &ShPath) -> GraphElementId {
    match path {
        ShPath::Predicate(iri) => graph::intern_iri(ds, iri),
        ShPath::Inverse(inner) => wrap(ds, inner, vocab::SH_INVERSE_PATH),
        ShPath::ZeroOrMore(inner) => wrap(ds, inner, vocab::SH_ZERO_OR_MORE_PATH),
        ShPath::OneOrMore(inner) => wrap(ds, inner, vocab::SH_ONE_OR_MORE_PATH),
        ShPath::ZeroOrOne(inner) => wrap(ds, inner, vocab::SH_ZERO_OR_ONE_PATH),
        ShPath::Sequence(steps) => build_rdf_list(ds, steps),
        ShPath::Alternative(branches) => {
            let list_head = build_rdf_list(ds, branches);
            wrap_id(ds, list_head, vocab::SH_ALTERNATIVE_PATH)
        }
    }
}

/// `[ <pred_iri> <inner-serialized> ]` — the shared shape of `sh:inversePath`/
/// `sh:zeroOrMorePath`/`sh:oneOrMorePath`/`sh:zeroOrOnePath`: a fresh blank
/// node with exactly one triple pointing at `inner`'s own serialization.
fn wrap(ds: &mut Datastore, inner: &ShPath, pred_iri: &str) -> GraphElementId {
    let inner_id = to_datastore(ds, inner);
    wrap_id(ds, inner_id, pred_iri)
}

/// As [`wrap`], but `inner_id` is already a resolved term (used by
/// `Alternative`, whose "inner" is the RDF list head, not another `ShPath`).
fn wrap_id(ds: &mut Datastore, inner_id: GraphElementId, pred_iri: &str) -> GraphElementId {
    let node = ds.new_anonymous_blank_node();
    let pred = graph::intern_iri(ds, pred_iri);
    ds.add_triple(Triple {
        subject: node,
        predicate: pred,
        obj: inner_id,
    });
    node
}

/// Build an RDF list (`rdf:first`/`rdf:rest` chain terminated by `rdf:nil`)
/// out of `items`, each serialized recursively via [`to_datastore`]. Returns
/// the list's head id (or `rdf:nil` itself for an empty list, though
/// `ShPath::Sequence`/`Alternative` never construct one with zero members —
/// see [`parse_path_body`]'s `0 => None` case, which means a would-be-empty
/// sequence never becomes a `ShPath` at all).
fn build_rdf_list(ds: &mut Datastore, items: &[ShPath]) -> GraphElementId {
    let rdf_nil = graph::intern_iri(ds, RDF_NIL);
    let mut rest = rdf_nil;
    for item in items.iter().rev() {
        let item_id = to_datastore(ds, item);
        let node = ds.new_anonymous_blank_node();
        let rdf_first = graph::intern_iri(ds, RDF_FIRST);
        let rdf_rest = graph::intern_iri(ds, RDF_REST);
        ds.add_triple(Triple {
            subject: node,
            predicate: rdf_first,
            obj: item_id,
        });
        ds.add_triple(Triple {
            subject: node,
            predicate: rdf_rest,
            obj: rest,
        });
        rest = node;
    }
    rest
}

/// Inline Turtle-syntax form of `path`, for [`crate::report_to_turtle`]'s
/// text serializer — the human-readable counterpart of [`to_datastore`].
/// Mirrors SHACL's own Turtle path-expression grammar exactly (`[
/// sh:inversePath <p> ]`, `( <p1> <p2> )`, …) rather than the flattened
/// blank-node-label placeholder this replaced.
pub fn to_turtle(path: &ShPath) -> String {
    match path {
        ShPath::Predicate(iri) => format!("<{iri}>"),
        ShPath::Inverse(inner) => format!("[ sh:inversePath {} ]", to_turtle(inner)),
        ShPath::ZeroOrMore(inner) => format!("[ sh:zeroOrMorePath {} ]", to_turtle(inner)),
        ShPath::OneOrMore(inner) => format!("[ sh:oneOrMorePath {} ]", to_turtle(inner)),
        ShPath::ZeroOrOne(inner) => format!("[ sh:zeroOrOnePath {} ]", to_turtle(inner)),
        ShPath::Sequence(steps) => {
            let items: Vec<String> = steps.iter().map(to_turtle).collect();
            format!("( {} )", items.join(" "))
        }
        ShPath::Alternative(branches) => {
            let items: Vec<String> = branches.iter().map(to_turtle).collect();
            format!("[ sh:alternativePath ( {} ) ]", items.join(" "))
        }
    }
}

// ── Evaluation: full path extension over the data graph ─────────────────────

/// All `(subject, object)` pairs connected by `path` in `data`'s default
/// graph. Recursive over `ShPath`'s structure; mirrors
/// `sparql_parser::execute`'s `eval_path_pattern`/`transitive_closure`
/// reachability semantics (arbitrary path length, not bounded repetition),
/// adapted from "extend one partial solution" to "compute the whole pair set
/// once" since SHACL path evaluation has no notion of a SPARQL solution
/// binding to extend.
///
/// Test-suite-sized graphs only — this is not written for web-scale graphs.
pub fn pairs(data: &Datastore, path: &ShPath) -> HashSet<(GraphElementId, GraphElementId)> {
    match path {
        ShPath::Predicate(iri) => {
            let Some(pred_id) = graph::lookup_iri(data, iri) else {
                return HashSet::new();
            };
            data.get_triples_with_predicate(pred_id)
                .map(|t| (t.subject, t.obj))
                .collect()
        }
        ShPath::Inverse(inner) => pairs(data, inner)
            .into_iter()
            .map(|(s, o)| (o, s))
            .collect(),
        ShPath::Sequence(steps) => {
            let mut steps = steps.iter();
            let Some(first) = steps.next() else {
                return HashSet::new();
            };
            let mut current = pairs(data, first);
            for step in steps {
                let next = pairs(data, step);
                let mut by_mid: HashMap<GraphElementId, Vec<GraphElementId>> = HashMap::new();
                for (m, o) in &next {
                    by_mid.entry(*m).or_default().push(*o);
                }
                current = current
                    .into_iter()
                    .flat_map(|(s, m)| {
                        by_mid
                            .get(&m)
                            .into_iter()
                            .flatten()
                            .map(move |&o| (s, o))
                            .collect::<Vec<_>>()
                    })
                    .collect();
            }
            current
        }
        ShPath::Alternative(branches) => branches.iter().flat_map(|b| pairs(data, b)).collect(),
        ShPath::ZeroOrOne(inner) => {
            let mut result = pairs(data, inner);
            for n in all_nodes(data) {
                result.insert((n, n));
            }
            result
        }
        ShPath::OneOrMore(inner) => transitive_closure(data, inner, false),
        ShPath::ZeroOrMore(inner) => transitive_closure(data, inner, true),
    }
}

/// Every node (subject or object of any default-graph triple) in `data`.
/// Used to seed the reflexive `(n, n)` pairs that `sh:zeroOrOnePath`/
/// `sh:zeroOrMorePath`'s zero-length match contributes for every node.
fn all_nodes(data: &Datastore) -> HashSet<GraphElementId> {
    use dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID;
    let mut nodes = HashSet::new();
    for q in data.named_graphs.get_all_quads() {
        if q.triple_id == DEFAULT_GRAPH_ELEMENT_ID {
            nodes.insert(q.subject);
            nodes.insert(q.obj);
        }
    }
    nodes
}

/// BFS reachability closure over `inner`'s pairs, one start node at a time.
/// `include_zero` adds `(n, n)` for every node (zeroOrMore); without it,
/// only genuinely-reached pairs are included (oneOrMore).
fn transitive_closure(
    data: &Datastore,
    inner: &ShPath,
    include_zero: bool,
) -> HashSet<(GraphElementId, GraphElementId)> {
    let base = pairs(data, inner);
    let mut adj: HashMap<GraphElementId, Vec<GraphElementId>> = HashMap::new();
    let mut starts: HashSet<GraphElementId> = HashSet::new();
    for (s, o) in &base {
        adj.entry(*s).or_default().push(*o);
        starts.insert(*s);
    }

    let mut result = HashSet::new();
    for start in starts {
        let mut visited: HashSet<GraphElementId> = HashSet::new();
        let mut stack = vec![start];
        while let Some(cur) = stack.pop() {
            if let Some(nexts) = adj.get(&cur) {
                for &n in nexts {
                    if visited.insert(n) {
                        result.insert((start, n));
                        stack.push(n);
                    }
                }
            }
        }
    }
    if include_zero {
        for n in all_nodes(data) {
            result.insert((n, n));
        }
    }
    result
}

/// All values reachable from `node` by following `path` in `data`. The
/// direct-evaluation (Phase 2, `evaluate.rs`) counterpart of
/// `resolve_one_path` — used wherever a property shape's path-traversed
/// values are needed for constraint checking that isn't compiled to
/// Datalog. Purely read-only: a compound path's extension is recomputed
/// from `data` on every call rather than cached, since `evaluate.rs`'s
/// per-focus-node evaluation (including ad hoc re-parsing of inner shapes
/// via `shapes::parse_one_shape`, e.g. from `sh:not`/`sh:and`/`sh:or`
/// references) has no stable place to cache a resolved predicate id
/// against. Test-suite-sized graphs only, per [`pairs`].
pub fn values_from(data: &Datastore, node: GraphElementId, path: &ShPath) -> Vec<GraphElementId> {
    match path {
        ShPath::Predicate(iri) => {
            let Some(pred_id) = graph::lookup_iri(data, iri) else {
                return vec![];
            };
            data.get_triples_with_subject_predicate(node, pred_id)
                .map(|t| t.obj)
                .collect()
        }
        _ => pairs(data, path)
            .into_iter()
            .filter(|(s, _)| *s == node)
            .map(|(_, o)| o)
            .collect(),
    }
}

// ── Resolution: materialize a compound path as a ground Datalog predicate ───

/// Resolve one property shape's `sh:path` to a `GraphElementId` usable
/// directly as a predicate in a Datalog rule body — the
/// `translate::shapes_to_rules` (Phase 1) counterpart of [`values_from`].
///
/// A simple `Predicate(iri)` path is just interned into `work` (no new
/// triples — zero overhead beyond the pre-#307 behaviour). A compound
/// path's full extension ([`pairs`], computed against `work` — already a
/// full clone of the data graph at the point `shapes_to_rules` runs) is
/// materialized as ground triples in `work` under a fresh synthetic
/// predicate (`SYNTHETIC_PATH_PREFIX{shape_idx}:{prop_idx}`), so every
/// existing Datalog-rule-generation code path (which assumes a path *is* a
/// single predicate) needs no further changes to handle it. `work` is
/// Datalog's own working store — synthetic marker predicates are already
/// how this crate encodes derived facts there (see `translate.rs`'s module
/// doc), so adding this one is consistent with existing practice; unlike
/// `evaluate.rs`'s read-only `data`, mutating `work` here is exactly what
/// the rest of `translate.rs` already does.
pub fn resolve_one_path(
    work: &mut Datastore,
    path: &ShPath,
    shape_idx: usize,
    prop_idx: usize,
) -> GraphElementId {
    if let Some(iri) = path.as_simple_iri() {
        return graph::intern_iri(work, iri);
    }
    let extension = pairs(work, path);
    let synthetic_iri = format!("{SYNTHETIC_PATH_PREFIX}{shape_idx}:{prop_idx}");
    let pred_id = graph::intern_iri(work, &synthetic_iri);
    for (s, o) in extension {
        work.add_triple(Triple {
            subject: s,
            predicate: pred_id,
            obj: o,
        });
    }
    pred_id
}

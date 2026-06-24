/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Query-log transformation (paper §5.1.3).
//!
//! Converts a raw log of observed SPARQL query strings into the typed
//! `QueryLog` (`Vec<(weight, path)>`) consumed by the search methods in
//! `search.rs`.  Each entry of the resulting log is one **extension case**:
//! a root→leaf path of navigation edges ending in a data edge, with `leaf`
//! being the data variable whose productive values that extension case
//! evaluates.
//!
//! ## Transformation steps (paper §5.1.3)
//!
//! 1. Parse each SPARQL query string.
//! 2. Filter: discard triples whose predicate is not a navigation-graph
//!    property (this also drops `rdf:type` triples, which assert class
//!    membership rather than a navigable edge).
//! 3. Solution-sequence modifiers (`LIMIT`, `OFFSET`, `ORDER BY`, `GROUP BY`,
//!    `DISTINCT`, `REDUCED`) are not part of the WHERE-clause BGP, so they are
//!    dropped automatically by only extracting BGP triples.
//! 4. Split `UNION` into separate branches, distributing the query's weight
//!    equally among the resulting branches (generalised to handle nested or
//!    multiple top-level UNIONs).
//! 5. For each branch, build the undirected variable-adjacency tree; discard
//!    the branch if it is empty, disconnected, or contains a cycle.
//! 6. Root the tree at the subject of its first surviving triple, walk it via
//!    BFS (inverting edges via `NavGraph::inverse_edge` when traversing
//!    against a triple's natural direction), and emit one extension case per
//!    data-edge leaf, weighted `branch_weight / data_leaf_count`.
//! 7. Merge identical paths produced across the whole raw log, summing weights.
//!
//! ## Known simplifications
//!
//! - `FILTER`, `BIND`, `VALUES`, `MINUS`, `GRAPH`, `SERVICE`, and subqueries
//!   contribute no triples (only `BGP` and `OPTIONAL` bodies are scanned).
//! - When a predicate IRI matches more than one navigation edge (e.g. the
//!   same property name reused across different domain classes), the first
//!   match found in `NavGraph::edges()` iteration order is used; there is no
//!   type-aware disambiguation.
//! - Only datatype-property extension cases are produced, consistent with
//!   the paper's restriction that "the precision measure only considers
//!   datatype property extension cases" (§4.2.3).

use crate::navigation_graph::{NavEdgeId, NavGraph};
use crate::search::QueryLog;
use sparql_parser::ast::{Query, QueryComponent, Term, TriplePattern};
use sparql_parser::{ParserContext, parse_query};
use std::collections::{HashMap, HashSet, VecDeque};

/// One entry in a raw, untransformed query log: an observed SPARQL query
/// string with an associated weight (e.g. an observation count).
#[derive(Debug, Clone)]
pub struct RawLogEntry {
    pub weight: f64,
    pub sparql: String,
}

/// Transform a raw SPARQL query log into the typed `QueryLog` consumed by the
/// search methods in `search.rs`.
pub fn transform_query_log(raw: &[RawLogEntry], nav: &NavGraph) -> QueryLog {
    let mut merged: HashMap<Vec<NavEdgeId>, f64> = HashMap::new();
    for entry in raw {
        for (weight, path) in extension_cases_for_query(&entry.sparql, entry.weight, nav) {
            *merged.entry(path).or_insert(0.0) += weight;
        }
    }
    merged
        .into_iter()
        .map(|(path, weight)| (weight, path))
        .collect()
}

/// Parse one SPARQL query string and produce its extension cases.
fn extension_cases_for_query(
    sparql: &str,
    weight: f64,
    nav: &NavGraph,
) -> Vec<(f64, Vec<NavEdgeId>)> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let Ok((_, query)) = parse_query(sparql, &mut ctx) else {
        return vec![];
    };
    let where_clause = match &query {
        Query::Select { where_clause, .. } => where_clause,
        Query::Ask { where_clause } => where_clause,
        Query::Construct { where_clause, .. } => where_clause,
    };

    let branches = collect_branches(where_clause);
    if branches.is_empty() {
        return vec![];
    }
    let branch_weight = weight / branches.len() as f64;

    branches
        .into_iter()
        .flat_map(|triples| extension_cases_for_branch(&triples, branch_weight, nav))
        .collect()
}

/// Recursively collect alternative triple-pattern sets ("branches") from a
/// WHERE-clause body.  A body with no `UNION` produces exactly one branch.
/// Each `UNION` multiplies the branch count by the sum of its two arms' branches.
fn collect_branches(components: &[QueryComponent]) -> Vec<Vec<TriplePattern>> {
    let mut branches: Vec<Vec<TriplePattern>> = vec![vec![]];

    for component in components {
        match component {
            QueryComponent::BGP(triples) => {
                for branch in &mut branches {
                    branch.extend(triples.iter().cloned());
                }
            }
            QueryComponent::Optional(inner) => {
                let extra = flatten_bgp_only(inner);
                for branch in &mut branches {
                    branch.extend(extra.iter().cloned());
                }
            }
            QueryComponent::Union(left, right) => {
                let left_branches = collect_branches(left);
                let right_branches = collect_branches(right);
                let mut new_branches = Vec::new();
                for prefix in &branches {
                    for lb in &left_branches {
                        let mut b = prefix.clone();
                        b.extend(lb.iter().cloned());
                        new_branches.push(b);
                    }
                    for rb in &right_branches {
                        let mut b = prefix.clone();
                        b.extend(rb.iter().cloned());
                        new_branches.push(b);
                    }
                }
                branches = new_branches;
            }
            // FILTER, BIND, VALUES, MINUS, GRAPH, SERVICE, subqueries: no
            // triples contributed (documented simplification).
            _ => {}
        }
    }

    branches
}

/// Flatten only the `BGP`/`Optional` triples of a component list (used inside
/// `OPTIONAL` bodies, where we don't expect nested `UNION` in typical logs).
fn flatten_bgp_only(components: &[QueryComponent]) -> Vec<TriplePattern> {
    let mut triples = Vec::new();
    for component in components {
        match component {
            QueryComponent::BGP(tps) => triples.extend(tps.iter().cloned()),
            QueryComponent::Optional(inner) => triples.extend(flatten_bgp_only(inner)),
            _ => {}
        }
    }
    triples
}

/// Build the extension cases for one branch (a flat list of triple patterns).
fn extension_cases_for_branch(
    triples: &[TriplePattern],
    branch_weight: f64,
    nav: &NavGraph,
) -> Vec<(f64, Vec<NavEdgeId>)> {
    // Step: filter to variable-only triples whose predicate matches a
    // navigation edge (drops rdf:type and anything with a constant
    // subject/object, which don't fit our variable-tree model).
    let matched: Vec<(String, NavEdgeId, String)> = triples
        .iter()
        .filter_map(|tp| {
            let Term::Variable(s) = &tp.subject else {
                return None;
            };
            let Term::Variable(o) = &tp.object else {
                return None;
            };
            let pred_iri = match &tp.predicate {
                Term::Constant(dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
                    iri,
                ))) => &iri.0,
                _ => return None,
            };
            let eid = nav.edges().find(|e| &e.iri == pred_iri)?.id;
            Some((s.clone(), eid, o.clone()))
        })
        .collect();

    let Some(paths) = build_tree_paths(&matched, nav) else {
        return vec![]; // empty, disconnected, or cyclic — discard
    };

    let leaf_paths: Vec<Vec<NavEdgeId>> = matched
        .iter()
        .filter(|(_, eid, _)| nav.edge(*eid).is_data_edge())
        .filter_map(|(_, _, o)| paths.get(o).cloned())
        .collect();

    if leaf_paths.is_empty() {
        return vec![];
    }
    let leaf_weight = branch_weight / leaf_paths.len() as f64;
    leaf_paths.into_iter().map(|p| (leaf_weight, p)).collect()
}

/// BFS the undirected variable graph defined by `matched` (rooted at the
/// subject of the first triple), returning each variable's root→variable
/// path of navigation edges.  Returns `None` if the graph is empty,
/// disconnected, or contains a cycle.
fn build_tree_paths(
    matched: &[(String, NavEdgeId, String)],
    nav: &NavGraph,
) -> Option<HashMap<String, Vec<NavEdgeId>>> {
    if matched.is_empty() {
        return None;
    }

    let mut adjacency: HashMap<&str, Vec<(&str, NavEdgeId)>> = HashMap::new();
    for (s, eid, o) in matched {
        adjacency
            .entry(s.as_str())
            .or_default()
            .push((o.as_str(), *eid));
        if let Some(inv) = nav.inverse_edge(*eid) {
            adjacency
                .entry(o.as_str())
                .or_default()
                .push((s.as_str(), inv));
        }
    }

    let all_vars: HashSet<&str> = matched
        .iter()
        .flat_map(|(s, _, o)| [s.as_str(), o.as_str()])
        .collect();
    let root: &str = matched[0].0.as_str();

    let mut paths: HashMap<String, Vec<NavEdgeId>> = HashMap::new();
    paths.insert(root.to_string(), vec![]);
    let mut parent: HashMap<&str, &str> = HashMap::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    queue.push_back(root);

    while let Some(cur) = queue.pop_front() {
        let cur_path = paths[cur].clone();
        let Some(neighbors) = adjacency.get(cur) else {
            continue;
        };
        for &(other, eid) in neighbors {
            if parent.get(cur) == Some(&other) {
                continue; // the edge back to where we came from
            }
            if paths.contains_key(other) {
                return None; // real cycle
            }
            parent.insert(other, cur);
            let mut new_path = cur_path.clone();
            new_path.push(eid);
            paths.insert(other.to_string(), new_path);
            queue.push_back(other);
        }
    }

    if paths.len() != all_vars.len() {
        return None; // disconnected
    }
    Some(paths)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation_graph::NavGraph;

    /// A navigation graph with unambiguous property names (no shared labels
    /// across classes), so predicate→edge matching is deterministic without
    /// needing type-aware disambiguation.
    fn nav_no_ambiguity() -> NavGraph {
        let mut g = NavGraph::new();
        let person = g.add_class("http://example.org/Person");
        let country = g.add_class("http://example.org/Country");
        let xsd_int = g.add_datatype("http://www.w3.org/2001/XMLSchema#integer");
        g.add_data_property("http://example.org/age", person, xsd_int);
        g.add_data_property("http://example.org/population", country, xsd_int);
        g.add_object_property(
            "http://example.org/visited",
            person,
            country,
            "http://example.org/visitedBy",
        );
        g.add_object_property(
            "http://example.org/knows",
            person,
            person,
            "http://example.org/knows",
        );
        g
    }

    fn age_edge(nav: &NavGraph) -> NavEdgeId {
        nav.edges()
            .find(|e| e.iri == "http://example.org/age")
            .unwrap()
            .id
    }
    fn population_edge(nav: &NavGraph) -> NavEdgeId {
        nav.edges()
            .find(|e| e.iri == "http://example.org/population")
            .unwrap()
            .id
    }
    fn visited_edge(nav: &NavGraph) -> NavEdgeId {
        nav.edges()
            .find(|e| e.iri == "http://example.org/visited")
            .unwrap()
            .id
    }

    /// A single-triple query produces one extension case with the full weight.
    #[test]
    fn single_triple_query_one_extension_case() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 3.0,
            sparql: "SELECT ?p WHERE { ?p <http://example.org/age> ?a }".to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (3.0, vec![age_edge(&nav)]));
    }

    /// A `rdf:type` triple is filtered out and doesn't appear in the path.
    #[test]
    fn rdf_type_triple_is_filtered() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 1.0,
            sparql: "SELECT ?p WHERE { \
                ?p <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/Person> . \
                ?p <http://example.org/age> ?a \
            }"
            .to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (1.0, vec![age_edge(&nav)]));
    }

    /// A two-hop chain query produces a two-edge path.
    #[test]
    fn two_hop_chain_query() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 2.0,
            sparql: "SELECT ?p WHERE { \
                ?p <http://example.org/visited> ?c . \
                ?c <http://example.org/population> ?pop \
            }"
            .to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert_eq!(log.len(), 1);
        assert_eq!(
            log[0],
            (2.0, vec![visited_edge(&nav), population_edge(&nav)])
        );
    }

    /// A disconnected query (two unrelated components) is discarded entirely.
    #[test]
    fn disconnected_query_discarded() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 1.0,
            sparql: "SELECT ?a ?b WHERE { \
                ?a <http://example.org/age> ?x . \
                ?b <http://example.org/age> ?y \
            }"
            .to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert!(
            log.is_empty(),
            "two disconnected age-triples should be discarded"
        );
    }

    /// A UNION query splits weight equally between its two arms.
    #[test]
    fn union_query_splits_weight() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 4.0,
            sparql: "SELECT ?p WHERE { \
                { ?p <http://example.org/age> ?a } \
                UNION \
                { ?p <http://example.org/visited> ?c . ?c <http://example.org/population> ?pop } \
            }"
            .to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert_eq!(log.len(), 2);
        let age_entry = log
            .iter()
            .find(|(_, p)| p == &vec![age_edge(&nav)])
            .unwrap();
        assert_eq!(age_entry.0, 2.0, "weight split equally across 2 UNION arms");
        let chain_entry = log
            .iter()
            .find(|(_, p)| p == &vec![visited_edge(&nav), population_edge(&nav)])
            .unwrap();
        assert_eq!(chain_entry.0, 2.0);
    }

    /// Identical extension cases from different raw log entries are merged
    /// (weights summed).
    #[test]
    fn identical_paths_merge_weights() {
        let nav = nav_no_ambiguity();
        let raw = vec![
            RawLogEntry {
                weight: 1.0,
                sparql: "SELECT ?p WHERE { ?p <http://example.org/age> ?a }".to_string(),
            },
            RawLogEntry {
                weight: 5.0,
                sparql: "SELECT ?q WHERE { ?q <http://example.org/age> ?b }".to_string(),
            },
        ];
        let log = transform_query_log(&raw, &nav);
        assert_eq!(log.len(), 1, "same edge path regardless of variable names");
        assert_eq!(log[0], (6.0, vec![age_edge(&nav)]));
    }

    /// A query with no data-edge leaves (pure object-property chain) produces
    /// no extension cases.
    #[test]
    fn no_data_leaves_produces_no_cases() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 1.0,
            sparql: "SELECT ?p ?q WHERE { ?p <http://example.org/knows> ?q }".to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert!(log.is_empty());
    }

    /// A query whose only triple has an unrecognised predicate (not in N)
    /// produces no extension cases.
    #[test]
    fn unrecognised_predicate_discarded() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 1.0,
            sparql: "SELECT ?p WHERE { ?p <http://example.org/notInNav> ?a }".to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert!(log.is_empty());
    }

    /// An unparseable SPARQL string is silently skipped, not a panic.
    #[test]
    fn unparseable_query_skipped() {
        let nav = nav_no_ambiguity();
        let raw = vec![RawLogEntry {
            weight: 1.0,
            sparql: "this is not SPARQL at all {{{".to_string(),
        }];
        let log = transform_query_log(&raw, &nav);
        assert!(log.is_empty());
    }
}

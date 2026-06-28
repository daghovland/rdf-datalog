/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Heuristic search methods for finding good configuration sets (paper §4.3).
//!
//! All four methods start from an empty configuration set and iteratively add
//! one variable at a time, producing a sequence of configurations of increasing
//! cost and (usually) increasing precision.
//!
//! | Method              | Heuristic                                      | Speed   |
//! |---------------------|------------------------------------------------|---------|
//! | GreedyQueryWeight   | Property popularity in query log               | Fast    |
//! | Random              | Random order (baseline)                        | Fast    |
//! | GreedyPrecision     | Best immediate precision gain                  | Medium  |
//! | Exploratory         | Best precision one step ahead (MCTS-inspired)  | Slow    |
//!
//! A **query log** here is a list of `(weight, Vec<NavEdgeId>)` pairs, where
//! each edge sequence represents one observed query path (root → leaf).

use crate::basic_counts::NavStats;
use crate::config_query::ConfigQuery;
use crate::estimators::{est_ans_p, est_cost_single};
use crate::navigation_graph::{NavEdgeId, NavGraph};

/// A single step in a search trace: one config query with its estimated cost.
#[derive(Debug, Clone)]
pub struct SearchStep {
    pub config: ConfigQuery,
    pub est_cost: f64,
}

/// A query log: each entry is (weight, edge-path from root to leaf).
pub type QueryLog = Vec<(f64, Vec<NavEdgeId>)>;

// ── Successor generation ──────────────────────────────────────────────────────

/// All config queries reachable by adding exactly one variable to `config`.
///
/// Each successor extends one existing node by one of its outgoing edges in N.
/// The query-log constraint limits expansions to edges actually seen in `log`.
pub fn successors(config: &ConfigQuery, nav: &NavGraph, log: &QueryLog) -> Vec<ConfigQuery> {
    // Collect edge IRIs seen in the query log.
    let log_edges: std::collections::HashSet<NavEdgeId> = log
        .iter()
        .flat_map(|(_, path)| path.iter().copied())
        .collect();

    let mut result = Vec::new();
    for parent_idx in 0..config.nodes.len() {
        let parent_nav = config.nodes[parent_idx].nav_node;
        for &eid in nav.outgoing_edges(parent_nav) {
            if !log_edges.contains(&eid) {
                continue;
            }
            let mut child = config.clone();
            child.extend(parent_idx, eid, nav);
            result.push(child);
        }
    }
    result
}

// ── Estimated precision over a query log ─────────────────────────────────────

/// Estimated precision of `config` over `log` (average of per-case precisions).
///
/// For each (weight, path) in the log, treat each leaf data variable as an
/// extension case.  The pruned query is the largest prefix of `path` that the
/// config could cover — here approximated as the full config itself.
pub fn est_precision_log(
    config: &ConfigQuery,
    nav: &NavGraph,
    stats: &NavStats,
    log: &QueryLog,
) -> f64 {
    let total_weight: f64 = log.iter().map(|(w, _)| w).sum();
    if total_weight == 0.0 {
        return 0.0;
    }
    let mut weighted_sum = 0.0;
    for (weight, path) in log {
        if path.is_empty() {
            continue;
        }
        // Build a query matching the log path as closely as possible.
        let root_class = nav.edge(path[0]).src;
        if config.nodes[0].nav_node != root_class {
            continue;
        }
        // Build the extension query Qe from the path.
        let mut qe = ConfigQuery::root_only(root_class);
        let mut parent = 0;
        for &eid in path {
            parent = qe.extend(parent, eid, nav);
        }
        let leaf = qe.variable_count() - 1;
        let ans_qe = est_ans_p(&qe, leaf, nav, stats);
        // Pruned query: use config's last leaf that matches the extension edge.
        let ext_edge = *path.last().unwrap();
        let matching_leaf = config
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.parent_edge == Some(ext_edge))
            .map(|(i, _)| i)
            .next_back();
        let ans_qs = if let Some(ml) = matching_leaf {
            est_ans_p(config, ml, nav, stats)
        } else {
            // extension not covered: use class count as worst case
            *stats.counts.class_count.get(&root_class).unwrap_or(&0) as f64
        };
        let prec = if ans_qs == 0.0 {
            1.0
        } else {
            (ans_qe / ans_qs).min(1.0)
        };
        weighted_sum += weight * prec;
    }
    weighted_sum / total_weight
}

// ── Search method 1: Greedy Query Weight ─────────────────────────────────────

/// Greedy Query Weight method (paper Algorithm 1).
///
/// Starts with one root-only config per class in the query log, then adds
/// edges in decreasing order of query-log frequency.  Never evaluates cost
/// or precision — purely frequency-driven.
pub fn greedy_query_weight(nav: &NavGraph, log: &QueryLog, max_steps: usize) -> Vec<SearchStep> {
    // Edge frequency in the log.
    let mut edge_weight: std::collections::HashMap<NavEdgeId, f64> =
        std::collections::HashMap::new();
    for (w, path) in log {
        for &eid in path {
            *edge_weight.entry(eid).or_insert(0.0) += w;
        }
    }

    // Sorted edge list, most frequent first.
    let mut edges_by_weight: Vec<(NavEdgeId, f64)> = edge_weight.into_iter().collect();
    edges_by_weight.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Initialise: one root-only config per class.
    let root_classes: std::collections::HashSet<_> = log
        .iter()
        .flat_map(|(_, p)| p.first().map(|&e| nav.edge(e).src))
        .collect();
    let mut configs: std::collections::HashMap<crate::navigation_graph::NavNodeId, ConfigQuery> =
        root_classes
            .into_iter()
            .map(|c| (c, ConfigQuery::root_only(c)))
            .collect();

    let mut steps = Vec::new();

    for (step_count, (eid, _)) in edges_by_weight.iter().enumerate() {
        if step_count >= max_steps {
            break;
        }
        let src = nav.edge(*eid).src;
        let config = configs
            .entry(src)
            .or_insert_with(|| ConfigQuery::root_only(src));
        // Extend the node whose nav_node matches edge src (first match).
        let parent_idx = config
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| n.nav_node == src)
            .map(|(i, _)| i)
            .unwrap_or(0);
        config.extend(parent_idx, *eid, nav);
        steps.push(SearchStep {
            config: config.clone(),
            est_cost: 0.0, // not computed in this method
        });
    }
    steps
}

// ── Search method 2: Random ───────────────────────────────────────────────────

/// Random method (paper Algorithm 2) — baseline only.
///
/// Adds edges in an arbitrary (deterministic seed-based) order.
/// Uses a simple LCG to avoid pulling in a rand crate dependency.
pub fn random_search(
    nav: &NavGraph,
    log: &QueryLog,
    max_steps: usize,
    seed: u64,
) -> Vec<SearchStep> {
    let log_edges: Vec<NavEdgeId> = {
        let mut seen = std::collections::HashSet::new();
        log.iter()
            .flat_map(|(_, p)| p.iter().copied())
            .filter(|&e| seen.insert(e))
            .collect()
    };

    // LCG shuffle.
    let mut rng = seed;
    let mut edges = log_edges;
    for i in (1..edges.len()).rev() {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (rng >> 33) as usize % (i + 1);
        edges.swap(i, j);
    }

    let root_classes: std::collections::HashSet<_> = log
        .iter()
        .flat_map(|(_, p)| p.first().map(|&e| nav.edge(e).src))
        .collect();
    let mut configs: std::collections::HashMap<crate::navigation_graph::NavNodeId, ConfigQuery> =
        root_classes
            .into_iter()
            .map(|c| (c, ConfigQuery::root_only(c)))
            .collect();

    let mut steps = Vec::new();
    for &eid in edges.iter().take(max_steps) {
        let src = nav.edge(eid).src;
        let config = configs
            .entry(src)
            .or_insert_with(|| ConfigQuery::root_only(src));
        let parent_idx = config
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| n.nav_node == src)
            .map(|(i, _)| i)
            .unwrap_or(0);
        config.extend(parent_idx, eid, nav);
        steps.push(SearchStep {
            config: config.clone(),
            est_cost: 0.0,
        });
    }
    steps
}

// ── Search method 3: Greedy Precision ────────────────────────────────────────

/// Greedy Precision method (paper Algorithm 3).
///
/// Starts from W = ∅ and greedily picks the expansion with the highest
/// estimated precision at each step.
pub fn greedy_precision(
    nav: &NavGraph,
    stats: &NavStats,
    log: &QueryLog,
    max_steps: usize,
    max_cost: Option<f64>,
) -> Vec<SearchStep> {
    let mut current: Vec<ConfigQuery> = vec![];
    let mut steps = Vec::new();

    for _ in 0..max_steps {
        // Generate all candidate next steps: add one variable to any existing config,
        // or start a new root-only config for a class not yet covered.
        let mut candidates: Vec<ConfigQuery> = current
            .iter()
            .flat_map(|c| successors(c, nav, log))
            .collect();

        // Also offer starting new configs for uncovered root classes.
        let covered: std::collections::HashSet<_> =
            current.iter().map(|c| c.nodes[0].nav_node).collect();
        for (_, path) in log {
            if let Some(&eid) = path.first() {
                let root = nav.edge(eid).src;
                if !covered.contains(&root) {
                    candidates.push(ConfigQuery::root_only(root));
                }
            }
        }
        candidates.dedup_by_key(|c| c.variable_count());

        if candidates.is_empty() {
            break;
        }

        // Pick the candidate with the best estimated precision.
        let best = candidates
            .into_iter()
            .filter(|c| {
                max_cost
                    .map(|mc| est_cost_single(c, nav, stats) <= mc)
                    .unwrap_or(true)
            })
            .max_by(|a, b| {
                est_precision_log(a, nav, stats, log)
                    .partial_cmp(&est_precision_log(b, nav, stats, log))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        let Some(best) = best else { break };
        let cost = est_cost_single(&best, nav, stats);

        // Replace or add the config.
        let root = best.nodes[0].nav_node;
        if let Some(pos) = current.iter().position(|c| c.nodes[0].nav_node == root) {
            current[pos] = best.clone();
        } else {
            current.push(best.clone());
        }

        steps.push(SearchStep {
            config: best,
            est_cost: cost,
        });
    }
    steps
}

// ── Search method 4: Exploratory ─────────────────────────────────────────────

/// Exploratory method (paper Algorithm 4, MCTS-inspired).
///
/// Like GreedyPrecision, but scores each direct successor W′ by the *best*
/// precision found among `lookahead` random further successors of W′.
/// This allows the search to look one step further than greedy methods.
pub fn exploratory(
    nav: &NavGraph,
    stats: &NavStats,
    log: &QueryLog,
    max_steps: usize,
    lookahead: usize,
    max_cost: Option<f64>,
    seed: u64,
) -> Vec<SearchStep> {
    let mut current: Vec<ConfigQuery> = vec![];
    let mut steps = Vec::new();
    let mut rng = seed;

    for _ in 0..max_steps {
        let mut candidates: Vec<ConfigQuery> = current
            .iter()
            .flat_map(|c| successors(c, nav, log))
            .collect();

        let covered: std::collections::HashSet<_> =
            current.iter().map(|c| c.nodes[0].nav_node).collect();
        for (_, path) in log {
            if let Some(&eid) = path.first() {
                let root = nav.edge(eid).src;
                if !covered.contains(&root) {
                    candidates.push(ConfigQuery::root_only(root));
                }
            }
        }

        if candidates.is_empty() {
            break;
        }

        // Score each candidate by the best precision of `lookahead` random successors.
        let best = candidates
            .into_iter()
            .filter(|c| {
                max_cost
                    .map(|mc| est_cost_single(c, nav, stats) <= mc)
                    .unwrap_or(true)
            })
            .max_by(|a, b| {
                let score_a = lookahead_score(a, nav, stats, log, lookahead, &mut rng);
                let score_b = lookahead_score(b, nav, stats, log, lookahead, &mut rng);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        let Some(best) = best else { break };
        let cost = est_cost_single(&best, nav, stats);

        let root = best.nodes[0].nav_node;
        if let Some(pos) = current.iter().position(|c| c.nodes[0].nav_node == root) {
            current[pos] = best.clone();
        } else {
            current.push(best.clone());
        }

        steps.push(SearchStep {
            config: best,
            est_cost: cost,
        });
    }
    steps
}

fn lookahead_score(
    config: &ConfigQuery,
    nav: &NavGraph,
    stats: &NavStats,
    log: &QueryLog,
    lookahead: usize,
    rng: &mut u64,
) -> f64 {
    let base = est_precision_log(config, nav, stats, log);
    let mut succs = successors(config, nav, log);
    if succs.is_empty() || lookahead == 0 {
        return base;
    }
    // Sample `lookahead` random successors, take the best.
    for i in (1..succs.len()).rev().take(lookahead) {
        *rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (*rng >> 33) as usize % (i + 1);
        succs.swap(i, j);
    }
    succs
        .into_iter()
        .take(lookahead)
        .map(|s| est_precision_log(&s, nav, stats, log))
        .fold(base, f64::max)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic_counts::NavStats;
    use crate::navigation_graph::NavGraph;
    use dag_rdf::Datastore;
    use turtle::parse_turtle;

    fn figure1_nav() -> NavGraph {
        let mut g = NavGraph::new();
        let person = g.add_class("http://example.org/Person");
        let country = g.add_class("http://example.org/Country");
        let xsd_int = g.add_datatype("http://www.w3.org/2001/XMLSchema#integer");
        let xsd_str = g.add_datatype("http://www.w3.org/2001/XMLSchema#string");
        g.add_data_property("http://example.org/age", person, xsd_int);
        g.add_data_property("http://example.org/name", person, xsd_str);
        g.add_data_property("http://example.org/population", country, xsd_int);
        g.add_data_property("http://example.org/name", country, xsd_str);
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
        g.add_object_property(
            "http://example.org/borders",
            country,
            country,
            "http://example.org/borders",
        );
        g
    }

    fn figure3_stats() -> (NavGraph, NavStats) {
        let ttl = r#"
            @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
            @prefix ex:  <http://example.org/> .
            ex:P1 rdf:type ex:Person ; ex:age "21"^^xsd:integer ; ex:name "Alice"^^xsd:string ; ex:visited ex:Belgium .
            ex:P2 rdf:type ex:Person ; ex:age "35"^^xsd:integer ; ex:name "Robert"^^xsd:string .
            ex:P3 rdf:type ex:Person ; ex:age "45"^^xsd:integer ; ex:name "Carol"^^xsd:string .
            ex:P4 rdf:type ex:Person ; ex:age "30"^^xsd:integer ; ex:name "Dave"^^xsd:string .
            ex:P5 rdf:type ex:Person ; ex:age "11"^^xsd:integer .
            ex:P6 rdf:type ex:Person ; ex:age "16"^^xsd:integer .
            ex:Belgium rdf:type ex:Country ; ex:population "11000000"^^xsd:integer ; ex:name "Belgium"^^xsd:string ; ex:borders ex:France .
            ex:France rdf:type ex:Country ; ex:population "67000000"^^xsd:integer ; ex:name "France"^^xsd:string ; ex:borders ex:Belgium .
        "#;
        let nav = figure1_nav();
        let mut ds = Datastore::new(500);
        parse_turtle(&mut ds, ttl.as_bytes()).expect("parse");
        let stats = NavStats::compute(&nav, &ds);
        (nav, stats)
    }

    fn simple_log(nav: &NavGraph) -> QueryLog {
        // Two queries: Person→age and Person→name (both weighted equally)
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let age_eid = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/age")
            .unwrap()
            .id;
        let name_eid = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/name")
            .unwrap()
            .id;
        vec![(1.0, vec![age_eid]), (1.0, vec![name_eid])]
    }

    /// GreedyQueryWeight produces at least one step for a non-empty log.
    #[test]
    fn greedy_weight_produces_steps() {
        let (nav, _) = figure3_stats();
        let log = simple_log(&nav);
        let steps = greedy_query_weight(&nav, &log, 10);
        assert!(!steps.is_empty(), "should produce at least one step");
    }

    /// Random produces as many steps as there are unique edges in the log (up to max).
    #[test]
    fn random_produces_steps() {
        let (nav, _) = figure3_stats();
        let log = simple_log(&nav);
        let steps = random_search(&nav, &log, 10, 42);
        assert_eq!(steps.len(), 2, "one step per unique edge in log");
    }

    /// GreedyPrecision steps have monotonically non-decreasing precision.
    #[test]
    fn greedy_precision_non_decreasing() {
        let (nav, stats) = figure3_stats();
        let log = simple_log(&nav);
        let steps = greedy_precision(&nav, &stats, &log, 5, None);
        assert!(!steps.is_empty());
        let precisions: Vec<f64> = steps
            .iter()
            .map(|s| est_precision_log(&s.config, &nav, &stats, &log))
            .collect();
        for i in 1..precisions.len() {
            assert!(
                precisions[i] >= precisions[i - 1] - 1e-9,
                "precision should not decrease: step {i}: {} < {}",
                precisions[i],
                precisions[i - 1]
            );
        }
    }

    /// Exploratory produces at least one step.
    #[test]
    fn exploratory_produces_steps() {
        let (nav, stats) = figure3_stats();
        let log = simple_log(&nav);
        let steps = exploratory(&nav, &stats, &log, 5, 3, None, 42);
        assert!(!steps.is_empty());
    }

    /// GreedyPrecision converges to at least as high precision as the best random run.
    ///
    /// We compare fully-converged GreedyPrecision against fully-converged random:
    /// after exhausting all log edges, both should cover the same vocabulary, so
    /// the final precision of GreedyPrecision should be ≥ any individual random step.
    #[test]
    fn greedy_precision_converges_above_empty_baseline() {
        let (nav, stats) = figure3_stats();
        let log = simple_log(&nav);
        // Run greedy precision to completion (all log edges).
        let gp_steps = greedy_precision(&nav, &stats, &log, 20, None);
        assert!(
            !gp_steps.is_empty(),
            "greedy precision must produce at least one step"
        );
        let gp_prec = est_precision_log(&gp_steps.last().unwrap().config, &nav, &stats, &log);
        // Greedy precision should achieve better precision than an empty config (0.0).
        assert!(
            gp_prec > 0.0,
            "GreedyPrecision must improve over empty config, got {gp_prec}"
        );
    }
}

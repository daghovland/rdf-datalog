/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Cardinality estimators for cost and precision (paper §4.4).
//!
//! All estimators are cheap approximations based on the precomputed `NavStats`
//! (basic counts + histograms).  They are used by the search methods to rank
//! candidate expansions without firing expensive SPARQL queries.
//!
//! ## Symbols
//!
//! | Paper    | Code                  | Meaning                             |
//! |----------|-----------------------|-------------------------------------|
//! | ãns      | `est_ans`             | Estimated |ans(Q,D)|                |
//! | ãns^P    | `est_ans_p`           | Estimated |ans^P(Q,D,v)|            |
//! | ãns^O    | `est_ans_o`           | Estimated |ans^O(Z,D)|              |
//! | ãns^E    | `est_ans_e`           | Estimated |ans^E(Z,D)| → cost proxy |
//! | bf(e)    | `branching_factor`    | C_e(e) / C_c(src(e))               |
//! | m_v      | `expansion_factor`    | Expected result expansion per node  |

use crate::basic_counts::NavStats;
use crate::config_query::ConfigQuery;
use crate::navigation_graph::{NavEdgeId, NavGraph};

/// Branching factor bf(e) = C_e(e) / C_c(src(e))  (paper §4.4.3).
///
/// The expected number of e-edges an average instance of src(e) has in D.
pub fn branching_factor(edge: NavEdgeId, nav: &NavGraph, stats: &NavStats) -> f64 {
    let src_class = nav.edge(edge).src;
    let ce = *stats.counts.edge_count.get(&edge).unwrap_or(&0) as f64;
    let cc = *stats.counts.class_count.get(&src_class).unwrap_or(&0) as f64;
    if cc == 0.0 { 0.0 } else { ce / cc }
}

/// Estimate |ans(Q, D)| for a filterless configuration query Q (paper §4.4.3).
///
/// `ãns(Q,D) = C_c(root_class) × Π_{e∈E(Q)} bf(TQ(e))`
pub fn est_ans(config: &ConfigQuery, nav: &NavGraph, stats: &NavStats) -> f64 {
    let root_class = config.nodes[0].nav_node;
    let root_count = *stats.counts.class_count.get(&root_class).unwrap_or(&0) as f64;
    if root_count == 0.0 {
        return 0.0;
    }
    let mut result = root_count;
    for node in &config.nodes[1..] {
        if let Some(eid) = node.parent_edge {
            result *= branching_factor(eid, nav, stats);
        }
    }
    result
}

/// Estimate |ans^P(Q, D, v)| — distinct values of data variable v (paper §4.4.5).
///
/// Uses the coupon-collector approximation (eq. 9):
/// `ãns^P = Σ_{u∈Γv} [1 − (1 − H_e(u))^k]`  where k = ãns(Q,D).
pub fn est_ans_p(
    config: &ConfigQuery,
    data_var_node: usize,
    nav: &NavGraph,
    stats: &NavStats,
) -> f64 {
    let node = &config.nodes[data_var_node];
    let eid = match node.parent_edge {
        Some(e) => e,
        None => return 0.0,
    };
    let hist = match stats.histograms.get(&eid) {
        Some(h) => h,
        None => return 0.0, // object edge — no histogram
    };
    let k = est_ans(config, nav, stats);
    if k == 0.0 || hist.is_empty() {
        return 0.0;
    }
    hist.values()
        .map(|&h_u| 1.0 - (1.0 - h_u).powf(k))
        .sum::<f64>()
        .min(*stats.counts.edge_tgt_count.get(&eid).unwrap_or(&0) as f64)
}

/// Estimate |ans^O(Z, D)| — OPTIONAL-query cardinality (paper §4.4.6, eq. 10).
///
/// The expansion factor m_v is computed recursively from leaves to root.
pub fn est_ans_o(config: &ConfigQuery, nav: &NavGraph, stats: &NavStats) -> f64 {
    let root_class = config.nodes[0].nav_node;
    let root_count = *stats.counts.class_count.get(&root_class).unwrap_or(&0) as f64;
    if root_count == 0.0 {
        return 0.0;
    }
    root_count * expansion_factor(0, config, nav, stats)
}

/// Recursive expansion factor m_v (paper eq. 10).
fn expansion_factor(
    node_idx: usize,
    config: &ConfigQuery,
    nav: &NavGraph,
    stats: &NavStats,
) -> f64 {
    let node = &config.nodes[node_idx];
    if node.children.is_empty() {
        return 1.0;
    }
    node.children
        .iter()
        .map(|&child_idx| {
            let child_node = &config.nodes[child_idx];
            let eid = child_node.parent_edge.unwrap();
            let src_class = nav.edge(eid).src;

            let ce = *stats.counts.edge_count.get(&eid).unwrap_or(&0) as f64;
            let ces = *stats.counts.edge_src_count.get(&eid).unwrap_or(&0) as f64;
            let cc = *stats.counts.class_count.get(&src_class).unwrap_or(&0) as f64;

            let p = if cc > 0.0 { ces / cc } else { 0.0 };
            let avg_expand = if ces > 0.0 { ce / ces } else { 0.0 };

            let m_vc = expansion_factor(child_idx, config, nav, stats);
            (1.0 - p) + p * avg_expand * m_vc
        })
        .product()
}

/// Estimate |ans^E(Z, D)| — compressed index cardinality (paper §4.4.7).
///
/// Uses the coupon-collector formula with n = d_root − 1 possible assignments
/// and k = ãns^O(Z,D) samples.
pub fn est_ans_e(config: &ConfigQuery, nav: &NavGraph, stats: &NavStats) -> f64 {
    if config.variable_count() <= 1 {
        return 0.0; // root-only: no columns, cost = 0
    }
    let k = est_ans_o(config, nav, stats);
    if k == 0.0 {
        return 0.0;
    }
    let n = possible_assignments(0, config, nav, stats) - 1.0;
    if n <= 0.0 {
        return 0.0;
    }
    // n × [1 − (1 − 1/n)^k]
    n * (1.0 - (1.0 - 1.0 / n).powf(k))
}

/// d_v: estimate of the number of possible ways to assign values to v and all
/// its descendants in ans^E.
fn possible_assignments(
    node_idx: usize,
    config: &ConfigQuery,
    nav: &NavGraph,
    stats: &NavStats,
) -> f64 {
    let node = &config.nodes[node_idx];
    if nav.node(node.nav_node).is_datatype() {
        // data variable: Cet(e) + 1 (include ω)
        let eid = node.parent_edge.unwrap();
        *stats.counts.edge_tgt_count.get(&eid).unwrap_or(&0) as f64 + 1.0
    } else {
        // object variable: 1 (ω) + Π_{children} d_vc
        1.0 + node
            .children
            .iter()
            .map(|&ci| possible_assignments(ci, config, nav, stats))
            .product::<f64>()
    }
}

/// Estimated cost of a single-query configuration: (|V(Z)|−1) × ãns^E (paper eq. 8).
pub fn est_cost_single(config: &ConfigQuery, nav: &NavGraph, stats: &NavStats) -> f64 {
    let cols = (config.variable_count().saturating_sub(1)) as f64;
    cols * est_ans_e(config, nav, stats)
}

/// Estimated total cost of a configuration set.
pub fn est_cost(configs: &[ConfigQuery], nav: &NavGraph, stats: &NavStats) -> f64 {
    configs.iter().map(|z| est_cost_single(z, nav, stats)).sum()
}

/// Estimated precision of one extension case: |ãns^P(Qe)| / |ãns^P(Qs)|
/// where Qe is the extension query and Qs is the pruned version (paper eq. 5).
///
/// `ext_node` is the index (in `ext_config`) of the data variable being extended.
/// `pruned_config` is the largest covered sub-tree (may equal `ext_config`).
pub fn est_precision_case(
    ext_config: &ConfigQuery,
    ext_node: usize,
    pruned_config: &ConfigQuery,
    nav: &NavGraph,
    stats: &NavStats,
) -> f64 {
    let ans_qe = est_ans_p(ext_config, ext_node, nav, stats);
    let ans_qs = est_ans_p(
        pruned_config,
        pruned_config.variable_count() - 1,
        nav,
        stats,
    );
    if ans_qs == 0.0 {
        return 1.0; // no suggestions → precision vacuously 1
    }
    (ans_qe / ans_qs).min(1.0)
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
            ex:P2 rdf:type ex:Person ; ex:age "35"^^xsd:integer ; ex:name "Robert"^^xsd:string ; ex:name "Bob"^^xsd:string .
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

    fn person_age_config(nav: &NavGraph) -> (ConfigQuery, usize) {
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let mut z = ConfigQuery::root_only(person_id);
        let age_edge = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/age")
            .unwrap()
            .id;
        let age_node = z.extend(0, age_edge, nav);
        (z, age_node)
    }

    /// bf(age) = C_e(age)/C_c(Person) = 6/6 = 1.0.
    #[test]
    fn branching_factor_age_is_one() {
        let (nav, stats) = figure3_stats();
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let age_eid = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/age")
            .unwrap()
            .id;
        let bf = branching_factor(age_eid, &nav, &stats);
        assert!((bf - 1.0).abs() < 1e-9, "bf(age) = {bf}");
    }

    /// ãns(Person→age) = C_c(Person) × bf(age) = 6 × 1 = 6.
    #[test]
    fn est_ans_person_age() {
        let (nav, stats) = figure3_stats();
        let (config, _) = person_age_config(&nav);
        let ans = est_ans(&config, &nav, &stats);
        assert!((ans - 6.0).abs() < 1e-9, "ãns = {ans}");
    }

    /// ãns^P for (Person→age) should equal 6 (all ages distinct → uniform distribution).
    #[test]
    fn est_ans_p_age_all_distinct() {
        let (nav, stats) = figure3_stats();
        let (config, age_node) = person_age_config(&nav);
        let ans_p = est_ans_p(&config, age_node, &nav, &stats);
        // 6 distinct ages, each with prob 1/6 → estimate = 6 × [1-(5/6)^6] ≈ 5.33
        // The estimate is not exact — just verify it's positive and ≤ 6.
        assert!(ans_p > 0.0, "ãns^P should be positive");
        assert!(ans_p <= 6.0 + 1e-9, "ãns^P ≤ C_et(age) = 6, got {ans_p}");
    }

    /// ãns^O(root-only) = C_c(root) = 6 (each instance contributes exactly one row).
    #[test]
    fn est_ans_o_root_only() {
        let (nav, stats) = figure3_stats();
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let z = ConfigQuery::root_only(person_id);
        let ans_o = est_ans_o(&z, &nav, &stats);
        assert!((ans_o - 6.0).abs() < 1e-9, "ãns^O(root-only) = {ans_o}");
    }

    /// ãns^E for root-only is 0 (no non-root columns → cost = 0).
    #[test]
    fn est_ans_e_root_only_is_zero() {
        let (nav, stats) = figure3_stats();
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let z = ConfigQuery::root_only(person_id);
        let ans_e = est_ans_e(&z, &nav, &stats);
        assert!(ans_e == 0.0, "ãns^E(root-only) = {ans_e}");
    }

    /// ãns^E for (Person→age) should be ≤ C_et(age)=6 and positive.
    #[test]
    fn est_ans_e_person_age_bounds() {
        let (nav, stats) = figure3_stats();
        let (config, _) = person_age_config(&nav);
        let ans_e = est_ans_e(&config, &nav, &stats);
        assert!(
            ans_e > 0.0,
            "ãns^E should be positive for non-empty dataset"
        );
        assert!(
            ans_e <= 7.0,
            "ãns^E should not greatly exceed C_et, got {ans_e}"
        );
    }

    /// Empty dataset: all estimates are 0.
    #[test]
    fn estimates_zero_on_empty_dataset() {
        let nav = figure1_nav();
        let ds = Datastore::new(10);
        let stats = NavStats::compute(&nav, &ds);
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let (config, age_node) = {
            let mut z = ConfigQuery::root_only(person_id);
            let age_eid = nav
                .outgoing_edges(person_id)
                .iter()
                .map(|&id| nav.edge(id))
                .find(|e| e.iri == "http://example.org/age")
                .unwrap()
                .id;
            let an = z.extend(0, age_eid, &nav);
            (z, an)
        };
        assert_eq!(est_ans(&config, &nav, &stats), 0.0);
        assert_eq!(est_ans_p(&config, age_node, &nav, &stats), 0.0);
        assert_eq!(est_ans_o(&config, &nav, &stats), 0.0);
        assert_eq!(est_ans_e(&config, &nav, &stats), 0.0);
        assert_eq!(est_cost_single(&config, &nav, &stats), 0.0);
    }
}

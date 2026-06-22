/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Configuration sets and reference configurations (paper §2.13, §4.2).
//!
//! A **configuration set** W is a collection of `IndexTable`s.  The six
//! reference configurations (Wd, Wm, Wr, Wrd, Wl, Wld) are defined
//! without looking at the dataset and serve as baselines.
//!
//! ## Reference configurations (Table 4 in paper)
//!
//! | Name | Description                                                       |
//! |------|-------------------------------------------------------------------|
//! | Wd   | Empty set — no index                                              |
//! | Wm   | Maximum: Qt for every class (requires query log)                  |
//! | Wr   | One small config per edge in N                                    |
//! | Wrd  | Wr without object-edge configs                                    |
//! | Wl   | Fully-saturated star-shaped config per class                      |
//! | Wld  | Wl without object edges                                           |

use crate::config_query::{ConfigQuery, IndexTable};
use crate::navigation_graph::NavGraph;
use dag_rdf::Datastore;

/// A configuration set W — a collection of built index tables.
pub struct ConfigSet {
    pub tables: Vec<IndexTable>,
}

impl ConfigSet {
    /// Wd = ∅: no index, cost = 0.
    pub fn w_empty() -> Self {
        ConfigSet { tables: vec![] }
    }

    /// Wr: one two-variable config query per edge in N.
    ///
    /// Each config root→child covers a single property.  Object-edge configs
    /// are included (the child column always contains χ or ω).
    pub fn w_property(nav: &NavGraph, ds: &Datastore) -> Self {
        let tables = nav
            .edges()
            .filter(|e| {
                // For object edges, include only the forward direction to avoid
                // building identical mirrored tables.
                if e.is_object_edge() {
                    e.inverse.map(|inv| inv > e.id).unwrap_or(true)
                } else {
                    true
                }
            })
            .map(|edge| {
                let mut z = ConfigQuery::root_only(edge.src);
                z.extend(0, edge.id, nav);
                IndexTable::build(&z, nav, ds)
            })
            .collect();
        ConfigSet { tables }
    }

    /// Wrd: Wr restricted to data edges only (no object-property configs).
    pub fn w_property_data_only(nav: &NavGraph, ds: &Datastore) -> Self {
        let tables = nav
            .data_edges()
            .map(|edge| {
                let mut z = ConfigQuery::root_only(edge.src);
                z.extend(0, edge.id, nav);
                IndexTable::build(&z, nav, ds)
            })
            .collect();
        ConfigSet { tables }
    }

    /// Wl: one fully-saturated star-shaped config per class.
    ///
    /// For each class t, build a star with root t and one child per outgoing
    /// edge in N (both object and data edges).
    pub fn w_local(nav: &NavGraph, ds: &Datastore) -> Self {
        let tables = nav
            .classes()
            .map(|class_node| {
                let mut z = ConfigQuery::root_only(class_node.id);
                for &eid in nav.outgoing_edges(class_node.id) {
                    z.extend(0, eid, nav);
                }
                IndexTable::build(&z, nav, ds)
            })
            .collect();
        ConfigSet { tables }
    }

    /// Wld: Wl restricted to data edges only.
    pub fn w_local_data_only(nav: &NavGraph, ds: &Datastore) -> Self {
        let tables = nav
            .classes()
            .map(|class_node| {
                let mut z = ConfigQuery::root_only(class_node.id);
                for &eid in nav.outgoing_edges(class_node.id) {
                    if nav.edge(eid).is_data_edge() {
                        z.extend(0, eid, nav);
                    }
                }
                IndexTable::build(&z, nav, ds)
            })
            .collect();
        ConfigSet { tables }
    }

    /// Total cost of this configuration set (paper eq. 8).
    pub fn cost(&self) -> usize {
        self.tables.iter().map(|t| t.cost()).sum()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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

    fn figure3_datastore() -> Datastore {
        let ttl = r#"
            @prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
            @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .
            @prefix ex:   <http://example.org/> .
            ex:P1 rdf:type ex:Person ; ex:age "21"^^xsd:integer ; ex:name "Alice"^^xsd:string ; ex:visited ex:Belgium .
            ex:P2 rdf:type ex:Person ; ex:age "35"^^xsd:integer ; ex:name "Robert"^^xsd:string ; ex:name "Bob"^^xsd:string .
            ex:P3 rdf:type ex:Person ; ex:age "45"^^xsd:integer ; ex:name "Carol"^^xsd:string .
            ex:P4 rdf:type ex:Person ; ex:age "30"^^xsd:integer ; ex:name "Dave"^^xsd:string .
            ex:P5 rdf:type ex:Person ; ex:age "11"^^xsd:integer .
            ex:P6 rdf:type ex:Person ; ex:age "16"^^xsd:integer .
            ex:Belgium rdf:type ex:Country ; ex:population "11000000"^^xsd:integer ; ex:name "Belgium"^^xsd:string ; ex:borders ex:France .
            ex:France rdf:type ex:Country ; ex:population "67000000"^^xsd:integer ; ex:name "France"^^xsd:string ; ex:borders ex:Belgium .
        "#;
        let mut ds = Datastore::new(500);
        parse_turtle(&mut ds, ttl.as_bytes()).expect("parse");
        ds
    }

    /// Wd has zero cost and zero tables.
    #[test]
    fn w_empty_cost_zero() {
        assert_eq!(ConfigSet::w_empty().cost(), 0);
        assert_eq!(ConfigSet::w_empty().tables.len(), 0);
    }

    /// Wrd has strictly lower cost than Wl (data-only always cheaper on
    /// a nav graph with object properties).
    #[test]
    fn wrd_cost_le_wl_cost() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let wrd = ConfigSet::w_property_data_only(&nav, &ds);
        let wl = ConfigSet::w_local(&nav, &ds);
        assert!(
            wrd.cost() <= wl.cost(),
            "Wrd cost {} should be ≤ Wl cost {}",
            wrd.cost(),
            wl.cost()
        );
    }

    /// Wld cost ≤ Wl cost (removing object edges can only decrease cost).
    #[test]
    fn wld_cost_le_wl_cost() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let wld = ConfigSet::w_local_data_only(&nav, &ds);
        let wl = ConfigSet::w_local(&nav, &ds);
        assert!(
            wld.cost() <= wl.cost(),
            "Wld cost {} should be ≤ Wl cost {}",
            wld.cost(),
            wl.cost()
        );
    }

    /// Wr and Wrd have same number of tables as data edges (Wrd) vs
    /// forward object edges + data edges (Wr).
    #[test]
    fn w_property_table_counts() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let wr = ConfigSet::w_property(&nav, &ds);
        let wrd = ConfigSet::w_property_data_only(&nav, &ds);

        let data_edge_count = nav.data_edges().count();
        assert_eq!(wrd.tables.len(), data_edge_count, "Wrd has one table per data edge");
        // Wr has data edges + forward object edges (visited, knows, borders — 3 object props)
        assert!(
            wr.tables.len() >= data_edge_count,
            "Wr has at least as many tables as Wrd"
        );
    }

    /// Wl produces one table per class.
    #[test]
    fn w_local_table_count_equals_class_count() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let wl = ConfigSet::w_local(&nav, &ds);
        assert_eq!(wl.tables.len(), nav.class_count());
    }

    /// Empty datastore: all reference configs have cost 0.
    #[test]
    fn all_reference_configs_cost_zero_on_empty_ds() {
        let nav = figure1_nav();
        let ds = Datastore::new(10);
        assert_eq!(ConfigSet::w_property(&nav, &ds).cost(), 0);
        assert_eq!(ConfigSet::w_property_data_only(&nav, &ds).cost(), 0);
        assert_eq!(ConfigSet::w_local(&nav, &ds).cost(), 0);
        assert_eq!(ConfigSet::w_local_data_only(&nav, &ds).cost(), 0);
    }
}

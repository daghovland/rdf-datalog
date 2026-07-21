/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Configuration query Z and index table (paper §2.9–2.12).
//!
//! A **configuration query** Z is a simple, filterless, rooted, tree-shaped
//! query over the navigation graph N.  Its index `ans^E(Z, D)` is a compressed
//! table that covers every subquery of Z efficiently.
//!
//! # Index construction (paper §2.11)
//!
//! 1. Generate SPARQL with every non-root branch wrapped in `OPTIONAL { }`.
//! 2. Execute over the datastore → `ans^O(Z, D)`.
//! 3. Replace every instance (non-literal) value with the existence symbol χ.
//! 4. Remove assignments that are sub-functions of another: if φ(v) = φ′(v) or
//!    φ(v) = ω for every column, remove φ and keep φ′.
//! 5. Drop the root column (always χ) → store |V(Z)| − 1 columns.
//!
//! cost(Z, D) = (|V(Z)| − 1) × |ans^E(Z, D)|

use crate::navigation_graph::{NavEdgeId, NavGraph, NavNodeId};
use dag_rdf::{Datastore, GraphElement, RdfResource};
use std::collections::HashMap;

/// A node in the configuration-query tree.
///
/// The tree is stored as a flat `Vec<CqNode>` with parent indices.
/// The root is always index 0.
#[derive(Debug, Clone)]
pub struct CqNode {
    /// Nav-graph class (or datatype for leaf nodes) of this variable.
    pub nav_node: NavNodeId,
    /// Edge in N that connects parent → this node.  `None` for the root.
    pub parent_edge: Option<NavEdgeId>,
    /// Index of parent node in the owning `ConfigQuery::nodes` vec.  `None` for root.
    pub parent: Option<usize>,
    /// Indices of children in `ConfigQuery::nodes`.
    pub children: Vec<usize>,
}

/// A simple, filterless, rooted, tree-shaped query over N (paper §2.9).
#[derive(Debug, Clone)]
pub struct ConfigQuery {
    /// All nodes; `nodes[0]` is the root.
    pub nodes: Vec<CqNode>,
}

impl ConfigQuery {
    /// Create a single-variable root-only configuration query for class `root_class`.
    pub fn root_only(root_class: NavNodeId) -> Self {
        ConfigQuery {
            nodes: vec![CqNode {
                nav_node: root_class,
                parent_edge: None,
                parent: None,
                children: vec![],
            }],
        }
    }

    /// Add a child variable reached via `edge` (which must leave `parent_idx`'s class).
    /// Returns the index of the new node.
    pub fn extend(&mut self, parent_idx: usize, edge: NavEdgeId, nav: &NavGraph) -> usize {
        let child_nav_node = nav.edge(edge).tgt;
        let child_idx = self.nodes.len();
        self.nodes.push(CqNode {
            nav_node: child_nav_node,
            parent_edge: Some(edge),
            parent: Some(parent_idx),
            children: vec![],
        });
        self.nodes[parent_idx].children.push(child_idx);
        child_idx
    }

    /// Number of variables (vertices) in Z.
    pub fn variable_count(&self) -> usize {
        self.nodes.len()
    }

    /// Emit the OPTIONAL-wrapped SPARQL query whose results are `ans^O(Z, D)`.
    ///
    /// The query binds one SPARQL variable per node (`?v0` for the root,
    /// `?v1`, `?v2`, … for children).  Every non-root edge is wrapped in
    /// `OPTIONAL { }` so that nodes without matching data produce `UNDEF`
    /// (which we translate to ω in the index rows).
    pub fn to_sparql_optional(&self, nav: &NavGraph) -> String {
        let root_class_iri = &nav.node(self.nodes[0].nav_node).iri;
        let mut out = format!("SELECT * WHERE {{\n  ?v0 a <{}> .\n", root_class_iri);
        self.write_node_sparql(0, nav, &mut out);
        out.push_str("}\n");
        out
    }

    fn write_node_sparql(&self, node_idx: usize, nav: &NavGraph, out: &mut String) {
        let node = &self.nodes[node_idx];
        for &child_idx in &node.children {
            let child = &self.nodes[child_idx];
            let edge = nav.edge(child.parent_edge.unwrap());
            let pred_iri = &edge.iri;
            out.push_str(&format!(
                "  OPTIONAL {{\n    ?v{} <{}> ?v{} .\n",
                node_idx, pred_iri, child_idx
            ));
            self.write_node_sparql(child_idx, nav, out);
            out.push_str("  }\n");
        }
    }

    /// All subtrees of Z as independent `ConfigQuery` values (paper §2.9).
    ///
    /// A subtree is any connected sub-graph that includes the root.  For a
    /// tree-shaped query this corresponds to choosing, for each node, whether
    /// to include its subtree or not.
    pub fn subtrees(&self) -> Vec<ConfigQuery> {
        let mut result = vec![];
        self.collect_subtrees(0, &mut result);
        result
    }

    fn collect_subtrees(&self, node_idx: usize, result: &mut Vec<ConfigQuery>) {
        // Collect all combinations of child inclusions.
        let node = &self.nodes[node_idx];
        let child_count = node.children.len();
        // Each child can be included or excluded → 2^child_count subsets.
        for mask in 0u32..(1u32 << child_count) {
            let mut sub = ConfigQuery {
                nodes: vec![CqNode {
                    nav_node: node.nav_node,
                    parent_edge: node.parent_edge,
                    parent: None,
                    children: vec![],
                }],
            };
            for (i, &child_idx) in node.children.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    self.attach_subtree(child_idx, 0, &mut sub);
                }
            }
            result.push(sub);
        }
    }

    fn attach_subtree(&self, src_idx: usize, new_parent: usize, dst: &mut ConfigQuery) {
        let src_node = &self.nodes[src_idx];
        let new_idx = dst.nodes.len();
        dst.nodes.push(CqNode {
            nav_node: src_node.nav_node,
            parent_edge: src_node.parent_edge,
            parent: Some(new_parent),
            children: vec![],
        });
        dst.nodes[new_parent].children.push(new_idx);
        for &child in &src_node.children {
            self.attach_subtree(child, new_idx, dst);
        }
    }
}

/// The special existence symbol χ: "an instance exists here, but its URI is not stored."
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IndexCell {
    /// χ — an instance (non-literal) was found.
    Exists,
    /// ω — no value (OPTIONAL branch produced UNDEF).
    Null,
    /// A concrete data value (literal).
    Value(GraphElement),
}

/// One row in the index table `ans^E(Z, D)`.
///
/// Has `|V(Z)| − 1` cells (root column omitted — always χ).
/// Indexed by non-root node index (1, 2, …, |V(Z)| − 1).
pub type IndexRow = Vec<IndexCell>;

/// The pre-computed index table for one configuration query Z over dataset D.
///
/// Stores `ans^E(Z, D)`: the minimal set of assignments after replacing
/// instances with χ and removing sub-functions.
pub struct IndexTable {
    pub config: ConfigQuery,
    pub rows: Vec<IndexRow>,
}

impl IndexTable {
    /// Compute `ans^E(Z, D)` by executing the OPTIONAL-wrapped SPARQL and
    /// applying the compression steps from paper §2.11.
    pub fn build(config: &ConfigQuery, nav: &NavGraph, ds: &Datastore) -> Self {
        let n = config.variable_count();

        // ── Execute OPTIONAL SPARQL → raw bindings ───────────────────────────
        let sparql = config.to_sparql_optional(nav);
        let rows = if n == 1 {
            // Root-only: no non-root columns, no SPARQL needed.
            // ans^E has a single row with zero columns if any instance of root
            // class exists — but cost = 0 regardless, so we return empty.
            return IndexTable {
                config: config.clone(),
                rows: vec![],
            };
        } else {
            execute_optional_sparql(&sparql, ds, n)
        };

        // ── Compress: replace instances with χ ───────────────────────────────
        let compressed: Vec<IndexRow> = rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| match cell {
                        IndexCell::Value(GraphElement::NodeOrEdge(RdfResource::Iri(_))) => {
                            IndexCell::Exists
                        }
                        IndexCell::Value(GraphElement::NodeOrEdge(
                            RdfResource::AnonymousBlankNode(_),
                        )) => IndexCell::Exists,
                        other => other,
                    })
                    .collect()
            })
            .collect();

        // ── Remove sub-functions ─────────────────────────────────────────────
        let minimal = remove_subfunctions(compressed);

        IndexTable {
            config: config.clone(),
            rows: minimal,
        }
    }

    /// cost(Z, D) = (|V(Z)| − 1) × |ans^E(Z, D)|  (paper eq. 6).
    pub fn cost(&self) -> usize {
        let cols = self.config.variable_count().saturating_sub(1);
        cols * self.rows.len()
    }

    /// Look up productive values for a data variable at column index `col_idx`
    /// (1-based non-root node index), given that certain filter cells must match.
    ///
    /// Returns the set of data values suggested by this index for the given
    /// partial query state.
    pub fn lookup_values(
        &self,
        col_idx: usize,
        filters: &HashMap<usize, IndexCell>,
    ) -> Vec<GraphElement> {
        let mut result = Vec::new();
        for row in &self.rows {
            // Check all filter columns match.
            if filters.iter().all(|(&fc, fv)| {
                let cell = row.get(fc.saturating_sub(1)).unwrap_or(&IndexCell::Null);
                cell == fv || *fv == IndexCell::Exists && matches!(cell, IndexCell::Exists)
            }) && let Some(IndexCell::Value(elem)) = row.get(col_idx.saturating_sub(1))
            {
                result.push(elem.clone());
            }
        }
        result
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Execute the OPTIONAL-wrapped SPARQL query and return raw index cells.
///
/// Each row has `n − 1` cells (one per non-root variable `?v1..?v_{n-1}`).
/// Unbound (OPTIONAL miss) → `IndexCell::Null`.
/// Literal → `IndexCell::Value(GraphElement::GraphLiteral(...))`.
/// IRI/blank → `IndexCell::Value(GraphElement::NodeOrEdge(...))`.
fn execute_optional_sparql(sparql: &str, ds: &Datastore, n: usize) -> Vec<IndexRow> {
    use sparql_parser::{NetworkPolicy, ParserContext, QueryResult, execute, parse_query};
    use std::collections::HashMap as HM;

    let mut ctx = ParserContext {
        prefixes: HM::new(),
        base: None,
    };
    let (_, query) = match parse_query(sparql, &mut ctx) {
        Ok(q) => q,
        Err(_) => return vec![],
    };
    let select = match execute(&query, ds, NetworkPolicy::Deny) {
        Ok(QueryResult::Select(r)) => r,
        _ => return vec![],
    };

    select
        .rows
        .into_iter()
        .map(|binding| {
            (1..n)
                .map(|i| {
                    let var_name = format!("v{i}");
                    match binding.get(&var_name) {
                        Some(elem) => IndexCell::Value(elem.clone()),
                        None => IndexCell::Null,
                    }
                })
                .collect()
        })
        .collect()
}

/// Remove rows that are sub-functions of another row (paper §2.11).
///
/// Row φ is a sub-function of φ′ when φ(v) = φ′(v) or φ(v) = ω for every
/// column v.  Such rows are dominated by φ′ and can be discarded.
fn remove_subfunctions(mut rows: Vec<IndexRow>) -> Vec<IndexRow> {
    // Deduplicate first.
    rows.sort_unstable();
    rows.dedup();

    let n = rows.len();
    let mut keep = vec![true; n];

    for i in 0..n {
        if !keep[i] {
            continue;
        }
        for j in 0..n {
            if i == j || !keep[j] {
                continue;
            }
            if is_subfunction(&rows[i], &rows[j]) {
                keep[i] = false;
                break;
            }
        }
    }

    rows.into_iter()
        .zip(keep)
        .filter_map(|(r, k)| if k { Some(r) } else { None })
        .collect()
}

/// True when `phi` is a sub-function of `phi_prime`:
/// every cell of `phi` equals the corresponding cell of `phi_prime`, or is ω.
fn is_subfunction(phi: &IndexRow, phi_prime: &IndexRow) -> bool {
    if phi == phi_prime {
        return false; // identical rows are not strict sub-functions
    }
    phi.iter()
        .zip(phi_prime.iter())
        .all(|(a, b)| a == &IndexCell::Null || a == b)
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

            ex:P1 rdf:type ex:Person ;
                  ex:age "21"^^xsd:integer ;
                  ex:name "Alice"^^xsd:string ;
                  ex:visited ex:Belgium .

            ex:P2 rdf:type ex:Person ;
                  ex:age "35"^^xsd:integer ;
                  ex:name "Robert"^^xsd:string ;
                  ex:name "Bob"^^xsd:string .

            ex:P3 rdf:type ex:Person ;
                  ex:age "45"^^xsd:integer ;
                  ex:name "Carol"^^xsd:string .

            ex:P4 rdf:type ex:Person ;
                  ex:age "30"^^xsd:integer ;
                  ex:name "Dave"^^xsd:string .

            ex:P5 rdf:type ex:Person ;
                  ex:age "11"^^xsd:integer .

            ex:P6 rdf:type ex:Person ;
                  ex:age "16"^^xsd:integer .

            ex:Belgium rdf:type ex:Country ;
                       ex:population "11000000"^^xsd:integer ;
                       ex:name "Belgium"^^xsd:string ;
                       ex:borders ex:France .

            ex:France rdf:type ex:Country ;
                      ex:population "67000000"^^xsd:integer ;
                      ex:name "France"^^xsd:string ;
                      ex:borders ex:Belgium .
        "#;
        let mut ds = Datastore::new(500);
        parse_turtle(&mut ds, ttl.as_bytes()).expect("figure3 parse");
        ds
    }

    /// Figure 5 / Table 3 from the paper: Z2 has root Person → (age, name, visited → Country).
    ///
    /// Expected: 7 rows, cost = 3 × 7 = 21.
    fn z2_config(nav: &NavGraph) -> ConfigQuery {
        // Z2: root=Person, children: age(Integer), name(String), visited→Country
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let mut z = ConfigQuery::root_only(person_id);

        let age_edge = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/age")
            .unwrap()
            .id;
        let name_edge = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/name")
            .unwrap()
            .id;
        let visited_edge = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/visited")
            .unwrap()
            .id;

        z.extend(0, age_edge, nav);
        z.extend(0, name_edge, nav);
        z.extend(0, visited_edge, nav);
        z
    }

    /// A root-only config query has no columns and cost = 0.
    #[test]
    fn index_root_only_cost_zero() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let z = ConfigQuery::root_only(person_id);
        let idx = IndexTable::build(&z, &nav, &ds);
        assert_eq!(idx.cost(), 0);
        assert!(idx.rows.is_empty());
    }

    /// Z2 from the paper: 7 rows, cost = 21.
    ///
    /// Table 3 (paper §2.12) lists 7 rows for (o'1=χ, d'1, o'2, d'2) where
    /// d'1 is the name and d'2 is the age (or vice versa, depending on column order).
    #[test]
    fn index_z2_cost() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let z = z2_config(&nav);
        let idx = IndexTable::build(&z, &nav, &ds);
        // 3 non-root columns × |rows| = 21 → |rows| = 7
        assert_eq!(idx.config.variable_count(), 4, "Z2 has 4 variables");
        assert_eq!(idx.rows.len(), 7, "ans^E(Z2,D) has 7 rows");
        assert_eq!(idx.cost(), 21, "cost(Z2,D) = 21");
    }

    /// Every row of Z2 has exactly 3 cells (non-root columns).
    #[test]
    fn index_row_width_matches_variable_count() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let z = z2_config(&nav);
        let idx = IndexTable::build(&z, &nav, &ds);
        for row in &idx.rows {
            assert_eq!(row.len(), 3);
        }
    }

    /// All instances in the result are replaced by χ.
    #[test]
    fn index_instances_replaced_by_chi() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let z = z2_config(&nav);
        let idx = IndexTable::build(&z, &nav, &ds);
        // Column 2 (0-based) is `visited` → a Country (object) → should be χ or ω
        for row in &idx.rows {
            let visited_cell = &row[2];
            assert!(
                matches!(visited_cell, IndexCell::Exists | IndexCell::Null),
                "visited column should be χ or ω, got {visited_cell:?}"
            );
        }
    }

    /// `is_subfunction` correctly identifies dominated rows.
    #[test]
    fn subfunction_detection() {
        let full = vec![
            IndexCell::Exists,
            IndexCell::Value(GraphElement::GraphLiteral(
                dag_rdf::RdfLiteral::LiteralString("Alice".to_string()),
            )),
        ];
        let partial = vec![IndexCell::Null, IndexCell::Null];
        let other = vec![
            IndexCell::Exists,
            IndexCell::Value(GraphElement::GraphLiteral(
                dag_rdf::RdfLiteral::LiteralString("Bob".to_string()),
            )),
        ];

        assert!(
            is_subfunction(&partial, &full),
            "all-null is subfunction of full"
        );
        assert!(
            !is_subfunction(&full, &partial),
            "full is not subfunction of all-null"
        );
        assert!(
            !is_subfunction(&full, &other),
            "different values — not subfunction"
        );
    }
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Basic dataset statistics used by the cost/precision estimators (paper §4.4.1–4.4.2).
//!
//! For a navigation graph N and dataset D, we collect four scalar counts per edge
//! and one histogram per data edge:
//!
//! | Symbol   | Meaning                                                       |
//! |----------|---------------------------------------------------------------|
//! | C_c(t)   | Number of distinct instances in D typed to class t            |
//! | C_e(e)   | Number of triples in D matching navigation edge e             |
//! | C_es(e)  | Number of distinct source instances of edge e in D            |
//! | C_et(e)  | Number of distinct target values/instances of edge e in D     |
//! | H_e(u)   | Fraction of C_e(e) answers where the target equals u          |

use crate::navigation_graph::{NavEdgeId, NavGraph, NavNodeId};
use dag_rdf::{Datastore, GraphElement, GraphElementId, IriReference, RdfLiteral, RdfResource};
use ingress::RDF_TYPE;
use std::collections::{HashMap, HashSet};

/// The four scalar counts per class/edge (paper §4.4.1).
#[derive(Debug, Default, Clone)]
pub struct BasicCounts {
    /// C_c(t): distinct instances typed to nav class t.
    pub class_count: HashMap<NavNodeId, u64>,
    /// C_e(e): triples matching nav edge e (same label, compatible source/target types).
    pub edge_count: HashMap<NavEdgeId, u64>,
    /// C_es(e): distinct source instances of edge e.
    pub edge_src_count: HashMap<NavEdgeId, u64>,
    /// C_et(e): distinct target values/instances of edge e.
    pub edge_tgt_count: HashMap<NavEdgeId, u64>,
}

/// Edge target distribution H_e (paper §4.4.2).
///
/// Maps each data value `u` to the probability that a random edge-e answer has
/// target `u`.  Only defined for data edges; values sum to 1.0.
pub type Histogram = HashMap<GraphElement, f64>;

/// Combined statistics for one (NavGraph, Datastore) pair.
#[derive(Debug, Default, Clone)]
pub struct NavStats {
    pub counts: BasicCounts,
    /// Histograms H_e, keyed by data-edge id.  Object edges have no histogram.
    pub histograms: HashMap<NavEdgeId, Histogram>,
}

impl NavStats {
    /// Compute all basic counts and histograms in two passes over the datastore.
    ///
    /// Pass 1: build the typing function T_D (instance → nav class set) from
    ///         `rdf:type` triples.
    /// Pass 2: for each triple, match it against every navigation edge that shares
    ///         the same property label, accumulate counts.
    pub fn compute(nav: &NavGraph, ds: &Datastore) -> Self {
        // ── Pass 1: typing function T_D ──────────────────────────────────────
        let typing = build_typing(nav, ds);

        // ── C_c: count instances per nav class ──────────────────────────────
        let mut class_count: HashMap<NavNodeId, u64> = HashMap::new();
        for classes in typing.values() {
            for &class_id in classes {
                *class_count.entry(class_id).or_insert(0) += 1;
            }
        }

        // ── Build a label → edge-id index for fast triple matching ───────────
        // Multiple nav edges can share the same property label (e.g. when
        // the same property connects different classes).
        let mut edges_by_label: HashMap<String, Vec<NavEdgeId>> = HashMap::new();
        for edge in nav.edges() {
            edges_by_label
                .entry(edge.iri.clone())
                .or_default()
                .push(edge.id);
        }

        // ── Pass 2: match triples against navigation edges ───────────────────
        let mut edge_count: HashMap<NavEdgeId, u64> = HashMap::new();
        let mut src_sets: HashMap<NavEdgeId, HashSet<GraphElementId>> = HashMap::new();
        let mut tgt_sets: HashMap<NavEdgeId, HashSet<GraphElement>> = HashMap::new();
        let mut raw_hist: HashMap<NavEdgeId, HashMap<GraphElement, u64>> = HashMap::new();

        for quad in &ds.named_graphs.quad_list {
            let pred_iri = match iri_of(ds, quad.predicate) {
                Some(s) => s,
                None => continue,
            };
            let Some(edge_ids) = edges_by_label.get(pred_iri.as_str()) else {
                continue;
            };

            let obj_elem = ds.resources.get_graph_element(quad.obj).clone();

            for &eid in edge_ids {
                let edge = nav.edge(eid);

                // source must be typed to edge.src
                if !typing
                    .get(&quad.subject)
                    .is_some_and(|c| c.contains(&edge.src))
                {
                    continue;
                }

                // target must match edge.tgt (typed instance or compatible literal)
                let tgt_ok = if edge.is_data_edge() {
                    let dt_iri = &nav.node(edge.tgt).iri;
                    is_literal_of_type(&obj_elem, dt_iri)
                } else {
                    typing
                        .get(&quad.obj)
                        .is_some_and(|c| c.contains(&edge.tgt))
                };
                if !tgt_ok {
                    continue;
                }

                *edge_count.entry(eid).or_insert(0) += 1;
                src_sets.entry(eid).or_default().insert(quad.subject);
                tgt_sets.entry(eid).or_default().insert(obj_elem.clone());

                if edge.is_data_edge() {
                    *raw_hist
                        .entry(eid)
                        .or_default()
                        .entry(obj_elem.clone())
                        .or_insert(0) += 1;
                }
            }
        }

        let edge_src_count = src_sets
            .into_iter()
            .map(|(id, s)| (id, s.len() as u64))
            .collect();
        let edge_tgt_count = tgt_sets
            .into_iter()
            .map(|(id, s)| (id, s.len() as u64))
            .collect();

        let histograms = raw_hist
            .into_iter()
            .map(|(eid, counts)| {
                let total: u64 = counts.values().sum();
                let hist = counts
                    .into_iter()
                    .map(|(v, c)| (v, c as f64 / total as f64))
                    .collect();
                (eid, hist)
            })
            .collect();

        NavStats {
            counts: BasicCounts {
                class_count,
                edge_count,
                edge_src_count,
                edge_tgt_count,
            },
            histograms,
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Build typing function T_D: instance GraphElementId → set of NavNodeIds.
fn build_typing(nav: &NavGraph, ds: &Datastore) -> HashMap<GraphElementId, HashSet<NavNodeId>> {
    let mut typing: HashMap<GraphElementId, HashSet<NavNodeId>> = HashMap::new();
    let Some(rdf_type_id) = lookup_iri(ds, RDF_TYPE) else {
        return typing;
    };
    for triple in ds.get_triples_with_predicate(rdf_type_id) {
        if let Some(class_iri) = iri_of(ds, triple.obj)
            && let Some(nav_id) = nav.node_by_iri(&class_iri)
        {
            typing.entry(triple.subject).or_default().insert(nav_id);
        }
    }
    typing
}

fn lookup_iri(ds: &Datastore, iri: &str) -> Option<GraphElementId> {
    let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_owned())));
    ds.resources.resource_map.get(&elem).copied()
}

fn iri_of(ds: &Datastore, id: GraphElementId) -> Option<String> {
    match ds.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s))) => Some(s.clone()),
        _ => None,
    }
}

/// True when `elem` is a literal whose effective datatype matches `datatype_iri`.
fn is_literal_of_type(elem: &GraphElement, datatype_iri: &str) -> bool {
    const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
    const RDF_LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";
    match elem {
        GraphElement::GraphLiteral(lit) => match lit {
            RdfLiteral::TypedLiteral { type_iri, .. } => type_iri.0 == datatype_iri,
            RdfLiteral::LiteralString(_) => datatype_iri == XSD_STRING,
            RdfLiteral::LangLiteral { .. } => datatype_iri == RDF_LANG_STRING,
            // Native-typed literals (BooleanLiteral, IntegerLiteral, etc.) are not
            // produced by the Turtle parser — those come from programmatic insertion.
            // We don't match them against nav-graph datatypes here.
            _ => false,
        },
        _ => false,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation_graph::NavGraph;
    use turtle::parse_turtle;

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Build the paper's Figure 1 navigation graph.
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

    /// Load the paper's Figure 3 dataset (6 persons, 2 countries).
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
                  ex:name "Bob"^^xsd:string ;
                  ex:visited ex:Belgium .

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
        parse_turtle(&mut ds, ttl.as_bytes()).expect("figure3 turtle parse");
        ds
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// C_c(Person) = 6 persons, C_c(Country) = 2 countries.
    #[test]
    fn basic_counts_class_count() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let stats = NavStats::compute(&nav, &ds);

        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let country_id = nav.node_by_iri("http://example.org/Country").unwrap();

        assert_eq!(stats.counts.class_count[&person_id], 6, "C_c(Person)");
        assert_eq!(stats.counts.class_count[&country_id], 2, "C_c(Country)");
    }

    /// C_e, C_es, C_et for the `age` data edge.
    ///
    /// 6 persons each have one age → C_e = 6, C_es = 6.
    /// Ages are 21, 35, 45, 30, 11, 16 — all distinct → C_et = 6.
    #[test]
    fn basic_counts_age_edge() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let stats = NavStats::compute(&nav, &ds);

        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let age_edge = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/age")
            .expect("age edge");

        assert_eq!(stats.counts.edge_count[&age_edge.id], 6, "C_e(age)");
        assert_eq!(stats.counts.edge_src_count[&age_edge.id], 6, "C_es(age)");
        assert_eq!(stats.counts.edge_tgt_count[&age_edge.id], 6, "C_et(age)");
    }

    /// Person `name` edge: P1, P2 (×2), P3, P4 have names → C_e = 5, C_es = 4, C_et = 5.
    #[test]
    fn basic_counts_person_name_edge() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let stats = NavStats::compute(&nav, &ds);

        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let name_edge = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/name" && nav.node(e.tgt).iri.contains("string"))
            .expect("person name edge");

        assert_eq!(stats.counts.edge_count[&name_edge.id], 5, "C_e(name/Person)");
        assert_eq!(stats.counts.edge_src_count[&name_edge.id], 4, "C_es(name/Person)");
        assert_eq!(stats.counts.edge_tgt_count[&name_edge.id], 5, "C_et(name/Person)");
    }

    /// Object edge `visited` (Person → Country): P1 and P2 visited Belgium → C_e=2, C_es=2, C_et=1.
    #[test]
    fn basic_counts_visited_edge() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let stats = NavStats::compute(&nav, &ds);

        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let visited_edge = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/visited")
            .expect("visited edge");

        assert_eq!(stats.counts.edge_count[&visited_edge.id], 2, "C_e(visited)");
        assert_eq!(stats.counts.edge_src_count[&visited_edge.id], 2, "C_es(visited)");
        assert_eq!(stats.counts.edge_tgt_count[&visited_edge.id], 1, "C_et(visited)");
    }

    /// The `age` histogram sums to 1.0 and each of the 6 ages has probability 1/6.
    #[test]
    fn histogram_age_sums_to_one() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let stats = NavStats::compute(&nav, &ds);

        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let age_eid = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/age")
            .expect("age edge")
            .id;

        let hist = stats.histograms.get(&age_eid).expect("age histogram");
        assert_eq!(hist.len(), 6, "6 distinct ages");

        let total: f64 = hist.values().sum();
        assert!((total - 1.0).abs() < 1e-9, "histogram sums to 1.0, got {total}");

        for &p in hist.values() {
            assert!((p - 1.0 / 6.0).abs() < 1e-9, "each age has prob 1/6, got {p}");
        }
    }

    /// Object edges have no histogram entry.
    #[test]
    fn histogram_absent_for_object_edges() {
        let nav = figure1_nav();
        let ds = figure3_datastore();
        let stats = NavStats::compute(&nav, &ds);

        let person_id = nav.node_by_iri("http://example.org/Person").unwrap();
        let visited_eid = nav
            .outgoing_edges(person_id)
            .iter()
            .map(|&id| nav.edge(id))
            .find(|e| e.iri == "http://example.org/visited")
            .expect("visited edge")
            .id;

        assert!(
            !stats.histograms.contains_key(&visited_eid),
            "object edges have no histogram"
        );
    }

    /// Empty datastore → all counts are zero.
    #[test]
    fn basic_counts_empty_datastore() {
        let nav = figure1_nav();
        let ds = Datastore::new(10);
        let stats = NavStats::compute(&nav, &ds);
        assert!(stats.counts.class_count.is_empty());
        assert!(stats.counts.edge_count.is_empty());
        assert!(stats.histograms.is_empty());
    }
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Detection of cyclic BGP join graphs (issue
//! [#732](https://github.com/daghovland/rdf-datalog/issues/732)).
//!
//! Originally an internal-only regression check in
//! `tests/join_reorder_benchmark.rs` (issue
//! [#720](https://github.com/daghovland/rdf-datalog/issues/720)/
//! [#507](https://github.com/daghovland/rdf-datalog/issues/507)), promoted
//! here so [`crate::execute::execute_with_base`] can warn users at
//! query-plan time: since dagalog's executor only does binary joins (no
//! worst-case-optimal/Leapfrog-Triejoin support — see
//! [#39](https://github.com/daghovland/rdf-datalog/issues/39), parked as
//! exploratory), a genuinely cyclic BGP can hit the `O(n^2)`-intermediate-row
//! ceiling #720 measured. This is purely diagnostic: it never changes a
//! query's results, only whether a `log::warn!` is emitted once per query
//! execution.
//!
//! The detection functions are re-exported (`#[doc(hidden)] pub`, see
//! `crate::lib`) solely so `tests/join_reorder_benchmark.rs` can exercise
//! the one real implementation directly instead of maintaining its own
//! duplicate copy that could silently drift out of sync.

use crate::ast::{QueryComponent, Term, TriplePattern};
use std::collections::{HashMap, HashSet};

/// Does this one BGP's join **hypergraph** contain a cycle, in the sense
/// relevant to worst-case-optimal join literature (no acyclic "join tree"
/// / GYO reduction)?
///
/// Each triple pattern is one *hyperedge* connecting all of its variable
/// terms — **not** a clique of ordinary pairwise edges. That distinction
/// matters: a single pattern with three distinct variables (`?s ?p ?o .`)
/// touches three variables at once, but it is one atom, not a join between
/// atoms — so it must never be reported as cyclic on its own. Modeling it
/// as a pairwise clique (an earlier draft of this check did exactly that)
/// gets this wrong: a 3-variable single pattern looks identical, pairwise,
/// to a genuine 3-pattern triangle join. Standard hypergraph acyclicity
/// (the [Graham/Yu-Özsoyoğlu (GYO) reduction](https://en.wikipedia.org/wiki/GYO_algorithm))
/// avoids this: repeatedly (a) drop a hyperedge that's a subset of
/// another (redundant — joining through it adds nothing), and (b) drop an
/// "ear" vertex that occurs in only one remaining hyperedge (it can't be
/// part of any cycle). The hypergraph is acyclic iff this reduces every
/// hyperedge away to nothing. Constants and `TripleTerm` subjects don't
/// contribute vertices — consistent with `join_ordering::bound_count`'s
/// treatment of what actually drives join selectivity.
#[doc(hidden)]
pub fn bgp_has_cyclic_join_graph(patterns: &[&TriplePattern]) -> bool {
    let mut edges: Vec<HashSet<String>> = patterns
        .iter()
        .map(|p| {
            [&p.subject, &p.predicate, &p.object]
                .into_iter()
                .filter_map(|t| match t {
                    Term::Variable(v) => Some(v.clone()),
                    _ => None,
                })
                .collect::<HashSet<String>>()
        })
        .filter(|vars| !vars.is_empty())
        .collect();

    loop {
        let total_before: usize = edges.iter().map(|e| e.len()).sum();
        let edges_before = edges.len();

        // (a) Drop any hyperedge that's a subset of another (ties broken
        // by index so exact duplicates collapse to one survivor).
        let keep: Vec<bool> = (0..edges.len())
            .map(|i| {
                !(0..edges.len()).any(|j| {
                    i != j
                        && edges[i].is_subset(&edges[j])
                        && (edges[i].len() < edges[j].len() || i < j)
                })
            })
            .collect();
        edges = edges
            .into_iter()
            .zip(keep)
            .filter_map(|(e, k)| k.then_some(e))
            .collect();

        // (b) Drop every "ear" vertex: one that occurs in only one
        // remaining hyperedge. It can't be part of a join cycle.
        let mut vertex_count: HashMap<String, usize> = HashMap::new();
        for e in &edges {
            for v in e {
                *vertex_count.entry(v.clone()).or_insert(0) += 1;
            }
        }
        for e in edges.iter_mut() {
            e.retain(|v| vertex_count[v] != 1);
        }
        edges.retain(|e| !e.is_empty());

        let total_after: usize = edges.iter().map(|e| e.len()).sum();
        if edges.len() == edges_before && total_after == total_before {
            break; // fixpoint: nothing changed this pass
        }
    }

    !edges.is_empty()
}

/// Recursively collect every BGP's pattern list reachable from a component
/// list, descending into every nesting construct (`OPTIONAL`/`UNION`/
/// `MINUS`/`GRAPH`/`SERVICE`/bare `GROUP`/`{ SELECT ... }` subqueries) —
/// each is its own evaluation scope but still a place a cyclic BGP could
/// hide.
///
/// All `BGP` entries that are *direct siblings* within one component list
/// are merged into a single pattern list before being pushed to `out`,
/// rather than pushed as separate entries. The parser splits a single
/// textual `{ ... }` block into multiple adjacent `QueryComponent::BGP`
/// entries whenever a `FILTER`/`BIND` sits between triple patterns (e.g.
/// `?a :e ?b . FILTER(...) ?b :e ?c . ?c :e ?a .` becomes two `BGP`
/// siblings), but those patterns are still joined together at evaluation
/// time — splitting them for this check would hide a real cross-pattern
/// cycle that only closes across the `FILTER`.
#[doc(hidden)]
pub fn collect_bgps<'a>(components: &'a [QueryComponent], out: &mut Vec<Vec<&'a TriplePattern>>) {
    let mut merged: Vec<&'a TriplePattern> = Vec::new();
    for c in components {
        match c {
            QueryComponent::BGP(patterns) => merged.extend(patterns.iter()),
            QueryComponent::Optional(inner)
            | QueryComponent::Minus(inner)
            | QueryComponent::Graph(_, inner)
            | QueryComponent::Service(_, inner, _)
            | QueryComponent::Group(inner) => collect_bgps(inner, out),
            QueryComponent::Union(left, right) => {
                collect_bgps(left, out);
                collect_bgps(right, out);
            }
            QueryComponent::Subquery(q) => collect_bgps(crate::execute::where_clause_of(q), out),
            QueryComponent::PathPattern(..)
            | QueryComponent::Filter(_)
            | QueryComponent::Bind(..)
            | QueryComponent::Values(..) => {}
        }
    }
    if !merged.is_empty() {
        out.push(merged);
    }
}

/// Scan `where_clause` for cyclic BGPs and emit one `log::warn!` per
/// occurrence, linking to [#39](https://github.com/daghovland/rdf-datalog/issues/39).
/// Called once per [`crate::execute::execute_with_base`] call (i.e. once
/// per query execution, not per row or per solution) — negligible next to
/// actual join execution, and never changes query results.
pub(crate) fn warn_on_cyclic_bgps(where_clause: &[QueryComponent]) {
    let mut bgps: Vec<Vec<&TriplePattern>> = Vec::new();
    collect_bgps(where_clause, &mut bgps);
    for patterns in &bgps {
        if bgp_has_cyclic_join_graph(patterns) {
            log::warn!(
                "SPARQL query has a cyclic BGP join graph ({} triple patterns: {}) — \
                 dagalog's executor only supports binary joins with no worst-case-optimal \
                 join strategy for this shape, so this query may hit an O(n^2) \
                 intermediate-row blowup at scale. See \
                 https://github.com/daghovland/rdf-datalog/issues/39",
                patterns.len(),
                patterns
                    .iter()
                    .map(|p| crate::explain::render_triple_pattern(p))
                    .collect::<Vec<_>>()
                    .join(" . ")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Query;
    use crate::{parse_query, NetworkPolicy, ParserContext};
    use dag_rdf::Datastore;
    use std::collections::HashMap as StdHashMap;

    fn tp(s: &str, p: &str, o: &str) -> TriplePattern {
        TriplePattern {
            subject: Term::Variable(s.to_string()),
            predicate: Term::Variable(p.to_string()),
            object: Term::Variable(o.to_string()),
        }
    }

    fn parse(query: &str) -> Query {
        let mut ctx = ParserContext {
            prefixes: StdHashMap::new(),
            base: None,
        };
        parse_query(query, &mut ctx).unwrap().1
    }

    fn bgps_of(query: &Query) -> Vec<Vec<TriplePattern>> {
        let where_clause = crate::execute::where_clause_of(query);
        let mut out: Vec<Vec<&TriplePattern>> = Vec::new();
        collect_bgps(where_clause, &mut out);
        out.into_iter()
            .map(|v| v.into_iter().cloned().collect())
            .collect()
    }

    /// Core positive control: a genuine 3-pattern triangle on pairwise
    /// shared variables (no variable shared by all three) is cyclic.
    #[test]
    fn triangle_is_cyclic() {
        let a = tp("x", "e", "y");
        let b = tp("y", "e", "z");
        let c = tp("z", "e", "x");
        assert!(bgp_has_cyclic_join_graph(&[&a, &b, &c]));
    }

    /// Core negative control: a star (all three patterns sharing one hub
    /// variable) is acyclic.
    #[test]
    fn star_is_acyclic() {
        let a = tp("s", "p1", "o1");
        let b = TriplePattern {
            subject: Term::Variable("s".to_string()),
            predicate: Term::Variable("p2".to_string()),
            object: Term::Variable("o2".to_string()),
        };
        let c = TriplePattern {
            subject: Term::Variable("s".to_string()),
            predicate: Term::Variable("p3".to_string()),
            object: Term::Variable("o3".to_string()),
        };
        assert!(!bgp_has_cyclic_join_graph(&[&a, &b, &c]));
    }

    /// A single pattern with three distinct variables must never be
    /// reported as cyclic on its own — it is one hyperedge, not a join
    /// between atoms. This is the exact false positive a pairwise-clique
    /// model (an earlier draft) produced.
    #[test]
    fn single_three_variable_pattern_is_acyclic() {
        let a = tp("s", "p", "o");
        assert!(!bgp_has_cyclic_join_graph(&[&a]));
    }

    /// Adversarial case: a duplicate pattern repeated verbatim must not
    /// manufacture a false cycle — the GYO subset-elimination step should
    /// collapse the duplicate to one hyperedge before the ear-removal
    /// step runs.
    #[test]
    fn duplicate_pattern_is_acyclic() {
        let a = tp("x", "e", "y");
        let b = tp("x", "e", "y");
        assert!(!bgp_has_cyclic_join_graph(&[&a, &b]));
    }

    /// Adversarial case: a deeply nested OPTIONAL/UNION/GRAPH/subquery
    /// chain must still surface a cyclic BGP buried at the bottom — the
    /// recursive descent in `collect_bgps` must not stop early at the
    /// first nesting level.
    #[test]
    fn deeply_nested_cyclic_bgp_is_found() {
        let query = parse(
            "PREFIX : <http://example.org/> SELECT * WHERE { \
               ?a :outer ?b . \
               OPTIONAL { \
                 GRAPH ?g { \
                   { \
                     ?x :e ?y . ?y :e ?z . ?z :e ?x . \
                   } UNION { \
                     ?p :f ?q . \
                   } \
                 } \
               } \
             }",
        );
        let bgps = bgps_of(&query);
        let cyclic: Vec<_> = bgps
            .iter()
            .filter(|patterns| {
                let refs: Vec<&TriplePattern> = patterns.iter().collect();
                bgp_has_cyclic_join_graph(&refs)
            })
            .collect();
        assert_eq!(
            cyclic.len(),
            1,
            "expected exactly one cyclic BGP, found among: {bgps:?}"
        );
        assert_eq!(cyclic[0].len(), 3);
    }

    /// Adversarial case from the issue's "Key risk" section: a BGP split
    /// across a `FILTER` boundary (`?a :e ?b . FILTER(...) ?b :e ?c . ?c
    /// :e ?a .`) is parsed as two adjacent `BGP` sibling components, but
    /// the patterns are still joined together at evaluation time — the
    /// cycle only closes across the FILTER, so `collect_bgps` must merge
    /// the siblings rather than check each `BGP` component in isolation.
    #[test]
    fn bgp_split_across_filter_boundary_still_detects_cycle() {
        let query = parse(
            "PREFIX : <http://example.org/> SELECT * WHERE { \
               ?a :e ?b . FILTER(?b != :nothing) ?b :e ?c . ?c :e ?a . \
             }",
        );
        let bgps = bgps_of(&query);
        assert_eq!(
            bgps.len(),
            1,
            "FILTER-split BGP siblings must merge into one BGP entry: {bgps:?}"
        );
        let refs: Vec<&TriplePattern> = bgps[0].iter().collect();
        assert!(
            bgp_has_cyclic_join_graph(&refs),
            "the cycle spans the FILTER boundary and must still be detected"
        );
    }

    /// `warn_on_cyclic_bgps` must not panic and must not alter query
    /// results — purely diagnostic. Runs end-to-end through `execute` on
    /// a genuinely cyclic query against a tiny store, confirming results
    /// are unaffected by the warning check (same count whether or not the
    /// check runs, since it is observationally side-effect-free on the
    /// data).
    #[test]
    fn executing_a_cyclic_query_is_unaffected_by_the_warning_check() {
        use dag_rdf::{GraphElement, IriReference, Quad, RdfResource};

        fn iri_node(iri: &str) -> GraphElement {
            GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
        }

        let mut ds = Datastore::new(10);
        let e = ds.add_resource(iri_node("http://ex.org/e"));
        let a = ds.add_resource(iri_node("http://ex.org/a"));
        let b = ds.add_resource(iri_node("http://ex.org/b"));
        let c = ds.add_resource(iri_node("http://ex.org/c"));
        ds.add_quad(Quad {
            triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
            subject: a,
            predicate: e,
            obj: b,
        });
        ds.add_quad(Quad {
            triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
            subject: b,
            predicate: e,
            obj: c,
        });
        ds.add_quad(Quad {
            triple_id: dag_rdf::DEFAULT_GRAPH_ELEMENT_ID,
            subject: c,
            predicate: e,
            obj: a,
        });

        let query = parse(
            "PREFIX ex: <http://ex.org/> SELECT * WHERE { \
               ?x ex:e ?y . ?y ex:e ?z . ?z ex:e ?x . \
             }",
        );
        let result = crate::execute::execute(&query, &ds, NetworkPolicy::Deny).unwrap();
        match result {
            crate::execute::QueryResult::Select(select) => {
                assert_eq!(
                    select.rows.len(),
                    3,
                    "expected the 3 cyclic rotations of a,b,c"
                );
            }
            _ => panic!("expected a SELECT result"),
        }
    }
}

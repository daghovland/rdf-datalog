/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Visual query builder — SPARQL generation.
//!
//! This module owns the pure state→SPARQL mapping.  The JavaScript in
//! `frontend.html` is a port of the same logic; the Layer 3a self-test
//! harness (`QB_SELF_TESTS` in the page) mirrors every test case below so
//! Rust↔JS drift is caught automatically.
//!
//! # Phase linking
//! Tests are tagged `"QB Phase N"` in their `#[ignore]` reason.  To activate
//! all tests for a phase run:
//!
//! ```bash
//! grep -rn "QB Phase 1" sparql_endpoint/
//! ```
//!
//! Un-ignore each hit, implement the corresponding logic, re-run `cargo test`.

// ── Public types ──────────────────────────────────────────────────────────────

/// A single node in the query graph, representing a class variable.
///
/// `var_name` is the bare SPARQL variable name without `?`
/// (e.g. `"s"` for `?s`, `"n1"` for `?n1`).
#[derive(Debug, Clone)]
pub struct QueryNode {
    pub var_name: String,
    pub class_iri: String,
    pub data_props: Vec<DataProp>,
    pub links: Vec<ObjectLink>,
}

/// A data (literal-valued) property on a node.
///
/// `var_name` is pre-sanitised (e.g. `"s_label"` for `?s_label`).
/// Only `checked == true` properties appear in the generated query.
#[derive(Debug, Clone)]
pub struct DataProp {
    pub prop_iri: String,
    pub var_name: String,
    pub checked: bool,
}

/// A directed object-property link to another node.
///
/// `Box` breaks the otherwise-infinite recursive size.
#[derive(Debug, Clone)]
pub struct ObjectLink {
    pub prop_iri: String,
    pub target: Box<QueryNode>,
}

// ── Builder helpers ───────────────────────────────────────────────────────────

impl QueryNode {
    pub fn new(var_name: impl Into<String>, class_iri: impl Into<String>) -> Self {
        Self {
            var_name: var_name.into(),
            class_iri: class_iri.into(),
            data_props: vec![],
            links: vec![],
        }
    }

    pub fn with_data_prop(
        mut self,
        prop_iri: impl Into<String>,
        var_name: impl Into<String>,
        checked: bool,
    ) -> Self {
        self.data_props.push(DataProp {
            prop_iri: prop_iri.into(),
            var_name: var_name.into(),
            checked,
        });
        self
    }

    pub fn with_link(mut self, prop_iri: impl Into<String>, target: QueryNode) -> Self {
        self.links.push(ObjectLink {
            prop_iri: prop_iri.into(),
            target: Box::new(target),
        });
        self
    }
}

// ── Generation stub ───────────────────────────────────────────────────────────

/// Generate a SPARQL SELECT query from a query-graph rooted at `root`.
///
/// Output format (2-space indent, full IRIs, no trailing newline):
/// ```text
/// SELECT ?s ?s_label WHERE {
///   ?s a <http://example.org/Person> .
///   OPTIONAL { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label }
/// }
/// ```
///
/// Rules:
/// - The `?node a <Class>` triple is always required (not OPTIONAL).
/// - Object-property links are required (inner-join semantics).
/// - Data properties are OPTIONAL (instances without the property still appear).
/// - SELECT projects all node variables + all checked data-property variables,
///   in depth-first traversal order.
///
/// Implement in QB Phase 1.
pub fn generate_sparql(_root: &QueryNode) -> String {
    unimplemented!("QB Phase 1: implement generate_sparql")
}

// ── Unit tests ────────────────────────────────────────────────────────────────
//
// All tests are mirrored in the JS `QB_SELF_TESTS` array in frontend.html.
// If you add a case here, add the identical case there (and vice versa).
//
// Un-ignore a batch with:  grep -rn "QB Phase 1" sparql_endpoint/src/query_builder.rs

#[cfg(test)]
mod tests {
    use super::*;

    // ── QB Phase 1: single-level (class + data properties) ───────────────────
    // Unignore when implementing Phase 1 of the visual query builder.

    #[test]
    #[ignore = "QB Phase 1: generate_sparql not yet implemented"]
    fn single_node_no_props() {
        let root = QueryNode::new("s", "http://example.org/Person");
        let expected = "\
SELECT ?s WHERE {
  ?s a <http://example.org/Person> .
}";
        assert_eq!(generate_sparql(&root), expected);
    }

    #[test]
    #[ignore = "QB Phase 1: generate_sparql not yet implemented"]
    fn single_node_one_checked_data_prop() {
        let root = QueryNode::new("s", "http://example.org/Person").with_data_prop(
            "http://www.w3.org/2000/01/rdf-schema#label",
            "s_label",
            true,
        );
        let expected = "\
SELECT ?s ?s_label WHERE {
  ?s a <http://example.org/Person> .
  OPTIONAL { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label }
}";
        assert_eq!(generate_sparql(&root), expected);
    }

    #[test]
    #[ignore = "QB Phase 1: generate_sparql not yet implemented"]
    fn unchecked_data_prop_excluded_from_select_and_where() {
        let root = QueryNode::new("s", "http://example.org/Person").with_data_prop(
            "http://www.w3.org/2000/01/rdf-schema#label",
            "s_label",
            false, // not checked
        );
        let sparql = generate_sparql(&root);
        assert!(
            !sparql.contains("?s_label"),
            "unchecked prop var should not appear in output: {sparql}"
        );
        assert!(
            !sparql.contains("OPTIONAL"),
            "no OPTIONAL block for unchecked prop: {sparql}"
        );
    }

    #[test]
    #[ignore = "QB Phase 1: generate_sparql not yet implemented"]
    fn multiple_checked_data_props_each_get_own_optional() {
        let root = QueryNode::new("s", "http://example.org/Person")
            .with_data_prop(
                "http://www.w3.org/2000/01/rdf-schema#label",
                "s_label",
                true,
            )
            .with_data_prop("http://example.org/age", "s_age", true);
        let expected = "\
SELECT ?s ?s_label ?s_age WHERE {
  ?s a <http://example.org/Person> .
  OPTIONAL { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label }
  OPTIONAL { ?s <http://example.org/age> ?s_age }
}";
        assert_eq!(generate_sparql(&root), expected);
    }

    #[test]
    #[ignore = "QB Phase 1: generate_sparql not yet implemented"]
    fn mixed_checked_and_unchecked_props() {
        let root = QueryNode::new("s", "http://example.org/Person")
            .with_data_prop(
                "http://www.w3.org/2000/01/rdf-schema#label",
                "s_label",
                true,
            )
            .with_data_prop("http://example.org/age", "s_age", false);
        let sparql = generate_sparql(&root);
        assert!(sparql.contains("?s_label"), "checked prop should appear");
        assert!(
            !sparql.contains("?s_age"),
            "unchecked prop should not appear"
        );
        assert_eq!(
            sparql.matches("OPTIONAL").count(),
            1,
            "exactly one OPTIONAL block"
        );
    }

    // ── QB Phase 2: multi-hop object-property links ───────────────────────────
    // Unignore when implementing Phase 2 of the visual query builder.

    #[test]
    #[ignore = "QB Phase 2: multi-hop links not yet implemented"]
    fn two_node_chain_via_object_prop() {
        let bob_node = QueryNode::new("n1", "http://example.org/Person");
        let root = QueryNode::new("s", "http://example.org/Person")
            .with_link("http://example.org/knows", bob_node);
        let expected = "\
SELECT ?s ?n1 WHERE {
  ?s a <http://example.org/Person> .
  ?s <http://example.org/knows> ?n1 .
  ?n1 a <http://example.org/Person> .
}";
        assert_eq!(generate_sparql(&root), expected);
    }

    #[test]
    #[ignore = "QB Phase 2: multi-hop links not yet implemented"]
    fn two_node_chain_with_data_props_on_both_nodes() {
        let linked = QueryNode::new("n1", "http://example.org/Person").with_data_prop(
            "http://www.w3.org/2000/01/rdf-schema#label",
            "n1_label",
            true,
        );
        let root = QueryNode::new("s", "http://example.org/Person")
            .with_data_prop(
                "http://www.w3.org/2000/01/rdf-schema#label",
                "s_label",
                true,
            )
            .with_link("http://example.org/knows", linked);
        let expected = "\
SELECT ?s ?s_label ?n1 ?n1_label WHERE {
  ?s a <http://example.org/Person> .
  OPTIONAL { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?s_label }
  ?s <http://example.org/knows> ?n1 .
  ?n1 a <http://example.org/Person> .
  OPTIONAL { ?n1 <http://www.w3.org/2000/01/rdf-schema#label> ?n1_label }
}";
        assert_eq!(generate_sparql(&root), expected);
    }

    #[test]
    #[ignore = "QB Phase 2: multi-hop links not yet implemented"]
    fn three_node_chain_emits_triples_in_dfs_order() {
        let leaf = QueryNode::new("n2", "http://example.org/Topic");
        let mid = QueryNode::new("n1", "http://example.org/Article")
            .with_link("http://example.org/about", leaf);
        let root = QueryNode::new("s", "http://example.org/Person")
            .with_link("http://example.org/wrote", mid);
        let sparql = generate_sparql(&root);
        // Verify depth-first ordering: s triples, then n1 triples, then n2 triples.
        let pos_s_type = sparql.find("?s a <").unwrap();
        let pos_s_link = sparql.find("?s <http://example.org/wrote>").unwrap();
        let pos_n1_type = sparql.find("?n1 a <").unwrap();
        let pos_n1_link = sparql.find("?n1 <http://example.org/about>").unwrap();
        let pos_n2_type = sparql.find("?n2 a <").unwrap();
        assert!(pos_s_type < pos_s_link, "s type before s link");
        assert!(pos_s_link < pos_n1_type, "s link before n1 type");
        assert!(pos_n1_type < pos_n1_link, "n1 type before n1 link");
        assert!(pos_n1_link < pos_n2_type, "n1 link before n2 type");
    }

    #[test]
    #[ignore = "QB Phase 2: multi-hop links not yet implemented"]
    fn fan_out_two_object_links_from_root_both_in_select() {
        let company = QueryNode::new("n1", "http://example.org/Company");
        let colleague = QueryNode::new("n2", "http://example.org/Person");
        let root = QueryNode::new("s", "http://example.org/Person")
            .with_link("http://example.org/worksFor", company)
            .with_link("http://example.org/knows", colleague);
        let sparql = generate_sparql(&root);
        assert!(sparql.starts_with("SELECT ?s ?n1 ?n2 WHERE"));
        assert!(sparql.contains("?s <http://example.org/worksFor> ?n1 ."));
        assert!(sparql.contains("?s <http://example.org/knows> ?n2 ."));
        assert!(sparql.contains("?n1 a <http://example.org/Company> ."));
        assert!(sparql.contains("?n2 a <http://example.org/Person> ."));
    }
}

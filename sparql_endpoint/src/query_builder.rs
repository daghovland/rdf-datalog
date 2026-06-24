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
/// When `filter` is `Some(text)` the triple becomes required (not OPTIONAL)
/// and a `FILTER(regex(?var, text, "i"))` is appended.
#[derive(Debug, Clone)]
pub struct DataProp {
    pub prop_iri: String,
    pub var_name: String,
    pub checked: bool,
    pub filter: Option<String>,
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
            filter: None,
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
pub fn generate_sparql(root: &QueryNode) -> String {
    let mut select_vars = Vec::new();
    let mut body_lines = Vec::new();
    collect_node(root, &mut select_vars, &mut body_lines);
    format!(
        "SELECT {} WHERE {{\n{}\n}}",
        select_vars
            .iter()
            .map(|v| format!("?{v}"))
            .collect::<Vec<_>>()
            .join(" "),
        body_lines.join("\n"),
    )
}

fn collect_node(node: &QueryNode, select: &mut Vec<String>, body: &mut Vec<String>) {
    select.push(node.var_name.clone());
    body.push(format!("  ?{} a <{}> .", node.var_name, node.class_iri));
    for dp in &node.data_props {
        if dp.checked {
            select.push(dp.var_name.clone());
            match dp.filter.as_deref().filter(|f| !f.trim().is_empty()) {
                None => {
                    body.push(format!(
                        "  OPTIONAL {{ ?{} <{}> ?{} }}",
                        node.var_name, dp.prop_iri, dp.var_name,
                    ));
                }
                Some(f) => {
                    let escaped = f.replace('\\', "\\\\").replace('"', "\\\"");
                    body.push(format!(
                        "  ?{} <{}> ?{} .",
                        node.var_name, dp.prop_iri, dp.var_name,
                    ));
                    body.push(format!(
                        "  FILTER(regex(?{}, \"{}\", \"i\"))",
                        dp.var_name, escaped,
                    ));
                }
            }
        }
    }
    for link in &node.links {
        body.push(format!(
            "  ?{} <{}> ?{} .",
            node.var_name, link.prop_iri, link.target.var_name,
        ));
        collect_node(&link.target, select, body);
    }
}

/// Decide whether a typed filter value could still match at least one known
/// productive value, using the same case-insensitive substring semantics as
/// the `regex(?var, text, "i")` filter `generate_sparql` emits.
///
/// An empty/blank typed value is never "unproductive" — there's no
/// constraint yet, so it can't dead-end the query.
pub fn filter_value_is_productive(typed: &str, productive_values: &[String]) -> bool {
    if typed.trim().is_empty() {
        return true;
    }
    let needle = typed.to_lowercase();
    productive_values
        .iter()
        .any(|v| v.to_lowercase().contains(&needle))
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

    fn single_node_no_props() {
        let root = QueryNode::new("s", "http://example.org/Person");
        let expected = "\
SELECT ?s WHERE {
  ?s a <http://example.org/Person> .
}";
        assert_eq!(generate_sparql(&root), expected);
    }

    #[test]
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

    // ── VQS productive-value hint ─────────────────────────────────────────────
    // Mirrored by filterValueIsProductive() / QB_SELF_TESTS in frontend.html.

    #[test]
    fn empty_filter_is_always_productive() {
        assert!(filter_value_is_productive("", &[]));
        assert!(filter_value_is_productive(
            "  ",
            &["Alice".to_string(), "Bob".to_string()]
        ));
    }

    #[test]
    fn filter_matching_known_value_case_insensitively_is_productive() {
        let values = vec!["Alice".to_string(), "Bob".to_string()];
        assert!(filter_value_is_productive("ali", &values));
        assert!(filter_value_is_productive("ALICE", &values));
    }

    #[test]
    fn filter_matching_no_known_value_is_unproductive() {
        let values = vec!["Alice".to_string(), "Bob".to_string()];
        assert!(!filter_value_is_productive("zzz", &values));
    }

    #[test]
    fn nonempty_filter_against_no_known_values_is_unproductive() {
        assert!(!filter_value_is_productive("anything", &[]));
    }
}

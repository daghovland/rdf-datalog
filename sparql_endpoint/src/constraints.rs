/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Constraint checking for SPARQL Update transactions.
//!
//! After every successful SPARQL Update, the `check_owl_nothing` function
//! inspects the default graph for instances of `owl:Nothing`.  If any are
//! found, the transaction must be rolled back and the caller returns HTTP 409.
//!
//! This mechanism lets users write Datalog rules that derive `?x a owl:Nothing`
//! as integrity constraints.  Any INSERT that would violate a constraint is
//! rejected before it becomes visible to other clients.
//!
//! Related: [#127](https://github.com/daghovland/rdf-datalog/issues/127)

use dag_rdf::Datastore;
use dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID;
use ingress::{OWL_NOTHING, RDF_TYPE};

/// A single constraint violation: a subject that has been inferred to be an
/// instance of `owl:Nothing`, together with a sample of its properties.
pub struct ViolationInfo {
    /// N-Triples-style representation of the violating subject.
    pub subject: String,
    /// Up to `max_props` (predicate, object) pairs for the subject, for
    /// diagnostic purposes only.
    pub properties: Vec<(String, String)>,
}

/// Scan the default graph for instances of `owl:Nothing`.
///
/// Returns up to `max_violations` violating subjects, each with up to
/// `max_props` sample properties.  An empty `Vec` means no violation was
/// found.
///
/// The function is read-only and does **not** modify the store.
pub fn check_owl_nothing(
    store: &Datastore,
    max_violations: usize,
    max_props: usize,
) -> Vec<ViolationInfo> {
    // If rdf:type or owl:Nothing have never been interned, there can be no
    // instances of owl:Nothing in the store.
    let rdf_type_id = match store.lookup_named_graph_id(RDF_TYPE) {
        Some(id) => id,
        None => return vec![],
    };
    let owl_nothing_id = match store.lookup_named_graph_id(OWL_NOTHING) {
        Some(id) => id,
        None => return vec![],
    };

    // Collect up to max_violations quads of the form
    // (DEFAULT_GRAPH, ?x, rdf:type, owl:Nothing).
    let violating_subjects: Vec<_> = store
        .named_graphs
        .get_quads_with_object_predicate(owl_nothing_id, rdf_type_id)
        .filter(|q| q.triple_id == DEFAULT_GRAPH_ELEMENT_ID)
        .take(max_violations)
        .map(|q| q.subject)
        .collect();

    violating_subjects
        .into_iter()
        .map(|subject_id| {
            let subject_repr = format!("{}", store.resources.get_graph_element(subject_id));

            let properties: Vec<_> = store
                .named_graphs
                .get_quads_with_id_subject(DEFAULT_GRAPH_ELEMENT_ID, subject_id)
                .take(max_props)
                .map(|pq| {
                    let pred = format!("{}", store.resources.get_graph_element(pq.predicate));
                    let obj = format!("{}", store.resources.get_graph_element(pq.obj));
                    (pred, obj)
                })
                .collect();

            ViolationInfo {
                subject: subject_repr,
                properties,
            }
        })
        .collect()
}

/// Build an HTTP 409 response body describing the constraint violations.
pub fn format_409_body(violations: &[ViolationInfo]) -> String {
    let mut body =
        String::from("Transaction rejected: owl:Nothing has instances after reasoning.\n");

    for (i, v) in violations.iter().enumerate() {
        body.push_str(&format!("\nInstance {}: {}\n", i + 1, v.subject));
        for (p, o) in &v.properties {
            body.push_str(&format!("  {p} {o}\n"));
        }
    }

    let n = violations.len();
    body.push_str(&format!(
        "\n(showing {n} of {n} instance{})\n",
        if n == 1 { "" } else { "s" }
    ));
    body
}

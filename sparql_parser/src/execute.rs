/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SPARQL query execution against a [`Datastore`].
//!
//! Translates the parser AST into [`QuadPattern`] queries and evaluates them
//! via the index in `dag_rdf`.

use crate::ast::{ProjectionElement, Query, QueryComponent, Term, TriplePattern};
use dag_rdf::query::get_default_graph_pattern;
use dag_rdf::{Datastore, GraphElement, GraphElementId, QuadPattern, Term as DagTerm};
use std::collections::HashMap;

/// A single bound solution: variable name → concrete graph element.
pub type SolutionRow = HashMap<String, GraphElement>;

/// The result of executing a SPARQL SELECT query.
pub struct SelectResult {
    /// Variable names in projection order.
    pub variables: Vec<String>,
    /// Each row maps projected variable names to their bound value.
    pub rows: Vec<SolutionRow>,
}

/// Execute a parsed SPARQL query against `datastore`.
///
/// Currently supports SELECT with basic graph patterns (BGP).
/// Returns an error string for unsupported query forms.
pub fn execute(query: &Query, datastore: &Datastore) -> Result<SelectResult, String> {
    match query {
        Query::Select { projection, where_clause, limit, offset, distinct, .. } => {
            let variables = projection_variables(projection);
            let patterns = collect_bgp_patterns(where_clause)?;
            let solutions = eval_bgp(&patterns, datastore);

            // Project
            let mut rows: Vec<SolutionRow> = solutions
                .into_iter()
                .map(|sub| project(&sub, &variables, datastore))
                .collect();

            if *distinct {
                rows.dedup();
            }

            if let Some(off) = offset {
                let off = *off as usize;
                if off < rows.len() { rows = rows[off..].to_vec(); } else { rows.clear(); }
            }
            if let Some(lim) = limit {
                rows.truncate(*lim as usize);
            }

            Ok(SelectResult { variables, rows })
        }
    }
}

// ── Projection ────────────────────────────────────────────────────────────────

fn projection_variables(proj: &[ProjectionElement]) -> Vec<String> {
    proj.iter()
        .filter_map(|p| match p {
            ProjectionElement::Variable(v) => Some(v.clone()),
            ProjectionElement::Star => None, // handled separately if needed
            ProjectionElement::Expression(_, alias) => Some(alias.clone()),
        })
        .collect()
}

fn project(
    sub: &HashMap<String, GraphElementId>,
    variables: &[String],
    datastore: &Datastore,
) -> SolutionRow {
    variables
        .iter()
        .filter_map(|v| {
            sub.get(v).map(|&id| {
                let el = datastore.resources.get_graph_element(id).clone();
                (v.clone(), el)
            })
        })
        .collect()
}

// ── BGP pattern collection ────────────────────────────────────────────────────

fn collect_bgp_patterns(components: &[QueryComponent]) -> Result<Vec<TriplePattern>, String> {
    let mut patterns = Vec::new();
    for comp in components {
        match comp {
            QueryComponent::BGP(tps) => patterns.extend(tps.clone()),
            QueryComponent::Graph(_, inner) => {
                // Flatten named-graph patterns into the default graph for now
                patterns.extend(collect_bgp_patterns(inner)?);
            }
            QueryComponent::Filter(_) => {} // filters not yet evaluated
            other => {
                return Err(format!("Unsupported query component: {:?}", other));
            }
        }
    }
    Ok(patterns)
}

// ── BGP evaluation ────────────────────────────────────────────────────────────

/// A partial substitution during BGP evaluation: variable → interned ID.
type PartialSub = HashMap<String, GraphElementId>;

fn eval_bgp(patterns: &[TriplePattern], datastore: &Datastore) -> Vec<PartialSub> {
    let mut solutions: Vec<PartialSub> = vec![HashMap::new()];

    for pattern in patterns {
        solutions = solutions
            .into_iter()
            .flat_map(|sub| eval_triple_pattern(pattern, &sub, datastore))
            .collect();
        if solutions.is_empty() {
            break;
        }
    }
    solutions
}

fn eval_triple_pattern(
    tp: &TriplePattern,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Vec<PartialSub> {
    // If any constant in the pattern is absent from the store it can never match.
    for term in [&tp.subject, &tp.predicate, &tp.object] {
        if let Term::Constant(gel) = term {
            if !datastore.resources.resource_map.contains_key(gel) {
                return Vec::new();
            }
        }
    }

    let quad_pattern = triple_pattern_to_quad_pattern(tp, sub, datastore);
    let mut new_solutions = Vec::new();

    // Use the dag_rdf QueryExecutor via quads_matching
    let g = match &quad_pattern.graph { DagTerm::Resource(id) => Some(*id), _ => None };
    let s = match &quad_pattern.subject { DagTerm::Resource(id) => Some(*id), _ => None };
    let p = match &quad_pattern.predicate { DagTerm::Resource(id) => Some(*id), _ => None };
    let o = match &quad_pattern.object { DagTerm::Resource(id) => Some(*id), _ => None };

    for quad in datastore.quads_matching(g, s, p, o) {
        let mut new_sub = sub.clone();
        let mut ok = true;

        macro_rules! bind {
            ($term:expr, $val:expr) => {
                if let Term::Variable(v) = $term {
                    match new_sub.get(v) {
                        Some(&existing) if existing != $val => { ok = false; }
                        _ => { new_sub.insert(v.clone(), $val); }
                    }
                }
            };
        }

        bind!(&tp.subject, quad.subject);
        bind!(&tp.predicate, quad.predicate);
        bind!(&tp.object, quad.obj);

        if ok {
            new_solutions.push(new_sub);
        }
    }
    new_solutions
}

/// Translate a `TriplePattern` (with any already-bound variables substituted)
/// into a `QuadPattern` in the default graph.
fn triple_pattern_to_quad_pattern(
    tp: &TriplePattern,
    sub: &PartialSub,
    datastore: &Datastore,
) -> QuadPattern {
    get_default_graph_pattern(
        ast_term_to_dag_term(&tp.subject, sub, datastore),
        ast_term_to_dag_term(&tp.predicate, sub, datastore),
        ast_term_to_dag_term(&tp.object, sub, datastore),
    )
}

fn ast_term_to_dag_term(term: &Term, sub: &PartialSub, datastore: &Datastore) -> DagTerm {
    match term {
        Term::Variable(v) => match sub.get(v) {
            Some(&id) => DagTerm::Resource(id),
            None => DagTerm::Variable(v.clone()),
        },
        Term::Constant(gel) => {
            // Look up or insert the constant in the resource manager.
            // We need a shared reference so we can only look up, not insert.
            // Use the existing map for lookup; unknown constants yield no matches.
            match datastore.resources.resource_map.get(gel) {
                Some(&id) => DagTerm::Resource(id),
                // Constant not in store → use a sentinel variable that will never bind
                None => DagTerm::Variable(format!("__unknown_{:?}", gel)),
            }
        }
    }
}

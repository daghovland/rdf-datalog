/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::ingress::{DEFAULT_GRAPH_ELEMENT_ID, GraphElementId};
use std::fmt;

/// A term in a query pattern — either a concrete resource ID or a named variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Term {
    Resource(GraphElementId),
    Variable(String),
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Resource(id) => write!(f, "_{}", id),
            Term::Variable(name) => write!(f, "?{}", name),
        }
    }
}

/// A pattern over quads (graph, subject, predicate, object), where each position
/// is either a concrete resource or a variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QuadPattern {
    pub graph: Term,
    pub subject: Term,
    pub predicate: Term,
    pub object: Term,
}

impl QuadPattern {
    pub fn get_variables(&self) -> Vec<&str> {
        [&self.graph, &self.subject, &self.predicate, &self.object]
            .iter()
            .filter_map(|t| match t {
                Term::Variable(v) => Some(v.as_str()),
                _ => None,
            })
            .collect()
    }
}

impl fmt::Display for QuadPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.graph == Term::Resource(DEFAULT_GRAPH_ELEMENT_ID) {
            write!(f, "[{}, {}, {}]", self.subject, self.predicate, self.object)
        } else {
            write!(
                f,
                "[{}, {}, {}] {}",
                self.subject, self.predicate, self.object, self.graph
            )
        }
    }
}

/// Constructs a `QuadPattern` in the default graph.
pub fn get_default_graph_pattern(subject: Term, predicate: Term, object: Term) -> QuadPattern {
    QuadPattern {
        graph: Term::Resource(DEFAULT_GRAPH_ELEMENT_ID),
        subject,
        predicate,
        object,
    }
}

use crate::datastore::Datastore;
use std::collections::HashMap;

pub struct QueryExecutor<'a> {
    pub datastore: &'a Datastore,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub map: HashMap<String, GraphElementId>,
}

impl<'a> QueryExecutor<'a> {
    pub fn new(datastore: &'a Datastore) -> Self {
        QueryExecutor { datastore }
    }

    pub fn execute_select(
        &self,
        patterns: &[QuadPattern],
        projection: &[String],
    ) -> Vec<HashMap<String, GraphElementId>> {
        let bindings = self.execute_bgp(patterns);
        bindings
            .into_iter()
            .map(|b| {
                let mut row = HashMap::new();
                for var in projection {
                    if let Some(id) = b.map.get(var) {
                        row.insert(var.clone(), *id);
                    }
                }
                row
            })
            .collect()
    }

    pub fn execute_bgp(&self, patterns: &[QuadPattern]) -> Vec<Binding> {
        let mut results = vec![Binding {
            map: HashMap::new(),
        }];

        for pattern in patterns {
            let mut next_results = Vec::new();
            for binding in results {
                // Ground the pattern with current binding
                let g = self.ground(&pattern.graph, &binding);
                let s = self.ground(&pattern.subject, &binding);
                let p = self.ground(&pattern.predicate, &binding);
                let o = self.ground(&pattern.object, &binding);

                let matching_quads = self.datastore.quads_matching(g, s, p, o);
                for quad in matching_quads {
                    let mut new_binding = binding.clone();
                    let mut possible = true;

                    if let Term::Variable(v) = &pattern.graph
                        && !self.bind_var(&mut new_binding, v, quad.triple_id)
                    {
                        possible = false;
                    }
                    if possible
                        && let Term::Variable(v) = &pattern.subject
                        && !self.bind_var(&mut new_binding, v, quad.subject)
                    {
                        possible = false;
                    }
                    if possible
                        && let Term::Variable(v) = &pattern.predicate
                        && !self.bind_var(&mut new_binding, v, quad.predicate)
                    {
                        possible = false;
                    }
                    if possible
                        && let Term::Variable(v) = &pattern.object
                        && !self.bind_var(&mut new_binding, v, quad.obj)
                    {
                        possible = false;
                    }

                    if possible {
                        next_results.push(new_binding);
                    }
                }
            }
            results = next_results;
            if results.is_empty() {
                break;
            }
        }

        results
    }

    fn ground(&self, term: &Term, binding: &Binding) -> Option<GraphElementId> {
        match term {
            Term::Resource(id) => Some(*id),
            Term::Variable(v) => binding.map.get(v).cloned(),
        }
    }

    fn bind_var(&self, binding: &mut Binding, var: &str, id: GraphElementId) -> bool {
        if let Some(&existing) = binding.map.get(var) {
            existing == id
        } else {
            binding.map.insert(var.to_string(), id);
            true
        }
    }
}

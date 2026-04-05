/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use std::fmt;
use crate::ingress::{GraphElementId, DEFAULT_GRAPH_ELEMENT_ID};

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
        write!(
            f,
            "{} [{}, {}, {}]",
            self.graph, self.subject, self.predicate, self.object
        )
    }
}

/// Convenience: build a quad pattern in the default graph.
pub fn get_default_graph_pattern(subject: Term, predicate: Term, object: Term) -> QuadPattern {
    QuadPattern {
        graph: Term::Resource(DEFAULT_GRAPH_ELEMENT_ID),
        subject,
        predicate,
        object,
    }
}

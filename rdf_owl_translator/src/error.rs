/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Error types for RDF → OWL translation.
//!
//! Scoped to the malformed-`rdf:List` panics fixed under
//! <https://github.com/daghovland/rdf-datalog/issues/363>. Other panic sites
//! in this crate (`try_get_individual`, `try_get_literal`, the
//! multiple-`owl:members` cases in `axiom_parser.rs`) are a follow-up under
//! the same issue and are not yet represented here.

use std::fmt;

/// Errors that can occur while translating RDF triples into an OWL 2
/// ontology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslatorError {
    /// The RDF encoding of an `rdf:List` structure (used for
    /// `owl:intersectionOf`, `owl:unionOf`, `owl:members`, property chains,
    /// etc.) is malformed: a cycle, or a node with the wrong number of
    /// `rdf:first`/`rdf:rest` triples.
    MalformedRdfList(String),
}

impl fmt::Display for TranslatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranslatorError::MalformedRdfList(msg) => {
                write!(f, "malformed rdf:List: {msg}")
            }
        }
    }
}

impl std::error::Error for TranslatorError {}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Error types for RDF → OWL translation.
//!
//! Scoped to the malformed-`rdf:List` panics, the anonymous class-expression
//! dependency cycle, the multiple-`owl:members` panics, and the
//! `try_get_individual` panic, all fixed under
//! <https://github.com/daghovland/rdf-datalog/issues/363>. `try_get_literal`
//! was deleted outright (dead code, zero call sites) rather than converted.

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
    /// The dependency graph among anonymous (blank-node) OWL class
    /// expressions contains a cycle — e.g. two blank-node
    /// `owl:intersectionOf`/`owl:unionOf` expressions whose member lists
    /// reference each other — so no topological order exists.
    CyclicDependency(String),
    /// A subject of `rdf:type owl:AllDisjointClasses` or
    /// `rdf:type owl:AllDisjointProperties` has more than one `owl:members`
    /// triple, so which list to use is ambiguous.
    MultipleOwlMembers(String),
    /// A graph element that was expected to denote an OWL individual (an
    /// IRI resource or a blank node) turned out to be a literal, or an RDF
    /// 1.2 triple term (which cannot be an OWL individual at all — full RDF
    /// 1.2 support is tracked in
    /// <https://github.com/daghovland/rdf-datalog/issues/143>).
    InvalidIndividual(String),
}

impl fmt::Display for TranslatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranslatorError::MalformedRdfList(msg) => {
                write!(f, "malformed rdf:List: {msg}")
            }
            TranslatorError::CyclicDependency(msg) => {
                write!(f, "cyclic dependency: {msg}")
            }
            TranslatorError::MultipleOwlMembers(msg) => {
                write!(f, "multiple owl:members: {msg}")
            }
            TranslatorError::InvalidIndividual(msg) => {
                write!(f, "invalid OWL individual: {msg}")
            }
        }
    }
}

impl std::error::Error for TranslatorError {}

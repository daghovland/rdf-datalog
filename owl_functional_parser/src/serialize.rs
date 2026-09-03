/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Serialize an [`owl_ontology::Ontology`] to OWL 2 Functional-Style Syntax
//! text (the reverse direction of [`crate::parse`]).
//!
//! Follow-up to the parser (issue
//! [#180](https://github.com/daghovland/rdf-datalog/issues/180)/PR
//! [#627](https://github.com/daghovland/rdf-datalog/pull/627)), tracked in
//! [#181](https://github.com/daghovland/rdf-datalog/issues/181). See
//! `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`'s "Serialiser" section
//! for the design.
//!
//! Stub: implementation lands per-test, TDD-style, per this repo's
//! `CLAUDE.md`.

use owl_ontology::Ontology;

/// Serialize `ontology` to OWL 2 Functional-Style Syntax text.
///
/// Only `ontology.axioms` is serialized (not `ontology.all_axioms()`'s
/// built-in `owl:Thing`/`xsd:integer`/... declarations, which are implicit
/// and never need restating).
pub fn serialize(_ontology: &Ontology) -> String {
    unimplemented!("owl_functional_parser::serialize -- see issue #181")
}

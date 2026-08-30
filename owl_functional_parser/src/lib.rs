/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! OWL 2 Functional-Style Syntax parser.
//!
//! Parses [OWL 2 Functional-Style Syntax](https://www.w3.org/TR/owl2-syntax/)
//! (`.ofn`) documents into an [`owl_ontology::Ontology`].
//!
//! See `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md` for the grammar
//! subset this parser covers and the module layout. Issue
//! [#180](https://github.com/daghovland/rdf-datalog/issues/180) tracks this
//! feature; SWRL `Rule(...)` parsing is deferred to
//! [#625](https://github.com/daghovland/rdf-datalog/issues/625).

use owl_ontology::Ontology;

/// Parse an OWL 2 Functional-Style Syntax `ontologyDocument` and produce an
/// [`owl_ontology::Ontology`].
///
/// Stub: implementation lands phase-by-phase per
/// `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md` (issue #180).
pub fn parse(_input: &str) -> Result<Ontology, String> {
    Err("owl_functional_parser::parse is not yet implemented (#180)".to_string())
}

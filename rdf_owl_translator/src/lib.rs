/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Translation of RDF triples (in a [`dag_rdf::Datastore`]) into an OWL 2
//! [`owl_ontology::OntologyDocument`].
//!
//! Implements the mapping defined in
//! <https://www.w3.org/TR/owl2-mapping-to-rdf/> (Sections 3.1–3.2 and
//! Tables 7–17).
//!
//! Entry point: [`rdf2owl`].

pub mod ingress;
mod class_expression_parser;
mod axiom_parser;
mod translator;

pub use translator::rdf2owl;

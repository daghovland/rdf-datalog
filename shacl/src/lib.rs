/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SHACL (Shapes Constraint Language) validation.
//!
//! Spec: <https://www.w3.org/TR/shacl/>
//!
//! `validate` is a stub; see `SHACL_PLAN.md` for the implementation roadmap.

use dag_rdf::Datastore;

/// Severity of a SHACL validation result (`sh:resultSeverity`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Violation,
    Warning,
    Info,
}

/// A single entry in a SHACL validation report (`sh:ValidationResult`).
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub focus_node: Option<String>,
    pub severity: Severity,
    pub message: Option<String>,
    pub result_path: Option<String>,
    pub source_shape: Option<String>,
    pub source_constraint: Option<String>,
    pub value: Option<String>,
}

/// The outcome of validating a data graph against a shapes graph (`sh:ValidationReport`).
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub conforms: bool,
    pub results: Vec<ValidationResult>,
}

/// Validate `data` against the SHACL shapes in `shapes`.
///
/// Not yet implemented — see `SHACL_PLAN.md`.
pub fn validate(_data: &Datastore, _shapes: &Datastore) -> Result<ValidationReport, String> {
    todo!("SHACL Core validation is not yet implemented")
}

/// Serialize a `ValidationReport` as a Turtle SHACL report graph.
///
/// Not yet implemented — see `SHACL_PLAN.md`.
pub fn report_to_turtle(_report: &ValidationReport) -> String {
    todo!("SHACL report serialization is not yet implemented")
}

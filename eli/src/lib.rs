/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

pub mod axioms;
pub mod eli2rl;
pub mod extractor;

pub use axioms::*;
pub use eli2rl::generate_tbox_rl;
pub use extractor::eli_axiom_extractor;

use dag_rdf::GraphElementManager;
use datalog::types::Rule;
use owl_ontology::ClassAxiom;

/// Translate an OWL 2 class axiom into datalog rules via the ELI pathway.
/// Returns `None` if the axiom is not ELI-expressible.
pub fn owl2datalog(resources: &mut GraphElementManager, axiom: &ClassAxiom) -> Option<Vec<Rule>> {
    eli_axiom_extractor(axiom).map(|formulas| generate_tbox_rl(resources, formulas))
}

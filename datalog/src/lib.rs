/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

pub mod types;
pub mod datalog;
pub mod unification;
pub mod stratifier;
pub mod reasoner;

pub use types::*;
pub use datalog::{
    empty_substitution, is_fact, is_safe_rule,
    constant_quad_pattern, wildcard_quad_pattern,
    get_substitutions, apply_substitution_quad,
    evaluate_pattern, evaluate_positive, evaluate,
    get_partial_matches, merge_partial_match_maps, get_matches_for_rule,
};
pub use unification::{quad_patterns_unifiable, PatternEdge, depending_rules, intentional_rules};
pub use stratifier::RulePartitioner;
pub use reasoner::{DatalogProgram, evaluate_rules};

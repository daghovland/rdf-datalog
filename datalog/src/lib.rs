/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

pub mod datalog;
pub mod incremental;
pub mod reasoner;
pub mod stratifier;
pub mod types;
pub mod unification;

pub use datalog::{
    apply_substitution_quad, constant_quad_pattern, direct_wildcard_pattern, empty_substitution,
    evaluate, evaluate_pattern, evaluate_positive, get_matches_for_rule, get_partial_matches,
    get_substitutions, is_fact, is_safe_rule, merge_partial_match_maps, wildcard_quad_pattern,
};
pub use incremental::IncrementalReasoner;
pub use reasoner::{DatalogProgram, evaluate_rules};
pub use stratifier::RulePartitioner;
pub use types::*;
pub use unification::{PatternEdge, depending_rules, intentional_rules, quad_patterns_unifiable};

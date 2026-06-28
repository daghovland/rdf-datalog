/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! VQS Productive-Extension Index.
//!
//! Implements the configuration-query index from:
//! Klungre, Soylu, Giese — "Avoiding unproductive SPARQL queries through optimized
//! indices", World Wide Web 29:32 (2026). <https://doi.org/10.1007/s11280-026-01419-6>
//!
//! The index allows an interactive query builder to instantly detect which filter
//! values would make a partially-formed SPARQL query return no results, without
//! firing expensive live SPARQL queries after each user interaction.

pub mod basic_counts;
pub mod config_query;
pub mod config_set;
pub mod estimators;
pub mod navigation_graph;
pub mod query_log;
pub mod search;

pub use basic_counts::{BasicCounts, Histogram, NavStats};
pub use config_query::{ConfigQuery, IndexCell, IndexTable};
pub use config_set::ConfigSet;
pub use navigation_graph::{NavEdgeId, NavGraph, NavNodeId, NavNodeKind};
pub use query_log::{RawLogEntry, transform_query_log};

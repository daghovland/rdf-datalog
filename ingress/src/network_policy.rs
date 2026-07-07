/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

/// Controls how operations that require remote HTTP fetches are handled.
///
/// Applies to: SPARQL `LOAD`, JSON-LD external `@context` URLs, SPARQL `SERVICE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkPolicy {
    /// Remote fetches return a descriptive error. This is the default.
    ///
    /// Pass `--network=allow` on the command line to enable remote access.
    #[default]
    Deny,
    /// Remote fetch operations are silently skipped.
    ///
    /// Preserves the previous behaviour where network operations were ignored.
    Ignore,
    /// Remote fetches are performed via HTTP.
    ///
    /// # Security
    ///
    /// Only enable this in environments where **all** SPARQL clients are trusted.
    /// Any client that can send a `LOAD <url>` query can make the server issue
    /// outbound HTTP requests — a Server-Side Request Forgery (SSRF) risk.
    ///
    /// Planned hardening (private-IP blocking, redirect policy, body cap): [#135](https://github.com/daghovland/rdf-datalog/issues/135).
    /// A URL prefix allowlist variant (`AllowList`) is planned in [#136](https://github.com/daghovland/rdf-datalog/issues/136).
    ///
    /// Applies to: SPARQL `LOAD` ([#119](https://github.com/daghovland/rdf-datalog/issues/119)),
    /// JSON-LD `@context` URLs ([#82](https://github.com/daghovland/rdf-datalog/issues/82)),
    /// SPARQL `SERVICE` ([#51](https://github.com/daghovland/rdf-datalog/issues/51)).
    Allow,
}

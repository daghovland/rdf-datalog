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
///
/// CLI: `--network=deny|ignore|allow|allow:<prefix>[,<prefix>…]`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// SSRF hardening (private-IP blocking, redirect policy, body cap) is active:
    /// [#135](https://github.com/daghovland/rdf-datalog/issues/135).
    ///
    /// Applies to: SPARQL `LOAD` ([#119](https://github.com/daghovland/rdf-datalog/issues/119)),
    /// JSON-LD `@context` URLs ([#82](https://github.com/daghovland/rdf-datalog/issues/82)),
    /// SPARQL `SERVICE` ([#51](https://github.com/daghovland/rdf-datalog/issues/51)).
    Allow,
    /// Only URLs whose string representation starts with one of the given prefixes are fetched.
    ///
    /// The SSRF hardening from [#135](https://github.com/daghovland/rdf-datalog/issues/135)
    /// (IP blocking, redirect policy, body cap) still applies on top of the prefix check.
    ///
    /// CLI: `--network allow:https://example.org/,https://data.gov/`
    AllowList(Vec<String>),
}

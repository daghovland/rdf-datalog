/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Typed error for SPARQL query *evaluation* (as opposed to parsing, which
//! uses `nom`'s own error types).
//!
//! Before this, [`crate::execute::execute`]/[`crate::execute::execute_with_base`]
//! and every internal evaluator helper (BGP/OPTIONAL/UNION/MINUS/GRAPH join
//! chain, property-path evaluation, `Deadline::check`) returned
//! `Result<_, String>`. Callers that need to distinguish failure modes were
//! reduced to matching on message substrings — e.g.
//! `sparql_endpoint::query::query_execution_error_response` used to check
//! `message.contains("exceeded the configured timeout")` to decide between
//! HTTP 503 and 500. See
//! [#460](https://github.com/daghovland/rdf-datalog/issues/460), part of
//! the error-handling epic [#453](https://github.com/daghovland/rdf-datalog/issues/453).
//!
//! There are only three places the executor actually *constructs* an error
//! (everywhere else propagates one of these via `?`, mainly
//! `deadline.check()?` at loop-iteration boundaries) — see
//! `sparql_parser/src/execute/mod.rs::execute_inner` and
//! `sparql_parser/src/deadline.rs::Deadline::check`.

use std::fmt;

/// An error that occurred while evaluating a parsed SPARQL query against a
/// [`dag_rdf::Datastore`] (as opposed to parsing the query text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    /// A non-`SILENT` `SERVICE <endpoint> { ... }` clause was rejected
    /// because remote network access is disabled
    /// ([`ingress::NetworkPolicy::Deny`], the default). `endpoint` is the
    /// `Debug`-formatted SERVICE IRI term.
    ///
    /// See <https://github.com/daghovland/rdf-datalog/issues/51>.
    ServiceDenied {
        /// The rejected SERVICE clause's endpoint term, `Debug`-formatted.
        endpoint: String,
    },
    /// A non-`SILENT` `SERVICE` clause was attempted with
    /// `--network=allow`, but SPARQL federation execution itself isn't
    /// implemented yet.
    ///
    /// See <https://github.com/daghovland/rdf-datalog/issues/51>.
    ServiceNotImplemented,
    /// The query's configured wall-clock timeout elapsed before evaluation
    /// completed. Cooperative cancellation — see [`crate::deadline`] for why
    /// — so this is only ever raised at a loop-iteration boundary; work
    /// already in flight between two checks runs to completion first.
    ///
    /// See <https://github.com/daghovland/rdf-datalog/issues/372>.
    Timeout,
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecError::ServiceDenied { endpoint } => write!(
                f,
                "SERVICE <{endpoint}> was rejected: remote network access is disabled. \
                 Start the server with --network=allow to enable federated queries. \
                 See https://github.com/daghovland/rdf-datalog/issues/51"
            ),
            ExecError::ServiceNotImplemented => write!(
                f,
                "SERVICE federation is not yet implemented even with --network=allow. \
                 Track progress at https://github.com/daghovland/rdf-datalog/issues/51"
            ),
            ExecError::Timeout => write!(f, "query exceeded the configured timeout"),
        }
    }
}

impl std::error::Error for ExecError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_display_matches_previous_string_message() {
        // `sparql_endpoint::query::query_execution_error_response` used to
        // match this exact substring; keep the wording stable.
        assert_eq!(
            ExecError::Timeout.to_string(),
            "query exceeded the configured timeout"
        );
    }

    #[test]
    fn service_denied_display_mentions_endpoint_and_flag() {
        let msg = ExecError::ServiceDenied {
            endpoint: "Iri(\"https://example.org/sparql\")".to_string(),
        }
        .to_string();
        assert!(msg.contains("https://example.org/sparql"));
        assert!(msg.contains("--network=allow"));
    }

    #[test]
    fn service_not_implemented_display_mentions_flag() {
        let msg = ExecError::ServiceNotImplemented.to_string();
        assert!(msg.contains("not yet implemented"));
    }

    #[test]
    fn variants_are_matchable_not_just_string_comparable() {
        let e = ExecError::Timeout;
        assert!(matches!(e, ExecError::Timeout));
    }
}

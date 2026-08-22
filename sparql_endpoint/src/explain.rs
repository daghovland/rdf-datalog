/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! HTTP-layer glue for `?explain=true` (issue
//! [#537](https://github.com/daghovland/rdf-datalog/issues/537)).
//!
//! `sparql_parser::explain` produces the plan/report data as plain Rust
//! structs (no `serde` dependency in that crate — see
//! `docs/plans/EXPLAIN_ENDPOINT_537_PLAN.md`, Decision 3); this module
//! converts them to JSON here, where `serde_json` is already a dependency.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dag_rdf::Datastore;
use ingress::NetworkPolicy;
use serde_json::json;
use sparql_parser::execute::QueryResult;
use sparql_parser::explain::{ExplainPlan, PlanNode, explain_query, query_type_label};
use sparql_parser::{ast::Query, execute_with_base};
use std::time::{Duration, Instant};

/// True iff the `explain` query parameter requests EXPLAIN output.
/// URL-query-param only (see the plan doc's "Smaller decisions" — a
/// `application/x-www-form-urlencoded` POST body's `explain` field, if any,
/// is not read). Loose-boolean parsing (`"true"`/`"1"`, case-insensitive)
/// matches this codebase's existing convention for boolean-ish config
/// parameters.
pub(crate) fn is_explain_requested(params: &std::collections::HashMap<String, String>) -> bool {
    params
        .get("explain")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
}

/// Build the `?explain=true` JSON response: the static plan (always
/// present, computed without executing anything) plus the query's actual
/// execution outcome — on success, a query-type-appropriate result summary
/// and total wall-clock time; on failure (including the #372 cooperative
/// timeout), the same HTTP status `query_execution_error_response` would
/// have used, with the plan and elapsed time still included since the plan
/// is exactly what's most useful to see for a failing/slow query.
pub(crate) fn explain_query_response(
    query: &Query,
    store: &Datastore,
    network: NetworkPolicy,
    base: Option<&str>,
    timeout: Option<Duration>,
) -> Response {
    let plan = explain_query(query, store);
    let query_type = query_type_label(query);

    let start = Instant::now();
    let result = execute_with_base(query, store, network, base, timeout);
    let total_time_ms = start.elapsed().as_secs_f64() * 1000.0;

    let plan_json = plan_to_json(&plan);

    match result {
        Ok(query_result) => {
            let mut body = json!({
                "queryType": query_type,
                "totalTimeMs": total_time_ms,
                "plan": plan_json,
            });
            match query_result {
                QueryResult::Select(select_result) => {
                    body["rowCount"] = json!(select_result.rows.len());
                }
                QueryResult::Ask(boolean) => {
                    body["result"] = json!(boolean);
                }
                QueryResult::Construct(triples) | QueryResult::Describe(triples) => {
                    body["tripleCount"] = json!(triples.len());
                }
            }
            (StatusCode::OK, axum::Json(body)).into_response()
        }
        Err(message) => {
            let status = if message.contains("exceeded the configured timeout") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let body = json!({
                "queryType": query_type,
                "totalTimeMs": total_time_ms,
                "plan": plan_json,
                "error": message,
            });
            (status, axum::Json(body)).into_response()
        }
    }
}

fn plan_to_json(plan: &ExplainPlan) -> serde_json::Value {
    serde_json::Value::Array(plan.nodes.iter().map(node_to_json).collect())
}

fn node_to_json(node: &PlanNode) -> serde_json::Value {
    match node {
        PlanNode::Bgp { patterns } => json!({
            "kind": "BGP",
            "patterns": patterns.iter().map(|p| json!({
                "position": p.position,
                "pattern": p.pattern,
                "estimatedCardinality": p.estimated_cardinality,
                "indexUsed": p.index_used,
            })).collect::<Vec<_>>(),
        }),
        PlanNode::PathPattern { detail } => json!({"kind": "PathPattern", "detail": detail}),
        PlanNode::Subquery { plan } => json!({"kind": "Subquery", "plan": plan_to_json(plan)}),
        PlanNode::Optional { children } => {
            json!({"kind": "Optional", "children": plan_to_json(children)})
        }
        PlanNode::Union { left, right } => json!({
            "kind": "Union",
            "left": plan_to_json(left),
            "right": plan_to_json(right),
        }),
        PlanNode::Filter { detail } => json!({"kind": "Filter", "detail": detail}),
        PlanNode::Bind { detail } => json!({"kind": "Bind", "detail": detail}),
        PlanNode::Values { detail } => json!({"kind": "Values", "detail": detail}),
        PlanNode::Minus { children } => {
            json!({"kind": "Minus", "children": plan_to_json(children)})
        }
        PlanNode::Graph { detail, children } => json!({
            "kind": "Graph",
            "detail": detail,
            "children": plan_to_json(children),
        }),
        PlanNode::Group { children } => {
            json!({"kind": "Group", "children": plan_to_json(children)})
        }
        PlanNode::Service { detail } => json!({"kind": "Service", "detail": detail}),
    }
}

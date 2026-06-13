/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Authentication middleware for the SPARQL endpoint.
//!
//! See AUTH.md for the full design.  This module implements Tier 1 (static API key).

use axum::{
    extract::State,
    http::{
        HeaderMap, Method, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;

use crate::{AppState, AuthConfig};

// ── Permission classification ─────────────────────────────────────────────────

/// Which class of permission a request requires.
#[derive(Debug, PartialEq, Eq)]
pub enum Permission {
    Read,
    Write,
    Admin,
}

/// Map an HTTP method + path to the permission it requires.
///
/// Follows the operation table in AUTH.md:
/// - Admin: `POST /$/datasets` (create), `DELETE /$/datasets/{name}` (drop).
/// - Write: SPARQL Update (`POST …/update`), upload, GSP mutating methods.
/// - Read: everything else, including `POST /sparql` and `POST /{name}/sparql`
///   which carry SPARQL SELECT queries, not updates.
pub fn classify(method: &Method, path: &str) -> Permission {
    // Admin operations must be checked before the generic DELETE/PUT rule.
    if method == Method::POST && path == "/$/datasets" {
        return Permission::Admin;
    }
    if method == Method::DELETE && path.starts_with("/$/datasets/") {
        return Permission::Admin;
    }

    // Explicit write-POST endpoints (POST on SPARQL query endpoints is a Read).
    if method == Method::POST && (path == "/upload" || path.ends_with("/update")) {
        return Permission::Write;
    }

    // All PUT and DELETE (except the admin paths already matched above) are writes.
    if method == Method::PUT || method == Method::DELETE {
        return Permission::Write;
    }

    // POST on Graph Store Protocol endpoints is a write (append).
    if method == Method::POST
        && (path == "/rdf-graph-store"
            || path.starts_with("/rdf-graphs/")
            || path.ends_with("/data"))
    {
        return Permission::Write;
    }

    // GET, HEAD, OPTIONS, POST /sparql, POST /{name}/sparql, POST /{name}/query …
    Permission::Read
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

/// Constant-time byte-slice comparison (content only; length check is non-constant).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        "Unauthorized",
    )
        .into_response()
}

// ── Middleware ────────────────────────────────────────────────────────────────

/// Axum middleware that enforces the configured authentication policy.
///
/// Applied globally; the `classify` function decides per-request which
/// permission is needed and whether the configured auth mode requires a check.
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let required = classify(request.method(), request.uri().path());

    match &state.config.auth {
        AuthConfig::None => next.run(request).await,
        AuthConfig::ApiKey {
            key,
            require_for_reads,
        } => {
            let needs_check = match required {
                Permission::Write | Permission::Admin => true,
                Permission::Read => *require_for_reads,
            };

            if !needs_check {
                return next.run(request).await;
            }

            match extract_bearer(request.headers()) {
                Some(token) if constant_time_eq(token.as_bytes(), key.as_bytes()) => {
                    next.run(request).await
                }
                _ => unauthorized_response(),
            }
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_sparql_is_read() {
        assert_eq!(classify(&Method::GET, "/sparql"), Permission::Read);
    }

    #[test]
    fn post_sparql_query_is_read() {
        // Canary: POST /sparql carries a SELECT body, not an update.
        assert_eq!(classify(&Method::POST, "/sparql"), Permission::Read);
    }

    #[test]
    fn post_dataset_sparql_is_read() {
        assert_eq!(classify(&Method::POST, "/ds/sparql"), Permission::Read);
    }

    #[test]
    fn post_dataset_query_is_read() {
        assert_eq!(classify(&Method::POST, "/ds/query"), Permission::Read);
    }

    #[test]
    fn post_update_is_write() {
        assert_eq!(classify(&Method::POST, "/ds/update"), Permission::Write);
    }

    #[test]
    fn post_upload_is_write() {
        assert_eq!(classify(&Method::POST, "/upload"), Permission::Write);
    }

    #[test]
    fn put_gsp_is_write() {
        assert_eq!(
            classify(&Method::PUT, "/rdf-graph-store"),
            Permission::Write
        );
    }

    #[test]
    fn delete_gsp_is_write() {
        assert_eq!(
            classify(&Method::DELETE, "/rdf-graph-store"),
            Permission::Write
        );
    }

    #[test]
    fn post_gsp_is_write() {
        assert_eq!(
            classify(&Method::POST, "/rdf-graph-store"),
            Permission::Write
        );
    }

    #[test]
    fn put_dataset_data_is_write() {
        assert_eq!(classify(&Method::PUT, "/ds/data"), Permission::Write);
    }

    #[test]
    fn delete_dataset_data_is_write() {
        assert_eq!(classify(&Method::DELETE, "/ds/data"), Permission::Write);
    }

    #[test]
    fn post_dataset_data_is_write() {
        assert_eq!(classify(&Method::POST, "/ds/data"), Permission::Write);
    }

    #[test]
    fn post_datasets_is_admin() {
        assert_eq!(classify(&Method::POST, "/$/datasets"), Permission::Admin);
    }

    #[test]
    fn delete_dataset_is_admin() {
        assert_eq!(
            classify(&Method::DELETE, "/$/datasets/myds"),
            Permission::Admin
        );
    }

    #[test]
    fn get_datasets_is_read() {
        assert_eq!(classify(&Method::GET, "/$/datasets"), Permission::Read);
    }

    #[test]
    fn get_server_info_is_read() {
        assert_eq!(classify(&Method::GET, "/$/server"), Permission::Read);
    }
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! `GET /backlog` — a read-only, schema-*specific* dashboard over this
//! repository's own `bl:`/`agp:` backlog and provenance data.
//!
//! Deliberately a separate route and file from [`crate::frontend`]
//! (`GET /`), which is schema-*agnostic* by design. See
//! `docs/plans/BACKLOG_PROVENANCE_DASHBOARD_PLAN.md` ("Decision: dogfood
//! tool, not a product feature") and
//! [issue #381](https://github.com/daghovland/rdf-datalog/issues/381) for
//! the rationale. Follows the exact same `include_str!` + plain handler
//! pattern as `frontend.rs`.

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

const BACKLOG_FRONTEND_HTML: &str = include_str!("backlog_frontend.html");

pub async fn serve_backlog_frontend() -> Response {
    (StatusCode::OK, Html(BACKLOG_FRONTEND_HTML)).into_response()
}

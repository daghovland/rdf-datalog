/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

const FRONTEND_HTML: &str = include_str!("frontend.html");

pub async fn serve_frontend() -> Response {
    (StatusCode::OK, Html(FRONTEND_HTML)).into_response()
}

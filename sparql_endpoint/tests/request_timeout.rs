/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the request-level `TimeoutLayer`
//! (`Config::request_timeout_secs`), covering
//! <https://github.com/daghovland/rdf-datalog/issues/367>.
//!
//! Before this fix, the router applied no timeout at all: a stalled or
//! adversarially slow client (e.g. one that sends a `Content-Length` header
//! but then never finishes sending the body) could hold a connection open
//! indefinitely, since axum's body extraction just waits for more bytes.
//!
//! These tests avoid making a request slow by making its *payload* large
//! (parse time is machine-dependent and would flake in CI); instead they
//! open a raw TCP connection, send a well-formed request line + headers
//! advertising more body bytes than are actually sent, and then stop writing
//! — a request that is slow "by construction", deterministically, regardless
//! of machine speed.

mod common;

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Extract the `host:port` authority from a `http://host:port` base URL.
fn authority(base_url: &str) -> &str {
    base_url
        .strip_prefix("http://")
        .expect("test server base_url must be http://host:port")
}

/// A request whose body never fully arrives must be cut off by the
/// `request_timeout_secs` `TimeoutLayer` with a `408 Request Timeout`,
/// instead of hanging until the test (or the connection) times out on its
/// own.
#[tokio::test]
async fn slow_request_is_cut_off_with_408() {
    let server = common::TestServer::start_writable_with_request_timeout("", 1).await;

    let mut stream = TcpStream::connect(authority(&server.base_url))
        .await
        .expect("connect failed");

    // Advertise a 10000-byte body, but only send 10 bytes and then stop —
    // the server will keep waiting for the rest until the timeout fires.
    let request = "POST /upload HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: text/turtle\r\n\
         Content-Length: 10000\r\n\
         Connection: close\r\n\r\n\
         @prefix ";
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write failed");

    let mut response = Vec::new();
    let read_result =
        tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut response)).await;

    let response = String::from_utf8_lossy(&response).to_string();
    assert!(
        read_result.is_ok(),
        "server must respond (or close the connection) within 10s of the \
         1s configured request_timeout_secs — it hung instead"
    );
    assert!(
        response.starts_with("HTTP/1.1 408") || response.starts_with("HTTP/1.0 408"),
        "expected a 408 Request Timeout response, got: {response:?}"
    );
}

/// A normal, fast request must still succeed under a short configured
/// timeout — the timeout must not fire on ordinary, promptly-completed
/// traffic.
#[tokio::test]
async fn fast_request_succeeds_under_short_timeout() {
    let server = common::TestServer::start_writable_with_request_timeout("", 1).await;

    let resp = server
        .client
        .get(format!("{}/$/ping", server.base_url))
        .send()
        .await
        .expect("request failed");
    assert!(
        resp.status().is_success(),
        "a fast request must succeed even under a 1s request_timeout_secs, got {}",
        resp.status()
    );
}

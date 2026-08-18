/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the write-route concurrency limit
//! (`Config::max_concurrent_writes`, a shared `GlobalConcurrencyLimitLayer`
//! across `/upload`, `/rdf-graph-store`, `/rdf-graphs/*`, `/{name}/data`,
//! `/{name}/rml`, `/{name}/shacl`), covering
//! <https://github.com/daghovland/rdf-datalog/issues/526>.
//!
//! The (N+1)th-request-queues assertion below is deliberately *not* a
//! wall-clock race. #367's original deferral of this feature flagged
//! exactly this kind of test as flakiness-prone (a real semaphore probe or
//! synthetic slow requests, timing-sensitive), and this session hit that
//! same flakiness class first-hand on issue #527/PR #543 (a debug-build-only
//! timing margin that broke under CI's release profile).
//!
//! Instead: two raw TCP connections send `/upload` requests with an exact
//! `Content-Length`. Connection A's very last body byte is withheld under
//! the *test's* explicit control (not a timer) — the server cannot respond
//! to A until we choose to send that byte. While A is held open, connection
//! B (a complete, valid request) is sent in full. We assert B has received
//! *no* bytes at all while A is withheld: for a correct implementation this
//! holds for any wait duration, so that assertion can only ever produce a
//! false negative (silently missing a real regression on a pathologically
//! slow CI box) rather than a false positive (a flaky failure on correct
//! code) — the two failure directions PR #543 taught to tell apart. Only
//! the initial "let A's connection get routed and reserve its permit before
//! B connects" ordering uses a fixed sleep, and that sleep bounds OS/tokio
//! scheduling latency (accepting a loopback connection, parsing headers,
//! one `poll_ready`), not a CPU-bound computation whose duration depends on
//! debug vs. release optimization — the exact axis that broke PR #543's
//! original margin.

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

/// Build a raw HTTP/1.1 `POST /upload` request with an exact `Content-Length`
/// matching `body`, and `Connection: close` so the server closes the socket
/// once it has responded (letting `read_to_end` detect completion).
fn upload_request(body: &str) -> String {
    format!(
        "POST /upload HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: text/turtle\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {}",
        body.len(),
        body
    )
}

/// With `max_concurrent_writes: 1`, a second concurrent write request to a
/// write-class route must not even begin (get zero response bytes) until the
/// first one's handler has fully completed and released its permit.
#[tokio::test]
async fn second_concurrent_write_queues_until_first_completes() {
    let server = common::TestServer::start_writable_with_max_concurrent_writes("", 1).await;

    let body_a = "<http://example.org/a> <http://example.org/p> <http://example.org/o> .\n";
    let body_b = "<http://example.org/b> <http://example.org/p> <http://example.org/o> .\n";
    let request_a = upload_request(body_a);
    let request_b = upload_request(body_b);

    // Withhold A's very last byte: the server has A's full headers (so the
    // route is dispatched and the single write-concurrency permit is
    // reserved) but the Turtle body extractor is left waiting on that last
    // byte, holding the permit open under the test's control.
    let (a_prefix, a_last_byte) = request_a.split_at(request_a.len() - 1);

    let mut stream_a = TcpStream::connect(authority(&server.base_url))
        .await
        .expect("connect A failed");
    stream_a
        .write_all(a_prefix.as_bytes())
        .await
        .expect("write A prefix failed");

    // Let the server actually route A's request and acquire the permit
    // before B connects. This bounds OS/tokio scheduling latency for a
    // single loopback accept + header parse + poll_ready, not a CPU-bound
    // computation — generous headroom here only wastes wall-clock, it
    // cannot make the test flaky (see module doc comment).
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut stream_b = TcpStream::connect(authority(&server.base_url))
        .await
        .expect("connect B failed");
    stream_b
        .write_all(request_b.as_bytes())
        .await
        .expect("write B failed");

    // B must receive nothing while A holds the only write permit. This can
    // only under-detect a bug (false negative on a very slow box), never
    // flake on correct code: a correct implementation produces zero bytes
    // on B's socket regardless of how long we wait here.
    let mut probe = [0u8; 1];
    let premature =
        tokio::time::timeout(Duration::from_millis(500), stream_b.read(&mut probe)).await;
    assert!(
        premature.is_err(),
        "B must still be queued behind A's held write-concurrency permit, \
         but received data before A completed"
    );

    // Release A by sending its final byte, completing its body.
    stream_a
        .write_all(a_last_byte.as_bytes())
        .await
        .expect("write A final byte failed");

    let mut response_a = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        stream_a.read_to_end(&mut response_a),
    )
    .await
    .expect("A must respond promptly once its body completes")
    .expect("read A response failed");
    let response_a = String::from_utf8_lossy(&response_a);
    assert!(
        response_a.starts_with("HTTP/1.1 200"),
        "expected A to succeed once its permit was released, got: {response_a:?}"
    );

    // Now that A released the permit, B must proceed and succeed too.
    let mut response_b = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        stream_b.read_to_end(&mut response_b),
    )
    .await
    .expect("B must respond promptly once A's permit was released")
    .expect("read B response failed");
    let response_b = String::from_utf8_lossy(&response_b);
    assert!(
        response_b.starts_with("HTTP/1.1 200"),
        "expected B to succeed after being unblocked, got: {response_b:?}"
    );
}

/// Ordinary, non-concurrent write traffic must still succeed under a small
/// configured limit — the limit must not fire on traffic that never exceeds
/// it.
#[tokio::test]
async fn sequential_writes_succeed_under_small_limit() {
    let server = common::TestServer::start_writable_with_max_concurrent_writes("", 1).await;

    for i in 0..3 {
        let resp = server
            .client
            .post(format!("{}/upload", server.base_url))
            .header("Content-Type", "text/turtle")
            .body(format!(
                "<http://example.org/s{i}> <http://example.org/p> <http://example.org/o> .\n"
            ))
            .send()
            .await
            .expect("upload request failed");
        assert!(
            resp.status().is_success(),
            "sequential upload {i} must succeed under max_concurrent_writes=1, got {}",
            resp.status()
        );
    }
}

/// The production default (`Config::default().max_concurrent_writes`) must
/// be generous enough that ordinary concurrent traffic in tests elsewhere
/// (and in real deployments) is never throttled by it.
#[tokio::test]
async fn default_limit_does_not_throttle_ordinary_concurrency() {
    let server = common::TestServer::start_writable("").await;

    let mut handles = Vec::new();
    for i in 0..8 {
        let client = server.client.clone();
        let url = format!("{}/upload", server.base_url);
        handles.push(tokio::spawn(async move {
            client
                .post(url)
                .header("Content-Type", "text/turtle")
                .body(format!(
                    "<http://example.org/s{i}> <http://example.org/p> <http://example.org/o> .\n"
                ))
                .send()
                .await
                .expect("upload request failed")
                .status()
        }));
    }
    for handle in handles {
        let status = handle.await.expect("task panicked");
        assert!(
            status.is_success(),
            "concurrent upload under the default max_concurrent_writes must succeed, got {status}"
        );
    }
}

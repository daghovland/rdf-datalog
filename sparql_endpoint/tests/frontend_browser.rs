/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Browser-automation tests for the Dagalog web UI.
//!
//! These tests require geckodriver (Firefox WebDriver) to be running.
//! Start it before running this test file:
//!
//! ```bash
//! geckodriver --port 4444 &
//! cargo test --test frontend_browser
//! ```
//!
//! Override the WebDriver URL with `WEBDRIVER_URL=http://host:port`.
//! Tests are skipped silently when geckodriver is unreachable.
//! Requires the `browser-tests` feature: `cargo test -p sparql-endpoint --features browser-tests`

#![cfg(feature = "browser-tests")]

mod common;

use fantoccini::{ClientBuilder, Locator};
use serde_json::{Map, json};
use std::time::{Duration, Instant};

// ── Test fixture ─────────────────────────────────────────────────────────────

const FIXTURE: &str = r#"
@prefix ex:   <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:  <http://www.w3.org/2002/07/owl#> .

ex:alice a ex:Person ;
    rdfs:label "Alice" ;
    ex:knows ex:bob .

ex:bob a ex:Person ;
    rdfs:label "Bob" .

ex:Person a owl:Class .
"#;

// ── WebDriver helpers ─────────────────────────────────────────────────────────

/// Connect to geckodriver (headless Firefox). Returns `None` and prints a skip
/// message when geckodriver is not reachable.
async fn connect_driver() -> Option<fantoccini::Client> {
    let url =
        std::env::var("WEBDRIVER_URL").unwrap_or_else(|_| "http://localhost:4444".to_string());

    let mut caps = Map::new();
    caps.insert(
        "moz:firefoxOptions".to_string(),
        json!({ "args": ["-headless"] }),
    );

    match ClientBuilder::native()
        .capabilities(caps)
        .connect(&url)
        .await
    {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!(
                "[SKIP] frontend_browser tests: geckodriver not available at {url}: {e}\n\
                 Start with: geckodriver --port 4444 &"
            );
            None
        }
    }
}

/// Poll until `selector` is visible and non-empty, or timeout elapses.
async fn wait_for(client: &fantoccini::Client, selector: &str, timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Ok(el) = client.find(Locator::Css(selector)).await {
            if el.text().await.map(|t| !t.is_empty()).unwrap_or(false) {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Resource browser shows outgoing edges for a known IRI.
///
/// alice has three outgoing triples: rdf:type, rdfs:label, ex:knows.
#[tokio::test]
async fn resource_browser_shows_outgoing_edges() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;

    let resource_iri = "http://example.org/alice";
    let url = format!(
        "{}/?resource={}",
        server.base_url,
        urlencoding::encode(resource_iri)
    );
    driver.goto(&url).await.unwrap();

    assert!(
        wait_for(&driver, "#out-table .count", 3000).await,
        "outgoing-edge count never appeared"
    );

    let count_text = driver
        .find(Locator::Css("#out-table .count"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        count_text.contains('3'),
        "expected 3 outgoing edges for alice, got: {count_text}"
    );

    driver.close().await.unwrap();
}

/// Resource browser shows incoming edges for a known IRI.
///
/// bob is the object of `ex:alice ex:knows ex:bob`, so has one incoming edge.
#[tokio::test]
async fn resource_browser_shows_incoming_edges() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;

    let resource_iri = "http://example.org/bob";
    let url = format!(
        "{}/?resource={}",
        server.base_url,
        urlencoding::encode(resource_iri)
    );
    driver.goto(&url).await.unwrap();

    assert!(
        wait_for(&driver, "#in-table .count", 3000).await,
        "incoming-edge count never appeared"
    );

    let count_text = driver
        .find(Locator::Css("#in-table .count"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        count_text.contains('1'),
        "expected 1 incoming edge for bob, got: {count_text}"
    );

    driver.close().await.unwrap();
}

/// The resource heading shows a shortened IRI when a prefix applies.
///
/// `http://example.org/alice` has no common prefix, so the full IRI appears
/// in `#resource-full-iri`. The heading should say "Resource" (no known prefix).
#[tokio::test]
async fn resource_browser_displays_iri() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;

    let resource_iri = "http://example.org/alice";
    let url = format!(
        "{}/?resource={}",
        server.base_url,
        urlencoding::encode(resource_iri)
    );
    driver.goto(&url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let full_iri_text = driver
        .find(Locator::Css("#resource-full-iri"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(
        full_iri_text, resource_iri,
        "#resource-full-iri should contain the full IRI"
    );

    // A well-known namespace IRI should show a shortened heading.
    let owl_url = format!(
        "{}/?resource={}",
        server.base_url,
        urlencoding::encode("http://www.w3.org/2002/07/owl#Class")
    );
    driver.goto(&owl_url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let heading = driver
        .find(Locator::Css("#resource-heading"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(
        heading, "owl:Class",
        "heading should be the shortened owl:Class"
    );

    driver.close().await.unwrap();
}

/// Clicking an IRI link in SPARQL query results navigates to the resource browser.
#[tokio::test]
async fn clicking_iri_in_results_navigates_to_resource_browser() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;

    // Load the query view with a query that returns alice as ?s.
    let query = "SELECT ?s WHERE { <http://example.org/alice> ?p ?s } LIMIT 10";
    let url = format!("{}/?query={}", server.base_url, urlencoding::encode(query));
    driver.goto(&url).await.unwrap();

    assert!(
        wait_for(&driver, "#query-result .count", 3000).await,
        "query results never appeared"
    );

    // The result contains ex:bob as an object IRI — click that link.
    let bob_link = driver
        .find(Locator::LinkText("http://example.org/bob"))
        .await
        .unwrap_or_else(|_| panic!("link for bob not found in results"));
    bob_link.click().await.unwrap();

    // Should now be on the resource browser for bob.
    assert!(
        wait_for(&driver, "#out-table .count", 3000).await,
        "resource browser for bob never loaded"
    );

    let current = driver.current_url().await.unwrap();
    assert!(
        current.as_str().contains("resource="),
        "URL should contain 'resource=', got: {current}"
    );
    assert!(
        current.as_str().contains("example.org%2Fbob")
            || current.as_str().contains("example.org/bob"),
        "URL should reference bob, got: {current}"
    );

    driver.close().await.unwrap();
}

/// The back link on the resource page navigates to the query editor root.
#[tokio::test]
async fn back_link_returns_to_query_editor() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;

    let url = format!(
        "{}/?resource={}",
        server.base_url,
        urlencoding::encode("http://example.org/alice")
    );
    driver.goto(&url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    driver
        .find(Locator::Css("#back-link"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // After clicking back, the query editor should be visible.
    let query_view = driver.find(Locator::Css("#query-view")).await.unwrap();
    let display = query_view.attr("style").await.unwrap().unwrap_or_default();
    assert!(
        !display.contains("display: none") && !display.contains("display:none"),
        "query-view should be visible after clicking back, style={display}"
    );

    driver.close().await.unwrap();
}

/// Navigating to a resource with no triples shows empty result tables (not an error).
#[tokio::test]
async fn resource_browser_handles_unknown_iri_gracefully() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;

    let url = format!(
        "{}/?resource={}",
        server.base_url,
        urlencoding::encode("http://example.org/nobody")
    );
    driver.goto(&url).await.unwrap();

    assert!(
        wait_for(&driver, "#out-table .count", 3000).await,
        "outgoing count never appeared for unknown IRI"
    );

    let out_text = driver
        .find(Locator::Css("#out-table .count"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        out_text.contains('0'),
        "expected 0 outgoing edges for unknown IRI, got: {out_text}"
    );

    driver.close().await.unwrap();
}

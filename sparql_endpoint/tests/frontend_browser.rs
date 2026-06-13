/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Browser-automation tests for the Dagalog web UI.
//!
//! These tests require geckodriver (Firefox WebDriver) to be running:
//!
//! ```bash
//! geckodriver --port 4444 &
//! cargo test --test frontend_browser
//! ```
//!
//! Override the WebDriver URL with `WEBDRIVER_URL`. Tests are skipped silently
//! when geckodriver is unreachable.

mod common;

use std::time::{Duration, Instant};
use thirtyfour::components::SelectElement;
use thirtyfour::prelude::*;

// ── Fixtures ──────────────────────────────────────────────────────────────────

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
ex:Animal a owl:Class .
ex:Person rdfs:subClassOf ex:Animal .
"#;

// ── WebDriver helpers ─────────────────────────────────────────────────────────

async fn connect_driver() -> Option<WebDriver> {
    let url =
        std::env::var("WEBDRIVER_URL").unwrap_or_else(|_| "http://localhost:4444".to_string());
    let mut caps = DesiredCapabilities::firefox();
    caps.set_headless().ok();
    match WebDriver::new(&url, caps).await {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!(
                "[SKIP] frontend_browser: geckodriver not available at {url}: {e}\n\
                 Start with: geckodriver --port 4444 &"
            );
            None
        }
    }
}

async fn wait_for_text(driver: &WebDriver, selector: &str, timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Ok(el) = driver.find(By::Css(selector)).await {
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

async fn wait_for_element(driver: &WebDriver, selector: &str, timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if driver.find(By::Css(selector)).await.is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

// ── Resource browser ──────────────────────────────────────────────────────────

#[tokio::test]
async fn resource_browser_shows_outgoing_edges() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver
        .goto(&format!(
            "{}/?resource={}",
            server.base_url,
            urlencoding::encode("http://example.org/alice")
        ))
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#out-table .count", 4000).await);
    let text = driver
        .find(By::Css("#out-table .count"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.contains('3'), "expected 3 outgoing edges, got: {text}");
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn resource_browser_shows_incoming_edges() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver
        .goto(&format!(
            "{}/?resource={}",
            server.base_url,
            urlencoding::encode("http://example.org/bob")
        ))
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#in-table .count", 4000).await);
    let text = driver
        .find(By::Css("#in-table .count"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.contains('1'), "expected 1 incoming edge, got: {text}");
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn resource_browser_shows_rdfs_label_as_heading() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver
        .goto(&format!(
            "{}/?resource={}",
            server.base_url,
            urlencoding::encode("http://example.org/alice")
        ))
        .await
        .unwrap();
    wait_for_text(&driver, "#out-table .count", 4000).await;
    let heading = driver
        .find(By::Css("#resource-heading"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(heading, "Alice");
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn resource_browser_shortens_known_namespace_in_heading() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver
        .goto(&format!(
            "{}/?resource={}",
            server.base_url,
            urlencoding::encode("http://www.w3.org/2002/07/owl#Class")
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let heading = driver
        .find(By::Css("#resource-heading"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(heading, "owl:Class");
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn resource_browser_handles_unknown_iri_gracefully() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver
        .goto(&format!(
            "{}/?resource={}",
            server.base_url,
            urlencoding::encode("http://example.org/nobody")
        ))
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#out-table .count", 4000).await);
    let text = driver
        .find(By::Css("#out-table .count"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.contains('0'), "expected 0 outgoing edges, got: {text}");
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn clicking_iri_in_results_navigates_to_resource_browser() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    let query = "SELECT ?o WHERE { <http://example.org/alice> <http://example.org/knows> ?o }";
    driver
        .goto(&format!(
            "{}/?query={}",
            server.base_url,
            urlencoding::encode(query)
        ))
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#query-result .count", 4000).await);
    driver
        .find(By::Css("#query-result td a.uri"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    assert!(
        wait_for_text(&driver, "#out-table .count", 4000).await,
        "resource browser never loaded"
    );
    let cur = driver.current_url().await.unwrap();
    assert!(
        cur.as_str().contains("resource="),
        "URL should contain resource=, got: {cur}"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn back_link_returns_to_query_editor() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver
        .goto(&format!(
            "{}/?resource={}",
            server.base_url,
            urlencoding::encode("http://example.org/alice")
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css(".back-link"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let style = driver
        .find(By::Css("#query-view"))
        .await
        .unwrap()
        .attr("style")
        .await
        .unwrap()
        .unwrap_or_default();
    assert!(
        !style.contains("display: none") && !style.contains("display:none"),
        "query-view should be visible after back, style={style}"
    );
    driver.quit().await.unwrap();
}

// ── Graph tab ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn three_variable_query_shows_graph_tab() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    let query = "SELECT ?s ?p ?o WHERE { ?s ?p ?o }";
    driver
        .goto(&format!(
            "{}/?query={}",
            server.base_url,
            urlencoding::encode(query)
        ))
        .await
        .unwrap();
    assert!(
        wait_for_element(&driver, "#tab-graph", 4000).await,
        "Graph tab never appeared for 3-variable query"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn two_variable_query_has_no_graph_tab() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    let query = "SELECT ?s ?p WHERE { ?s ?p <http://example.org/bob> }";
    driver
        .goto(&format!(
            "{}/?query={}",
            server.base_url,
            urlencoding::encode(query)
        ))
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#query-result .count", 4000).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let tab = driver.find(By::Css("#tab-graph")).await;
    assert!(
        tab.is_err(),
        "Graph tab should NOT appear for 2-variable query"
    );
    driver.quit().await.unwrap();
}

// ── Query templates ───────────────────────────────────────────────────────────

#[tokio::test]
async fn query_template_fills_textarea() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver.goto(&server.base_url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let select_el = driver.find(By::Css("#query-template")).await.unwrap();
    SelectElement::new(&select_el)
        .await
        .unwrap()
        .select_by_value("classes")
        .await
        .unwrap();

    let val = driver
        .find(By::Css("#query"))
        .await
        .unwrap()
        .value()
        .await
        .unwrap()
        .unwrap_or_default();
    assert!(
        val.contains("owl:Class") || val.contains("owl#Class"),
        "template should fill textarea, got: {val}"
    );
    driver.quit().await.unwrap();
}

// ── Keyboard shortcut ─────────────────────────────────────────────────────────

#[tokio::test]
async fn ctrl_enter_runs_query() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver.goto(&server.base_url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let textarea = driver.find(By::Css("#query")).await.unwrap();
    textarea.click().await.unwrap();
    textarea
        .send_keys(Key::Control + Key::Return)
        .await
        .unwrap();
    assert!(
        wait_for_text(&driver, "#query-result .count", 4000).await,
        "Ctrl+Enter did not trigger query"
    );
    driver.quit().await.unwrap();
}

// ── Result export ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn export_buttons_appear_after_query() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    let query = "SELECT ?s WHERE { ?s a <http://example.org/Person> }";
    driver
        .goto(&format!(
            "{}/?query={}",
            server.base_url,
            urlencoding::encode(query)
        ))
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#query-result .count", 4000).await);
    assert!(
        wait_for_element(&driver, "#btn-export-csv", 2000).await,
        "CSV export button never appeared"
    );
    assert!(
        wait_for_element(&driver, "#btn-export-json", 2000).await,
        "JSON export button never appeared"
    );
    driver.quit().await.unwrap();
}

// ── Class hierarchy view ──────────────────────────────────────────────────────

#[tokio::test]
async fn class_hierarchy_view_renders_subclass_tree() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver
        .goto(&format!("{}/?view=classes", server.base_url))
        .await
        .unwrap();
    // The tree should not say "No rdfs:subClassOf triples found"
    assert!(
        wait_for_text(&driver, "#class-tree", 4000).await,
        "class-tree never populated"
    );
    let text = driver
        .find(By::Css("#class-tree"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !text.contains("No rdfs:subClassOf triples found"),
        "class hierarchy reported no subClassOf triples, got: {text}"
    );
    assert!(
        text.contains("Animal") || text.contains("Person"),
        "expected Animal or Person in class tree, got: {text}"
    );
    driver.quit().await.unwrap();
}

// ── Drag-and-drop upload ──────────────────────────────────────────────────────

#[tokio::test]
async fn upload_panel_has_drag_drop_zone() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    driver.goto(&server.base_url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let zone = driver.find(By::Css("#upload-dropzone")).await;
    assert!(zone.is_ok(), "drag-and-drop upload zone not found");
    driver.quit().await.unwrap();
}

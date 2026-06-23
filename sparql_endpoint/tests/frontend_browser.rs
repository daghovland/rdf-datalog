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
        if let Ok(el) = driver.find(By::Css(selector)).await
            && el.text().await.map(|t| !t.is_empty()).unwrap_or(false)
        {
            return true;
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

// ═══════════════════════════════════════════════════════════════════════════════
// Visual Query Builder — browser tests (Layer 3a + 3b).
// All ignored; activate a phase with:
//   grep -n "QB Phase 1" sparql_endpoint/tests/frontend_browser.rs
//
// QB_FIXTURE is richer than FIXTURE: two classes, object + data properties.
// ═══════════════════════════════════════════════════════════════════════════════

const QB_FIXTURE: &str = r#"
@prefix ex:   <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:  <http://www.w3.org/2002/07/owl#> .

ex:alice a ex:Person ; rdfs:label "Alice" ; ex:age "30" ;
         ex:knows ex:bob ; ex:worksFor ex:acme .
ex:bob   a ex:Person ; rdfs:label "Bob"   ; ex:age "25" .
ex:acme  a ex:Company ; rdfs:label "Acme Corp" ; ex:revenue "1000000" .

ex:Person  a owl:Class .
ex:Company a owl:Class .
"#;

// ── QB Layer 3a: JS self-test harness ────────────────────────────────────────
// Un-ignore after QB Phase 1 implements generateSparql() so QB_SELF_TESTS pass.

#[tokio::test]
async fn qb_js_self_test_suite_passes() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start("").await;
    driver
        .goto(&format!("{}/?view=build-selftest", server.base_url))
        .await
        .unwrap();
    assert!(
        wait_for_text(&driver, "#qb-test-results", 5000).await,
        "#qb-test-results never populated"
    );
    let json_text = driver
        .find(By::Css("#qb-test-results"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let results: serde_json::Value =
        serde_json::from_str(&json_text).expect("#qb-test-results must be valid JSON");
    assert_eq!(
        results["failed"],
        0,
        "JS self-tests failed:\n{}",
        serde_json::to_string_pretty(&results["errors"]).unwrap_or_default()
    );
    driver.quit().await.unwrap();
}

// ── QB Phase 1: class picker + single-level property pane ────────────────────
// Un-ignore when Phase 1 HTML+JS is implemented (#build-view, #class-picker,
// #data-prop-list, #obj-prop-list, #qb-generated, #btn-qb-run, #qb-results).

#[tokio::test]
async fn qb_build_view_is_reachable() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let view = driver.find(By::Css("#build-view")).await;
    assert!(view.is_ok(), "#build-view not found");
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn qb_class_picker_populates_from_store() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    assert!(
        wait_for_element(&driver, "#class-list option", 4000).await,
        "#class-list option never appeared"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn qb_selecting_class_populates_property_panes() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(
        wait_for_element(&driver, "#data-prop-list .prop-row", 4000).await,
        "data-prop-list never populated"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn qb_checking_data_prop_updates_sparql_preview() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#data-prop-list input[type=checkbox]", 4000).await);
    driver
        .find(By::Css("#data-prop-list input[type=checkbox]"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#qb-generated", 2000).await);
    let sparql = driver
        .find(By::Css("#qb-generated"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        sparql.contains("OPTIONAL"),
        "#qb-generated should contain OPTIONAL, got:\n{sparql}"
    );
    driver.quit().await.unwrap()
}

#[tokio::test]
async fn qb_run_button_executes_generated_query_and_shows_results() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#btn-qb-run"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    assert!(
        wait_for_text(&driver, "#qb-results .count", 4000).await,
        "result count never appeared after Run"
    );
    let err = driver.find(By::Css("#qb-results .msg.err")).await;
    assert!(err.is_err(), "Run must not produce an error banner");
    driver.quit().await.unwrap();
}

// ── QB Phase 2: multi-hop object-property expansion ──────────────────────────
// Un-ignore when Phase 2 is implemented (#node-canvas .node-card, .btn-follow,
// .btn-remove-node, active-node switching).

#[tokio::test]
async fn qb_following_obj_prop_adds_second_node_card() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#obj-prop-list .btn-follow", 4000).await);
    driver
        .find(By::Css("#obj-prop-list .btn-follow"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let cards = driver
        .find_all(By::Css("#node-canvas .node-card"))
        .await
        .unwrap();
    assert_eq!(
        cards.len(),
        2,
        "expected 2 node cards after following an object prop"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn qb_clicking_second_card_shifts_active_node() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#obj-prop-list .btn-follow", 4000).await);
    driver
        .find(By::Css("#obj-prop-list .btn-follow"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let cards = driver
        .find_all(By::Css("#node-canvas .node-card"))
        .await
        .unwrap();
    assert_eq!(cards.len(), 2);
    cards[1].click().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let classes = cards[1].class_name().await.unwrap().unwrap_or_default();
    assert!(
        classes.contains("active"),
        "second card should be .active after click, classes={classes}"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn qb_removing_linked_node_shrinks_generated_sparql() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#obj-prop-list .btn-follow", 4000).await);
    driver
        .find(By::Css("#obj-prop-list .btn-follow"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = driver
        .find(By::Css("#qb-generated"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    driver
        .find(By::Css(
            "#node-canvas .node-card:last-child .btn-remove-node",
        ))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = driver
        .find(By::Css("#qb-generated"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        before.len() > after.len(),
        "SPARQL should shrink after removing a node (before={}, after={})",
        before.len(),
        after.len()
    );
    driver.quit().await.unwrap();
}

// ── QB Phase 3: data-property filters ────────────────────────────────────────
// Un-ignore when Phase 3 is implemented (.prop-filter-input, FILTER in output).

#[tokio::test]
async fn qb_filter_input_appears_when_prop_checked() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#data-prop-list input[type=checkbox]", 4000).await);
    driver
        .find(By::Css("#data-prop-list input[type=checkbox]"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let filter = driver
        .find(By::Css("#data-prop-list .prop-filter-input"))
        .await;
    assert!(
        filter.is_ok(),
        "filter input should appear after checking a data property"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn qb_filter_text_appears_in_generated_sparql() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(QB_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#data-prop-list input[type=checkbox]", 4000).await);
    driver
        .find(By::Css("#data-prop-list input[type=checkbox]"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    driver
        .find(By::Css("#data-prop-list .prop-filter-input"))
        .await
        .unwrap()
        .send_keys("Alice")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let sparql = driver
        .find(By::Css("#qb-generated"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        sparql.contains("FILTER"),
        "#qb-generated should contain FILTER after typing filter value:\n{sparql}"
    );
    driver.quit().await.unwrap();
}

// ── VQS productive-extension index wiring ────────────────────────────────────
// Unlike QB_FIXTURE, this declares rdfs:domain/rdfs:range for ex:age so the
// navigation graph is non-empty and /vqs/productive-values reports `covered`.
// A single data property keeps the "first checkbox" selector unambiguous.

const VQS_FIXTURE: &str = r#"
@prefix ex:   <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

ex:age rdfs:domain ex:Person ; rdfs:range xsd:integer .

ex:alice a ex:Person ; ex:age 30 .
ex:bob   a ex:Person ; ex:age 25 .

ex:Person a owl:Class .
"#;

#[tokio::test]
async fn qb_checking_covered_data_prop_shows_known_value_count() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(VQS_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#data-prop-list input[type=checkbox]", 4000).await);
    driver
        .find(By::Css("#data-prop-list input[type=checkbox]"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    assert!(
        wait_for_text(&driver, "#data-prop-list .qb-prod-hint", 4000).await,
        "productive-value hint never populated after checking a covered data prop"
    );
    let hint = driver
        .find(By::Css("#data-prop-list .qb-prod-hint"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        hint.contains("2 known value"),
        "expected hint to report 2 known values, got:\n{hint}"
    );
    let datalist_options = driver
        .find_all(By::Css("#data-prop-list datalist option"))
        .await
        .unwrap();
    assert_eq!(
        datalist_options.len(),
        2,
        "expected 2 datalist options for the covered property"
    );
    driver.quit().await.unwrap();
}

#[tokio::test]
async fn qb_typing_unproductive_filter_value_shows_warning() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(VQS_FIXTURE).await;
    driver
        .goto(&format!("{}/?view=build", server.base_url))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    driver
        .find(By::Css("#class-picker"))
        .await
        .unwrap()
        .send_keys("http://example.org/Person\n")
        .await
        .unwrap();
    assert!(wait_for_element(&driver, "#data-prop-list input[type=checkbox]", 4000).await);
    driver
        .find(By::Css("#data-prop-list input[type=checkbox]"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    assert!(wait_for_text(&driver, "#data-prop-list .qb-prod-hint", 4000).await);
    driver
        .find(By::Css("#data-prop-list .prop-filter-input"))
        .await
        .unwrap()
        .send_keys("99")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let hint = driver
        .find(By::Css("#data-prop-list .qb-prod-hint"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        hint.contains("no known value matches"),
        "expected a dead-end warning after typing an unproductive value, got:\n{hint}"
    );
    driver.quit().await.unwrap();
}

// ── CONSTRUCT query rendering ─────────────────────────────────────────────────

/// Regression: `CONSTRUCT {?s ?p ?o} WHERE { ?s ?p ?o }` must render as a
/// `<pre>` block in the query result area — not trigger a JSON parse error.
#[tokio::test]
async fn construct_wildcard_renders_as_pre_block() {
    let driver = match connect_driver().await {
        Some(d) => d,
        None => return,
    };
    let server = common::TestServer::start(FIXTURE).await;
    let query = "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }";
    driver
        .goto(&format!(
            "{}/?query={}",
            server.base_url,
            urlencoding::encode(query)
        ))
        .await
        .unwrap();

    assert!(
        wait_for_element(&driver, "#query-result pre.msg", 4000).await,
        "expected a <pre class='msg'> in #query-result"
    );

    // Must not show an error message
    let err_present = driver.find(By::Css("#query-result .msg.err")).await.is_ok();
    assert!(
        !err_present,
        "CONSTRUCT result should not show an error message"
    );

    // The pre block must contain turtle/n-triples content with Alice's IRI
    let pre_text = driver
        .find(By::Css("#query-result pre.msg"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        pre_text.contains("http://example.org/alice") || pre_text.contains("(empty result)"),
        "pre block should contain turtle output, got:\n{pre_text}"
    );

    driver.quit().await.unwrap();
}

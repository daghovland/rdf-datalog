/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! HTTP-level checks for the LLM chat UI panel (#626, part of epic #184).
//!
//! `frontend_browser.rs` covers the same feature with WebDriver interaction
//! tests, but those silently skip without geckodriver (not installed in CI
//! or in this sandbox — see that file's module doc). This test instead GETs
//! the served frontend HTML directly with a plain HTTP client and asserts on
//! the markup/storage-key names, so at least one check for this feature
//! actually runs in CI.

mod common;

#[tokio::test]
async fn chat_panel_markup_present_with_distinct_storage_keys() {
    let server = common::TestServer::start("").await;
    let resp = reqwest::get(&server.base_url).await.expect("GET / failed");
    assert!(resp.status().is_success());
    let html = resp.text().await.expect("body");

    // Toggle button + panel + privacy modal exist.
    assert!(html.contains(r#"id="chat-toggle-btn""#));
    assert!(html.contains(r#"id="chat-panel""#));
    assert!(html.contains(r#"id="chat-privacy-modal""#));

    // Provider-key UI exists.
    assert!(html.contains(r#"id="chat-apikey-input""#));
    assert!(html.contains(r#"id="chat-clear-key-btn""#));

    // The provider key's storage key must be distinct from the dagalog
    // bearer-token/API-key storage keys, and the dagalog storage keys must
    // never be referenced from the new chat code (no accidental conflation
    // of the two credentials — see docs/plans/LLM_CHAT_ARCHITECTURE_PLAN.md §1).
    assert!(html.contains("dagalog-llm-provider-key"));
    // The dagalog bearer token / API key use sessionStorage, not localStorage;
    // the new provider key is explicitly localStorage-backed (persists across
    // sessions) and uses a visibly different key name than either dagalog
    // credential ('dagalog-api-key', 'dagalog-oidc-token').
    assert!(html.contains("localStorage.setItem(CHAT_APIKEY_KEY"));
    assert!(html.contains("const CHAT_APIKEY_KEY = 'dagalog-llm-provider-key'"));

    // Privacy notice is localStorage-gated ("don't show again" pattern, PR #255).
    assert!(html.contains("dagalog-llm-privacy-ack"));

    // Message history rendering uses textContent, never innerHTML, since
    // #628 will feed LLM-provided (untrusted) text through here.
    assert!(html.contains("body.textContent = text"));
    assert!(!html.contains("chat-msg').innerHTML"));
}

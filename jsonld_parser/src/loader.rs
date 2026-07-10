/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Document loader trait and implementations for external JSON-LD context fetching.
//!
//! # Overview
//!
//! When a JSON-LD document uses `"@context": "<url>"`, the parser must retrieve
//! the context document at that URL.  How (or whether) that fetch happens is
//! controlled by a [`DocumentLoader`].
//!
//! Three implementations ship out of the box:
//! - [`StaticDocumentLoader`] — a map of URL → JSON string; used for tests and
//!   the built-in static cache of well-known vocabularies.
//!
//! See [#82](https://github.com/daghovland/rdf-datalog/issues/82) for the full
//! implementation roadmap (HTTP fetching via `NetworkPolicy::Allow`).

use serde_json::Value;
use std::collections::HashMap;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Pluggable strategy for loading external JSON-LD context documents.
///
/// Implement this trait to control how `"@context": "<url>"` entries are
/// resolved when parsing JSON-LD with [`crate::parse_jsonld_with_loader`].
///
/// # Example
///
/// ```
/// use jsonld_parser::{DocumentLoader, StaticDocumentLoader};
/// let loader = StaticDocumentLoader::new([
///     ("https://example.org/ctx".to_string(),
///      r#"{"@context": {"ex": "https://example.org/"}}"#.to_string()),
/// ]);
/// ```
pub trait DocumentLoader: Send + Sync {
    /// Load the JSON-LD context document at `url`.
    ///
    /// Returns the parsed JSON value of the fetched document, or an error
    /// message string on failure.
    fn load(&self, url: &str) -> Result<Value, String>;
}

// ── StaticDocumentLoader ──────────────────────────────────────────────────────

/// A [`DocumentLoader`] backed by an in-memory map of URL → pre-parsed JSON value.
///
/// JSON strings are parsed once at construction time; subsequent `load()` calls
/// clone the pre-parsed [`Value`], avoiding repeated JSON parsing.
///
/// Useful for:
/// - Unit tests that need a deterministic, offline context.
/// - The built-in static cache of well-known vocabulary contexts.
///   Call [`StaticDocumentLoader::with_schema_org`] to get a loader that
///   pre-populates a minimal schema.org context stub.
pub struct StaticDocumentLoader {
    entries: HashMap<String, Value>,
}

impl StaticDocumentLoader {
    /// Create a loader from an iterable of `(url, json_string)` pairs.
    ///
    /// Panics if any JSON string is invalid (acceptable for static, compile-time entries).
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        let entries = entries
            .into_iter()
            .map(|(url, json)| {
                let value = serde_json::from_str(&json).unwrap_or_else(|e| {
                    panic!("StaticDocumentLoader: invalid JSON for \"{url}\": {e}")
                });
                (url, value)
            })
            .collect();
        Self { entries }
    }

    /// Create a loader pre-populated with a minimal schema.org context stub.
    ///
    /// The stub covers the most commonly used schema.org terms.  It is **not**
    /// the full schema.org vocabulary.  Extend with [`StaticDocumentLoader::new`]
    /// if you need additional terms.
    ///
    /// All four common schema.org URL forms resolve to the same stub:
    /// `https://schema.org/`, `http://schema.org/`, `https://schema.org`, `http://schema.org`.
    pub fn with_schema_org() -> Self {
        let stub: Value = serde_json::from_str(SCHEMA_ORG_CONTEXT_STUB)
            .expect("SCHEMA_ORG_CONTEXT_STUB is valid JSON");
        let mut entries = HashMap::new();
        entries.insert("https://schema.org/".to_string(), stub.clone());
        entries.insert("http://schema.org/".to_string(), stub.clone());
        entries.insert("https://schema.org".to_string(), stub.clone());
        entries.insert("http://schema.org".to_string(), stub);
        Self { entries }
    }
}

impl DocumentLoader for StaticDocumentLoader {
    fn load(&self, url: &str) -> Result<Value, String> {
        self.entries.get(url).cloned().ok_or_else(|| {
            format!(
                "StaticDocumentLoader: no entry for URL \"{url}\". \
                 Add it with StaticDocumentLoader::new([(\"{url}\".to_string(), json_str)]). \
                 See https://github.com/daghovland/rdf-datalog/issues/82"
            )
        })
    }
}

// ── Static schema.org context stub ───────────────────────────────────────────

/// Minimal schema.org context stub, sufficient for the most common terms.
///
/// This is **not** the full schema.org vocabulary.  It is intentionally small
/// to keep compile times low and avoid bundling a huge constant into every
/// binary that links `jsonld-parser`.
///
/// Tracks: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
// Terms covered by @vocab alone (bare name expands to https://schema.org/<name>) are omitted.
// Only entries that add @type coercion (IRI or datatype) are listed explicitly.
const SCHEMA_ORG_CONTEXT_STUB: &str = r#"{
  "@context": {
    "@vocab": "https://schema.org/",
    "schema": "https://schema.org/",

    "url":          { "@id": "schema:url",          "@type": "@id" },
    "image":        { "@id": "schema:image",         "@type": "@id" },
    "sameAs":       { "@id": "schema:sameAs",        "@type": "@id" },
    "author":       { "@id": "schema:author",        "@type": "@id" },
    "member":       { "@id": "schema:member",        "@type": "@id" },
    "memberOf":     { "@id": "schema:memberOf",      "@type": "@id" },
    "employee":     { "@id": "schema:employee",      "@type": "@id" },
    "founder":      { "@id": "schema:founder",       "@type": "@id" },
    "knows":        { "@id": "schema:knows",         "@type": "@id" },
    "location":     { "@id": "schema:location",      "@type": "@id" },

    "birthDate":    { "@id": "schema:birthDate",     "@type": "http://www.w3.org/2001/XMLSchema#date" },
    "startDate":    { "@id": "schema:startDate",     "@type": "http://www.w3.org/2001/XMLSchema#date" },
    "endDate":      { "@id": "schema:endDate",       "@type": "http://www.w3.org/2001/XMLSchema#date" },
    "datePublished":{ "@id": "schema:datePublished", "@type": "http://www.w3.org/2001/XMLSchema#date" }
  }
}"#;

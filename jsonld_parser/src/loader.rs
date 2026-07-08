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

/// A [`DocumentLoader`] backed by an in-memory map of URL → JSON string.
///
/// Useful for:
/// - Unit tests that need a deterministic, offline context.
/// - The built-in static cache of well-known vocabulary contexts.
///   Call [`StaticDocumentLoader::with_schema_org`] to get a loader that
///   pre-populates a minimal schema.org context stub.
pub struct StaticDocumentLoader {
    entries: HashMap<String, String>,
}

impl StaticDocumentLoader {
    /// Create a loader from an iterable of `(url, json_string)` pairs.
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// Create a loader pre-populated with a minimal schema.org context stub.
    ///
    /// The stub covers the most commonly used schema.org terms.  It is **not**
    /// the full schema.org vocabulary.  Extend with [`StaticDocumentLoader::new`]
    /// if you need additional terms.
    ///
    /// Both `https://schema.org/` and `http://schema.org/` resolve to the
    /// same stub.
    pub fn with_schema_org() -> Self {
        let mut entries = HashMap::new();
        let stub = SCHEMA_ORG_CONTEXT_STUB.to_string();
        entries.insert("https://schema.org/".to_string(), stub.clone());
        entries.insert("http://schema.org/".to_string(), stub);
        Self { entries }
    }
}

impl DocumentLoader for StaticDocumentLoader {
    fn load(&self, url: &str) -> Result<Value, String> {
        let raw = self.entries.get(url).ok_or_else(|| {
            format!(
                "StaticDocumentLoader: no entry for URL \"{url}\". \
                 Add it with StaticDocumentLoader::new([(\"{url}\".to_string(), json_str)]). \
                 See https://github.com/daghovland/rdf-datalog/issues/82"
            )
        })?;
        serde_json::from_str(raw)
            .map_err(|e| format!("StaticDocumentLoader: could not parse JSON for \"{url}\": {e}"))
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
const SCHEMA_ORG_CONTEXT_STUB: &str = r#"{
  "@context": {
    "@vocab": "https://schema.org/",
    "schema": "https://schema.org/",

    "name":              { "@id": "schema:name" },
    "description":       { "@id": "schema:description" },
    "url":               { "@id": "schema:url",         "@type": "@id" },
    "image":             { "@id": "schema:image",       "@type": "@id" },
    "identifier":        { "@id": "schema:identifier" },
    "sameAs":            { "@id": "schema:sameAs",      "@type": "@id" },

    "Person":            { "@id": "schema:Person" },
    "Organization":      { "@id": "schema:Organization" },
    "Place":             { "@id": "schema:Place" },
    "Product":           { "@id": "schema:Product" },
    "Event":             { "@id": "schema:Event" },
    "CreativeWork":      { "@id": "schema:CreativeWork" },
    "Article":           { "@id": "schema:Article" },
    "WebPage":           { "@id": "schema:WebPage" },
    "WebSite":           { "@id": "schema:WebSite" },

    "givenName":         { "@id": "schema:givenName" },
    "familyName":        { "@id": "schema:familyName" },
    "email":             { "@id": "schema:email" },
    "telephone":         { "@id": "schema:telephone" },
    "jobTitle":          { "@id": "schema:jobTitle" },
    "birthDate":         { "@id": "schema:birthDate",   "@type": "http://www.w3.org/2001/XMLSchema#date" },

    "addressLocality":   { "@id": "schema:addressLocality" },
    "addressRegion":     { "@id": "schema:addressRegion" },
    "addressCountry":    { "@id": "schema:addressCountry" },
    "postalCode":        { "@id": "schema:postalCode" },
    "streetAddress":     { "@id": "schema:streetAddress" },
    "address":           { "@id": "schema:address" },

    "author":            { "@id": "schema:author",      "@type": "@id" },
    "datePublished":     { "@id": "schema:datePublished" },
    "headline":          { "@id": "schema:headline" },
    "text":              { "@id": "schema:text" },

    "member":            { "@id": "schema:member",      "@type": "@id" },
    "memberOf":          { "@id": "schema:memberOf",    "@type": "@id" },
    "employee":          { "@id": "schema:employee",    "@type": "@id" },
    "founder":           { "@id": "schema:founder",     "@type": "@id" },

    "knows":             { "@id": "schema:knows",       "@type": "@id" },
    "location":          { "@id": "schema:location",    "@type": "@id" },

    "startDate":         { "@id": "schema:startDate" },
    "endDate":           { "@id": "schema:endDate" },

    "price":             { "@id": "schema:price" },
    "priceCurrency":     { "@id": "schema:priceCurrency" }
  }
}"#;

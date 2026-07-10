/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! JSON-LD 1.1 parser and serialiser.

mod loader;
mod serialize;

pub use loader::{DocumentLoader, StaticDocumentLoader};

use dag_rdf::{Datastore, GraphElementId, IriReference, RdfLiteral, RdfResource, Triple};
use ingress::NetworkPolicy;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

// ── RDF IRI constants ─────────────────────────────────────────────────────────

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
const RDF_NIL: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";
const RDF_JSON: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#JSON";

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct JsonLdError(pub String);

impl std::fmt::Display for JsonLdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for JsonLdError {}
fn err(msg: impl Into<String>) -> JsonLdError {
    JsonLdError(msg.into())
}

// ── Context ───────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Context {
    /// term name → term definition
    terms: HashMap<String, TermDef>,
    /// @base IRI (for resolving relative @id values)
    base: Option<String>,
    /// @vocab (default vocabulary for unresolved terms)
    vocab: Option<String>,
    /// @language (default language tag)
    language: Option<String>,
    /// Network access policy — controls what happens when an external @context URL is encountered.
    ///
    /// Default: [`NetworkPolicy::Deny`] (the safe default).
    /// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
    network: NetworkPolicy,
    /// Optional document loader used when [`NetworkPolicy::Allow`] is in effect.
    ///
    /// When `Some`, external `@context` URL strings are resolved via this loader.
    /// When `None` and `network == Allow`, an error is returned.
    ///
    /// Use [`crate::parse_jsonld_with_loader`] to supply a loader.
    /// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
    loader: Option<Arc<dyn DocumentLoader>>,
    /// URLs currently being loaded; used for cycle detection.
    visited_urls: HashSet<String>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("terms", &self.terms)
            .field("base", &self.base)
            .field("vocab", &self.vocab)
            .field("language", &self.language)
            .field("network", &self.network)
            .field("loader", &self.loader.as_ref().map(|_| "<DocumentLoader>"))
            .field("visited_urls", &self.visited_urls)
            .finish()
    }
}

#[derive(Clone, Debug)]
struct TermDef {
    /// Expanded IRI for this term.
    id: String,
    /// How to coerce plain string values.
    type_coercion: TypeCoercion,
    /// Container type (@set, @list, @language, @index, @id, @type, @graph).
    container: ContainerType,
    #[allow(dead_code)]
    reverse: bool,
    /// Raw @context value for a property-scoped or type-scoped context.
    scoped_context: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
enum TypeCoercion {
    None,
    /// `"@type": "@id"` — string values are interpreted as IRIs.
    Id,
    /// `"@type": "@json"` — any JSON value becomes an rdf:JSON typed literal.
    Json,
    /// `"@type": "<datatype-iri>"` — string values become typed literals.
    Datatype(String),
}

#[derive(Clone, Debug, PartialEq)]
enum ContainerType {
    Default,
    Set,
    List,
    Language,
    Index,
    Id,
    Type,
    Graph,
}

impl Context {
    /// Build a new context by processing a JSON `@context` value on top of
    /// the current one.  Arrays of contexts are processed left-to-right.
    fn extend(&self, ctx_val: &Value) -> Result<Context, JsonLdError> {
        match ctx_val {
            Value::Array(items) => {
                let mut ctx = self.clone();
                for item in items {
                    ctx = ctx.extend(item)?;
                }
                Ok(ctx)
            }
            Value::Object(map) => self.extend_from_map(map),
            Value::String(url) => match self.network {
                NetworkPolicy::Deny => Err(err(format!(
                    "External @context URL \"{url}\" was not fetched: remote network access is \
                     disabled. Configure with --network=allow to enable external context loading. \
                     See https://github.com/daghovland/rdf-datalog/issues/82"
                ))),
                NetworkPolicy::Ignore => {
                    // Silently skip the external context — preserve current behaviour.
                    Ok(self.clone())
                }
                NetworkPolicy::Allow | NetworkPolicy::AllowList(_) => {
                    // AllowList prefix check: reject URLs not matching any configured
                    // prefix, mirroring the same gate in sparql_endpoint's fetch_rdf.
                    if let NetworkPolicy::AllowList(prefixes) = &self.network
                        && !prefixes.iter().any(|p| url.starts_with(p.as_str()))
                    {
                        return Err(err(format!(
                            "External @context URL \"{url}\": URL is not in the configured allow-list"
                        )));
                    }
                    match &self.loader {
                        Some(loader) => {
                            // Cycle detection: skip a URL we are already in the process of loading.
                            if self.visited_urls.contains(url.as_str()) {
                                return Ok(self.clone());
                            }
                            let ctx_doc = loader.load(url).map_err(|e| {
                                err(format!("Failed to load @context URL \"{url}\": {e}"))
                            })?;
                            // The fetched document may wrap the context under a top-level `@context`
                            // key (standard JSON-LD context document format), or it may already be
                            // the raw context object/array.  Handle both.
                            let ctx_val = match &ctx_doc {
                                Value::Object(map) if map.contains_key("@context") => {
                                    map["@context"].clone()
                                }
                                other => other.clone(),
                            };
                            // Mark this URL visited before recursing so chains don't loop.
                            let mut loading_ctx = self.clone();
                            loading_ctx.visited_urls.insert(url.clone());
                            loading_ctx.extend(&ctx_val)
                        }
                        None => Err(err(format!(
                            "External @context URL \"{url}\": NetworkPolicy::Allow requires a \
                             DocumentLoader. Use parse_jsonld_with_loader() to supply one. \
                             See https://github.com/daghovland/rdf-datalog/issues/82"
                        ))),
                    }
                }
            },
            // null resets the active context but preserves the loader and network policy so
            // subsequent URL strings in the same array can still be resolved.
            Value::Null => Ok(Context {
                network: self.network.clone(),
                loader: self.loader.clone(),
                visited_urls: self.visited_urls.clone(),
                ..Context::default()
            }),
            other => Err(err(format!("invalid @context value: {other}"))),
        }
    }

    fn extend_from_map(&self, map: &Map<String, Value>) -> Result<Context, JsonLdError> {
        let mut ctx = self.clone();

        if let Some(Value::String(base)) = map.get("@base") {
            ctx.base = Some(resolve_iri(base, self.base.as_deref()));
        }
        if let Some(Value::String(vocab)) = map.get("@vocab") {
            ctx.vocab = Some(expand_iri_in_context(vocab, self));
        }
        if let Some(Value::String(lang)) = map.get("@language") {
            ctx.language = Some(lang.clone());
        }

        // First pass: collect prefix definitions (simple string values).
        // Must happen before expanding term definitions that may use these prefixes.
        for (key, val) in map {
            if key.starts_with('@') {
                continue;
            }
            match val {
                Value::String(iri) => {
                    let expanded = expand_iri_in_context(iri, &ctx);
                    ctx.terms.insert(
                        key.clone(),
                        TermDef {
                            id: expanded,
                            type_coercion: TypeCoercion::None,
                            container: ContainerType::Default,
                            reverse: false,
                            scoped_context: None,
                        },
                    );
                }
                Value::Null => {
                    ctx.terms.remove(key);
                }
                _ => {}
            }
        }

        // Second pass: object-form term definitions.
        for (key, val) in map {
            if key.starts_with('@') {
                continue;
            }
            if let Value::Object(def) = val {
                let id = match def.get("@id") {
                    Some(Value::String(s)) => expand_iri_in_context(s, &ctx),
                    Some(Value::Null) => continue,
                    None => {
                        // No @id: use the key itself (already expanded).
                        expand_iri_in_context(key, &ctx)
                    }
                    Some(other) => return Err(err(format!("@id must be a string, got {other}"))),
                };

                let type_coercion = match def.get("@type") {
                    Some(Value::String(t)) if t == "@id" => TypeCoercion::Id,
                    Some(Value::String(t)) if t == "@vocab" => TypeCoercion::Id,
                    Some(Value::String(t)) if t == "@json" => TypeCoercion::Json,
                    Some(Value::String(t)) => {
                        TypeCoercion::Datatype(expand_iri_in_context(t, &ctx))
                    }
                    _ => TypeCoercion::None,
                };

                let container = match def.get("@container") {
                    Some(Value::String(c)) => match c.as_str() {
                        "@set" => ContainerType::Set,
                        "@list" => ContainerType::List,
                        "@language" => ContainerType::Language,
                        "@index" => ContainerType::Index,
                        "@id" => ContainerType::Id,
                        "@type" => ContainerType::Type,
                        "@graph" => ContainerType::Graph,
                        _ => ContainerType::Default,
                    },
                    Some(Value::Array(items)) => {
                        // Take the first meaningful container type.
                        items
                            .iter()
                            .find_map(|v| v.as_str())
                            .map(|c| match c {
                                "@set" => ContainerType::Set,
                                "@list" => ContainerType::List,
                                "@language" => ContainerType::Language,
                                "@index" => ContainerType::Index,
                                "@id" => ContainerType::Id,
                                "@type" => ContainerType::Type,
                                "@graph" => ContainerType::Graph,
                                _ => ContainerType::Default,
                            })
                            .unwrap_or(ContainerType::Default)
                    }
                    _ => ContainerType::Default,
                };

                let reverse = def.contains_key("@reverse");

                let scoped_context = def.get("@context").cloned();

                ctx.terms.insert(
                    key.clone(),
                    TermDef {
                        id,
                        type_coercion,
                        container,
                        reverse,
                        scoped_context,
                    },
                );
            }
        }

        Ok(ctx)
    }

    /// Expand a term or compact IRI to a full IRI using this context.
    fn expand_term(&self, term: &str) -> Option<String> {
        if term.starts_with('@') {
            return Some(term.to_owned());
        }
        // Direct term lookup (highest priority after keywords).
        if let Some(def) = self.terms.get(term) {
            return Some(def.id.clone());
        }
        // Compact IRI "prefix:local" — check context-defined prefix BEFORE
        // treating as an absolute IRI, so "schema:name" expands via the context
        // even though it syntactically looks like an IRI scheme.
        if let Some(colon) = term.find(':') {
            let prefix = &term[..colon];
            let local = &term[colon + 1..];
            if prefix != "_"
                && !local.starts_with("//")
                && let Some(def) = self.terms.get(prefix)
            {
                return Some(format!("{}{}", def.id, local));
            }
        }
        // Already an absolute IRI (unknown scheme, or known scheme like http/https/urn).
        if is_absolute_iri(term) {
            return Some(term.to_owned());
        }
        // @vocab fallback for bare terms.
        if let Some(vocab) = &self.vocab {
            return Some(format!("{vocab}{term}"));
        }
        None
    }

    fn term_def(&self, term: &str) -> Option<&TermDef> {
        self.terms.get(term)
    }
}

/// Expand an IRI string using the given context (used during context building).
fn expand_iri_in_context(s: &str, ctx: &Context) -> String {
    if s.starts_with('@') {
        return s.to_owned();
    }
    // Direct term lookup.
    if let Some(def) = ctx.terms.get(s) {
        return def.id.clone();
    }
    // Compact IRI "prefix:local" — try context prefix BEFORE absolute IRI check.
    if let Some(colon) = s.find(':') {
        let prefix = &s[..colon];
        let local = &s[colon + 1..];
        if prefix != "_"
            && !local.starts_with("//")
            && let Some(def) = ctx.terms.get(prefix)
        {
            return format!("{}{}", def.id, local);
        }
    }
    // Already absolute.
    if is_absolute_iri(s) {
        return s.to_owned();
    }
    // @vocab fallback.
    if let Some(vocab) = &ctx.vocab {
        return format!("{vocab}{s}");
    }
    s.to_owned()
}

fn resolve_iri(iri: &str, base: Option<&str>) -> String {
    if is_absolute_iri(iri) {
        return iri.to_owned();
    }
    match base {
        Some(b) => {
            // Very simple base resolution: just concatenate if relative.
            if b.ends_with('/') {
                format!("{b}{iri}")
            } else {
                // Strip last path segment from base.
                let slash = b.rfind('/').map(|i| i + 1).unwrap_or(b.len());
                format!("{}{}", &b[..slash], iri)
            }
        }
        None => iri.to_owned(),
    }
}

fn is_absolute_iri(s: &str) -> bool {
    !s.starts_with('@')
        && s.contains(':')
        && s.find(':')
            .map(|i| {
                i > 0
                    && s[..i]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '+' || c == '-' || c == '.')
            })
            .unwrap_or(false)
}

// ── Public API ────────────────────────────────────────────────────────────────

fn read_to_json(mut reader: impl Read) -> Result<Value, JsonLdError> {
    let mut buf = String::new();
    reader
        .read_to_string(&mut buf)
        .map_err(|e| err(format!("IO: {e}")))?;
    serde_json::from_str(&buf).map_err(|e| err(format!("JSON: {e}")))
}

/// Parse JSON-LD data from `reader` into `datastore`.
///
/// `network` controls how external `@context` URLs are handled:
/// - [`NetworkPolicy::Deny`] — return an error when an external URL is encountered (default).
/// - [`NetworkPolicy::Ignore`] — silently skip external contexts (previous behaviour).
/// - [`NetworkPolicy::Allow`] — loads external contexts using the built-in static loader.
///   For custom loaders, use [`parse_jsonld_with_loader`].
///
/// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
pub fn parse_jsonld<R: Read>(
    datastore: &mut Datastore,
    reader: R,
    network: NetworkPolicy,
) -> Result<(), JsonLdError> {
    let value = read_to_json(reader)?;
    let ctx = Context {
        network,
        ..Context::default()
    };
    process_document(datastore, &value, &ctx, None)
}

/// Parse JSON-LD data from `reader` into `datastore`, using `loader` to
/// resolve any external `@context` URL strings encountered during parsing.
///
/// External URLs are fetched via `loader.load(url)`.  The network policy is
/// implicitly [`NetworkPolicy::Allow`] when a loader is supplied.
///
/// # Example
/// ```
/// use dag_rdf::Datastore;
/// use jsonld_parser::{StaticDocumentLoader, parse_jsonld_with_loader};
/// use std::sync::Arc;
///
/// let mut ds = Datastore::new(1_000);
/// let loader = Arc::new(StaticDocumentLoader::with_schema_org());
/// let json = r#"{"@context": "https://schema.org/", "@id": "https://example.org/x", "name": "X"}"#;
/// parse_jsonld_with_loader(&mut ds, json.as_bytes(), loader).unwrap();
/// ```
///
/// Related: [#82](https://github.com/daghovland/rdf-datalog/issues/82)
pub fn parse_jsonld_with_loader<R: Read>(
    datastore: &mut Datastore,
    reader: R,
    loader: Arc<dyn DocumentLoader>,
) -> Result<(), JsonLdError> {
    let value = read_to_json(reader)?;
    let ctx = Context {
        network: NetworkPolicy::Allow,
        loader: Some(loader),
        ..Context::default()
    };
    process_document(datastore, &value, &ctx, None)
}

pub fn serialize_jsonld(ds: &Datastore) -> String {
    serialize::serialize_jsonld(ds)
}
pub fn serialize_jsonld_expanded(ds: &Datastore) -> String {
    serialize::serialize_jsonld_expanded(ds)
}
pub fn serialize_jsonld_flattened(ds: &Datastore) -> String {
    serialize::serialize_jsonld_flattened(ds)
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Emit one `rdf:type` triple per entry in `type_val` (string or array of strings).
fn emit_type_triples(
    ds: &mut Datastore,
    ctx: &Context,
    graph: Option<GraphElementId>,
    subject: GraphElementId,
    type_val: &Value,
) {
    let rdf_type = intern_iri(ds, RDF_TYPE);
    for type_str in iter_string_or_array(type_val) {
        let expanded = ctx.expand_term(&type_str).unwrap_or(type_str);
        if is_absolute_iri(&expanded) {
            let obj = intern_iri(ds, &expanded);
            add_to_graph(ds, graph, subject, rdf_type, obj);
        }
    }
}

// ── Document-level processing ─────────────────────────────────────────────────

fn process_document(
    ds: &mut Datastore,
    value: &Value,
    ctx: &Context,
    graph: Option<GraphElementId>,
) -> Result<(), JsonLdError> {
    match value {
        Value::Array(items) => {
            for item in items {
                process_document(ds, item, ctx, graph)?;
            }
        }
        Value::Object(map) => {
            // Resolve @context first.
            let ctx = if let Some(ctx_val) = map.get("@context") {
                ctx.extend(ctx_val)?
            } else {
                ctx.clone()
            };
            process_node(ds, map, &ctx, graph)?;
        }
        _ => {}
    }
    Ok(())
}

// ── Node processing ───────────────────────────────────────────────────────────

fn process_node(
    ds: &mut Datastore,
    map: &Map<String, Value>,
    ctx: &Context,
    graph: Option<GraphElementId>,
) -> Result<GraphElementId, JsonLdError> {
    // Determine subject: check @id, then keyword aliases for @id.
    let id_str: Option<&str> = map.get("@id").and_then(|v| v.as_str()).or_else(|| {
        ctx.terms
            .iter()
            .find(|(_, def)| def.id == "@id")
            .and_then(|(alias, _)| map.get(alias).and_then(|v| v.as_str()))
    });

    // `has_explicit_id` is used by the @graph handler: @graph without @id means
    // the default graph (items stored with graph=None).
    let has_explicit_id = id_str.is_some();
    let subject = match id_str {
        Some(iri) => {
            let expanded = ctx
                .expand_term(iri)
                .filter(|s| is_absolute_iri(s))
                .unwrap_or_else(|| resolve_iri(iri, ctx.base.as_deref()));
            ds.add_node_resource(RdfResource::Iri(IriReference(expanded)))
        }
        None => ds.new_anonymous_blank_node(),
    };

    // Collect type-scoped contexts: if any @type value has a scoped context
    // in its term definition, extend the working context before processing
    // other properties (the type-scoped context applies to the whole node).
    let type_scoped_ctx_owned;
    let ctx = {
        let type_vals = map.get("@type").or_else(|| {
            ctx.terms
                .iter()
                .find(|(_, d)| d.id == "@type")
                .and_then(|(alias, _)| map.get(alias))
        });
        let mut merged = ctx.clone();
        let mut changed = false;
        if let Some(tv) = type_vals {
            for type_str in iter_string_or_array(tv) {
                if let Some(def) = ctx.term_def(&type_str)
                    && let Some(sc_val) = &def.scoped_context
                {
                    merged = merged.extend(sc_val)?;
                    changed = true;
                }
            }
        }
        if changed {
            type_scoped_ctx_owned = merged;
            &type_scoped_ctx_owned
        } else {
            ctx
        }
    };

    for (key, val) in map {
        match key.as_str() {
            "@id" | "@context" => continue,

            "@type" => emit_type_triples(ds, ctx, graph, subject, val),

            "@graph" => {
                // If the enclosing node has an @id, it names the graph.
                // If not, @graph items go into the same graph as the enclosing context
                // (for the top-level node this means the default graph).
                let inner_graph = if has_explicit_id {
                    Some(subject)
                } else {
                    graph
                };
                let items = match val {
                    Value::Array(a) => a.as_slice(),
                    _ => std::slice::from_ref(val),
                };
                for item in items {
                    if let Value::Object(inner) = item {
                        let inner_ctx = if let Some(c) = inner.get("@context") {
                            ctx.extend(c)?
                        } else {
                            ctx.clone()
                        };
                        process_node(ds, inner, &inner_ctx, inner_graph)?;
                    }
                }
            }

            "@reverse" => {
                if let Value::Object(rev_map) = val {
                    for (rev_key, rev_val) in rev_map {
                        let pred_iri = ctx.expand_term(rev_key).or_else(|| {
                            if is_absolute_iri(rev_key) {
                                Some(rev_key.clone())
                            } else {
                                None
                            }
                        });
                        if let Some(pred_iri) = pred_iri {
                            let pred = intern_iri(ds, &pred_iri);
                            // Reversed: the listed nodes are the *subjects*, current node is object.
                            let items = match rev_val {
                                Value::Array(a) => a.clone(),
                                single => vec![single.clone()],
                            };
                            for item in &items {
                                let rev_subj = value_to_id(ds, ctx, item)?;
                                add_to_graph(ds, graph, rev_subj, pred, subject);
                            }
                        }
                    }
                }
            }

            "@included" => {
                let items = match val {
                    Value::Array(a) => a.as_slice(),
                    _ => std::slice::from_ref(val),
                };
                for item in items {
                    process_document(ds, item, ctx, graph)?;
                }
            }

            "@nest" => {
                // @nest groups properties transparently; process them at the same level.
                let items = match val {
                    Value::Array(a) => a.as_slice(),
                    _ => std::slice::from_ref(val),
                };
                for item in items {
                    if let Value::Object(inner) = item {
                        for (nk, nv) in inner {
                            process_property(ds, ctx, graph, subject, nk, nv)?;
                        }
                    }
                }
            }

            key if key.starts_with('@') => {} // ignore unknown keywords

            key => {
                // Resolve what this term maps to.
                match ctx.term_def(key).map(|d| d.id.as_str()) {
                    Some("@id") => {} // already handled via id_str above
                    Some("@type") => emit_type_triples(ds, ctx, graph, subject, val),
                    Some("@nest") => {
                        // @nest: process the nested object's properties at this level.
                        let items = match val {
                            Value::Array(a) => a.clone(),
                            single => vec![single.clone()],
                        };
                        for item in &items {
                            if let Value::Object(inner) = item {
                                for (nk, nv) in inner {
                                    process_property(ds, ctx, graph, subject, nk, nv)?;
                                }
                            }
                        }
                    }
                    Some(kw) if kw.starts_with('@') => {} // other keyword aliases: skip
                    _ => {
                        process_property(ds, ctx, graph, subject, key, val)?;
                    }
                }
            }
        }
    }

    Ok(subject)
}

fn process_property(
    ds: &mut Datastore,
    ctx: &Context,
    graph: Option<GraphElementId>,
    subject: GraphElementId,
    key: &str,
    val: &Value,
) -> Result<(), JsonLdError> {
    // Expand the property IRI.
    let pred_iri = match ctx.expand_term(key) {
        Some(iri) if is_absolute_iri(&iri) => iri,
        _ if is_absolute_iri(key) => key.to_owned(),
        _ => {
            log::debug!("skipping unresolvable property: {key}");
            return Ok(());
        }
    };

    let def = ctx.term_def(key);
    let type_coercion = def.map(|d| &d.type_coercion).unwrap_or(&TypeCoercion::None);
    let container = def.map(|d| &d.container).unwrap_or(&ContainerType::Default);

    // Apply property-scoped context for processing this property's values.
    let scoped_ctx_owned;
    let ctx = if let Some(sc_val) = def.and_then(|d| d.scoped_context.as_ref()) {
        scoped_ctx_owned = ctx.extend(sc_val)?;
        &scoped_ctx_owned
    } else {
        ctx
    };

    let pred = intern_iri(ds, &pred_iri);

    match container {
        ContainerType::Language => {
            // Language map: keys are language tags, values are strings.
            if let Value::Object(lang_map) = val {
                for (lang, lang_val) in lang_map {
                    let strings = match lang_val {
                        Value::String(s) => vec![s.clone()],
                        Value::Array(arr) => arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                        _ => continue,
                    };
                    for s in strings {
                        let obj = ds.add_literal_resource(RdfLiteral::LangLiteral {
                            literal: s,
                            lang: lang.clone(),
                        });
                        add_to_graph(ds, graph, subject, pred, obj);
                    }
                }
                return Ok(());
            }
        }
        ContainerType::Index => {
            // Index map: keys are index strings (annotations), values are nodes/values.
            if let Value::Object(idx_map) = val {
                for (_index_key, idx_val) in idx_map {
                    add_value_to_graph(ds, ctx, graph, subject, pred, idx_val, type_coercion)?;
                }
                return Ok(());
            }
        }
        ContainerType::Id => {
            // ID map: keys are @id values, values are nodes.
            if let Value::Object(id_map) = val {
                for (id_key, id_val) in id_map {
                    if let Value::Object(mut node_obj) = id_val.clone() {
                        // Inject the map key as @id if not already present.
                        node_obj
                            .entry("@id".to_string())
                            .or_insert_with(|| Value::String(id_key.clone()));
                        let node_val = Value::Object(node_obj);
                        let obj_id = process_node_value(ds, ctx, graph, &node_val)?;
                        add_to_graph(ds, graph, subject, pred, obj_id);
                    }
                }
                return Ok(());
            }
        }
        ContainerType::Type => {
            // Type map: keys are type IRIs, values are nodes of that type.
            if let Value::Object(type_map) = val {
                let rdf_type = intern_iri(ds, RDF_TYPE);
                for (type_key, type_val) in type_map {
                    let type_iri = ctx.expand_term(type_key).unwrap_or(type_key.clone());
                    let nodes = match type_val {
                        Value::Array(a) => a.clone(),
                        single => vec![single.clone()],
                    };
                    for node in &nodes {
                        if let Value::Object(mut node_obj) = node.clone() {
                            // Inject @type unless already set.
                            node_obj
                                .entry("@type".to_string())
                                .or_insert_with(|| Value::String(type_iri.clone()));
                            let node_val = Value::Object(node_obj);
                            let obj_id = process_node_value(ds, ctx, graph, &node_val)?;
                            add_to_graph(ds, graph, subject, pred, obj_id);
                        }
                        // Also emit the rdf:type triple for each node.
                        if is_absolute_iri(&type_iri)
                            && let Some(id_val) = node.get("@id")
                            && let Some(id_str) = id_val.as_str()
                        {
                            let type_id = intern_iri(ds, &type_iri);
                            let expanded = ctx.expand_term(id_str).unwrap_or(id_str.to_owned());
                            if is_absolute_iri(&expanded) {
                                let node_subj = intern_iri(ds, &expanded);
                                add_to_graph(ds, graph, node_subj, rdf_type, type_id);
                            }
                        }
                    }
                }
                return Ok(());
            }
        }
        ContainerType::Graph => {
            // Graph container: keys are graph IRIs, values are nodes in that graph.
            if let Value::Object(graph_map) = val {
                for (graph_key, graph_val) in graph_map {
                    let graph_iri = ctx.expand_term(graph_key).unwrap_or(graph_key.clone());
                    if is_absolute_iri(&graph_iri) {
                        let named_graph = intern_iri(ds, &graph_iri);
                        // Emit a triple from the outer subject to the named graph.
                        add_to_graph(ds, graph, subject, pred, named_graph);
                        // Process the nodes inside the named graph.
                        process_document(ds, graph_val, ctx, Some(named_graph))?;
                    }
                }
                return Ok(());
            }
        }
        ContainerType::List => {
            // Explicit list container: treat the value as a list regardless of @list.
            return process_list(ds, ctx, graph, subject, pred, val);
        }
        _ => {}
    }

    // Handle @list wrapper explicitly.
    if let Value::Object(map) = val
        && let Some(list_val) = map.get("@list")
    {
        return process_list(ds, ctx, graph, subject, pred, list_val);
    }

    add_value_to_graph(ds, ctx, graph, subject, pred, val, type_coercion)
}

/// Process a @list: encode as rdf:first / rdf:rest / rdf:nil chain.
fn process_list(
    ds: &mut Datastore,
    ctx: &Context,
    graph: Option<GraphElementId>,
    subject: GraphElementId,
    predicate: GraphElementId,
    list_val: &Value,
) -> Result<(), JsonLdError> {
    let items: Vec<&Value> = match list_val {
        Value::Array(arr) => arr.iter().collect(),
        single => vec![single],
    };

    let rdf_first = intern_iri(ds, RDF_FIRST);
    let rdf_rest = intern_iri(ds, RDF_REST);
    let rdf_nil = intern_iri(ds, RDF_NIL);

    if items.is_empty() {
        add_to_graph(ds, graph, subject, predicate, rdf_nil);
        return Ok(());
    }

    let mut head = ds.new_anonymous_blank_node();
    add_to_graph(ds, graph, subject, predicate, head);

    for (i, item) in items.iter().enumerate() {
        add_value_to_graph(ds, ctx, graph, head, rdf_first, item, &TypeCoercion::None)?;
        let next = if i + 1 < items.len() {
            ds.new_anonymous_blank_node()
        } else {
            rdf_nil
        };
        add_to_graph(ds, graph, head, rdf_rest, next);
        head = next;
    }

    Ok(())
}

/// Add a JSON value as one or more RDF triples (subject, predicate, ?).
fn add_value_to_graph(
    ds: &mut Datastore,
    ctx: &Context,
    graph: Option<GraphElementId>,
    subject: GraphElementId,
    pred: GraphElementId,
    val: &Value,
    type_coercion: &TypeCoercion,
) -> Result<(), JsonLdError> {
    // @json coercion: serialize any JSON value as an rdf:JSON typed literal.
    if *type_coercion == TypeCoercion::Json {
        let literal = val.to_string();
        let obj = ds.add_literal_resource(RdfLiteral::TypedLiteral {
            literal,
            type_iri: IriReference(RDF_JSON.to_owned()),
        });
        add_to_graph(ds, graph, subject, pred, obj);
        return Ok(());
    }

    match val {
        Value::String(s) => {
            let obj = match type_coercion {
                TypeCoercion::Id => {
                    let expanded = ctx
                        .expand_term(s)
                        .unwrap_or_else(|| resolve_iri(s, ctx.base.as_deref()));
                    ds.add_node_resource(RdfResource::Iri(IriReference(expanded)))
                }
                TypeCoercion::Json => ds.add_literal_resource(RdfLiteral::TypedLiteral {
                    literal: serde_json::to_string(val).unwrap_or(s.clone()),
                    type_iri: IriReference(RDF_JSON.to_owned()),
                }),
                TypeCoercion::Datatype(dt) => ds.add_literal_resource(RdfLiteral::TypedLiteral {
                    literal: s.clone(),
                    type_iri: IriReference(dt.clone()),
                }),
                TypeCoercion::None => {
                    if let Some(lang) = &ctx.language {
                        ds.add_literal_resource(RdfLiteral::LangLiteral {
                            literal: s.clone(),
                            lang: lang.clone(),
                        })
                    } else {
                        ds.add_literal_resource(RdfLiteral::LiteralString(s.clone()))
                    }
                }
            };
            add_to_graph(ds, graph, subject, pred, obj);
        }

        Value::Number(n) => {
            let obj = ds.add_literal_resource(RdfLiteral::LiteralString(n.to_string()));
            add_to_graph(ds, graph, subject, pred, obj);
        }

        Value::Bool(b) => {
            let obj = ds.add_literal_resource(RdfLiteral::LiteralString(b.to_string()));
            add_to_graph(ds, graph, subject, pred, obj);
        }

        Value::Array(items) => {
            for item in items {
                add_value_to_graph(ds, ctx, graph, subject, pred, item, type_coercion)?;
            }
        }

        Value::Object(map) => {
            if map.contains_key("@list") {
                // Handled by caller or process_list — shouldn't reach here normally.
                return process_list(ds, ctx, graph, subject, pred, &map["@list"]);
            }
            // Resolve keyword aliases once; used in the @value branch below.
            let value_key = ctx
                .terms
                .iter()
                .find(|(_, d)| d.id == "@value")
                .map(|(k, _)| k.as_str())
                .unwrap_or("@value");
            let lang_key = ctx
                .terms
                .iter()
                .find(|(_, d)| d.id == "@language")
                .map(|(k, _)| k.as_str())
                .unwrap_or("@language");
            let type_key_alias = ctx
                .terms
                .iter()
                .find(|(_, d)| d.id == "@type")
                .map(|(k, _)| k.as_str())
                .unwrap_or("@type");
            let is_value_object = map.contains_key(value_key);
            if let Some(Value::String(iri)) = map.get("@id") {
                // Node reference or nested node.
                let expanded = ctx
                    .expand_term(iri)
                    .unwrap_or_else(|| resolve_iri(iri, ctx.base.as_deref()));
                let obj = if map.len() == 1 {
                    // Pure @id reference.
                    intern_iri(ds, &expanded)
                } else {
                    // Nested node — process it and use its subject id.
                    process_node_value(ds, ctx, graph, val)?
                };
                add_to_graph(ds, graph, subject, pred, obj);
            } else if is_value_object {
                let inner_val = match map.get(value_key).or_else(|| map.get("@value")) {
                    Some(v) => v,
                    None => return Ok(()),
                };
                let lang_val = map.get(lang_key).or_else(|| map.get("@language"));
                let type_val = map.get(type_key_alias).or_else(|| map.get("@type"));
                let obj = if let Some(Value::String(lang)) = lang_val {
                    ds.add_literal_resource(RdfLiteral::LangLiteral {
                        literal: json_str(inner_val),
                        lang: lang.clone(),
                    })
                } else if let Some(Value::String(type_iri)) = type_val {
                    let expanded_type = ctx.expand_term(type_iri).unwrap_or(type_iri.clone());
                    if expanded_type == RDF_JSON || type_iri == "@json" {
                        ds.add_literal_resource(RdfLiteral::TypedLiteral {
                            literal: inner_val.to_string(),
                            type_iri: IriReference(RDF_JSON.to_owned()),
                        })
                    } else {
                        ds.add_literal_resource(RdfLiteral::TypedLiteral {
                            literal: json_str(inner_val),
                            type_iri: IriReference(expanded_type),
                        })
                    }
                } else {
                    ds.add_literal_resource(RdfLiteral::LiteralString(json_str(inner_val)))
                };
                add_to_graph(ds, graph, subject, pred, obj);
            } else {
                // Nested anonymous node.
                let obj = process_node_value(ds, ctx, graph, val)?;
                add_to_graph(ds, graph, subject, pred, obj);
            }
        }

        Value::Null => {}
    }
    Ok(())
}

/// Process a value that should be a node object; return its subject ID.
fn process_node_value(
    ds: &mut Datastore,
    ctx: &Context,
    graph: Option<GraphElementId>,
    val: &Value,
) -> Result<GraphElementId, JsonLdError> {
    match val {
        Value::Object(map) => {
            let inner_ctx = if let Some(c) = map.get("@context") {
                ctx.extend(c)?
            } else {
                ctx.clone()
            };
            process_node(ds, map, &inner_ctx, graph)
        }
        Value::String(s) => {
            let expanded = ctx.expand_term(s).unwrap_or(s.clone());
            Ok(intern_iri(ds, &expanded))
        }
        _ => Err(err(format!("expected node object, got {val}"))),
    }
}

/// Convert a node/value object to its @id as a graph element, without
/// processing properties (used for @reverse subjects).
fn value_to_id(
    ds: &mut Datastore,
    ctx: &Context,
    val: &Value,
) -> Result<GraphElementId, JsonLdError> {
    match val {
        Value::Object(map) => match map.get("@id") {
            Some(Value::String(iri)) => {
                let expanded = ctx.expand_term(iri).unwrap_or(iri.clone());
                Ok(intern_iri(ds, &expanded))
            }
            _ => Ok(ds.new_anonymous_blank_node()),
        },
        Value::String(s) => {
            let expanded = ctx.expand_term(s).unwrap_or(s.clone());
            Ok(intern_iri(ds, &expanded))
        }
        _ => Ok(ds.new_anonymous_blank_node()),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn intern_iri(ds: &mut Datastore, iri: &str) -> GraphElementId {
    ds.add_node_resource(RdfResource::Iri(IriReference(iri.to_owned())))
}

fn add_to_graph(
    ds: &mut Datastore,
    graph: Option<GraphElementId>,
    subject: GraphElementId,
    predicate: GraphElementId,
    object: GraphElementId,
) {
    let triple = Triple {
        subject,
        predicate,
        obj: object,
    };
    match graph {
        None => ds.add_triple(triple),
        Some(g) => ds.add_named_graph_triple(g, triple),
    }
}

fn iter_string_or_array(val: &Value) -> Vec<String> {
    match val {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

fn json_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

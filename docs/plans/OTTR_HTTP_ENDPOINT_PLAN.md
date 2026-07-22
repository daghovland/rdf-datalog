# OTTR HTTP Endpoint Plan

> Tracked under [#232](https://github.com/daghovland/rdf-datalog/issues/232).
> Related: [#13 OTTR template expansion epic](https://github.com/daghovland/rdf-datalog/issues/13)
> (the `ottr` crate itself — parser, expander, `ottr::expand_documents`).

## Goal

Expose the existing `ottr` crate (`ottr::parse_stottr`, `ottr::expand_documents`)
on the `sparql_endpoint` HTTP server, so stOTTR templates + instances can be
expanded directly into a running, named dataset over HTTP — mirroring how
`POST /{name}/rml` and `POST /{name}/shacl` already expose the `rml` and
`shacl` crates.

## Design decisions

### Route: `POST /{name}/ottr`

Dataset-scoped, consistent with the existing `/{name}/shacl`, `/{name}/rml`,
`/{name}/update`, `/{name}/data` Fuseki-compatible routes. The issue text asks
for "a way to run OTTR mappings on the endpoint" where "the user probably
wants the result stored" — a single dataset-scoped mutating endpoint answers
that directly, with no separate upload-then-trigger step.

**Alternative considered and rejected:** a stateful two-call flow (`POST
.../ottr/templates` to stage templates, then `POST .../ottr/expand` to run
instances against the staged set). Rejected because:
- It introduces server-side session state (whose templates? for how long?
  cleared when?) that the codebase has no existing pattern for — RML and
  SHACL are both single-call.
- stOTTR's own `ast::StottrDocument` already unifies templates and instances
  in one type, and `ottr::expand_documents(docs: &[StottrDocument], ...)`
  already accepts *multiple* documents and merges their templates before
  expanding — the crate-level API already gives us the "templates in one
  file, instances in another" flexibility as a single call. A second HTTP
  round-trip would only add complexity for a case the crate API resolves for
  free.

### Request shape: `multipart/form-data`, N unnamed stOTTR-document parts

Unlike RML (which needs external CSV/XML/JSON source files with real
filenames the mapping references by name), stOTTR documents are entirely
self-contained text — `ottr::ast::StottrDocument` has no notion of an
external file reference. So the RML precedent's "one `mapping` part +
filename-addressed source parts" shape doesn't apply here. But a single raw
body (the `shacl` endpoint's shape: one `text/turtle` body) is too limiting:
issue #232 anticipates a real workflow with reusable templates, and requiring
the client to concatenate a template library with instance data into one
string on every call is exactly the friction `expand_documents`'s
multi-document API was built to avoid.

Chosen shape: **multipart/form-data with one or more parts**, each part's
body being one stOTTR document (`.stottr` text — a template file, an
instance file, or a file with both). Part *names* are not semantically
significant (no `mapping`/`filename` distinction needed) — every part is
parsed independently with `ottr::parse_stottr`, and all resulting
`StottrDocument`s are passed together to `ottr::expand_documents`, which
already merges templates across documents before expanding instances. This
lets a client either:
- send one part with a self-contained stOTTR file, or
- send a `templates` part + a separate `instances` part (or several of each),
  reusing a template library across multiple instance sets in one call.

At least one part is required; a request with zero parts is `400 Bad
Request`.

### Response

`200 OK`, plain text: `"OTTR expansion applied: N triples inserted"` — same
shape as `dataset_rml_post`'s success response, for consistency across the
mapping-style endpoints.

### Error handling

- `403 Forbidden` if `state.config.read_only` (mutating endpoint, same guard
  as RML).
- `404 Not Found` if the named dataset doesn't exist.
- `400 Bad Request` if the multipart body has zero parts, or if any part
  fails `ottr::parse_stottr` (bad stOTTR syntax) or `ottr::expand_documents`
  fails (undefined template reference, arity mismatch, non-atomic argument
  type, etc. — whatever `OttrError` variant is produced). The error message
  is included in the response body.

### Persistence + store merge

Same pattern as `dataset_rml_post`: expand into a fresh, empty `Datastore`
first (`ottr::expand_documents` writes there), then:
1. If a changelog is configured, append `LogEntry::InsertQuad` for every
   resulting quad (graph-aware, same `graph_iri_for` helper reused from
   `rml_endpoint.rs` — OTTR's `ottr:Triple` base template can, in principle,
   target a non-default graph if a future `ottr:Triple` variant supports it;
   today it always targets the default graph, but the merge code doesn't
   need to assume that).
2. Copy every quad into the target dataset's live store via
   `store.add_resource` / `store.add_quad`, exactly like RML.

No incremental-reasoner hook is added in this first pass — `dataset_rml_post`
doesn't call into `IncrementalReasoner` either, so this endpoint stays
consistent with that (a materialization step, not itself materializing
inferred triples; a subsequent `/update` or reasoner-covered mutation would
pick those up under the existing reasoner wiring). Follow-up tracked
implicitly under epic #13 if this proves to be a real gap.

### Permission classification

`POST /{name}/ottr` mutates the store → `Permission::Write`, exactly like
`/rml` and `/update`. Extend `auth::classify`'s existing write-POST condition:

```rust
if method == Method::POST
    && (path == "/upload" || path.ends_with("/update") || path.ends_with("/rml") || path.ends_with("/ottr"))
{
    return Permission::Write;
}
```

### No stateless `/ottr/expand` counterpart (for now)

RML additionally exposes `POST /rml/map` (apply mapping, return RDF, touch no
dataset). The issue's framing ("the user probably wants the result stored")
argues the dataset-scoped call is the primary, and in-scope, need. A stateless
variant would be easy to add later following the exact same pattern
(`rml_map_post`) if requested; deferred to keep this change minimal and
matched to the issue as written.

## New/changed files

- `sparql_endpoint/src/ottr_endpoint.rs` (new) — `dataset_ottr_post` handler,
  multipart→`Vec<StottrDocument>` parsing, quad merge (adapted from
  `rml_endpoint.rs`'s `graph_iri_for` + merge loop).
- `sparql_endpoint/src/lib.rs` — add `pub mod ottr_endpoint;`.
- `sparql_endpoint/src/server.rs` — add the `/{name}/ottr` route (no special
  body-size layer needed — stOTTR documents are small text, unlike RML's
  binary source files; default 2 MB `DefaultBodyLimit` is ample).
- `sparql_endpoint/src/auth.rs` — extend `classify`'s write-POST condition,
  add one test (`post_dataset_ottr_is_write`).
- `sparql_endpoint/Cargo.toml` — add `ottr = { path = "../ottr" }` to
  `[dependencies]` (multipart/tempfile/reqwest-multipart already present from
  the RML work).
- `sparql_endpoint/tests/common/mod.rs` — add `dataset_ottr_url(&self,
  dataset: &str) -> String`.
- `sparql_endpoint/tests/ottr_endpoint.rs` (new) — integration tests,
  `#[ignore]`d until implementation (red phase).

## Test plan (red phase — all `#[ignore]`d initially)

1. `ottr_post_single_part_inserts_triples` — one multipart part containing a
   self-contained stOTTR file (template def + instance call); assert `200`
   and that a subsequent `SELECT` over `/{name}/sparql` returns the expanded
   triples.
2. `ottr_post_templates_and_instances_split_across_parts` — one part with
   only a template definition, a second part with only an instance calling
   that template; assert the expansion succeeds (proves cross-part template
   merging via `expand_documents`).
3. `ottr_post_zero_parts_is_bad_request` — empty multipart body; assert `400`.
4. `ottr_post_invalid_stottr_is_bad_request` — a part with malformed stOTTR
   syntax; assert `400` with the parse error in the body.
5. `ottr_post_unknown_template_is_bad_request` — an instance referencing an
   undefined template id; assert `400`.
6. `ottr_post_unknown_dataset_is_not_found` — POST to `/nonexistent/ottr`;
   assert `404`.
7. `ottr_post_read_only_server_is_forbidden` — POST against a read-only
   server; assert `403`.
8. `ottr_post_persists_across_restart` — (persistent-store variant, mirrors
   the equivalent RML test if one exists) POST, restart the server against
   the same `data_dir`, assert the expanded triples are still present.

## Spec / prior art references

- OTTR specification: <https://spec.ottr.xyz/>
- `docs/plans/OTTR_PLAN.md` — the `ottr` crate's own design (parser,
  expander, base templates); this plan only covers the HTTP adapter layer.
- `docs/plans/RML_REST_ENDPOINT_PLAN.md` — closest existing precedent for a
  mapping-crate-over-HTTP endpoint; this plan follows its shape wherever
  OTTR's simpler (file-less) input model doesn't call for a difference.

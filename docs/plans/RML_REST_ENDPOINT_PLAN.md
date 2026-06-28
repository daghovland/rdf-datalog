# RML REST Endpoint Plan

> **Status: COMPLETE** — `dataset_rml_post`/`rml_map_post`
> (`sparql_endpoint/src/rml_endpoint.rs`), routes wired in `server.rs` with
> per-route `DefaultBodyLimit`, `auth.rs::classify` updated for `Write`
> permission, 15 integration tests (`sparql_endpoint/tests/rml.rs`) and 1
> auth unit test all green. Documented in
> `docs/user/rml-mapping.md#applying-mappings-over-http` and `README.md`.

## Goal

Expose `rml::apply_rml_mapping` on the `sparql_endpoint` HTTP server, so RML
mappings (already usable via the Rust API and the `dagalog --mapping` CLI
flag) can also be applied to a running, named dataset over HTTP — or, via a
second stateless endpoint, just converted to RDF and returned directly.

## Design decisions (confirmed with user)

- **Route**: `POST /{name}/rml` — dataset-scoped, consistent with the existing
  `/{name}/shacl`, `/{name}/data`, `/{name}/update` Fuseki-compatible routes.
- **Source delivery**: `multipart/form-data`. The client sends the mapping
  Turtle as one part and each file referenced by `rml:source "..."` inside the
  mapping as additional parts. This keeps the request self-contained — no
  server-side file placement required — and mirrors the filesystem-based
  contract `apply_rml_mapping` already has (mapping file + base dir).
- **Upload size**: axum's `Multipart` extractor inherits the server-wide
  `DefaultBodyLimit` (2 MB), which is too small for real CSV/XML/JSON source
  files. Both RML routes get a larger, configurable, per-route limit — see
  [Upload size limit](#upload-size-limit) below. Every other route keeps the
  2 MB default.

## Architecture note: why this lives in `sparql_endpoint`

`rml_endpoint.rs` is an HTTP adapter, not where the mapping logic lives — all
translation (CSV/JSON/XML → RDF) stays in the `rml` crate, which has no
knowledge of HTTP, multipart, the dataset registry, or the changelog.
`rml_endpoint.rs` only handles concerns that are inherently tied to the axum
server: multipart extraction, `RmlError` → HTTP status mapping, dataset
lookup, and changelog/store merging. This mirrors `shacl_endpoint.rs` (an
adapter for the `shacl` crate) and `upload.rs`/`graph_store.rs` (adapters for
`turtle`/`jsonld_parser`) — no RDF/RML logic crate in this workspace knows
about axum.

## Upload size limit

Both `POST /{name}/rml` and `POST /rml/map` accept arbitrary source files
(CSV/XML/JSON), which routinely exceed axum's 2 MB `DefaultBodyLimit`. Axum
supports per-route limits without touching the global default:

```rust
.route(
    "/{name}/rml",
    post(crate::rml_endpoint::dataset_rml_post)
        .layer(DefaultBodyLimit::max(state.config.max_rml_upload_bytes)),
)
```

(See `axum_core::extract::DefaultBodyLimit` — "Different limits for different
routes".) Only these two routes get the raised limit; every other route
(including plain `/upload`) keeps the 2 MB default.

- New `Config` field: `pub max_rml_upload_bytes: usize`, default `64 * 1024 *
  1024` (64 MiB). Large enough for real-world CSV/XML exports, small enough
  to bound worst-case memory use per request (each multipart field is
  buffered fully in memory via `field.bytes()`, matching the existing
  `upload.rs` pattern of reading the whole body into `Bytes`).
- Exceeding the limit produces axum's standard `413 Payload Too Large`
  response — no custom handling needed.
- New test: `rml_post_accepts_upload_larger_than_2mb` — POST a source file
  comfortably over 2 MB (well under the 64 MiB default) and assert `200`,
  proving the per-route limit (not just the route's existence) is in effect.

## Request shape

`POST /{name}/rml`, `Content-Type: multipart/form-data; boundary=...`

- One part named `mapping` — the RML mapping document (Turtle). Required.
- Zero or more additional parts, each with a `Content-Disposition` `filename`
  matching exactly the string used in `rml:source "..."` inside the mapping
  (e.g. a part with `filename="people.csv"` satisfies `rml:source "people.csv"`).
  Parts are identified by `filename`, not by part `name` — this lets a client
  attach several source files without needing distinct field names.

Server behavior:
1. Reject with `403 Forbidden` if `state.config.read_only`.
2. Look up the dataset in the registry; `404 Not Found` if missing (same as
   `dataset_shacl_post`).
3. Reject with `400 Bad Request` if the request is not `multipart/form-data`,
   if no `mapping` part is present, or if any non-mapping part lacks a
   `filename`.
4. Create a temporary directory (`tempfile::TempDir`). Write the `mapping`
   part to `<tmp>/mapping.ttl` and every other part to `<tmp>/<filename>`.
5. Call `rml::apply_rml_mapping(<tmp>/mapping.ttl, <tmp>, &mut tmp_store)`
   where `tmp_store` is a fresh `Datastore`. On `RmlError`, return
   `400 Bad Request` with the error message (mirrors how Turtle parse errors
   are reported in `upload.rs` / `shacl_endpoint.rs`).
6. Drop the temp directory (automatic on `TempDir` drop) — no on-disk
   artifacts survive the request.
7. Iterate **all** quads in `tmp_store` (not just the default graph — RML
   mappings can use `rml:graphMap` to target named graphs), build changelog
   `LogEntry::InsertQuad` entries (graph `None` for the default graph, `Some(iri)`
   for named graphs, resolved via `tmp_store.resources.get_named_resource`),
   and append them to `state.changelog` if persistence is enabled.
8. Merge every quad from `tmp_store` into the target dataset's store,
   interning each term via `store.add_resource(...)` — same pattern as
   `upload.rs`, generalized to all graphs instead of only the default graph.
9. Return `200 OK` with a short plain-text summary (e.g. `"RML mapping
   applied: N triples inserted"`).

## Permission classification

`POST /{name}/rml` mutates the store, so it must classify as `Permission::Write`,
exactly like `POST /{name}/update`. `auth::classify` already has:

```rust
if method == Method::POST && (path == "/upload" || path.ends_with("/update")) {
    return Permission::Write;
}
```

Extend the condition to `path.ends_with("/update") || path.ends_with("/rml")`.
Add one test case (`post_dataset_rml_is_write`) to the classifier test table.

## New/changed files

- `sparql_endpoint/src/rml_endpoint.rs` (new) — the `dataset_rml_post` handler.
- `sparql_endpoint/src/lib.rs` — add `pub mod rml_endpoint;`; add
  `Config::max_rml_upload_bytes: usize` (default 64 MiB).
- `sparql_endpoint/src/server.rs` — add the `/{name}/rml` route with a
  per-route `DefaultBodyLimit::max(state.config.max_rml_upload_bytes)` layer.
- `sparql_endpoint/src/auth.rs` — extend `classify`, add one test.
- `sparql_endpoint/Cargo.toml` — add `rml = { path = "../rml" }` and
  `tempfile = "3"` to `[dependencies]`; add `features = ["multipart"]` to the
  `axum` dependency; add `features = ["multipart"]` to the `reqwest` dev-dependency
  (test client needs to build multipart bodies).
- `sparql_endpoint/tests/common/mod.rs` — add `dataset_rml_url(&self, dataset: &str) -> String`.
- `sparql_endpoint/tests/rml.rs` (new) — integration tests, `#[ignore]`d until
  implementation (red phase).
- `docs/user/rml-mapping.md` — documented (see "Applying mappings over HTTP").

## Test plan (red phase — all `#[ignore]`d initially)

1. `rml_post_csv_mapping_inserts_triples` — multipart POST with a mapping +
   one CSV source on a writable dataset; assert `200 OK` and that a
   subsequent `SELECT` over `/{name}/sparql` returns the mapped triples.
2. `rml_post_missing_mapping_part_is_bad_request` — multipart POST with only
   a source file, no `mapping` part; assert `400`.
3. `rml_post_unknown_dataset_is_not_found` — POST to `/nonexistent/rml`;
   assert `404`.
4. `rml_post_read_only_server_is_forbidden` — POST against a read-only
   server; assert `403`.
5. `rml_post_invalid_mapping_is_bad_request` — multipart POST with malformed
   Turtle as the `mapping` part; assert `400` and a body containing the
   parse error.
6. `rml_post_with_named_graph_inserts_into_named_graph` — mapping using
   `rml:graphMap`; assert the resulting triples are queryable via
   `GRAPH <iri> { ... }` and not visible in the default graph.
7. `rml_post_persists_to_changelog` — using `TestServer::start_writable_persistent`,
   apply a mapping, restart the server, and assert the mapped triples survive
   (changelog replay).
8. `rml_post_write_permission_required` — with `start_writable_with_key`,
   assert that a request without the API key is rejected (covers the
   `Permission::Write` classification end-to-end, complementing the unit test
   in `auth.rs`).
9. `rml_post_accepts_upload_larger_than_2mb` — source file comfortably over
   2 MB; assert `200`, proving the per-route `DefaultBodyLimit` override is
   actually in effect (see [Upload size limit](#upload-size-limit)).

## Stateless mapping endpoint: `POST /rml/map`

A second endpoint: apply a mapping to its sources and return the generated
RDF directly, without touching any dataset. Useful for testing a mapping
before committing it, or for one-off conversions that never need to land in
a store at all.

**Design decisions (confirmed with user):**

- **Route**: `POST /rml/map` — **root-level**, not dataset-scoped. No dataset
  is read or written, so there is no `{name}` segment; this also means it
  works identically regardless of which datasets exist or whether the server
  is in read-only mode. (Name chosen over `/rml/preview` because the
  endpoint's purpose is applying a mapping and getting the result back, not
  previewing — "preview" implies a side effect is otherwise about to happen,
  which isn't the case here.)
- **Response**: **content-negotiated**, reusing the RDF serialisation
  machinery already in `graph_store.rs` (`RdfFormat`, `negotiate_rdf_format`,
  `graph_response_parts` — currently private to that module, to be promoted
  to `pub(crate)` for reuse here rather than duplicated).
- **Upload size**: same per-route `DefaultBodyLimit::max(state.config.max_rml_upload_bytes)`
  override as `POST /{name}/rml` — see [Upload size limit](#upload-size-limit).

**Request shape**: identical multipart body to `POST /{name}/rml` (one
`mapping` part, zero or more named source parts).

**Server behavior:**
1. Reject with `400 Bad Request` under the same conditions as the dataset
   endpoint (not multipart, missing `mapping` part, parts without `filename`).
2. Materialise parts to a `tempfile::TempDir`, call
   `rml::apply_rml_mapping(...)` into a fresh `Datastore` — same as steps 4–6
   of the dataset endpoint, except the resulting store is never merged
   anywhere; it only exists to be serialised back to the client.
3. On `RmlError`, `400 Bad Request` with the error message (same as the
   dataset endpoint).
4. Negotiate the response format from `Accept` via `negotiate_rdf_format`:
   - `application/n-quads` → `serialize_nquads(&tmp)` (whole store, all
     graphs — same function used for whole-dataset GSP responses).
   - `application/trig` → `serialize_trig(&tmp)` (whole store, all graphs).
   - `text/turtle` (default) / `application/n-triples` / `application/ld+json`
     → delegate to `graph_response_parts(&tmp, DEFAULT_GRAPH_ELEMENT_ID, accept)`,
     i.e. **default-graph triples only** — these formats have no way to
     represent named graphs, mirroring the existing single-graph GSP
     behavior. Triples placed in a named graph via `rml:graphMap` are only
     visible when the client asks for `application/n-quads` or
     `application/trig`.
   - No matching/acceptable type → `406 Not Acceptable`.
5. No dataset lookup, no changelog entry, no `state.store` access at all —
   this handler never touches `AppState.registry` or `AppState.changelog`.

**Permission classification**: unaffected. `classify()`'s default case
(`Permission::Read`) already applies — this is a stateless transform, not a
mutation, so no change to `auth.rs` is needed for this endpoint.

**New/changed files (`/rml/map` endpoint):**
- `sparql_endpoint/src/rml_endpoint.rs` — add `rml_map_post` handler,
  alongside `dataset_rml_post`.
- `sparql_endpoint/src/graph_store.rs` — promote `RdfFormat`,
  `negotiate_rdf_format`, and `graph_response_parts` from private to
  `pub(crate)` so `rml_endpoint.rs` can reuse them.
- `sparql_endpoint/src/server.rs` — add the root `POST /rml/map` route, with
  the same `DefaultBodyLimit` override as `/{name}/rml`.
- `sparql_endpoint/tests/common/mod.rs` — add `rml_map_url(&self) -> String`.
- `sparql_endpoint/tests/rml.rs` — add the test cases below.

**Test plan (`/rml/map` endpoint, red phase — `#[ignore]`d initially):**

1. `rml_map_csv_mapping_returns_turtle_by_default` — POST mapping + CSV, no
   `Accept` header; assert `200`, `content-type: text/turtle`, and the body
   contains the expected triple.
2. `rml_map_missing_mapping_part_is_bad_request` — same as the dataset
   endpoint's case 2, but against `/rml/map`; assert `400`.
3. `rml_map_invalid_mapping_is_bad_request` — malformed Turtle as the
   `mapping` part; assert `400` with an error message in the body.
4. `rml_map_does_not_modify_any_dataset` — POST a mapping that would
   generate a recognisable subject IRI, then `SELECT` for that IRI via
   `/ds/sparql`; assert the bindings are empty (nothing leaked into the
   default dataset).
5. `rml_map_respects_accept_header_jsonld` — `Accept: application/ld+json`;
   assert `200`, `content-type: application/ld+json`, and the body parses as
   JSON-LD containing the expected data.
6. `rml_map_with_named_graph_returns_nquads` — mapping using `rml:graphMap`,
   `Accept: application/n-quads`; assert `200` and the body contains a quad
   with the expected graph IRI as its fourth term.

## Out of scope for this change

- Multiple mapping files per request (the CLI's `--mapping` flag accepts many;
  the REST endpoints accept exactly one per call — callers needing several
  mappings can issue several requests).
- `rml:JoinCondition`, SQL/JDBC sources, FunctionMap — already out of scope
  for the `rml` crate itself.

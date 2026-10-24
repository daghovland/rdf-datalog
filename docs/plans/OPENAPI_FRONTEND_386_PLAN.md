# OpenAPI / Swagger UI frontend for the REST API (#386)

## Goal

A standardized, generated API documentation page for `sparql_endpoint`'s HTTP
surface, aimed at developers integrating over HTTP (as opposed to the existing
`/` query-builder frontend, which is aimed at interactive end users). See
[#386](https://github.com/daghovland/rdf-datalog/issues/386).

## Crate choice

`utoipa` 5.5 + `utoipa-swagger-ui` 9.0 (axum feature). Actively maintained,
widely used with axum, no known RUSTSEC advisories for either crate or their
transitive deps at time of writing (checked via `cargo audit` — see below).

**Spec construction approach:** hand-built via `utoipa::openapi::OpenApiBuilder`
and the `Paths`/`PathItem`/`Operation`/`Parameter`/`Content` builders, rather
than annotating existing handlers with `#[utoipa::path(...)]` macros. The
handlers in this crate use ad-hoc extractors (raw `String`/`Bytes` bodies,
content-negotiated multi-format responses, `Query<HashMap<..>>`, etc.) built
up over ~10k lines across many files; retrofitting macro annotations onto all
of them would touch every handler signature for a purely additive
documentation feature, which is a large-blast-radius change for what should
be low-risk infrastructure. Building the `OpenApi` value programmatically in
one new module (`openapi.rs`) keeps the change additive and isolated.

## Route coverage (first pass)

Covered — SPARQL 1.1 Protocol + Graph Store Protocol + admin API, the routes
with clear, stable, mostly-textual request/response shapes:

- `GET /sparql`, `POST /sparql` (SPARQL Protocol, root dataset)
- `GET /{name}/sparql`, `POST /{name}/sparql`, `GET /{name}/query`, `POST /{name}/query`
- `POST /{name}/update` (SPARQL Update)
- `GET/HEAD/PUT/POST/DELETE /rdf-graph-store` (Graph Store Protocol, root)
- `GET/HEAD/PUT/POST/DELETE /rdf-graphs/{path}` (direct graph identification)
- `GET/HEAD/PUT/POST/DELETE /{name}/data`, `GET/HEAD /{name}/get`
- `GET /$/ping`, `GET /$/ready`, `GET /$/server`
- `GET/POST /$/datasets`, `GET/DELETE /$/datasets/{name}`, `POST /$/compact`
- `GET /auth/config`
- `GET /void`, `GET /.well-known/void`

Deferred (tracked in follow-up [#517](https://github.com/daghovland/rdf-datalog/issues/517)):
SHACL validation, RML/OTTR mapping endpoints, the runtime-ruleset endpoint, and
the proprietary transaction API — all have request bodies that are either
multipart, mapping-language-specific, or otherwise awkward to express
cleanly as a single OpenAPI schema in a first pass.

## Where served

- Spec: `GET /api-docs/openapi.json`
- Interactive UI: `GET /swagger-ui` (redirects to `/swagger-ui/`)

## Auth treatment

Both new routes are `GET`, so `auth::classify()` already classifies them as
`Permission::Read` with no code changes needed — identical treatment to the
existing `/` query-builder frontend (also `GET`, also `Permission::Read`).
Neither is hardcoded fully-public like `/auth/config`; both respect
`require_for_reads` / OIDC read-role gating exactly like every other read
route, which keeps this consistent with the existing frontend's behavior
rather than inventing a new exemption class.

## Tests

Integration tests in `sparql_endpoint/tests/` verifying:
- `GET /api-docs/openapi.json` returns 200 with `content-type: application/json`
  and body that parses as valid JSON containing `openapi`, `info`, `paths` keys,
  and specific expected paths (e.g. `/sparql`, `/rdf-graph-store`).
- `GET /swagger-ui/` returns 200 HTML referencing the spec URL.
- Existing auth tests extended/checked to confirm the new routes behave like
  other `GET` read routes under `AuthConfig::ApiKey { require_for_reads: true, .. }`.

## Follow-up coverage ([#517](https://github.com/daghovland/rdf-datalog/issues/517))

The routes deferred above were added to `build_openapi()` following the
same hand-built pattern. Design decisions specific to that follow-up:

- **Multipart bodies** (RML's `POST /{name}/rml`/`POST /rml/map`, OTTR's
  `POST /{name}/ottr`) are documented as a single opaque
  `multipart/form-data` string-schema body, not a per-part schema (utoipa's
  `Content`/`Schema` types don't have a clean way to express "one named part
  plus N arbitrarily-named file parts" short of a bespoke JSON Schema). This
  is the resolution to the "awkward to express cleanly as a single OpenAPI
  schema" problem cited above for these two areas.
- **`txId`** (proprietary transaction API) is documented only as a query
  parameter on the root `/sparql` route. `dataset_routes.rs`'s
  `/{name}/sparql`/`/{name}/query`/`/{name}/update` handlers don't look at
  `txId` at all, so adding the parameter there would misdocument behavior
  that doesn't exist.
- Root `POST /sparql`'s request body now documents two content types
  (`application/sparql-query` and `application/sparql-update`), since that
  handler dispatches on `Content-Type` to either a query or an update —
  the update path is the only way to write within a transaction, since
  `/{name}/update` doesn't honor `txId`.
- The rules endpoints (`POST /{name}/rules(/{ruleset_id})`,
  `DELETE /{name}/rules/{ruleset_id}`) and `POST /transaction/begin` return
  axum `Json<...>` bodies, i.e. plain `application/json` — distinct from
  the SPARQL-results-shaped `application/sparql-results+json` the admin/
  query routes use, so a separate `ok_plain_json_response` helper documents
  them correctly.

Several other wired routes were found undocumented along the way (some
incorrectly listed as covered above) and are out of scope for #517 — see
follow-up [#723](https://github.com/daghovland/rdf-datalog/issues/723).

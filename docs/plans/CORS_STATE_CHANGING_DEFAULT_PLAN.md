# Plan: restrict default CORS for state-changing methods

Issue: [#362](https://github.com/daghovland/rdf-datalog/issues/362)
Branch: `fix/362-cors-auth-default`

## Problem

`sparql_endpoint::server::build_router` sets `CorsLayer::new().allow_origin(Any)`
for every HTTP method (GET/HEAD/POST/PUT/DELETE/OPTIONS), applied globally.
Combined with the documented default `AuthConfig::None`, any web page visited
by a browser that also has network access to a dagalog instance can issue
cross-origin `POST /update`, `PUT /{name}/data`, `DELETE /$/datasets/{name}`,
etc. `allow_credentials` is correctly unset, so this doesn't leak
cookies/session credentials, but it still allows blind cross-origin state
changes against an unauthenticated instance.

## Design decision

Chosen approach: **(a) code-level default change**, not just a doc warning
(the doc warning (b) is added regardless, per the issue).

Split CORS behavior by method safety, decoupled from `AuthConfig` (simpler to
reason about, and strictly safer even when auth *is* configured — an
unauthenticated attacker page still shouldn't get free preflight approval to
attempt state-changing requests):

- **Safe methods** (`GET`, `HEAD`, plain `OPTIONS` probes) keep the previous
  permissive behavior: `Access-Control-Allow-Origin: *`. This preserves the
  legitimate cross-origin read use case (a web UI hosted on a different
  origin querying the endpoint) and is low-risk since no credentials are ever
  sent (`allow_credentials` stays unset).
- **State-changing methods** (`POST`, `PUT`, `DELETE`, `PATCH`, anything not
  GET/HEAD) require the request's `Origin` to exactly match one entry of a
  new, explicit allow-list: `Config::cors_allowed_origins` (`Vec<String>`,
  default empty). With the default empty list, no cross-origin
  state-changing request gets a CORS-approving preflight response, so
  browsers refuse to send the real request. Same-origin requests (no
  `Origin` header, or a browser that doesn't send one for same-origin fetch)
  are unaffected — CORS only ever restricts *cross-origin* browser requests,
  never same-origin ones or non-browser clients (curl, server-to-server).

Implementation mechanism: `tower_http::cors::AllowOrigin::predicate`, which
receives the `Origin` header value and `http::request::Parts`. For a CORS
preflight (`OPTIONS` with `Access-Control-Request-Method`), the *intended*
method is in that header, not `parts.method` (which is always `OPTIONS` for
preflights) — the predicate reads `Access-Control-Request-Method` when
present, else falls back to `parts.method` for simple (non-preflighted)
requests.

CLI/env wiring follows the existing `max_rdf_upload_bytes` precedent:
`--cors-allow-origin <ORIGIN>` (repeatable / comma-delimited),
env `DAGALOG_CORS_ALLOW_ORIGIN`.

## Tests (added first, ignored, then unignored one at a time)

New file `sparql_endpoint/tests/cors.rs`:

1. Cross-origin preflight for `PUT /{name}/data` with no allow-list configured
   → response carries no `Access-Control-Allow-Origin` header (browser would
   block the real request).
2. Same for `POST /{name}/update`.
3. Same for `DELETE /$/datasets/{name}`.
4. Cross-origin preflight for a state-changing method **with** the request's
   origin present in `cors_allowed_origins` → `Access-Control-Allow-Origin`
   echoes that origin.
5. Cross-origin preflight for a state-changing method with a **different**
   origin than the one allow-listed → still no CORS header.
6. Cross-origin `GET /sparql` (safe method) with no allow-list configured →
   still gets `Access-Control-Allow-Origin: *` (read use case preserved).

## Docs

`docs/user/deployment.md` gets a new warning section next to the existing
default-auth documentation, spelling out: default is unauthenticated +
CORS-restricted-to-same-origin-for-writes; operators who need a cross-origin
write use case must explicitly allow-list origins; operators who need real
protection should set `--api-key` or OIDC regardless of CORS.

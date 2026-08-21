# Dereferenceable dagalog resource IRIs — plan (#493)

Follow-up to [#441](https://github.com/daghovland/rdf-datalog/issues/441) /
[#440](https://github.com/daghovland/rdf-datalog/issues/440). #441's plan
([`DEREFERENCEABLE_URIS_441_PLAN.md`](DEREFERENCEABLE_URIS_441_PLAN.md))
covered only the **vocabulary/ontology** IRIs (`bl:*`/`agp:*` terms as
hash-URI fragments of `/ns/backlog` and `/ns/agentprov`), served as static
files by Caddy, and explicitly deferred **resource** IRIs (individual
`bl:Issue`/`bl:PullRequest`/`agp:AgentSession` instances) to this issue.

## Design questions resolved

### 1. 303 redirect target shape

Two new routes on `sparql_endpoint`, following the crate's existing
conventions (`/void`, `/vqs/productive-values` — plain GET routes with no
per-resource path segment for the "answer" endpoint):

- **`GET /describe?uri=<percent-encoded IRI>`** — the actual description
  endpoint. Runs a DESCRIBE-shaped query against the live `Datastore` for
  the given IRI and returns RDF, content-negotiated (see §2). Chosen over a
  dedicated per-resource route (e.g. `/resource/<encoded>`) because a query
  parameter, not a path segment, is the natural fit for a value that can be
  *any* IRI of *any* authority (see the `bl:Issue` finding below) — a path
  segment would need percent-encoding gymnastics for `/` and other
  reserved characters in arbitrary IRIs, whereas a query parameter's
  encoding is the standard one HTTP clients already do. It also matches the
  existing `?uri=`-shaped precedent in this crate
  (`sparql_endpoint/src/frontend.html`'s resource browser already builds
  `DESCRIBE <iri>` queries client-side against `/sparql?query=...`).

- **`GET /id/{*path}`** — the 303-redirect *source*: a generic dagalog.no
  slash-URI resource namespace. Reconstructs the full resource IRI as
  `state.config.base_iri` + the request path (`/id/foo/bar` → `<base_iri>/id/foo/bar`)
  and responds `303 See Other` with `Location: /describe?uri=<percent-encoded
  that IRI>` (a **relative** Location — see below). This is the generic
  mechanism the issue asks for: *any* IRI minted under `/id/*` on this
  deployment's own `base_iri` dereferences via 303 to its description.

  **Important finding, changes scope**: no `bl:`/`agp:` individual actually
  uses this `/id/*` namespace today (see §4 "What's actually queryable").
  `bl:Issue`/`bl:PullRequest` individuals are minted as the *real GitHub
  issue/PR URL* (`https://github.com/daghovland/rdf-datalog/issues/N`,
  `backlog/src/loader.rs::issue_iri`) — an authority dagalog does not own,
  so there is nothing for dagalog to 303-redirect (GitHub, not us, answers
  a dereference of that IRI, with its own HTML page). `agp:AgentSession`
  individuals are hash-URIs (`https://dagalog.no/ns/agentprov/session#pr1`,
  see `scripts/new-provenance-summary.sh`) — the #441 hash-URI convention
  applies to them, not the 303 pattern (fragment stripped client-side, only
  the namespace document `/ns/agentprov/session` is ever requested — which
  is itself not wired up; see the follow-up issue filed for this).
  `/id/{*path}` is therefore implemented and tested as the generic
  mechanism the issue describes, but is not yet the dereference path for
  any individual actually in the dataset. A follow-up issue is filed for
  minting real `/id/*`-scheme IRIs for a resource type dagalog itself
  originates (rather than borrowing GitHub's or using a hash-URI), which
  would be the first real consumer of this route.

  **Relative `Location`, not absolute.** An absolute `Location` built from
  `base_iri` would leak whatever `base_iri` is configured to (e.g. the
  internal `dagalog:3030` Docker hostname behind the public Caddy proxy) to
  external clients if `base_iri` is ever misconfigured relative to the
  public-facing domain. A relative `Location: /describe?uri=...` is valid
  per RFC 7231 §7.1.2 and resolves against whatever host the client actually
  used to reach `/id/*`, avoiding that failure mode entirely.

### 2. Content negotiation

Reuses `graph_store::negotiate_rdf_format` and `graph_store::graph_response_parts`
verbatim (both already `pub(crate)`) rather than writing a third negotiator —
`negotiate.rs` only covers SELECT/ASK result formats, GSP already has the RDF
negotiator this endpoint needs. `/describe` therefore supports the same
formats GSP GET does: Turtle (default and explicit `text/turtle`), N-Triples,
N-Quads, TriG, and JSON-LD (`application/ld+json`, via `jsonld_parser::serialize_jsonld`).
Turtle is the floor the issue requires; JSON-LD (the stated stretch goal) is
included at no extra design cost since the shared negotiator already has it.
An `Accept` header naming none of these gets `406 Not Acceptable`, matching
GSP's existing behavior.

### 3. Where served from

`sparql_endpoint` crate, in-process against the live `Arc<RwLock<Datastore>>`
in `AppState` — per the issue's own steer, confirmed correct: only this
crate has direct access to a live, queryable `Datastore` (the `backlog_endpoint`
crate is a separate process that itself only talks to `sparql_endpoint`'s
`/sparql` route over HTTP — see `backlog_endpoint/src/main.rs` — so it could
not serve this any more directly than proxying to `/describe` itself).

**Query shape.** SPARQL's `DESCRIBE <iri>` (via `sparql_parser`) is
deliberately **not** reused as-is: `Query::Describe`'s executor
(`sparql_parser/src/execute/mod.rs`, confirmed by
`sparql_parser/tests/describe_from_tests.rs::test_describe_iri_returns_subject_triples`)
only collects triples where the resource is the **subject** (outbound only).
This was a conscious, already-documented scope decision from
[#281](https://github.com/daghovland/rdf-datalog/issues/281) (visible via
this repo's own provenance record, `session:pr281Summary` — see "Prior
provenance checked" in the PR description), which left DESCRIBE outbound-only
and had the frontend's resource browser run a *second*, separate `SELECT ?s
?p WHERE { ?s ?p <iri> }` for inbound triples. For a `bl:Issue`, most of the
interesting data is exactly this inbound direction — `agp:AgentSession`
individuals point *at* the PR/issue via `agp:reasoningFor`, not the other way
around — so an outbound-only description of a `bl:Issue` would omit the
provenance links that are the whole reason this issue is worth doing. This
endpoint therefore reimplements the same "outbound ∪ inbound" merge the
frontend already does client-side, but server-side and in one query: look up
the IRI's `GraphElementId` (`Datastore::lookup_named_graph_id` — misleadingly
named for this use, but it is exactly "resolve an interned IRI to its ID",
already used generically elsewhere), then union
`QuadTable::get_quads_with_subject` and `QuadTable::get_quads_with_object`.
An IRI unknown to the store (never interned) returns `404 Not Found`.

### 4. Auth

**Decision: no bypass. `/describe` and `/id/*` are `GET`s, `auth::classify()`
already returns `Permission::Read` for any unmatched `GET` path via its
existing fallthrough, and no special-case is added.** This differs from
#441's vocabulary routes, which *were* carved out of `forward_auth` in
`deploy/Caddyfile`. The two cases are not analogous:

- #441's `/ns/backlog` and `/ns/agentprov` serve **static vocabulary files
  with no dataset content** — carving them out of auth leaks nothing about
  what's actually loaded into the running deployment.
- `/describe` serves **live dataset content** — anything currently in the
  `Datastore`, not just `bl:`/`agp:` triples. Per `deploy/Caddyfile`'s
  current routing (see its header comment), the entire SPARQL surface
  (`dagalog:3030`, including `/sparql` itself) sits behind the
  `forward_auth` catch-all today; only `/oauth2/*` and the two exact `/ns/*`
  paths are carved out. In other words, **this deployment's dataset already
  requires auth to read at all** — carving `/describe` out to be public
  would let an unauthenticated client read arbitrary dataset content via
  `/describe?uri=<anything present in the store>` that `/sparql` itself
  would refuse to serve them, which defeats the point of gating `/sparql`
  in the first place.

  This matches the task's own tie-breaker: *"if the dataset itself requires
  auth to read via SPARQL today, dereferencing should inherit the same
  read-auth requirement, not bypass it."* Verified directly against
  `deploy/Caddyfile` (read in full for this plan) rather than assumed.

No `Caddyfile` change is needed for this: `/describe` and `/id/*` are new
paths under the existing catch-all `handle` block, so they inherit
`forward_auth` automatically — nothing to add. If a future deployment makes
its dataset genuinely public (e.g. drops `forward_auth` from the catch-all,
or `AuthConfig` is later configured in-app instead of at the edge), this
route becomes public with zero code change here, since it was never given
special treatment to begin with — it just rides along with whatever `/sparql`'s
own gate is at the time.

### What's actually queryable today (the gap this issue's task explicitly
allows working around)

`backlog/examples/snapshot.ttl` (the `bl:`/`agp:`-populated fixture used by
this repo's own tests, e.g. `tests/backlog_queries.rs`) is regenerated
manually (`cargo run -p backlog --bin backlog-regenerate`, not on any
schedule or CI hook) and is **not** loaded into the production deployment's
`Datastore`: `deploy/docker-compose.public.yml`'s `dagalog` service starts
with `--data /data/dataset.ttl`, a path with no wiring anywhere in this repo
that copies or merges `backlog/examples/snapshot.ttl` (or a live-fetched
equivalent) into it. So today, no `bl:Issue`/`bl:PullRequest` instance is
actually queryable via a *running* `dagalog --serve` instance in production.

Per this issue's own instructions this is not blocking: the mechanism below
is implemented and tested against a `Datastore` loaded with
`backlog/examples/snapshot.ttl` + a `provenance/summaries/*.ttl` file
directly (the same fixtures the rest of the backlog test suite already
uses), and a follow-up issue is filed for wiring the snapshot (or a
live-fetched equivalent) into the production `--data` file.

## Non-goals for this PR

- Populating the production deployment's `/data/dataset.ttl` with real
  `bl:`/`agp:` data (follow-up issue filed).
- Minting any new dagalog.no-owned resource IRIs (e.g. actually putting
  something under `/id/*`) — this PR builds the generic mechanism, not a
  first real user of it (follow-up issue filed).
- Fixing `GET /ns/agentprov/session` (falls through to the auth catch-all
  and 404s today for every `agp:AgentSession` hash-URI's fragment-stripped
  dereference) — that's the #441 static-namespace-document pattern with an
  unbounded-growth problem (growing session list in one document), not this
  issue's 303 pattern. Follow-up issue filed.
- A rendered HTML representation for browsers (out of scope for both this
  and #441; Turtle/JSON-LD only).

## Validation plan

New test file `sparql_endpoint/tests/describe_resource.rs`, following
`void_endpoint.rs`'s harness (`common::TestServer::start(turtle)`). Written
`#[ignore]`d first, unignored one at a time during implementation, per this
repo's TDD process. Covers: route availability, outbound+inbound triple
merge, unknown-IRI 404, Turtle/JSON-LD content negotiation and 406 fallback,
`/id/*` 303 + relative `Location` construction, and `auth::classify()` unit
tests confirming both new paths classify as `Read` (added to
`sparql_endpoint/src/auth.rs`'s existing test module, not a new file, to sit
next to the other `classify()` cases).

# `agp:AgentSession` hash-URI dereference — plan (#567)

Follow-up from [#493](https://github.com/daghovland/rdf-datalog/issues/493)
(dereferenceable resource IRIs) — see
[`docs/plans/DEREFERENCEABLE_RESOURCE_IRIS_493_PLAN.md`](DEREFERENCEABLE_RESOURCE_IRIS_493_PLAN.md),
design question 1, which explicitly deferred this case.

## The problem

`agp:AgentSession` individuals are minted as hash-URIs under
`https://dagalog.no/ns/agentprov/session#<id>` (e.g. `session:pr493`, see
`scripts/new-provenance-summary.sh` and every `provenance/summaries/*.ttl`
file). Per the hash-URI convention (same as #441's `bl:`/`agp:` vocabulary
terms), a client dereferencing one of these strips the fragment and requests
`https://dagalog.no/ns/agentprov/session` (no fragment).

`deploy/Caddyfile`'s `handle /ns/agentprov` block is an **exact match** on
`/ns/agentprov` only, not `/ns/agentprov/session` — so this path falls
through to the auth catch-all and either 404s or redirects to Google
sign-in. No `agp:AgentSession` IRI in the repo's own provenance data
actually dereferences today.

## Data-flow investigation (before picking an approach)

The issue names two options and asks to validate which is more tractable
against how data actually flows in this repo, rather than assume.

**Is `provenance/summaries/*.ttl` loaded into any *live*, queryable
`Datastore` at runtime?**

- In the **production** deployment (`deploy/docker-compose.public.yml`),
  no: the `dagalog` service starts with a single
  `--data /data/dataset.ttl`, and nothing in this repo copies or merges
  `provenance/summaries/*.ttl` into that file. This is the same gap
  #493's plan doc already found and filed a follow-up for (wiring real
  `bl:`/`agp:` data into the production `--data` file) — it is still open.
- **Locally**, yes, but only via `scripts/serve-backlog.sh`, which passes
  every `provenance/summaries/*.ttl` file as its own `--data` argument
  alongside the backlog snapshot and vocab files, and `tests/serve_backlog_provenance.rs`
  proves this join works. So the mechanism to load this data into a live
  `Datastore` already exists and is exercised by tests — it just isn't
  wired into the *public* deployment yet (same open follow-up as above).
- The production **runtime container** (`Dockerfile`) copies only the
  compiled `dagalog` binary into the final image (`COPY --from=builder
  /build/target/release/dagalog /usr/local/bin/dagalog`) — the repo's
  `provenance/` directory is never present on disk in that container. So
  a design that reads `provenance/summaries/*.ttl` directly from the
  filesystem at request time is not viable for `dagalog` itself in
  production without adding a new bind mount (unlike `backlog/ontology`,
  which #441 already bind-mounts into the **Caddy** container as
  `/srv/vocab` for its two static vocabulary files).

This confirms the plan doc's own framing: "dynamically generated" cannot
mean "query the live `Datastore`" *today*, because production's
`Datastore` doesn't hold this data (same gap as #493, already tracked).
It also confirms it cannot mean "read the files directly from the
`dagalog` container's disk", because that container never has them.

## Design decision: Option 1, implemented like `/describe` (#493's
precedent), same open dependency #493 already accepted

**Chosen: a new `GET /ns/agentprov/session` route in `sparql_endpoint`,
querying the live `Datastore` for every triple whose subject is a
`https://dagalog.no/ns/agentprov/session#*` resource, content-negotiated
the same way `/describe` is.** Caddy gets a new unauthenticated carve-out
(`handle /ns/agentprov/session { reverse_proxy dagalog:3030 }`), mirroring
the existing `/ns/backlog` / `/ns/agentprov` carve-outs, but proxying to the
live app instead of serving a static file (since the document now has
per-session content instead of a fixed handful of vocabulary terms).

This is deliberately **not** a new standalone file-generation script (a
`backlog-regenerate`-style binary scanning `provenance/summaries/*.ttl` and
writing a static `backlog/ontology/agentprov-sessions.ttl` for Caddy to
serve as a plain file, the same way `/ns/agentprov` is served today).
That alternative was considered and rejected:

- It introduces a **second regeneration lifecycle** (on top of
  `backlog-regenerate`'s already-manual one) that goes stale the moment a
  new `provenance/summaries/pr-<N>.ttl` lands without someone remembering
  to re-run it — worse than the chosen approach, which is automatically
  correct for whatever the `Datastore` actually holds, with zero
  extra manual step.
- It duplicates the "merge many Turtle files, filter to individuals whose
  IRI falls under a namespace, serialise" logic that `serve-backlog.sh` +
  `--data` already does generically for any file list. Reimplementing that
  as a bespoke binary adds a maintenance surface for no benefit over just
  wiring the existing generic mechanism (loading `--data` files into a
  `Datastore`) into production, which #493 already filed as a follow-up
  every other resource-dereference route depends on too.
- It does not compose with the *already accepted* precedent from #493:
  `/describe` and `/id/*` were built and tested against a `Datastore`
  loaded from fixtures, deliberately **not** blocked on production's
  `--data` gap, with a follow-up filed to close that gap generally. This
  PR's route is the same shape of dependency — filing a *second*,
  narrower follow-up (a session-specific static generator) would create
  two different "make production data real" stories to reconcile later
  instead of one.

So: implement and test `GET /ns/agentprov/session` against a `Datastore`
loaded directly with fixture Turtle (this PR's own tests) and against the
real `provenance/summaries/*.ttl` shape used by `scripts/serve-backlog.sh`
(already covered by `tests/serve_backlog_provenance.rs`, unaffected by this
PR). No new follow-up is needed for "wire production's `--data`" — #493
already filed that one and it covers this route too, since it becomes real
the moment that follow-up lands, with no further code change here.

### Why not option 2 (slash-URI migration)?

Reconsidered and rejected, per the issue's own framing: it changes the
*minting convention* every future `provenance/summaries/pr-<N>.ttl` file
uses (`scripts/new-provenance-summary.sh`, documented in
`docs/plans/TRANSCRIPT_SUMMARY_GUIDELINES.md`), and every existing
hash-URI in the ~226 already-merged summary files would need either a
migration or would be left permanently non-dereferenceable under the new
scheme. Option 1 fixes the dereference story for *all* `agp:AgentSession`
individuals, past and future, with zero changes to how they're minted.

### Content shape and auth

- **Outbound-only.** Unlike `/describe` (which merges outbound + inbound
  because a `bl:Issue`'s interesting data mostly points *at* it), this is a
  namespace document in the #441 sense: it describes the resources *it
  defines* (every `session:*` individual — `agp:AgentSession`,
  `agp:TranscriptSummary`, `agp:Decision`), which is exactly their own
  outbound triples. Inbound triples (e.g. a `bl:PullRequest`
  `bl:closesIssue`-linking a `bl:Issue`) belong to a *different* namespace's
  document, not this one.
- **200, not 404, when empty.** `/describe` 404s for a single unknown IRI
  (nothing at all is known about that specific resource). This route
  describes a *set* that may legitimately be empty (e.g. in a store with no
  provenance data loaded, which is every production deployment until the
  #493 follow-up lands) — that is still a valid, if minimal, namespace
  document, not an error.
- **Unauthenticated**, unlike `/describe`/`/id/*` (#493 §4's decision).
  `/describe` is gated because it can expose *arbitrary* live dataset
  content the SPARQL endpoint itself requires auth to read. This route
  only ever exposes `session:*` triples — content whose entire purpose is
  public dereferenceability (same reasoning #441 already used for
  `/ns/backlog`/`/ns/agentprov`, and these are the exact same files:
  `provenance/summaries/*.ttl` are committed, publicly-readable-on-GitHub
  files, not sensitive dataset content). Gating it would mean external RDF
  clients/crawlers dereferencing a `session:*` hash-URI can never succeed,
  defeating the point.
- **Reuses `graph_store::graph_response_parts`** for content negotiation,
  same as `/describe` (Turtle default, N-Triples/N-Quads/TriG/JSON-LD via
  `Accept`, 406 for anything else).

## Validation plan

New test file `sparql_endpoint/tests/agentprov_session_document.rs`,
following `describe_resource.rs`'s harness (`common::TestServer::start`).
Written `#[ignore]`d first, unignored one at a time. Covers: route
availability and 200 status on both empty and populated stores, outbound
triples for a fixture `agp:AgentSession`/`agp:TranscriptSummary` included,
a triple for an unrelated `bl:Issue` (outside the `session:` namespace)
excluded, Turtle/JSON-LD content negotiation and 406 fallback.

No `auth.rs` `classify()` change is needed: `classify()` already returns
`Permission::Read` for any unmatched `GET` via its existing fallthrough
(same as `/describe`), and this route's "no auth" property is enforced at
the Caddy edge (see below), not inside `dagalog` itself — consistent with
`AppState`'s existing `AuthConfig::None`-at-the-edge deployment story (see
#493 §4).

## `deploy/Caddyfile` change

Add a new exact-match `handle /ns/agentprov/session` block, alongside the
existing `/ns/backlog`/`/ns/agentprov` blocks and *before* the auth
catch-all, proxying (not serving a static file) to `dagalog:3030`:

```
handle /ns/agentprov/session {
	reverse_proxy dagalog:3030
}
```

No `docker-compose.public.yml` change is needed — `dagalog` is already on
the `edge` network Caddy proxies to.

## Non-goals for this PR

- Wiring `provenance/summaries/*.ttl` (or the backlog snapshot) into the
  production `--data` file — already tracked by #493's own follow-up; this
  PR's route becomes populated automatically once that lands, no code
  change here.
- A rendered HTML representation for browsers (out of scope, matching
  #441/#493).
- Migrating `agp:AgentSession` minting to slash-URIs (option 2, rejected
  above).

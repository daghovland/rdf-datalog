# Dereferenceable dagalog.no vocabulary URIs — plan (#441)

Follow-up to [#440](https://github.com/daghovland/rdf-datalog/issues/440) (namespace
migration to `dagalog.no`, done). This plan covers only **sub-concern 1** from
[#441](https://github.com/daghovland/rdf-datalog/issues/441): making the
ontology/vocabulary IRIs (`https://dagalog.no/ns/backlog#*`,
`https://dagalog.no/ns/agentprov#*`) dereferenceable. Sub-concern 2 (resource
IRIs for individual `bl:Issue`/`bl:PullRequest`/`agp:AgentSession` instances,
served from dagalog's own SPARQL endpoint) is out of scope here and tracked in
a separate follow-up issue (filed as part of this PR — see the issue link in
the PR description).

## Design questions resolved

### 1. Hash-URI direct-response, not 303-see-other

Both `backlog/ontology/vocabulary.ttl` and
`backlog/ontology/agentprov-vocabulary.ttl` mint terms as **hash URIs**:

```turtle
@prefix bl:  <https://dagalog.no/ns/backlog#> .
@prefix agp: <https://dagalog.no/ns/agentprov#> .
```

Per the hash-URI convention (see W3C "Cool URIs for the Semantic Web" §4.2),
a client dereferencing `https://dagalog.no/ns/backlog#Issue` never actually
sends the fragment to the server — browsers and RDF libraries strip
everything from `#` onward before the request leaves the client. The
request that arrives at the server is for the **namespace IRI itself**,
`https://dagalog.no/ns/backlog` (no fragment), and the convention is that
the representation returned for that IRI describes *all* the fragment terms
defined within it — i.e. the whole vocabulary file is a valid, complete
answer to dereferencing any term in it.

This means no redirect is needed: the namespace IRI can be answered
**directly** by serving the vocabulary file's content. (Contrast with
slash-URIs — e.g. a hypothetical `https://dagalog.no/resource/issue/123` —
where the resource IRI and its RDF description are different things, and the
standard pattern is a `303 See Other` redirect from the resource IRI to a
separate "the description of that resource" IRI. That pattern is what
sub-concern 2, resource IRIs, will need — not this one.)

Concretely: `GET /ns/backlog` and `GET /ns/agentprov` (exact paths, no
trailing slash, matching the ontology IRIs declared in the `.ttl` files
themselves) respond `200 OK` directly with the vocabulary file's content. No
sub-paths exist under `/ns/backlog/*` or `/ns/agentprov/*` today (all terms
are fragments of the namespace IRI, not path children), so only the two
exact paths are handled; nothing else needs a route.

### 2. Content negotiation

Two audiences, two representations, no third format in this first version:

| `Accept` header | Response | Content-Type |
|---|---|---|
| Contains `text/turtle` (any RDF client, `curl -H "Accept: text/turtle"`) | Raw Turtle file | `text/turtle; charset=utf-8` |
| Contains `text/html` (browsers — Chrome/Firefox/Safari all send `text/html` in their default `Accept`) | Same Turtle file, served as viewable text (not a forced download) | `text/plain; charset=utf-8` |
| Missing, or `*/*`, or anything else not matching the above (e.g. a bare `curl <url>`, most non-browser HTTP clients, crawlers) | Raw Turtle file | `text/turtle; charset=utf-8` (Turtle is the sane default — these are machine-consumed vocabulary files first; a human with a bare `curl` still gets readable Turtle text either way) |

JSON-LD is **not** implemented in this first version — it's called out in
the issue as a "reasonable stretch goal," but Caddy alone can't produce it
(it would need dagalog engine involvement, e.g. `jsonld_parser`'s
serialiser, which contradicts sub-concern 1's explicit "no dagalog engine
involvement needed" framing). Deferred to the same follow-up issue as
sub-concern 2, since both would want a small serving layer beyond static
files.

A full HTML-rendered vocabulary page (styled documentation, cross-linked
terms, etc. — the "Cool URIs" ideal) is explicitly **out of scope** for this
first version. Caddy's built-in directives (`file_server`, `header`,
`rewrite`, header-based `@matcher`s) can serve a static file and set
`Content-Type` conditionally, but cannot template arbitrary HTML from
Turtle content without a custom serving layer. The practical minimum that
satisfies "browsers get something readable, not a raw download" is serving
the same Turtle text as `text/plain` for browser requests: modern browsers
render `text/plain` inline in the tab instead of prompting a file download,
which is sufficient for a first version. A nicer rendered HTML page is
natural follow-up scope alongside JSON-LD, once sub-concern 2 decides
whether a small Rust/axum service ends up serving both namespace and
resource IRIs together (in which case the vocabulary case could move off
static Caddy hosting onto that same service, gaining HTML templating for
free).

### 3. Where served from

Static file hosting via the existing Caddy instance (`deploy/Caddyfile`),
reusing the public deployment infrastructure from
[#438](https://github.com/daghovland/rdf-datalog/issues/438) /
[docs/deploy/PUBLIC_DEPLOYMENT.md](../deploy/PUBLIC_DEPLOYMENT.md). Two new
`handle` blocks are added for `/ns/backlog` and `/ns/agentprov`, each
serving one file via `file_server` + `rewrite`, with a header matcher to
adjust `Content-Type` for browser `Accept` headers.

The `backlog/ontology/` directory (containing the two `.ttl` files) is
bind-mounted read-only into the `caddy` container in
`deploy/docker-compose.public.yml`, alongside the existing `Caddyfile`
mount, so the served content always matches what's checked into the repo at
deploy time — no separate copy step or new build artifact.

**Public, no auth gate.** The existing Caddyfile structure puts everything
under a `forward_auth`-gated catch-all `handle` block, with only
`/oauth2/*` (oauth2-proxy's own sign-in/callback routes) carved out ahead of
it — the comment there explains why: those routes must be reachable
*before* the user is authenticated. The vocabulary routes need the same
treatment for a different reason: they exist specifically to be
dereferenced by arbitrary external RDF clients, crawlers, and Semantic Web
tooling that will never have (and should never need) a Google account on
this deployment. Gating `/ns/backlog` and `/ns/agentprov` behind
`forward_auth` would defeat the entire point of #441 — a client following
`bl:Issue` would hit a Google sign-in redirect instead of RDF. So both new
`handle` blocks are added as siblings to the `/oauth2/*` block, before the
authenticated catch-all, following the exact same "carve out of auth"
pattern already established in the file.

## Non-goals for this PR

- Resource IRI dereferencing (`bl:PullRequest`/`bl:Issue`/`agp:AgentSession`
  instances) — needs a 303-redirect design and SPARQL-`DESCRIBE`-shaped
  response from dagalog itself. Filed as a separate follow-up issue (see PR
  description for the issue number).
- JSON-LD content negotiation for the vocabulary case.
- Rendered HTML documentation pages for the vocabulary case (beyond
  `text/plain` viewability).
- Any change to `backlog/ontology/vocabulary.ttl` or
  `agentprov-vocabulary.ttl` content itself.

## Validation plan

No Rust code changes, so no `cargo test` equivalent. Validated via:

```bash
docker exec deploy-caddy-1 caddy adapt --config /etc/caddy/Caddyfile --pretty
```

against the modified `deploy/Caddyfile` to confirm valid Caddyfile syntax
(same method used for the earlier Caddyfile fixes in
[#477](https://github.com/daghovland/rdf-datalog/issues/477)/[#478](https://github.com/daghovland/rdf-datalog/issues/478)),
plus manual review of the diff against the existing working structure. The
live `deploy-caddy-1` container is not reloaded with this branch's config as
part of this PR — it continues running whatever is actually merged to
`main`.

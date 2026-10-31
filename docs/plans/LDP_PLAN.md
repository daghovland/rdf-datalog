# Linked Data Platform 1.0 implementation plan

Tracked in epic [#73](https://github.com/daghovland/rdf-datalog/issues/73).
This doc is the plan-writing deliverable for sub-issue
[#753](https://github.com/daghovland/rdf-datalog/issues/753); per this repo's
`CLAUDE.md`, it is the first phase of the TDD workflow for a new feature
area — written before any tests or code.

**Specifications:**
- LDP 1.0 core (W3C Recommendation, 26 February 2015):
  <https://www.w3.org/TR/ldp/> — all unqualified `§` references below are to
  this document.
- LDP Paging 1.0 (W3C Working Group **Note**, not a Recommendation):
  <https://www.w3.org/TR/ldp-paging/> — referenced as "Paging Note §N".

**Prior background:** `docs/plans/PROTOCOLS_IMPLEMENTATION.md` §LDP scope
note, `docs/architecture/PROTOCOLS.md` §6.

Every normative claim below was checked against the actual spec text (not
recalled from memory) before this doc was finalized.

---

## 1. Scope

Epic #73's framing: *"Implement Linked Data Platform 1.0 on top of the
existing Graph Store HTTP Protocol layer."* In scope for this plan:

- **LDP RDF Sources (LDP-RS)** — individually addressable RDF resources.
- **LDP Basic Containers (LDP-BC)** — containers whose membership triples
  are the containment triples (`ldp:contains`), §5.1/§5.2.
- The `Link: <http://www.w3.org/ns/ldp#Resource>; rel="type"` discovery
  header and the container-type variant, §4.2.1.4/§5.2.1.4.
- `Prefer` header handling: container GET filtering (`PreferContainment`/
  `PreferMembership`/`PreferMinimalContainer`, §7.2.2) and request-side
  interaction-model selection via `Link: rel="type"` on POST (§5.2.3.4).
- `Allow` (§4.2.8.2) / `Accept-Post` (§5.2.3.13/§7.1.2) discovery headers.
- `ETag`/conditional-write handling (§4.2.1.3, §4.2.4.5) — required by the
  core spec for *every* LDPR, not LDP-specific polish, so it is pulled into
  Phase A rather than deferred.
- The `ldp:constrainedBy` Link header on constraint-violating responses
  (§4.2.1.6).
- A reduced subset of container pagination (§4.1 defers full paging to the
  separate Note; see §4 below for exactly what subset this plan covers).

**Explicitly out of scope**, filed as real follow-up issues per CLAUDE.md's
rule that deferred work discovered mid-task becomes a tracked issue rather
than just plan-doc prose (each left at Status `Todo`, awaiting Dag's
review, and linked as a sub-issue of epic #73):

- LDP Direct Containers / Indirect Containers (§5.3) —
  [#755](https://github.com/daghovland/rdf-datalog/issues/755).
- LDP-NR (non-RDF Source / binary resource) support — this store has no
  binary-blob storage layer at all today —
  [#756](https://github.com/daghovland/rdf-datalog/issues/756).
- Full LDP-Paging conformance (sort criteria, `first`/`last`/`prev` Link
  relations, the optional 303-redirect flow) beyond the reduced subset in
  §4 — [#757](https://github.com/daghovland/rdf-datalog/issues/757).

## 2. Mapping onto the existing GSP layer

`sparql_endpoint/src/graph_store.rs` implements the SPARQL 1.1 GSP
direct-identification model (§4.1 of
<https://www.w3.org/TR/sparql11-http-rdf-update/>): `direct_gsp_get` /
`direct_gsp_put` / `direct_gsp_post` / `direct_gsp_delete` / `direct_gsp_head`
mounted at `/rdf-graphs/{*path}` (confirmed route registration:
`sparql_endpoint/src/server.rs:227`, with `direct_graph_iri(base_iri, path)`
building the graph IRI from the request path, `graph_store.rs:925`). This is
the right foundation for LDP, which is also a direct-identification model:
an LDP-RS's URL *is* its IRI, and `GET`/`PUT`/`POST`/`DELETE`/`HEAD` map
onto RDF-document-level CRUD the same way GSP's direct model already does.

Checked directly in the codebase (not assumed): **none of the direct-GSP
handlers currently set an `ETag`, `Allow`, `Accept-Post`, or `Link`
header** — `grep -rni etag sparql_endpoint/src` only turns up `ETag` on the
SPARQL *query* path (`query.rs`, store-generation-counter based, used for
transaction snapshot reads) and the transaction-precondition check in
`dataset_routes.rs`; `graph_store.rs` has no `etag`/`Allow`/`Accept-Post`
occurrences at all. So LDP's header requirements are new code across the
board, not a thin wrapper around headers GSP already emits — §3 below spells
out exactly what Phase A has to add.

| LDP concept | Existing building block | Reusable as-is | Needs new code |
|---|---|---|---|
| LDP-RS representation (GET/HEAD) | `direct_gsp_get`/`direct_gsp_head` → `graph_response_parts` → `negotiate_rdf_format` | Body serialisation (Turtle/N-Triples/N-Quads/TriG/JSON-LD) and the parse/validate pipeline | `Link: rel="type"`, `ETag` (§4.2.1.3, MUST — see §3.3), `Allow` |
| LDP-RS replace (PUT) | `direct_gsp_put` | Create (201)/replace (204) semantics, `parse_rdf_body`, reasoner-delta integration | Conditional-write support (`If-Match`/412/428, §4.2.4.5, SHOULD-level but still new code); `ldp:constrainedBy` Link header on rejections |
| LDP-RS delete | `direct_gsp_delete` | 204/404 semantics | Must also retract the owning container's `ldp:contains` triple (§5.2.5.2 derivable requirement — containment triples are server-managed) |
| LDP-BC representation (GET) | `direct_gsp_get`/`graph_response_parts` for serialisation once content is assembled | Serialisation machinery | Containment triples are not simply "the graph's stored content" — see the design decision below; `Prefer`-driven inclusion/omission (§7.2.2) |
| LDP-BC member creation (POST) | `direct_gsp_post`'s "merge into existing graph" shape is the closest existing analogue, but LDP POST creates a **new** resource, which is closer to `gsp_post_inner`'s server-assigned-IRI branch (`graph_store.rs`, the no-param case) | Parse pipeline, reasoner-delta integration, 201+`Location` pattern | New child-resource minting (`Slug` header, §4.2.3.1), atomic containment-triple append, honouring a client-requested interaction model via `Link: rel="type"` on the *request* (§5.2.3.4, MUST) |
| `Allow`/`Accept-Post` discovery | *(none today, confirmed above)* | — | New: `Allow` differs between a plain LDP-RS and an LDP-BC; `Accept-Post` is BC-only and MUST appear on OPTIONS (§5.2.3.13), SHOULD appear wherever POST is accepted (§7.1.2) |
| `OPTIONS` method | *(not routed anywhere in `sparql_endpoint` today — confirmed: no `Method::OPTIONS` handler exists outside the `tower_http` CORS preflight layer)* | CORS preflight already uses `tower_http::cors::CorsLayer` (`server.rs:131`) for browser preflight; the LDP `OPTIONS` *response* is a separate, non-preflight request a client sends to discover capabilities, so it needs its own handler registered alongside `CorsLayer`, not a replacement for it | New handler per resource kind, carrying `Allow`/`Accept-Post`/`Link` (§4.2.8.2) |

**Key design decision — how containment triples are stored.** Model each
LDP container as one GSP named graph (same direct-identification model as a
plain LDP-RS), whose *stored* triples are the container's own description
plus the authoritative set of `ldp:contains <member-iri>` triples, updated
transactionally by the POST/DELETE handlers rather than derived by scanning
other graphs for a magic predicate at read time. This keeps `graph_store.rs`
completely untouched; all LDP-specific logic lives in a new
`sparql_endpoint/src/ldp.rs` module that *calls* the existing helpers
(`parse_rdf_body`, `graph_response_parts`, `direct_graph_iri`,
`rdf_upload_format`, `negotiate_rdf_format`) rather than duplicating them.

## 3. Header and status-code requirements

### 3.1 `Link` header — `rel="type"` (§4.2.1.4, §5.2.1.4, both MUST)

> "LDP servers MUST advertise their LDP support by exposing a HTTP `Link`
> header" with target `http://www.w3.org/ns/ldp#Resource`, in **all**
> responses to requests made to an LDPR's Request-URI (§4.2.1.4) — i.e. not
> just GET: also HEAD, PUT, POST, DELETE, OPTIONS responses to that URI.

A container additionally advertises its container type the same way
(§5.2.1.4; valid types: `BasicContainer`, `DirectContainer`,
`IndirectContainer` — only `BasicContainer` is in scope here, §1). Both
headers are present together — `ldp:BasicContainer` is additive, not a
replacement for `ldp:Resource` (a container is still an RDF Source).

Implementation: a small helper appending these headers via
`HeaderMap`/`axum::response::AppendHeaders` at the LDP-module layer, kept
orthogonal to `graph_response_parts` (which stays GSP-only/LDP-agnostic).

### 3.2 `Allow` / `Accept-Post` (§4.2.8.2, §5.2.3.13, §7.1.2)

| Resource kind | `Allow` | `Accept-Post` |
|---|---|---|
| LDP-RS (non-container) | `GET, HEAD, OPTIONS, PUT, DELETE` | *(absent — POST to a plain, non-container RDF Source is not defined by this plan; see §2's POST row)* |
| LDP-BC | `GET, HEAD, OPTIONS, POST, PUT, DELETE` | the exact set `rdf_upload_format` already recognises, derived from one shared const so the two never drift: `text/turtle, application/n-triples, application/n-quads, application/trig, application/ld+json` |

- `Allow` is a MUST on the `OPTIONS` response (§4.2.8.2's literal wording is
  about advertising method support via OPTIONS).
- `Accept-Post` is a MUST on the container's `OPTIONS` response (§5.2.3.13)
  and a SHOULD on any other response from a resource that supports POST
  (§7.1.2) — **not** a MUST on every container GET/HEAD. Phase B still adds
  it to GET/HEAD too (the SHOULD is cheap and avoids a client needing a
  separate OPTIONS round-trip just to discover POST support), but this is a
  documented SHOULD-level choice, not a spec MUST.

**Known rough edge, to resolve during Phase B, not pre-decided here:**
`Accept-Post`'s RDF-format list currently mirrors `rdf_upload_format`
verbatim, which includes `application/n-quads`/`application/trig` — formats
`gsp_post_inner`'s *existing* GSP semantics route to whole-dataset
operations, which makes no sense for "create one LDP-RS from this POST
body." Phase B's implementation needs to either restrict the LDP POST path
to single-graph formats (Turtle/N-Triples/JSON-LD) or define quad-aware
behaviour explicitly — not silently inherit GSP's dataset-level meaning.

### 3.3 `ETag` and conditional writes (§4.2.1.3 MUST, §4.2.4.5)

> "LDP servers MUST use entity tags (either weak or strong ones) as
> response headers for any resources that are LDP-RSs" (§4.2.1.3) — this
> applies to **all** LDPR GET and successful HEAD responses, not just
> containers. Since the codebase check above confirmed `graph_store.rs`'s
> direct-GSP routes emit no ETag today, Phase A must add one — reusing the
> existing *mechanism* (`query.rs`'s store-generation-counter-based ETag,
> `format!("\"{}\"", store.generation)`) rather than inventing a new scheme.

Conditional-write handling is SHOULD-level, not mandated: clients SHOULD
send `If-Match` (§4.2.4.5); servers SHOULD require it to detect lost
updates; **if** a conditional request is made, a server MUST return 412
Precondition Failed on mismatch, and a server that *requires* conditional
requests MUST return 428 Precondition Required when the precondition is
missing. This plan treats `If-Match` support as part of Phase A (the ETag
plumbing makes it cheap once ETag exists) but does not make the server
*require* conditional PUT (no 428s issued) — a design choice flagged
explicitly rather than silently decided, since nothing forces it either way.

### 3.4 `ldp:constrainedBy` (§4.2.1.6, MUST)

> Servers "MUST publish any constraints on LDP clients' ability to create or
> update LDPRs" via a `Link` header with
> `rel="http://www.w3.org/ns/ldp#constrainedBy"`, on responses to requests
> that fail because they violate those constraints.

Applies to: PUT attempting to change containment triples (§5.2.4.1, see
§3.5) and any other constraint rejection this plan's handlers introduce.
Target: a small static documentation resource (e.g.
`{base_iri}/ldp/constraints`) describing the constraints in prose — the
spec does not mandate this target be machine-readable.

### 3.5 PUT changing containment triples (§5.2.4.1, SHOULD-level)

> Servers "SHOULD NOT allow HTTP PUT to update a LDPC's containment
> triples"; if such a request is received, the server "SHOULD respond with
> a 409 (Conflict) status code."

Both SHOULD, not MUST — but this plan implements the 409 guard anyway (it's
cheap and avoids a confusing silent divergence between a client's PUT body
and what the server actually persists). The guard is written in Phase A
(where it is a no-op — no containers exist yet) and activated by Phase B.

### 3.6 `Prefer` header (§7.2.2, §5.2.1.5, §5.2.3.4)

Two independent uses, both container-related (there is no `Prefer`-based
POST-response-representation requirement in LDP 1.0 itself for ordinary
LDP-RS creation — that's an RFC 7240-generic, not LDP-specific, concern this
plan does not special-case):

1. **Container GET filtering** (§7.2.2, via `return=representation`'s
   `include`/`omit` parameters):
   - `http://www.w3.org/ns/ldp#PreferContainment` — containment triples.
   - `http://www.w3.org/ns/ldp#PreferMembership` — membership triples; for
     an LDP-BC these coincide with containment triples (§5.1), so this plan
     treats the two as synonyms when the resource is a Basic Container.
   - `http://www.w3.org/ns/ldp#PreferMinimalContainer` — shorthand for
     omitting both, returning only the container's own description
     triples. (`PreferEmptyContainer` is a deprecated alias per the spec;
     not supported.)
   - Honouring these hints is itself only a SHOULD for containers
     (§5.2.1.5) — a server ignoring `Prefer` entirely is still spec-
     conformant, and clients MUST tolerate that (§4.3.1.13). This plan
     honours them anyway (the filtering is simple once containment triples
     are known) but documents that it is not a hard requirement.
   - `Preference-Applied` echo: no LDP-specific MUST/SHOULD found in the
     spec text itself (RFC 7240 recommends it generically when a client
     can't otherwise tell what happened); treated as a nice-to-have, not a
     gating requirement for Phase B.
2. **POST interaction-model selection** (§5.2.3.4, **not** `Prefer` — a
   plain request `Link` header): a client requests the kind of resource to
   create by sending `Link: <http://www.w3.org/ns/ldp#BasicContainer>;
   rel="type"` on the POST itself. Per §5.2.3.4, if the server creates the
   resource from the RDF body, it "MUST honor the client's requested
   interaction model(s)" and "MUST fail the request" if it cannot (e.g. a
   server that doesn't support nested containers must reject a POST asking
   to create one, rather than silently creating a plain LDP-RS instead).
   This directly enables **nested Basic Containers** (a container creating
   a child container) without any Direct/Indirect Container machinery —
   Phase B must parse this request-side `Link` header and dispatch
   accordingly, not just assume every POST creates a plain LDP-RS member.

### 3.7 Status codes

| Operation | Success | Notable failure modes |
|---|---|---|
| GET LDP-RS / LDP-BC | 200 | 404 if missing |
| HEAD | 200, empty body | same as GET |
| OPTIONS | 200, empty body, headers only | — |
| PUT (create) | 201 + `Location` | 409 + `ldp:constrainedBy` if it would alter containment triples (§3.5); 412/428 for conditional-write failures (§3.3) |
| PUT (replace) | 204 | same guards as above |
| POST to LDP-BC (create member) | 201 + `Location` | request `Link: rel="type"` the server cannot honour → request failure, not silent fallback (§3.6 point 2); `Slug` collision → always disambiguate by suffixing rather than 409 (documented simplification — the spec permits either) |
| POST to non-container LDP-RS | *(not supported by this plan, see §2)* | 405 Method Not Allowed |
| DELETE | 204 | 404 if missing; must also retract the parent container's containment triple |

## 4. Pagination design

§4.1 of the core LDP REC explicitly defers paging to a separate document:
*"A separate LDP specification outlines the conformance rules around
pagination."* That document, LDP Paging 1.0, is a **W3C Working Group
Note**, not a Recommendation — i.e. advisory, not a conformance target with
the same weight as the core spec. This plan adopts a reduced, verified
subset of it (full conformance, including sort criteria and the optional
redirect flow, is filed as follow-up [#757](https://github.com/daghovland/rdf-datalog/issues/757)):

- **Request-side sizing**: `Prefer: return=representation;
  max-member-count="N"` (Paging Note §5.2.1; the request hint is a
  parameter of `return=representation`, not a bare `include=` URI — an
  earlier draft of this plan incorrectly guessed an `include="...#Page"`
  syntax; verified against the Note's own example in its §4.2, which reads
  `Prefer: return=representation; max-triple-count="500"` for the
  size-limited sibling case). `max-member-count` is this plan's choice
  (sized by container membership count, the natural unit for an LDP-BC)
  over `max-triple-count`/`max-kbyte-count`, both of which the Note also
  defines but which this plan does not implement.
- **Page identification**: every in-sequence page response MUST carry
  `Link: <http://www.w3.org/ns/ldp#Page>; rel="type"` (Paging Note §6.2.16,
  MUST) — in addition to, not instead of, the core spec's `ldp:Resource`/
  `ldp:BasicContainer` type links from §3.1.
- **Page navigation**: `Link: rel="next"` is MUST on every page except the
  last (Paging Note §6.2.12); `rel="first"`/`rel="last"`/`rel="prev"` are
  all MAY (§6.2.10/§6.2.11/§6.2.14) and this plan does not implement them
  in Phase C (follow-up #757). The Note defines **no** `ldp:nextPage` or
  `ldp:prevPage` body triples — navigation is Link-header-only, confirmed
  against the spec text (an earlier draft of this plan incorrectly assumed
  a triple-based mechanism).
- **First-page serving**: the Note permits (§6.2.3/§6.2.6) either serving
  the first page directly from the container's own URI, or a `303 See
  Other` redirect to a paging-aware first-page URI (SHOULD, for
  paging-aware clients specifically). This plan takes the simpler direct-
  serving route — no 303 — which the Note explicitly allows.
- **Ordering**: containment triples are paginated in a stable,
  deterministic order (the `QuadTable`'s existing storage/insertion order
  for that graph). No sort-criteria negotiation (`ldp:pageSortCriteria` /
  `ldp:pageSortCriterion` / `ldp:pageSortPredicate` / `ldp:pageSortOrder` /
  `ldp:pageSortCollation`, Paging Note §7, all MAY-level) — deferred to
  #757.
- **`ldp:pageSequence`** is *not* an integer or literal, contrary to this
  plan's first draft: it is a **Link relation**
  (`rel="http://www.w3.org/ns/ldp#pageSequence"`, Paging Note §7.3.2)
  pointing at a resource describing the sort criteria. Since sort criteria
  are out of scope here, this plan does not emit `ldp:pageSequence` at all
  in Phase C.

This subset gets a consumer something genuinely useful (bounded-size pages,
correctly typed and linked) without committing to the Note's optional
sort-order machinery, which has no concrete requirement driving it yet.

## 5. Phased implementation breakdown

Mirrors the Phase A/B/C style of `docs/plans/JOIN_REORDERING_PLAN.md` and
`docs/plans/NETWORK_POLICY_PLAN.md`. Each phase is scoped to be spinnable
into one or more concrete sub-issues of epic #73 once this plan is reviewed;
issue numbers are intentionally not assigned here (per this issue's
instructions, that's the orchestrating session's job after review) — except
for the three deferred-scope issues in §1, which were filed now per
CLAUDE.md's "follow-up work discovered mid-task" rule.

### Phase A — LDP-RS fundamentals on top of direct GSP (no containers yet)

Smallest vertical slice: every graph reachable via the LDP route mount (see
§6's open question on whether this reuses `/rdf-graphs/*path` or a separate
mount) is treated as a plain LDP-RS.

- New `sparql_endpoint/src/ldp.rs` module.
- `Link: <...ldp#Resource>; rel="type"` on every response (§3.1), wrapping
  `direct_gsp_get`/`head`/`put`/`delete`.
- `ETag` on GET/HEAD, reusing the store-generation-counter scheme from
  `query.rs` (§3.3); `If-Match`/412 conditional PUT (no 428 — server does
  not require conditionality).
- New `OPTIONS` handler (none exists anywhere in the endpoint today,
  confirmed in §2) with `Allow` per §3.2's non-container row.
- `ldp:constrainedBy` Link-header plumbing (§3.4) written now as a general
  mechanism, even though Phase A has no constraint to trigger it yet —
  Phase B's PUT-containment guard (§3.5) is its first real caller.
- Tests: `Link`/`ETag`/`Allow` header presence and content on
  GET/HEAD/PUT/DELETE/OPTIONS; `If-Match` match/mismatch/missing-with-
  existing-ETag behaviour (200/412, no 428 case); 404 behaviour reused from
  existing direct-GSP tests (regression check).

### Phase B — LDP Basic Containers: read + member creation (POST)

- Container GET: own triples plus `ldp:contains` triples, `Link:
  ...#BasicContainer` header (§3.1) alongside `ldp:Resource`; `Prefer`
  `include`/`omit` filtering for `PreferContainment`/`PreferMembership`/
  `PreferMinimalContainer` (§3.6 point 1) — a new small parser for RFC
  7240's token-list-with-quoted-URI-list grammar, since nothing in the repo
  parses this today.
- POST to a container (§3.6 point 2, §3.7): parse the request's `Link:
  rel="type"` to determine the desired interaction model (plain LDP-RS vs.
  nested LDP-BC), create the child resource (optionally informed by a
  client `Slug` header, §4.2.3.1, with the disambiguate-don't-409 policy),
  append `ldp:contains` to the container's own graph, both as one
  logically atomic operation against the GSP write lock (mirrors
  `gsp_post_inner`'s server-assigned-IRI branch, §2's table). Resolve the
  `Accept-Post` quad/trig-format rough edge flagged in §3.2 as part of this
  phase, not before.
- `Accept-Post` header: MUST on container OPTIONS, SHOULD (implemented)
  elsewhere (§3.2).
- DELETE of a member retracts its container's `ldp:contains` triple;
  decide during implementation whether container deletion requires
  emptiness first (spec permits either — a design note in the sub-issue,
  not pre-committed here).
- PUT-changing-containment guard (§3.5) activated: 409 + `ldp:constrainedBy`.
- Tests: POST creates reachable child + containment triple (both plain
  LDP-RS and nested-LDP-BC via request `Link: rel="type"`); request for an
  unsupported interaction model fails the request rather than silently
  falling back; GET with each `Prefer` variant; `Accept-Post` content;
  DELETE cascade/guard behaviour; PUT-containment 409 with
  `ldp:constrainedBy` present.

### Phase C — Pagination (reduced subset, §4)

- `Prefer: return=representation; max-member-count="N"` request parsing;
  `Link: rel="type"` → `ldp:Page` and `rel="next"` on non-final pages;
  direct first-page serving (no 303); stable insertion-order pagination.
- Tests: page boundaries, last page (no `next` link, still has `ldp:Page`
  type link), empty container, default vs. client-requested
  `max-member-count`, ordering stability across repeated requests with no
  intervening writes.

### Phase D — Hardening and spec-conformance sweep

- Cross-check every §-numbered requirement cited in this doc against the
  actual implementation (a checklist derived from this plan's own tables —
  not a vendored copy of the official Java/Jena-based W3C LDP test suite,
  which doesn't fit naturally into this Rust workspace; a footnoted
  follow-up if ever worth vendoring, not blocking).
- `docs/architecture/PROTOCOLS.md` §6 updated from "❌ Deferred" to
  "✅ Done (Basic Containers + LDP-RS; Direct/Indirect Containers and full
  paging tracked separately)" once Phase C lands and CI is green.

## 6. Open questions to resolve while filing Phase A's sub-issue

- **Route mount point.** Reusing `/rdf-graphs/*path` directly conflates
  "plain GSP direct-identification" with "LDP resource" semantics on the
  exact same URL space — attractive (one graph, two protocols, no
  duplication) but means a bare SPARQL-GSP `PUT` would suddenly also get
  LDP response headers (`Link`, `ETag`, etc. — which are harmless to add
  but still a behavior change to an existing, already-shipped protocol
  surface). Alternative: a separate `/ldp/*path` mount backed by the same
  underlying graph-storage calls, keeping the two protocol surfaces
  independently addressable. Leaning towards the separate mount for header
  hygiene and to avoid retroactively changing GSP's documented behaviour,
  but this is a decision for Phase A's sub-issue, not this plan.
- **Container root.** LDP doesn't mandate a single root container, but a
  concrete consumer will want one to `POST` into first. Candidate:
  `/ldp/` as a root Basic Container, auto-created at server start
  (configurable via `Config`, consistent with how `base_iri` and other
  server-identity settings already work).

## 7. Summary table (for `docs/architecture/PROTOCOLS.md` §6 once phases land)

| Phase | Deliverable | Depends on |
|---|---|---|
| A | LDP-RS fundamentals: `Link`/`ETag`/`Allow`/`OPTIONS`/conditional PUT/`constrainedBy` plumbing over existing direct-GSP graphs | — |
| B | Basic Containers: GET w/ `Prefer` filtering, POST create-member (incl. nested containers via request `Link: rel="type"`), DELETE cascade, PUT containment guard activated | A |
| C | Pagination (`Prefer: max-member-count`, `Link: rel="next"`/`ldp#Page`) | B |
| D | Conformance sweep + docs update | C |

Deferred, tracked as separate sub-issues of epic #73, not part of A–D above:
- [#755](https://github.com/daghovland/rdf-datalog/issues/755) — Direct/Indirect Containers.
- [#756](https://github.com/daghovland/rdf-datalog/issues/756) — LDP-NR (binary resources).
- [#757](https://github.com/daghovland/rdf-datalog/issues/757) — full LDP-Paging conformance (sort criteria, `first`/`last`/`prev`, 303 flow).

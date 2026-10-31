# Linked Data Platform 1.0 implementation plan

Tracked in epic [#73](https://github.com/daghovland/rdf-datalog/issues/73).
This doc is the plan-writing deliverable for sub-issue
[#753](https://github.com/daghovland/rdf-datalog/issues/753); per this repo's
`CLAUDE.md`, it is the first phase of the TDD workflow for a new feature
area — written before any tests or code.

**Specification:** <https://www.w3.org/TR/ldp/> (W3C Recommendation, "Linked
Data Platform 1.0", 26 February 2015).

**Prior background:** `docs/plans/PROTOCOLS_IMPLEMENTATION.md` §LDP scope
note, `docs/architecture/PROTOCOLS.md` §6.

---

## 1. Scope

Epic #73's framing: *"Implement Linked Data Platform 1.0 on top of the
existing Graph Store HTTP Protocol layer."* In scope for this plan:

- **LDP RDF Sources (LDP-RS)** — individually addressable RDF resources,
  §4.3, §5.2 of the spec.
- **LDP Basic Containers (LDP-BC)** — containers whose membership triples
  are the containment triples (`ldp:contains`), §5.1, §5.2.3, §5.4, §5.6 of
  the spec. (Direct Containers and Indirect Containers, §5.3, are
  out-of-scope for this plan — see §8 below.)
- The `Link: <http://www.w3.org/ns/ldp#Resource>; rel="type"` discovery
  header (and the matching `ldp#RDFSource` / `ldp#BasicContainer` link on
  the relevant resources), §4.2.1.4, §5.2.1.4.
- `Prefer` header handling for container GET (`PreferContainment` /
  `PreferMembership` / `PreferMinimalContainer`) and for POST responses
  (`return=representation` / `return=minimal`), §4.2.8, §4.3.4, §5.2.3.
- `Allow` / `Accept-Post` discovery headers, §4.2.1.3/§4.2.8.
- Pagination for large containers.

Out of scope for the implementation phases below (filed as follow-ups once
the phase breakdown is reviewed, per the "Follow-up work" rule in
`CLAUDE.md`):
- LDP Direct Containers / Indirect Containers (§5.3) — membership subject
  other than the container itself, membership predicate other than
  `ldp:contains`. These layer cleanly on top of Phase B's container model
  but are a materially larger surface (member-relation/membership-resource
  triples, `ldp:insertedContentRelation`) with no concrete consumer yet.
- LDP-NR (non-RDF Source / binary resource) support, since this store has
  no binary blob storage layer at all today.
- Full WAC/ACL-style authorization on a per-resource basis (`Allow` header
  content reflects *protocol* capability, not per-user ACLs — this repo
  has no ACL layer to consult).

## 2. Mapping onto the existing GSP layer

`sparql_endpoint/src/graph_store.rs` already implements the SPARQL 1.1 GSP
direct-identification model (§4.1 of
<https://www.w3.org/TR/sparql11-http-rdf-update/>): `direct_gsp_get` /
`direct_gsp_put` / `direct_gsp_post` / `direct_gsp_delete` / `direct_gsp_head`
under `/rdf-graphs/*path`, where the *request URI itself* (built by
`direct_graph_iri(base_iri, path)`) is the named graph's IRI. This is the
right foundation for LDP, which is also a direct-identification model: an
LDP-RS's URL *is* its IRI, and `GET`/`PUT`/`POST`/`DELETE`/`HEAD` map
one-to-one onto RDF-document-level CRUD the same way GSP's direct model
already does.

| LDP concept | Existing building block | Reusable as-is | Needs new code |
|---|---|---|---|
| LDP-RS representation (GET/HEAD) | `direct_gsp_get`/`direct_gsp_head` → `graph_response_parts` → `negotiate_rdf_format` | Body serialisation (Turtle/N-Triples/N-Quads/TriG/JSON-LD), `ETag`/`Last-Modified` (GSP already content-negotiates and the endpoint has caching headers per PROTOCOLS.md §8) | `Link: rel="type"` header injection; LDP requires `text/turtle` is a MUST-support default, already the GSP default |
| LDP-RS replace (PUT) | `direct_gsp_put` | Create (201)/replace (204) semantics, parse-and-validate pipeline (`parse_rdf_body`, `rdf_upload_format`), reasoner-delta integration | LDP §4.2.4.2: PUT to an LDP-RS that would change its `rdf:type` membership in a container must be rejected or handled per spec — needs a container-membership-triple guard (see §3) not present in plain GSP PUT |
| LDP-RS merge (PATCH-like POST-to-resource) | *(not applicable — LDP doesn't define POST to an LDP-RS; only to containers)* | — | n/a — the `direct_gsp_post` "merge triples" semantics is a GSP-only operation; LDP routes don't need it on a non-container resource |
| LDP-RS delete | `direct_gsp_delete` | 204/404 semantics | Must also remove the container's containment triple pointing at the deleted member (§5.2.5.2) |
| LDP-BC representation (GET) | `direct_gsp_get`/`graph_response_parts`, but the *content* must be assembled, not just the stored graph | Serialisation machinery once the containment triples exist as the graph's content | New: a container's "graph" isn't just stored triples — `ldp:contains` triples must be derived from which graphs the container "owns", and `Prefer`-driven inclusion/omission of containment/membership/minimal-container triples (§3) |
| LDP-BC member creation (POST) | `direct_gsp_post`'s "merge" shape is close, but LDP POST semantics differ sharply | Parse pipeline, reasoner-delta integration, 201+`Location` pattern already used by `gsp_post_inner`'s "create with server-assigned IRI" branch | New: POST to a container creates a **new child resource** at a server- or client-minted URL (slug-based via the `Slug` header, §4.2.3.1), *and* appends an `ldp:contains` triple to the container, atomically |
| LDP-BC member deletion side-effect | `direct_gsp_delete` | — | New: deleting a resource must retract the owning container's `ldp:contains` triple for it (§5.2.5.2) |
| `Allow`/`Accept-Post` discovery | *(none today)* | — | New: computed per-resource (`Allow` differs between plain LDP-RS and LDP-BC; `Accept-Post` is BC-only, listing RDF content-types GSP's upload pipeline already accepts via `rdf_upload_format`) |
| OPTIONS method | *(not routed at all today)* | — | New: `OPTIONS` isn't wired into the direct-GSP routes currently; needs a new handler per resource kind, primarily to carry `Allow`/`Accept-Post` |

**Key design decision:** model each LDP container as backed by one GSP named
graph under the direct-identification model (same as a plain LDP-RS), where
the graph's *stored* triples are the container's own description
(`dcterms:title` etc., if any) **plus** the authoritative set of
`ldp:contains <member-iri>` triples. Membership is not derived by scanning
other graphs for a magic predicate — it is stored exactly like GSP already
stores any other graph's content, which keeps the GSP layer untouched and
pushes all LDP-specific logic into a new `ldp.rs` module that wraps/calls
into `graph_store.rs`'s existing helpers (`parse_rdf_body`,
`graph_response_parts`, `direct_graph_iri`, `rdf_upload_format`,
`negotiate_rdf_format`) rather than duplicating them.

## 3. Header and status-code requirements

### 3.1 `Link` header (§4.2.1.4, §5.2.1.4)

Every LDP-RS response (GET/HEAD/OPTIONS, and the POST/PUT response per
§4.2.1.4 wording "must") includes:

```
Link: <http://www.w3.org/ns/ldp#Resource>; rel="type"
```

A Basic Container additionally includes:

```
Link: <http://www.w3.org/ns/ldp#BasicContainer>; rel="type"
```

Both headers are present on a container response — `ldp:BasicContainer` does
not replace `ldp:Resource`, it's additive (a container *is* an RDF Source).
Implementation: a small helper `ldp_link_headers(is_container: bool) ->
Vec<(&'static str, &'static str)>` appended to every LDP response via
`HeaderMap`/`axum::response::AppendHeaders`, not threaded through
`graph_response_parts` (which stays GSP-only and LDP-agnostic).

### 3.2 `Allow` / `Accept-Post` (§4.2.1.3, §4.2.8, §5.2.1.4)

| Resource kind | `Allow` | `Accept-Post` |
|---|---|---|
| LDP-RS (non-container) | `GET, HEAD, OPTIONS, PUT, DELETE` | *(absent — POST isn't supported on a plain RDF Source by this plan)* |
| LDP-BC | `GET, HEAD, OPTIONS, POST, PUT, DELETE` | `text/turtle, application/n-triples, application/n-quads, application/trig, application/ld+json` (exactly `rdf_upload_format`'s recognised set, kept in sync by deriving this list from one shared const rather than duplicating it) |

Both headers must be present on the `OPTIONS` response and — per §4.2.8 for
`Accept-Post` specifically — on every container response (GET/HEAD too),
not only OPTIONS.

### 3.3 `Prefer` header (§4.2.8, §5.2.3.1, §5.2.3.2, §5.2.3.3, §5.2.3.4)

Two independent uses:

1. **POST response representation** (§4.2.8, generic LDP, not container-
   specific): `Prefer: return=minimal` on a creating POST/PUT suppresses a
   response body (still 201, still `Location`); `return=representation`
   (or no `Prefer` header, since GSP-style endpoints default to returning a
   status only) is a MAY, not a MUST, for the response to include the new
   resource's representation. This plan takes the simpler, spec-compliant
   path of **never** including a body on a successful POST/PUT (matching
   existing GSP behaviour) — `return=minimal` is honoured implicitly by
   always doing the minimal thing, and `return=representation` is accepted
   but not honoured in Phase B (documented, not silently ignored: respond
   with the same no-body 201, since omitting an optional representation is
   spec-conformant). Honouring it for real is a candidate follow-up, not
   blocking.
2. **Container GET filtering** (§5.2.3, LDP-BC-specific) via the `include`/
   `omit` parameters referencing:
   - `http://www.w3.org/ns/ldp#PreferContainment` — include `ldp:contains`
     triples (default: included).
   - `http://www.w3.org/ns/ldp#PreferMembership` — include membership
     triples; for a Basic Container these *are* the containment triples,
     so this plan treats `PreferMembership` as a synonym of
     `PreferContainment` on LDP-BC (per §5.1's "for an LDP-BC, containment
     triples and membership triples are identical" framing — confirm
     exact wording against the spec text during Phase B implementation).
   - `http://www.w3.org/ns/ldp#PreferMinimalContainer` — a shorthand for
     `omit` both of the above plus `ldp:contains`-adjacent member
     properties, returning only the container resource's own description
     triples (effectively the GET analogue of a plain LDP-RS response for
     a container's "empty shell").

   Parsing: `Prefer: return=representation; include="http://www.w3.org/ns/ldp#PreferMinimalContainer"`
   — a small parser for the `Prefer` header's token-list-with-quoted-URI-
   list grammar (RFC 7240 §5.4's `include`/`omit` extension parameters as
   used by LDP) is needed; no existing HTTP-header-list parser in the repo
   covers this, so **new code**, not reuse.

   Response acknowledgement: per §5.2.3.1, the server MUST (if it honoured
   the preference) echo `Preference-Applied: return=representation` — only
   needed once a `Prefer` value was actually acted on; the "minimal
   container" / "containment-only" filtering counts, the POST
   return=minimal passthrough from point 1 above does not need an echo
   since it's the unconditional default behaviour, not a preference being
   specially honoured.

### 3.4 Status codes

| Operation | Success | Notable failure modes |
|---|---|---|
| GET LDP-RS / LDP-BC | 200 | 404 if resource/graph doesn't exist (matches existing GSP direct-GET 404) |
| HEAD | 200, empty body | same as GET |
| OPTIONS | 200, empty body, headers only | — |
| PUT (create) | 201 + `Location` | PUT that would alter containment membership (§4.2.4.2) → 409 Conflict, body explaining that containment triples are server-managed |
| PUT (replace) | 204 | same 409 guard as above |
| POST to LDP-BC (create member) | 201 + `Location` | 409 if a client-supplied `Slug` collides and the server chooses not to auto-disambiguate (this plan: always disambiguate by suffixing, never 409 on Slug collision, to keep client code simple — documented deviation from the spec's permitted-but-not-mandated 409 option) |
| POST to non-container LDP-RS | *(not supported by this plan, see §2)* | 405 Method Not Allowed |
| DELETE | 204 | 404 if missing; must also retract the parent container's containment triple |

## 4. Pagination design

LDP 1.0's core REC (§5.2.3.3's notion of `ldp:PreferMinimalContainer`) does
**not** itself define paging — that's a separate W3C Note, "Linked Data
Platform Paging 1.0" (`http://www.w3.org/TR/ldp-paging/`), built on the same
`Prefer` mechanism plus `ldp:Page`, `ldp:pageSequence`, and
`http://www.w3.org/ns/ldp#PageSortCollation` for a sort criterion. Given
epic #73 explicitly lists pagination as in-scope, this plan adopts a
pragmatic subset of that Note rather than its full grammar:

- A container GET may include `Prefer: return=representation;
  include="http://www.w3.org/ns/ldp#Page"`.
- When honoured, the response is itself an `ldp:Page` resource: the
  container's own triples plus `ldp:pageSequence` (an integer, matching the
  Note's base case rather than its arbitrary-sort extensions) and, when a
  further page exists, a `Link: <...?page=N+1>; rel="next"` header pointing
  at the next page (chosen over embedding `ldp:nextPage` as an object
  triple, matching this repo's existing convention of surfacing pagination
  via `Link` headers rather than payload triples — mirrors how
  `docs/architecture/PROTOCOLS.md` treats HTTP-level metadata as headers,
  not triples, elsewhere in this doc).
- Page size: server-side default (configurable via `Config`, analogous to
  other `sparql_endpoint::Config` knobs), with `?page-size=N` as a query
  parameter rather than inventing a request-side `ldp:pageSize` triple
  mechanism — simpler, and LDP Paging's own request-side signalling is
  itself only a non-normative example in the Note, not a MUST.
- Ordering: containment triples are paginated in a **stable, deterministic
  order** (insertion order, i.e. the existing `QuadTable`'s storage order
  for that graph) — no sort-key negotiation (`PageSortCollation`) in this
  plan; that is a candidate follow-up if a concrete consumer needs it.

This is intentionally a reduced subset: full `ldp-paging` conformance
(arbitrary sort criteria, `ldp:pageSequence` interacting with concurrent
mutation of the container) is explicitly deferred — flagged as a follow-up
issue once Phase C lands, not attempted here.

## 5. Phased implementation breakdown

Mirrors the Phase A/B/C style of `docs/plans/JOIN_REORDERING_PLAN.md` and
`docs/plans/NETWORK_POLICY_PLAN.md`. Each phase is scoped to be spinnable
into one or more concrete sub-issues of epic #73 once this plan is reviewed;
issue numbers are intentionally not assigned here (per this issue's
instructions, that's the orchestrating session's job after review).

### Phase A — LDP-RS on top of direct GSP (no containers yet)

Smallest possible vertical slice: treat every graph reachable via
`/rdf-graphs/*path` (or a new `/ldp/*path` mount — naming TBD during
implementation, see open question in §8) as an LDP-RS.

- New `sparql_endpoint/src/ldp.rs` module.
- `Link: <...ldp#Resource>; rel="type"` header injection wrapping
  `direct_gsp_get`/`direct_gsp_head`/`direct_gsp_put`/`direct_gsp_delete`.
- New `OPTIONS` handler (not present anywhere in the endpoint today) with
  `Allow` computed per §3.2's non-container row.
- PUT's §4.2.4.2 containment-triple guard is a **no-op** in this phase
  (no containers exist yet to protect), but the guard function should be
  written now with a clear doc-comment pointing at Phase B, so Phase B
  doesn't have to retrofit the PUT path.
- Tests: Link header presence/content on GET/HEAD/PUT/DELETE/OPTIONS;
  Allow header on OPTIONS; 404 behaviour reused from existing GSP direct-
  route tests (confirms no regression).

### Phase B — LDP Basic Containers: read + member creation (POST)

- Container representation: GET returns the container's own triples plus
  derived `ldp:contains` triples (§2's "membership triples are stored like
  any other graph's content" design) with the second `Link:
  ...#BasicContainer` header.
  - `Prefer` parsing (§3.3's `include`/`omit` grammar) for
    `PreferContainment`/`PreferMembership`/`PreferMinimalContainer`, with
    `Preference-Applied` echoed back when something was actually filtered.
- POST to a container: create a new child LDP-RS at a server-minted URL
  (optionally informed by a client `Slug` header, §4.2.3.1, with the
  disambiguation-not-409 policy from §3.4), append `ldp:contains` to the
  container's own graph, run both as one logically atomic operation against
  the GSP write lock (mirrors the existing create-with-server-assigned-IRI
  branch in `gsp_post_inner`, §2's table).
- `Accept-Post` header on every container response (§3.2).
- DELETE of a member retracts its container's `ldp:contains` triple
  (§2/§3.4); DELETE of the container itself cascades or refuses — **decide
  during implementation** whether container deletion requires it be empty
  first (spec permits either; simplest conformant choice is documented as a
  design note in the sub-issue rather than pre-committed here).
- Tests: POST creates reachable child + containment triple; GET with each
  `Prefer` variant; `Accept-Post` content; DELETE cascade/guard behaviour;
  PUT-changing-membership → 409 per §3.4 (this activates Phase A's guard
  stub).

### Phase C — Pagination

- Implements §4's reduced subset: `Prefer: include=".../ldp#Page"`,
  `?page-size=N`, `Link: rel="next"`, deterministic ordering.
- Tests: page boundaries, last-page (no `next` link), empty container,
  page-size default vs. override, stability of ordering across repeated
  requests with no intervening writes.

### Phase D — Hardening and spec-conformance sweep

- Cross-check every §-numbered requirement cited in this doc against an
  actual implementation pass (a lightweight conformance checklist derived
  from this plan's own tables, not a full external test-suite integration —
  the W3C LDP test suite itself is Java/Jena-based and not a natural fit to
  vendor into this Rust workspace; worth a footnoted follow-up issue, not
  blocking).
- `docs/architecture/PROTOCOLS.md` §6 updated from "❌ Deferred" to
  "✅ Done" (or a partial-conformance note, if Direct/Indirect Containers
  remain explicitly out of scope per §1) once Phase C lands and CI is
  green.

## 6. Open questions to resolve before/while filing Phase A's sub-issue

- **Route mount point.** Reusing `/rdf-graphs/*path` directly conflates
  "plain GSP direct-identification" with "LDP resource" semantics on the
  exact same URL space, which is attractive (one graph, two protocols, no
  duplication) but means a client doing a bare SPARQL-GSP `PUT` would
  suddenly also get LDP response headers. Alternative: a separate
  `/ldp/*path` mount backed by the *same* underlying graph-storage calls,
  keeping the two protocol surfaces addressable independently. Leaning
  towards the separate mount for header hygiene, but flagging as a decision
  for Phase A's sub-issue rather than this plan.
- **Container root.** LDP doesn't mandate a single root container, but a
  concrete consumer will likely want one well-known container to `POST`
  into first. Candidate: `/ldp/` itself as a root Basic Container,
  auto-created at server start (configurable via `Config`, consistent with
  how `base_iri` and other server-identity settings already work).

## 7. Summary table (for `docs/architecture/PROTOCOLS.md` §6 once phases land)

| Phase | Deliverable | Depends on |
|---|---|---|
| A | LDP-RS headers/OPTIONS over existing direct-GSP graphs | — |
| B | Basic Containers: GET w/ `Prefer` filtering, POST create-member, DELETE cascade, PUT membership guard | A |
| C | Pagination (`Prefer: .../ldp#Page`, `Link: rel="next"`) | B |
| D | Conformance sweep + docs update | C |

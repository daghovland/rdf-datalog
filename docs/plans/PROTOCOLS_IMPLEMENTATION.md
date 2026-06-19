# Protocol Compliance Implementation Plan

## Status Summary

| Priority | Protocol/Feature | Status |
|---|---|---|
| **P0** | SPARQL 1.1 Protocol — query endpoint (GET/POST) | ✅ Done |
| **P0** | SPARQL 1.1 Protocol — update via `POST /sparql` (`application/sparql-update`, form `update=`) | ✅ Done |
| **P0** | SPARQL 1.1 Protocol — update endpoint (INSERT/DELETE/CLEAR/DROP/CREATE) | ✅ Done |
| **P0** | CORS headers | ✅ Done |
| **P0** | Content negotiation — SELECT/ASK SPARQL JSON (default) | ✅ Done |
| **P0** | Content negotiation — SELECT/ASK SPARQL XML, CSV, 406 Not Acceptable | ✅ Done |
| **P0** | Content negotiation — CONSTRUCT correct `application/n-triples` Content-Type | ✅ Done |
| **P1** | SPARQL 1.1 Graph Store HTTP Protocol (indirect + direct) | ✅ Done |
| **P1** | SPARQL 1.1 Service Description (`GET /sparql` no query param) | ✅ Done |
| **P2** | VoID description endpoint (`/.well-known/void`, `/void`) | ✅ Done |
| **P2** | HTTP caching headers (ETag via generation counter on Datastore) | ✅ Done |
| **P3** | JSON-LD serialization output for Graph Store (`application/ld+json`) | ✅ Done |
| **P3** | Linked Data Platform 1.0 | ❌ Deferred (see scope note) |

---

## What is working

- `GET /sparql?query=<encoded>` and `POST /sparql` with `application/sparql-query`,
  `application/x-www-form-urlencoded` (query= and update=), and `application/sparql-update`.
- SELECT, ASK, and CONSTRUCT query types.
- SELECT/ASK content negotiation: SPARQL JSON (default), SPARQL XML, CSV; 406 for
  unrecognised Accept media types.
- SPARQL Update: INSERT DATA, DELETE DATA, CLEAR, DROP, CREATE via `POST /sparql`
  (direct and form body) and per-dataset `/{name}/update`.
- Graph Store HTTP Protocol: GET/PUT/POST/DELETE/HEAD on `/rdf-graph-store` (indirect)
  and `/rdf-graphs/*path` (direct).  Accepts Turtle, N-Triples, N-Quads, TriG, JSON-LD.
  Returns Turtle, N-Triples, N-Quads, TriG, JSON-LD.
- SPARQL 1.1 Service Description returned by `GET /sparql` with `Accept: text/turtle`
  and no `query=` parameter.
- CORS headers on all routes.
- VoID description at `GET /.well-known/void` and `GET /void` (Turtle, `void:Dataset`,
  `void:sparqlEndpoint`, `void:triples`).
- ETag header on all query responses, derived from `Datastore.generation` (u64 counter
  incremented on every write).
- Authentication: API key and OIDC JWT.
- Durable persistence: redb changelog replayed on restart.
- Admin API: Fuseki-compatible `/$/ping`, `/$/server`, `/$/datasets`.

---

## What was added in this plan

### 1. SELECT/ASK content negotiation (P0 fix)

**Bug fixed:** `negotiate_select_format()` result was discarded in `query.rs`.
All SELECT queries returned SPARQL JSON regardless of `Accept`.

**What was added:**
- `serialize/sparql_xml.rs` — `to_sparql_xml(result)` and `ask_to_sparql_xml(bool)`
- `serialize/sparql_csv.rs` — `to_sparql_csv(result)` with RFC 4180 quoting
- `negotiate.rs` — return type changed to `Option<SelectFormat>`; `None` → 406;
  `*/*` and `application/*` wildcards map to JSON.
- `query.rs` — dispatcher uses negotiated format; `POST /sparql` now handles
  `application/sparql-update` and form `update=` parameters.
- CONSTRUCT arm now uses `application/n-triples` Content-Type (was wrongly `text/turtle`).

### 2. VoID description endpoint (P2)

- `void.rs` — `void_handler` and `void_turtle(base_iri, triple_count) -> String`
- Routes `/.well-known/void` and `/void` added to router.
- Response: Turtle with `void:Dataset`, `void:sparqlEndpoint`, `void:triples`.

### 3. HTTP caching headers — ETag (P2)

- `dag_rdf/src/datastore.rs` — added `generation: u64` field.
  Incremented in `add_triple`, `add_quad`, `add_named_graph_triple`, `add_reified_triple`,
  `remove_quad`, and `remove_graph`.
- `sparql_update.rs` — `apply_insert`/`apply_delete` now call `store.add_quad()` /
  `store.remove_quad()` instead of `store.named_graphs.*` directly.
- `graph_store.rs` — `copy_default_graph_to` and `copy_dataset_to` use `dst.add_quad()`.
- `query.rs` — all query responses include `ETag: "{generation}"`.

### 4. JSON-LD output for Graph Store (P3)

- `graph_store.rs` — added `JsonLd` variant to `RdfFormat` and `application/ld+json`
  to `negotiate_rdf_format`.  `graph_response_parts` builds a temporary single-graph
  Datastore and calls `jsonld_parser::serialize_jsonld`.

---

## LDP scope note (P3 — deferred)

Linked Data Platform 1.0 requires: LDP Basic Containers, LDP RDF Sources,
`Link: <http://www.w3.org/ns/ldp#Resource>; rel="type"` response headers,
`Prefer: return=representation` handling, pagination.  This is a significant
surface area on top of an already-complete GSP layer.  Deferred until there
is a concrete consumer use-case for it.

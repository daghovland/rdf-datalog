# RDF/SPARQL Protocol Compliance

This document describes the W3C and de-facto standard protocols that the HTTP
endpoint in this project should implement, with links to the normative
specifications, the required behaviour, and notes on what is in scope for this
project.

Note on terminology:
- **HTTP protocol compliance target** in this document is SPARQL 1.1 Protocol.
- **Query language implementation target** in the parser/executor is SPARQL 1.2,
  currently focused on a `SELECT` subset (including `GRAPH` patterns).

---

## 1. SPARQL 1.1 Protocol (core, required)

**Specification:** <https://www.w3.org/TR/sparql11-protocol/>

The SPARQL Protocol defines how SPARQL queries and updates are submitted to an
endpoint over HTTP. This is the baseline that any public triplestore endpoint
must implement.

### 1.1 Query endpoint

Default path: `GET|POST /sparql`

| Method | Content-Type of request | Query location |
|---|---|---|
| `GET` | — | `?query=<url-encoded SPARQL>` |
| `POST` (form) | `application/x-www-form-urlencoded` | `query=<encoded SPARQL>` in body |
| `POST` (direct) | `application/sparql-query` | raw SPARQL in body |

The query endpoint handles SELECT, ASK, CONSTRUCT, and DESCRIBE.

#### Response content-type negotiation (Accept header)

| Query form | Recommended default | Also required |
|---|---|---|
| SELECT | `application/sparql-results+json` | `application/sparql-results+xml`, `text/csv`, `text/tab-separated-values` |
| ASK | `application/sparql-results+json` | `application/sparql-results+xml` |
| CONSTRUCT | `text/turtle` | `application/n-triples`, `application/rdf+xml` |
| DESCRIBE | `text/turtle` | `application/n-triples`, `application/rdf+xml` |

#### Required HTTP status codes

| Situation | Code |
|---|---|
| Successful result | `200 OK` |
| Malformed query | `400 Bad Request` |
| Unsupported query type | `422 Unprocessable Entity` |
| Internal error | `500 Internal Server Error` |

### 1.2 Update endpoint

Default path: `POST /sparql` (or a separate `/sparql/update`)

| Method | Content-Type | Body |
|---|---|---|
| `POST` (form) | `application/x-www-form-urlencoded` | `update=<encoded SPARQL Update>` |
| `POST` (direct) | `application/sparql-update` | raw SPARQL Update in body |

SPARQL Update operations: `INSERT DATA`, `DELETE DATA`, `INSERT/DELETE` with
`WHERE`, `LOAD`, `CLEAR`, `DROP`, `CREATE`, `ADD`, `MOVE`, `COPY`.

---

## 2. SPARQL 1.1 Graph Store HTTP Protocol (required)

**Specification:** <https://www.w3.org/TR/sparql11-http-rdf-update/>

This protocol defines CRUD operations on individual named graphs using plain
HTTP verbs. It is separate from the query/update protocol and operates on RDF
graph content directly rather than through SPARQL syntax.

### Indirect graph identification (via query parameter)

Base URL: `GET|PUT|POST|DELETE /rdf-graph-store?graph=<encoded graph IRI>`

Special case for the default graph: `?default` (no value).

### Direct graph identification

The graph IRI is the request URL itself. Requires the server to accept
arbitrary IRI paths and route them to the named graph store.

### Operations

| Verb | Semantics |
|---|---|
| `GET` | Return the RDF graph as the negotiated serialization |
| `PUT` | Replace the named graph with the request body |
| `POST` | Merge (add) triples from request body into the graph |
| `DELETE` | Delete the named graph |
| `HEAD` | Return headers only (size, ETag, Last-Modified) |
| `PATCH` | Apply an `application/sparql-update` patch to the graph |

### Content-Type for graph bodies

On request and response, the format is negotiated:
- `text/turtle` (preferred for human-readable)
- `application/n-triples` (preferred for streaming/bulk)
- `application/n-quads`
- `application/trig`
- `application/rdf+xml` (legacy, required for interop)
- `application/ld+json` (JSON-LD, optional)

---

## 3. SPARQL 1.1 Service Description (recommended)

**Specification:** <https://www.w3.org/TR/sparql11-service-description/>

A `GET /sparql` request without a `query` parameter (or with
`Accept: text/turtle`) must return an RDF document describing the endpoint's
capabilities. Clients and SPARQL federators use this to discover what features
are available.

Key properties to advertise:
- `sd:endpoint` — the query endpoint IRI
- `sd:supportedLanguage` — e.g., `sd:SPARQL11Query`, `sd:SPARQL11Update`
- `sd:resultFormat` — list of supported result formats
- `sd:feature` — e.g., `sd:DereferencesURIs`, `sd:BasicFederatedQuery`
- `sd:defaultDataset` / `sd:namedGraph` — graphs available

Minimum viable response (Turtle):
```turtle
@prefix sd: <http://www.w3.org/ns/sparql-service-description#> .
@prefix void: <http://rdfs.org/ns/void#> .

<> a sd:Service ;
    sd:endpoint <http://your-host/sparql> ;
    sd:supportedLanguage sd:SPARQL11Query, sd:SPARQL11Update ;
    sd:resultFormat <http://www.w3.org/ns/formats/SPARQL_Results_JSON>,
                    <http://www.w3.org/ns/formats/SPARQL_Results_XML>,
                    <http://www.w3.org/ns/formats/Turtle> ;
    sd:feature sd:BasicFederatedQuery ;
    sd:defaultDataset [ a sd:Dataset ] .
```

---

## 4. VoID Dataset Description (recommended)

**Specification:** <https://www.w3.org/TR/void/>

A `GET /.well-known/void` or `GET /void` endpoint returns an RDF document
describing the dataset (triple count, distinct subjects/predicates/objects,
example resources, data dumps). This is important for data consumers and
for discoverability.

Key properties:
- `void:triples` / `void:distinctSubjects` / `void:distinctPredicates`
- `void:sparqlEndpoint`
- `void:dataDump` (link to a bulk download)
- `void:vocabulary` (ontologies used)
- `dcterms:title`, `dcterms:description`, `dcterms:license`

---

## 5. HTTP Content Negotiation for RDF documents (required)

Any URL that returns RDF must support content negotiation via the `Accept`
header (RFC 7231). Minimum required serializations:

| MIME type | Format | Notes |
|---|---|---|
| `text/turtle` | Turtle | Default for human-friendly responses |
| `application/n-triples` | N-Triples | Simplest; good for streaming |
| `application/n-quads` | N-Quads | For quad/dataset responses |
| `application/trig` | TriG | Named-graph Turtle extension |
| `application/rdf+xml` | RDF/XML | Required for legacy interop |
| `application/sparql-results+json` | SPARQL JSON | SELECT/ASK results |
| `application/sparql-results+xml` | SPARQL XML | SELECT/ASK results |
| `text/csv` | CSV | SELECT results |

---

## 6. Linked Data Platform 1.0 (optional, future)

**Specification:** <https://www.w3.org/TR/ldp/>

LDP defines a REST API for reading and writing linked data resources using
standard HTTP. It sits above the Graph Store protocol and adds:
- LDP Basic Containers (LDPC) — collections of resources
- LDP RDF Sources (LDPR) — individual named resources
- Pagination with `Prefer: return=representation` / `hydra:PagedCollection`
- `Link` headers advertising LDP type

Worth implementing once the core SPARQL layer is stable.

---

## 7. CORS (required for browser clients)

Any public endpoint must return CORS headers so browser-based SPARQL clients
(e.g., YASGUI) can query it:

```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, POST, OPTIONS
Access-Control-Allow-Headers: Accept, Content-Type
```

---

## 8. HTTP Caching headers (recommended)

For read-only queries, return:
- `ETag` (hash of dataset version or result set)
- `Cache-Control: no-cache` for mutable datasets, or a TTL for static ones
- `Last-Modified`

For update responses: `Cache-Control: no-store`.

---

## Summary: implementation priority

| Priority | Protocol/Feature |
|---|---|
| **P0 — required** | SPARQL 1.1 Protocol (query + update) |
| **P0 — required** | Content negotiation (Turtle, N-Triples, SPARQL JSON/XML) |
| **P0 — required** | CORS headers |
| **P1 — important** | SPARQL 1.1 Graph Store HTTP Protocol |
| **P1 — important** | SPARQL 1.1 Service Description |
| **P2 — recommended** | VoID description endpoint |
| **P2 — recommended** | HTTP caching headers (ETag, Last-Modified) |
| **P3 — future** | Linked Data Platform 1.0 |
| **P3 — future** | JSON-LD serialization |

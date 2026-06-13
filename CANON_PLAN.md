# RDF Canonicalization Plan (RDF-C14N / URDNA2015)

## Goal

Implement RDF Dataset Normalization so `DagalogRecordBackend` can satisfy
`IRecordBackend.ToCanonString()`. The records library uses this to compute and verify the
MD5 checksum stored on each content graph in the record metadata graph.

## Spec reference

- RDF Dataset Normalization 1.0 (RDNA / URDNA2015) — <https://www.w3.org/TR/rdf-canon/>
- The algorithm is also known as **RDF-C14N** in older literature; the W3C CR is the
  authoritative spec.

## What the algorithm produces

Given an RDF dataset (or graph), the algorithm:
1. Renames all blank nodes to canonical labels `_:c14n0`, `_:c14n1`, … based on the
   graph structure (not the original blank node identifiers).
2. Serializes the result as **N-Quads**, one line per quad, sorted lexicographically.

The output is a deterministic string that is identical for any two isomorphic datasets.

---

## Implementation options

### Option A — Use the `rdf-canon` crate (preferred)

Check crates.io for a production-ready URDNA2015 implementation. At the time of writing
`rdf-canon` (by `nicowillis`) exists and passes the W3C test suite. If it provides a
`canonicalize(dataset) -> String` API over oxrdf types, the integration is:

```rust
// rdf_canon/src/lib.rs
use oxrdf::Dataset;
use rdf_canon::canonicalize;

pub fn canonicalize_datastore(store: &dag_rdf::Datastore) -> String {
    let dataset = datastore_to_oxrdf(store);
    canonicalize(&dataset)
}
```

`datastore_to_oxrdf` converts the `Datastore` quads into `oxrdf::Dataset` quads. This is
straightforward since `turtle/src/lib.rs` already converts in the other direction via
`oxrdf` types.

Create a new workspace crate `rdf_canon`:

```
rdf_canon/
  Cargo.toml   (depends on dag_rdf, oxrdf, rdf-canon)
  src/
    lib.rs
```

Add to workspace `Cargo.toml`:
```toml
[workspace]
members = [..., "rdf_canon"]
```

### Option B — Implement URDNA2015 natively

If `rdf-canon` is absent or inadequate, implement the algorithm directly. The algorithm has
three phases:

#### Phase 1: Hash all quads with a preliminary blank-node label

For each quad `(s, p, o, g)`:
- Replace blank nodes with a canonical placeholder `_:a`
- Hash the resulting N-Quads line with SHA-256

Build a map `bnode_id → [hash_of_each_quad_it_appears_in]`.

#### Phase 2: Assign simple canonical labels

For blank nodes that appear in a unique hash context (no collision):
- Sort blank nodes by their combined hash
- Assign `_:c14nN` labels in sort order

For colliding blank nodes:
- Apply the **hash-n-degree-quads** sub-algorithm, which explores the neighborhood of each
  blank node to break ties.
- This is the hard part; it is recursive and requires careful cycle detection.

#### Phase 3: Re-serialize with canonical labels

Sort all output N-Quads lines lexicographically and concatenate.

This is ~400 lines of careful Rust. Recommend going with Option A unless the crate is
unmaintained.

---

## New crate: `rdf_canon`

Regardless of which implementation is used, create a crate with this public API:

```rust
/// Canonicalize the full dataset to an N-Quads string (URDNA2015).
pub fn canonicalize_dataset(store: &Datastore) -> String { ... }

/// Canonicalize a single named graph to an N-Quads string.
pub fn canonicalize_graph(store: &Datastore, graph_id: GraphElementId) -> String { ... }
```

`canonicalize_graph` is what the records library uses for per-content-graph checksumming.

---

## Records backend usage

`IRecordBackend.ToCanonString()` calls the graph-level function for each content graph.
The MD5 hash of the result is compared with the value in:

```
<content_graph_iri> rec:checksum "md5hex"^^xsd:string .
```

In the `DagalogRecordBackend` (C# side):

```csharp
public async Task<string> ToCanonString() {
    var graphIri = await GetContentGraphIri();
    // GET /{dataset}/canon?graph=<graphIri>  -- new endpoint
    var response = await _http.GetStringAsync($"{_base}/{_dataset}/canon?graph={graphIri}");
    return response;
}
```

OR: the C# backend calls `serialize_trig` to get the N-Quads bytes and then runs
canonicalization client-side using dotNetRdf. Either approach works; putting it on the
server avoids pulling the data across the wire.

---

## HTTP endpoint (optional)

Add `GET /{name}/canon?graph=<iri>` to `sparql_endpoint`:

```
GET /myrecord/canon?graph=http%3A%2F%2Fexample.org%2Fg1
→ 200 text/plain
_:c14n0 <http://...> <http://...> .
...
```

This lets external clients request the canonical form without needing a local Rust library.

---

## Testing strategy

The W3C RDF-C14N test suite is at <https://w3c.github.io/rdf-canon/tests/>.
Each test consists of an input N-Quads file and an expected output N-Quads file.

Relevant test types:
- `rdfc10-urdna2015-*` — core algorithm tests
- Tests with blank nodes only, IRIs only, mixed
- Tests with cycles in blank-node neighborhoods

Minimum test set for initial implementation:
1. Dataset with no blank nodes → output == N-Quads sorted lexicographically
2. Dataset with distinct blank nodes → labels assigned in sort-of-hash order
3. Dataset with isomorphic subgraphs (hard case) → both isomorphic blank nodes get same
   canonical label; tie broken by hash-n-degree

---

## Ordering

| Step | Change | Crate |
|---|---|---|
| 1 | Research `rdf-canon` on crates.io; prototype `datastore_to_oxrdf` | new `rdf_canon` |
| 2 | If crate works: add `canonicalize_dataset` + `canonicalize_graph` wrappers | `rdf_canon` |
| 3 | If crate absent: implement URDNA2015 phases 1–3 | `rdf_canon` |
| 4 | Add `GET /{name}/canon` endpoint | `sparql_endpoint` |
| 5 | W3C test-suite integration tests | `rdf_canon` |

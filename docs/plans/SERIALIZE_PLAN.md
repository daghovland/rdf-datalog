# Serialization Plan: N-Quads and TriG

## Goal

Add `serialize_nquads` and `serialize_trig` functions to the `turtle` crate so dagalog can
output RDF datasets in the two formats needed by `DagalogRecordBackend`:
`application/n-quads` and `application/trig`.

## Background

The `turtle` crate (`turtle/src/serialize.rs`) already has `serialize_graph`, which serializes
a single named graph as N-Triples (one triple per line, fully expanded IRIs). The two new
serializers build on its helper functions.

The `QuadTable` stores quads as `(subject, predicate, obj, triple_id)` where `triple_id` is the
graph element ID. `DEFAULT_GRAPH_ELEMENT_ID = 0` identifies the default graph. All distinct
graph IDs are the keys of `QuadTable::triple_id_index`.

## N-Quads (`serialize_nquads`)

### Spec reference

W3C RDF 1.1 N-Quads — <https://www.w3.org/TR/n-quads/>

### Format

One line per quad:

```
<subject> <predicate> <object> [<graphname>] .
```

- **Subject**: `<IRI>` or `_:label`
- **Predicate**: `<IRI>` only
- **Object**: `<IRI>`, `_:label`, or `"literal"[@lang | ^^<datatype>]`
- **Graph name** (optional): `<IRI>` or `_:label`; absent for default-graph triples

Triples whose `triple_id == DEFAULT_GRAPH_ELEMENT_ID` are written without the fourth field
(making them valid N-Triples lines). All other triples include the graph IRI as the fourth
field.

### Implementation

File: `turtle/src/serialize.rs`

```rust
pub fn serialize_nquads(store: &Datastore) -> String {
    let mut out = String::new();
    for quad in store.named_graphs.get_all_quads() {
        let s = subject_term(store.resources.get_graph_element(quad.subject));
        let p = predicate_term(store.resources.get_graph_element(quad.predicate));
        let o = object_term(store.resources.get_graph_element(quad.obj));
        let (Some(s), Some(p), Some(o)) = (s, p, o) else { continue };
        if quad.triple_id == DEFAULT_GRAPH_ELEMENT_ID {
            out.push_str(&format!("{s} {p} {o} .\n"));
        } else {
            let g = graph_term(store.resources.get_graph_element(quad.triple_id));
            if let Some(g) = g {
                out.push_str(&format!("{s} {p} {o} {g} .\n"));
            }
        }
    }
    out
}
```

Add `graph_term(elem: &GraphElement) -> Option<String>` helper that handles IRI and blank node
graph names:

```rust
fn graph_term(elem: &GraphElement) -> Option<String> {
    match elem {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => {
            // Skip the internal default-graph sentinel
            if iri.0 == DEFAULT_GRAPH_IRI { return None; }
            Some(format!("<{}>", escape_iri(&iri.0)))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            Some(format!("_:b{id}"))
        }
        _ => None,
    }
}
```

Export from `turtle/src/lib.rs`:
```rust
pub use serialize::serialize_nquads;
```

### GSP integration

In `sparql_endpoint/src/graph_store.rs`, `gsp_get` and `dataset_data_get` already switch on
`RdfFormat`. Add:

```rust
Some(RdfFormat::NQuads) => (
    StatusCode::OK,
    [("content-type", "application/n-quads")],
    serialize_nquads(&*store),
).into_response()
```

Update `negotiate_rdf_format` (or extend `graph_store::detect_rdf_format`) so that an `Accept:
application/n-quads` header selects this branch. The existing `RdfFormat::NQuads` enum variant
already exists; it just needs to be connected on the output path.

---

## TriG (`serialize_trig`)

### Spec reference

W3C RDF 1.1 TriG — <https://www.w3.org/TR/trig/>

### Format

Named graphs are wrapped in `GRAPH <iri> { ... }` blocks. Default-graph triples are emitted
as bare N-Triples lines (no `GRAPH` keyword), consistent with the TriG grammar rule that the
default graph can appear outside any graph block.

```trig
<s1> <p1> <o1> .

GRAPH <http://example.org/g1> {
    <s2> <p2> <o2> .
    <s3> <p3> "literal" .
}

GRAPH <http://example.org/g2> {
    <s4> <p4> <o4> .
}
```

### Implementation

File: `turtle/src/serialize.rs`

```rust
pub fn serialize_trig(store: &Datastore) -> String {
    let mut out = String::new();

    // 1. Default graph: bare triples
    for quad in store.named_graphs.get_graph(DEFAULT_GRAPH_ELEMENT_ID) {
        let s = subject_term(store.resources.get_graph_element(quad.subject));
        let p = predicate_term(store.resources.get_graph_element(quad.predicate));
        let o = object_term(store.resources.get_graph_element(quad.obj));
        if let (Some(s), Some(p), Some(o)) = (s, p, o) {
            out.push_str(&format!("{s} {p} {o} .\n"));
        }
    }

    // 2. Named graphs
    for (&graph_id, _) in &store.named_graphs.triple_id_index {
        if graph_id == DEFAULT_GRAPH_ELEMENT_ID { continue; }
        let g = graph_term(store.resources.get_graph_element(graph_id));
        let Some(g) = g else { continue };
        out.push_str(&format!("\nGRAPH {g} {{\n"));
        for quad in store.named_graphs.get_graph(graph_id) {
            let s = subject_term(store.resources.get_graph_element(quad.subject));
            let p = predicate_term(store.resources.get_graph_element(quad.predicate));
            let o = object_term(store.resources.get_graph_element(quad.obj));
            if let (Some(s), Some(p), Some(o)) = (s, p, o) {
                out.push_str(&format!("    {s} {p} {o} .\n"));
            }
        }
        out.push_str("}\n");
    }
    out
}
```

Export from `turtle/src/lib.rs`:
```rust
pub use serialize::serialize_trig;
```

### GSP integration

Same pattern as N-Quads. When `GET /{name}/data` (no `?graph=` parameter) is requested with
`Accept: application/trig`, call `serialize_trig`. For `Accept: text/turtle` with a single
named graph, the existing `serialize_graph` path still applies.

Add a `negotiate_rdf_format(accept: Option<&str>) -> RdfFormat` helper (analogous to the
existing `negotiate_select_format`) that prefers:

1. `application/trig` → `RdfFormat::TriG`
2. `application/n-quads` → `RdfFormat::NQuads`
3. `text/turtle` / `*/*` / none → `RdfFormat::Turtle` (current default)

---

## Testing strategy

Tests live in `turtle/src/serialize.rs` (`#[cfg(test)]`). Each test:

1. Builds or parses a `Datastore`
2. Calls `serialize_nquads` or `serialize_trig`
3. Parses the output back with the corresponding parser
4. Asserts `quad_count` and named-graph presence are preserved

See the test cases already added in `turtle/src/serialize.rs` for the W3C spec examples.

---

## Ordering

| Step | Change | Crate |
|---|---|---|
| 1 | Add `graph_term`, `serialize_nquads`, `serialize_trig` | `turtle` |
| 2 | Re-export from `lib.rs` | `turtle` |
| 3 | Wire N-Quads output into GSP GET (default/all-graphs path) | `sparql_endpoint` |
| 4 | Wire TriG output into GSP GET (all-graphs path) | `sparql_endpoint` |
| 5 | Add `negotiate_rdf_format` to select output format from `Accept` | `sparql_endpoint` |

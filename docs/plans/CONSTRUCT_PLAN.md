# SPARQL CONSTRUCT Plan

## Status: IMPLEMENTED ✓

All steps completed. All tests pass. See `sparql_parser/tests/parser_tests.rs` for the test suite covering W3C §10.2.1 and §10.2.4 examples.

### Implementation notes (additions to original plan)

- **Blank nodes in templates must be fresh per solution** (W3C §10.2.1): each solution gets a per-solution `HashMap<orig_bnode_id, fresh_id>` driven by a global counter. Same label within one solution → same fresh ID; different solutions → different IDs.
- **Ill-formed triple rejection**: both literal-in-subject and non-IRI-in-predicate are silently skipped (spec §10.2.1). The plan only listed the predicate check; subject check was added.
- **`_:` prefix bug fixed**: `parse_prefixed_name` was silently matching `_:label` as a prefixed IRI `IRI("_:label")` before `parse_blank_node` could try. Fixed by adding `"_"` to the reserved-prefix rejection list.
- **Short form (`CONSTRUCT WHERE { ... }`)**: AST uses `template: vec![]`; executor fills it with `collect_bgps_from_components(where_clause)` at runtime.
- **Endpoint serialization**: `sparql_endpoint/src/serialize/construct.rs` serializes `Vec<ResolvedTriple>` as N-Triples (a valid subset of Turtle), returned as `text/turtle; charset=utf-8`.

---

## Goal

Add SPARQL CONSTRUCT query support to `sparql_parser` and `sparql_endpoint`.
This is needed for `IRecordBackend.ConstructQuery()` and `IRecordBackend.Sparql(string)`,
which issue CONSTRUCT queries to retrieve sub-graphs of a record dataset.

## Spec reference

SPARQL 1.1 Query Language §10 — <https://www.w3.org/TR/sparql11-query/#construct>

The two syntactic forms:

```sparql
# Full form
CONSTRUCT { <template_triple_patterns> }
WHERE     { <graph_pattern> }

# Short form (template == WHERE pattern)
CONSTRUCT WHERE { <graph_pattern> }
```

---

## Changes required

### 1. AST — `sparql_parser/src/ast.rs`

Add a `Construct` variant to `Query`:

```rust
pub enum Query {
    Select { ... },
    Ask { where_clause: Vec<QueryComponent> },
    Construct {
        template: Vec<TriplePattern>,  // may be empty for CONSTRUCT WHERE
        where_clause: Vec<QueryComponent>,
    },
}
```

`TriplePattern` already exists and is the right type for template triples (subject, predicate,
object are all `Term`, which can be `Variable` or `Constant`).

### 2. Parser — `sparql_parser/src/lib.rs`

`parse_query` currently recognises `ASK` and then `SELECT`. Remove `"construct"` from the
reserved-word rejection list and add a CONSTRUCT branch before the SELECT branch.

```
CONSTRUCT [{ template }] WHERE? { graph_pattern }
```

Steps:
1. `tag_no_case("CONSTRUCT")` 
2. Optional `{ template_triples }` block — parse using the existing triple-pattern parser.
   If the block is absent, this is the short form: the template is built from the WHERE
   pattern after evaluation (simpler: parse `template = vec![]` and handle in executor).
3. Optional `tag_no_case("WHERE")`
4. `{ graph_pattern }` — the WHERE clause, using `parse_group_graph_pattern`.

Template triples use the same grammar as BGP triples but must not contain variables in the
predicate position (SPARQL spec §10.1). This restriction is best enforced in the executor
rather than the parser to keep error messages readable.

### 3. Executor — `sparql_parser/src/execute.rs`

Add a `QueryResult::Construct(Vec<ResolvedTriple>)` variant:

```rust
pub enum QueryResult {
    Select(SelectResult),
    Ask(bool),
    Construct(Vec<ResolvedTriple>),
}

pub struct ResolvedTriple {
    pub subject:   GraphElement,
    pub predicate: GraphElement,
    pub object:    GraphElement,
}
```

Execution algorithm for `Query::Construct { template, where_clause }`:

```
solutions = evaluate_pattern(where_clause, datastore)
output_triples = Set::new()  // deduplicated

for each solution in solutions:
    for each pattern_triple in template:
        subject   = bind_term(pattern_triple.subject,   solution)  → Option<GraphElement>
        predicate = bind_term(pattern_triple.predicate, solution)  → Option<GraphElement>
        object    = bind_term(pattern_triple.object,    solution)  → Option<GraphElement>
        if all three are Some and predicate is an IRI:
            output_triples.insert(ResolvedTriple { subject, predicate, object })

return QueryResult::Construct(output_triples.into_iter().collect())
```

`bind_term` resolves a `Term` to a `GraphElement`:
- `Term::Constant(elem)` → `Some(elem)`
- `Term::Variable(name)` → `solution.get(name)` (the current `SolutionRow` already maps
  variable names to `GraphElement`)

Short form (`template.is_empty()`): instead of iterating `template`, collect all bound
variables from each solution as `(s, p, o)` triples.  Simpler: require the WHERE clause to
be a BGP and promote all triple patterns from it as the template. This matches the spec
(§10.1.3: "The template of a CONSTRUCT WHERE query is the same as the WHERE clause").
Concretely: parse the WHERE clause's BGPs into the template during the ASK-to-CONSTRUCT
lowering in the parser.

### 4. Endpoint — `sparql_endpoint/src/query.rs`

`run_select_query` handles `QueryResult::Select` and `QueryResult::Ask`. Add a
`QueryResult::Construct` arm:

```rust
QueryResult::Construct(triples) => {
    let accept = headers.get("accept").and_then(|v| v.to_str().ok());
    let body = serialize_construct_result(triples, accept);
    let ct   = negotiate_construct_format(accept);
    (StatusCode::OK, [("content-type", ct)], body).into_response()
}
```

`serialize_construct_result` serializes `Vec<ResolvedTriple>` as N-Triples (default) or
TriG depending on content negotiation. N-Triples is the simplest path: one line per triple
using `<IRI>` and `"literal"` notation. Reuse the helper functions already in
`turtle::serialize`.

Content types:
- `text/turtle` → N-Triples (valid Turtle)
- `application/n-triples` → N-Triples
- `application/trig` → TriG (single default graph)
- `application/ld+json` → JSON-LD (future)
- `*/*` / none → `text/turtle`

---

## Interaction with `IRecordBackend`

`FusekiRecordBackend.ConstructQuery` sends:

```
POST /{dataset}/sparql
Content-Type: application/sparql-query
Accept: text/turtle

CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <iri> { ?s ?p ?o } }
```

This exact pattern must work after this feature is implemented.

---

## Testing strategy

Unit tests in `sparql_parser/src/`:
- Parse a CONSTRUCT query string → check `Query::Construct { template, where_clause }` variant
- Parse the short form `CONSTRUCT WHERE { ... }`
- Execute CONSTRUCT against a populated `Datastore` → check output triples
- Execute CONSTRUCT with no matching solutions → empty output (no error)
- Variable not bound in template → skip that triple (not an error)

Integration tests in `sparql_endpoint/tests/`:
- `POST /sparql` with CONSTRUCT body → `200 text/turtle` response
- Parse the Turtle response and verify triples present
- CONSTRUCT with `GRAPH` clause inside WHERE

---

## Ordering

| Step | Change | Crate |
|---|---|---|
| 1 | Add `Construct` to `Query` AST, add `ResolvedTriple`, `QueryResult::Construct` | `sparql_parser` |
| 2 | Parse CONSTRUCT (full form) | `sparql_parser` |
| 3 | Parse CONSTRUCT short form | `sparql_parser` |
| 4 | Execute CONSTRUCT, collect deduped triples | `sparql_parser` |
| 5 | Serialize CONSTRUCT result as N-Triples | `sparql_endpoint` |
| 6 | Content negotiate CONSTRUCT response format | `sparql_endpoint` |
| 7 | Tests | both crates |

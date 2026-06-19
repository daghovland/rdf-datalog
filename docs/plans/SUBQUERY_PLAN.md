# SPARQL Subquery Implementation Plan

**Status**: Complete. All 12 targeted W3C eval tests pass; sq12/sq14 skipped (CONSTRUCT+TTL result).  
**Last updated**: 2026-06-19

---

## What is a subquery?

A SPARQL subquery is a SELECT query embedded inside a group graph pattern:

```sparql
SELECT ?x WHERE {
  { SELECT ?y WHERE { ?x :p ?y } LIMIT 10 }
  ?x :q ?z .
}
```

Key semantics:
- The inner SELECT is evaluated first, producing a result set.
- Only variables listed in the inner SELECT's **projection** are visible to the outer query.
  Variables bound inside the inner query but not projected (e.g. `?y` in `SELECT ?x`) are hidden.
- LIMIT / OFFSET / DISTINCT inside the subquery apply to the inner result.
- ORDER BY inside a subquery matters only when combined with LIMIT (it determines which rows survive).

---

## Scope of W3C conformance tests

Test data: `tests/testdata/w3c_sparql11/subquery/`  
Eval tests: sq01–sq14.  
12 tests use `.srx` expected output (SELECT results), comparison is order-insensitive multiset.  
sq12, sq14 are CONSTRUCT queries with `.ttl` expected output → skip (CONSTRUCT+subquery result comparison not yet implemented).

| Test | Key feature |
|---|---|
| sq01–sq07 | Basic subquery + GRAPH clause scoping |
| sq08 | Aggregate in subquery (`MAX(?y) AS ?max`) |
| sq09 | Nested subquery (subquery inside subquery) |
| sq10 | Subquery + FILTER EXISTS |
| sq11 | Subquery with ORDER BY + LIMIT (ORDER BY determines which rows LIMIT keeps) |
| sq12 | CONSTRUCT with SELECT subquery + CONCAT — skip (TTL result) |
| sq13 | Shared variable `?L` between inner and outer |
| sq14 | CONSTRUCT with SELECT subquery + LIMIT — skip (TTL result) |

---

## Changes required

### 1. AST (`sparql_parser/src/ast.rs`)

Add `Subquery` variant to `QueryComponent`:

```rust
QueryComponent::Subquery(Box<Query>)
```

The inner `Query` is always a `Query::Select`. Wrapping it in `Box` avoids recursive sizing.

### 2. Parser (`sparql_parser/src/lib.rs`)

**Problem**: `parse_query` takes `&mut ParserContext` (for PREFIX declarations), but group-graph-pattern parsers take `&ParserContext` (immutable). Subqueries inside `{ SELECT ... }` inherit the outer prefix context but do not declare new prefixes.

**Solution**: Refactor `parse_query` into:
- `parse_query` (public, `&mut ctx`) — handles PREFIX declarations, delegates to body
- `parse_select_body` (private, `&ctx`) — parses SELECT/ASK/CONSTRUCT without PREFIX

In `parse_group_graph_pattern_contents`, when `{` is encountered, peek for `SELECT`:
```rust
let inner = &remaining[1..].trim_start_matches(char::is_whitespace);
let is_subquery = inner.to_ascii_uppercase().starts_with("SELECT")
    && inner.chars().nth(6).map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(true);
if is_subquery {
    let (r, _) = char('{')(remaining)?;
    let (r, _) = multispace0(r)?;
    let (r, inner_query) = parse_select_body(ctx)(r)?;
    let (r, _) = multispace0(r)?;
    let (r, _) = char('}')(r)?;
    components.push(QueryComponent::Subquery(Box::new(inner_query)));
    remaining = r;
    continue;
}
// ... existing UNION / sub-group logic
```

### 3. Executor (`sparql_parser/src/execute.rs`)

Add case in `eval_component`:

```rust
QueryComponent::Subquery(inner_query) => {
    let inner_result = execute_select(inner_query, datastore, active_graph);
    // Join inner rows with outer solutions
    solutions.into_iter().flat_map(|outer_sub| {
        inner_result.iter().filter_map(|inner_row| {
            merge_compatible(outer_sub, inner_row)
        })
    }).collect()
}
```

Where:
- `execute_select` runs the inner query, applying DISTINCT/LIMIT/OFFSET/ORDER BY
- `merge_compatible` merges two solution rows if they agree on shared variables, or returns `None` on conflict

**Variable scoping**: Inner rows only contain projected variables (enforced by the `project` step in the inner execution).

**ORDER BY**: Implement `sort_solutions(solutions, order_by, datastore)` that sorts by each `OrderCondition` expression:
- Compare `GraphElement` values: numbers numerically, strings/IRIs lexicographically
- Need to extend `compare_graph_elements` to handle IRI comparison

Also update:
- `collect_vars_from_components`: add `Subquery(q)` case — collect projected variables from inner query's projection
- `collect_bgps_from_components`: add `Subquery` case (no BGPs visible from inside subquery to CONSTRUCT template)

### 4. ORDER BY implementation

ORDER BY is parsed but currently ignored (`..` in the Select arm). Implement it:

```rust
if !order_by.is_empty() {
    solutions.sort_by(|a, b| compare_by_order_conditions(a, b, order_by, datastore));
}
```

`compare_graph_elements` currently returns `None` for IRIs. Extend it to handle:
- `NodeOrEdge(Iri(iri))`: compare IRI strings lexicographically
- `NodeOrEdge(AnonymousBlankNode(id))`: compare ids numerically

ORDER BY affects the outer SELECT too (not just subqueries), so enabling it globally is correct.

---

## Test skip list

In `w3c_sparql11_subquery`, skip:
- `"sq12 - Subquery within CONSTRUCT"` — CONSTRUCT with TTL result, no SRX comparison
- `"sq14 - Subquery with CONSTRUCT"` — same

Syntax test skip list: remove `syntax-subquery-01.rq`, `02.rq`, `03.rq` after implementation.

---

## Implementation order (TDD)

1. Plan (this document) ✓
2. Un-ignore syntax tests, remove from skip list → red
3. Un-ignore eval test with skip list for sq12/sq14 → red
4. Implement AST + parser → syntax tests go green
5. Implement executor (Subquery eval component, ORDER BY, IRI comparison) → eval tests go green
6. One test at a time for sq01 through sq14 (skipping sq12/sq14)

---

## Quality gates

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --release
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

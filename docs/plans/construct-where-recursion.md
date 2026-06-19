# CONSTRUCT WHERE Recursion Bug

## Status: FIXED ✓

## Problem

`CONSTRUCT WHERE { … }` currently only collects triples from the inner
`QueryComponent::BGP` (basic graph pattern) nodes in the WHERE clause.  It
misses triples that arrive through other `QueryComponent` variants:

- `QueryComponent::Optional` (OPTIONAL { … })
- `QueryComponent::Union` (… UNION …)
- `QueryComponent::Graph` (GRAPH <g> { … })
- `QueryComponent::Subquery`

The relevant code is in `sparql_parser/src/execute.rs`,
function `collect_bgps_from_components`:

```rust
fn collect_bgps_from_components(components: &[QueryComponent]) -> Vec<TriplePattern> {
    let mut patterns = Vec::new();
    for comp in components {
        if let QueryComponent::BGP(triples) = comp {
            patterns.extend(triples.iter().cloned());
        }
        // OPTIONAL, UNION, GRAPH, Subquery are silently ignored
    }
    patterns
}
```

This means:

```sparql
CONSTRUCT WHERE { OPTIONAL { ?s <p> ?o } }
```

…produces an empty template (and thus an empty result), even though the short
form should use the entire WHERE pattern as the template.

The full form (`CONSTRUCT { … } WHERE { OPTIONAL { … } }`) is unaffected
because it uses the explicit template.

## Root cause

`collect_bgps_from_components` was written for the initial BGP-only case
and was never extended to recurse into composite patterns.

## Proposed fix

Recurse into nested patterns and collect all triple patterns:

```rust
fn collect_bgps_from_components(components: &[QueryComponent]) -> Vec<TriplePattern> {
    let mut patterns = Vec::new();
    for comp in components {
        match comp {
            QueryComponent::BGP(triples) => patterns.extend(triples.iter().cloned()),
            QueryComponent::Optional(inner) => {
                patterns.extend(collect_bgps_from_components(inner));
            }
            QueryComponent::Union(branches) => {
                for branch in branches {
                    patterns.extend(collect_bgps_from_components(branch));
                }
            }
            QueryComponent::Graph { components: inner, .. } => {
                patterns.extend(collect_bgps_from_components(inner));
            }
            _ => {}
        }
    }
    patterns
}
```

## Implementation checklist

- [x] Create this plan document
- [x] Create ignored integration tests (see `tests/sparql12_suite.rs` — search for
      `construct_where_with_optional`)
- [x] Implement the recursive fix in `sparql_parser/src/execute.rs`
- [x] Unignore the tests and verify they pass
- [x] Add CONSTRUCT WHERE tests for UNION and GRAPH patterns

# Plan: support `GRAPH <iri> { ... }` inside SPARQL UPDATE `INSERT DATA`/`DELETE DATA`

Issue: [#404](https://github.com/daghovland/rdf-datalog/issues/404)

## Problem

`INSERT DATA { GRAPH <iri> { ... } }` (and the equivalent `DELETE DATA`
form) is rejected with a parse error
(`GRAPH is not a valid subject or graph name`), even though SPARQL 1.1's
`QuadData` grammar for `INSERT DATA`/`DELETE DATA` explicitly allows
`GRAPH` blocks — this is standard, spec-mandated SPARQL Update syntax for
targeting a named graph, not an edge case.

## Root cause

`sparql_endpoint::sparql_update::parse_turtle_content` (the function that
parses the `{ ... }` body of `INSERT DATA`/`DELETE DATA`) calls
`turtle::parse_turtle`, which only understands plain Turtle (no `GRAPH`
blocks — that's TriG syntax). The `turtle` crate already has a fully
TriG-capable parser, `turtle::parse_trig` (same
`Result<(), TurtleParseError>` signature, already tested for default-graph,
named-graph, and mixed-graph TriG documents in `turtle/src/lib.rs`), it's
just not being used here.

## Fix

Minimal, drop-in: change `parse_turtle_content` to call `turtle::parse_trig`
instead of `turtle::parse_turtle`. TriG is a syntactic superset of Turtle
(a document with no `GRAPH` blocks parses identically under either), so this
requires no change to the `@prefix`-synthesis logic already in
`parse_turtle_content` (added by #392/PR #399), and should not affect any
existing plain-Turtle-content `INSERT DATA`/`DELETE DATA` test.

## Tests (TDD)

- Unit/integration test (in `sparql_endpoint/tests/sparql_update_prefix.rs`,
  alongside the existing #392 tests, or a new
  `sparql_endpoint/tests/sparql_update_named_graph.rs` — check which fits
  better) reproducing the issue's exact example: `INSERT DATA { GRAPH <iri>
  { <s> <p> <o> . } }` via the real HTTP update endpoint, then querying (via
  `GRAPH <iri> { ?s ?p ?o }` or the Graph Store Protocol) to confirm the
  triple landed in the named graph, not the default graph.
- Regression: existing plain (no `GRAPH`) `INSERT DATA`/`DELETE DATA` tests
  must still pass unchanged.
- A `DELETE DATA { GRAPH <iri> { ... } }` test too, since the issue's
  underlying use case (and the shared `parse_turtle_content` function) covers
  both ops identically.

## Out of scope

The issue also mentions "a related attempt to wrap N-Quads directly in
`INSERT DATA { ... }` also fails" but says "the main compatibility gap is
named graph `INSERT DATA` support" — this plan only targets the `GRAPH { }`
block form (TriG), not raw N-Quads syntax inside `INSERT DATA`, which SPARQL
1.1 doesn't require anyway (`QuadData` is TriG-shaped, not N-Quads-shaped).

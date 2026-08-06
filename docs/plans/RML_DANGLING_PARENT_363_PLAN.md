# Plan: fix `rml::translate::translate_triples_map` panic on dangling `rml:parentTriplesMap`

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (fourth cluster; epic [#218](https://github.com/daghovland/rdf-datalog/issues/218))

## Problem

`rml/src/translate.rs`'s `translate_triples_map` panics with
`panic!("unknown rml:parentTriplesMap {parent_id:?}")` when an RML mapping
document's `rml:parentTriplesMap` references a `TriplesMap` id that doesn't
exist in the document (`loader.rs` doesn't pre-validate referenced parent
ids). This crashes the whole `--serve` process on a malformed-but-parseable
RML mapping file.

## Fix

Trivial, minimal-diff — `translate_triples_map` already returns
`Result<(), RmlError>` and already uses `?` throughout for every other
fallible step in the same function. `RmlError` already has a generic
`MappingParse(String)` variant used elsewhere in the crate for this kind of
structural mapping-document problem. Change:

```rust
let parent_tm = parent_by_id.get(parent_id).unwrap_or_else(|| {
    panic!("unknown rml:parentTriplesMap {parent_id:?}")
});
```

to something like:

```rust
let parent_tm = parent_by_id.get(parent_id).ok_or_else(|| {
    RmlError::MappingParse(format!("unknown rml:parentTriplesMap {parent_id:?}"))
})?;
```

(check `RmlError`'s `Display`/`Debug` impl and existing `MappingParse` usage
elsewhere in the crate for the expected message convention before finalizing
wording).

## Tests (TDD)

- Unit test in `rml/src/translate.rs` (or wherever `translate_triples_map`
  already has test coverage — check first) constructing a `TriplesMap` whose
  `object_maps` has a `parent_triples_map` pointing at an `IriReference` not
  present in `parent_by_id`, and asserting `translate_triples_map` (or
  whatever the public entry point is — check callers) returns
  `Err(RmlError::MappingParse(_))` instead of panicking. Confirm it panics
  before the fix (red first).
- Regression: an existing valid parent-join mapping still translates
  successfully (should already be covered by existing tests — just confirm
  nothing breaks).
- If there's an integration-level RML test file (`tests/` at the workspace
  root, or `rml/tests/`) that loads a full `.rml.ttl` mapping document, add
  one exercising a dangling parent reference through the real load path
  (whatever function `loader.rs`/the crate's public API calls to go from a
  parsed mapping document to `translate_triples_map`), confirming a clean
  `Err` reaches the top-level caller.

## Out of scope

Remaining #363 clusters (`datalog.rs` unsafe-rule panics — deliberately
deferred until the in-flight stratifier-cycle-panic PR, which also edits
`datalog/src/reasoner.rs`, is merged, to avoid a same-file conflict;
`rdf_owl_translator`/`axiom_parser.rs` cyclic-list/malformed-arity sites)
stay as further follow-up PRs; #363 remains open. This PR only touches the
`rml` crate.

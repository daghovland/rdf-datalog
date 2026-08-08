# Plan: fix remaining rdf_owl_translator panics (class-expression cycle, multiple owl:members)

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (seventh cluster)

## Problem

Three panics remain in `rdf_owl_translator`, all now easy to fix because
PR #418 already threaded `Result<_, TranslatorError>` through their
enclosing call chains:

1. `topological_sort` (`rdf_owl_translator/src/ingress.rs`) panics with
   `"Cycle detected in OWL class expression dependency graph"` when a
   cyclic dependency exists among anonymous class expressions (e.g. two
   blank-node class expressions whose builders reference each other). Its
   one caller, `parse_anonymous_exprs` (`class_expression_parser.rs`),
   already returns `Result<(), TranslatorError>`.
2. `axiom_parser.rs`'s two "multiple owl:members" panics
   (`owl:AllDisjointClasses`/`owl:AllDisjointProperties` with more than one
   `owl:members` triple) sit inside a match arm that already uses `?` on a
   `get_rdf_list_elements(...)` call — i.e. the enclosing function already
   returns `Result<_, TranslatorError>`.

## Fix

1. Change `topological_sort(nodes: &[GraphElementId], predecessors: &HashMap<...>) -> Vec<GraphElementId>`
   to `-> Result<Vec<GraphElementId>, TranslatorError>`. Add a new
   `TranslatorError` variant (check `rdf_owl_translator/src/error.rs`,
   added by #418, for the existing convention — likely something like
   `CyclicDependency(String)`) and return `Err(...)` instead of panicking
   when `result.len() != nodes.len()`.
2. Update `parse_anonymous_exprs`'s one call site
   (`class_expression_parser.rs:658`, `let sorted = topological_sort(&ids_vec, &pred_map);`)
   to `let sorted = topological_sort(&ids_vec, &pred_map)?;` — the function
   already returns `Result`, so this is a one-line change.
3. Replace both `axiom_parser.rs` panics
   (`panic!("Multiple owl:members on owl:AllDisjointClasses")` and the
   `...AllDisjointProperties` sibling) with
   `return Err(TranslatorError::...(...));` using whatever variant fits
   best (a generic "malformed axiom" variant, or a new specific one —
   match the existing `TranslatorError` enum's granularity, check
   `error.rs` first rather than guessing).

## Tests (TDD)

- Unit test for `topological_sort` returning `Err` on a genuine cycle
  (construct two nodes each listing the other as a predecessor) instead of
  panicking — confirm red first. Regression: an acyclic input still
  returns `Ok` with a valid topological order.
- Integration test loading a small Turtle ontology with two blank-node
  class expressions (e.g. two `owl:intersectionOf` restrictions) that
  reference each other cyclically, through `rdf2owl`, asserting a clean
  `Err` instead of a crash.
- Unit/integration tests for both `owl:AllDisjointClasses` and
  `owl:AllDisjointProperties` with two `owl:members` triples on the same
  subject, asserting `Err` instead of a panic. Regression: the
  single-`owl:members` case still works.

## Out of scope

`try_get_individual`/`try_get_literal` (`rdf_owl_translator/src/ingress.rs`)
still panic and have ~9 call sites across `class_expression_parser.rs` and
`axiom_parser.rs`, some inside closures that don't currently return
`Result` (similar to the `try_get_bool_literal`/`owl:hasSelf` situation
fixed in PR #400) — a larger, separate follow-up. #363 stays open after
this PR for that remaining piece.

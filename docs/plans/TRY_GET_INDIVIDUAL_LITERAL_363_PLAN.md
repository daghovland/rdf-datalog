# Plan: stop `try_get_individual`/`try_get_literal` panicking on malformed OWL

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (eighth cluster — the remaining follow-up explicitly scoped out of PR #424)

## Problem

`rdf_owl_translator/src/ingress.rs`:

- `try_get_individual(gel: &GraphElement) -> Individual` panics if `gel` is a
  literal or a triple term — i.e. any malformed ontology that uses a literal
  where an individual IRI/blank node is expected (`owl:NamedIndividual`
  subject, `owl:sameAs`/`owl:differentFrom` subject/object,
  `owl:AllDifferent`'s `owl:members` list, `owl:oneOf`'s member list,
  `owl:hasValue`'s object when the property turns out to be an object
  property) crashes the whole `--serve` process instead of returning an
  error.
- `try_get_literal(gel: &GraphElement) -> &RdfLiteral` panics if `gel` is not
  a literal. **Dead code**: `grep -rn try_get_literal` across the whole
  workspace shows zero call sites (only the function definition itself and
  the `error.rs` doc-comment mentioning it). Since it's unused, delete it
  outright rather than convert it — there's nothing to convert for.

## Call sites of `try_get_individual` (7 total)

Two structurally different situations:

1. **`axiom_parser.rs`** (5 sites: lines ~141, ~256, ~451/452, ~470/471,
   ~477) — all inside `extract_axiom` or sibling functions that already
   return `Result<_, TranslatorError>` (confirmed: same pattern as the
   `owl:members`/`rdf:List` fixes already landed under #363). Convert
   `try_get_individual` to return `Result<Individual, TranslatorError>` and
   thread `?` through these 5 call sites directly — the easy majority.
2. **`class_expression_parser.rs`** (2 sites: ~531, ~851) — both **inside
   `ClassExprBuilder` closures** (`Box<dyn Fn(&OntologyDeclarations, &X) ->
   ClassExpression>`), which do NOT return `Result`. This is the same shape
   of problem PR #400 solved for `try_get_bool_literal`/`owl:hasSelf`:
   changing the closure's return type to thread `Result` through would be a
   much larger refactor (the closure type is used pervasively across
   `class_expression_parser.rs` for every anonymous class expression kind,
   not just these two). Follow #400's precedent instead: keep these two call
   sites non-fatal — on a malformed input (literal used as an individual),
   `log::warn!` and fall back to a defensible default rather than crash:
   - `owl:oneOf` (line ~531): the list-comprehension `.map(|&id|
     try_get_individual(...))` — skip the malformed element (log + filter it
     out of the resulting `Vec<Individual>`) rather than aborting the whole
     `ObjectOneOf` construction. An `ObjectOneOf` missing one malformed member
     is more useful than none at all, and mirrors "skip and warn" used
     elsewhere in this crate (e.g. `owl2rl2datalog::abox`'s
     non-atomic-class-expression skip).
   - `owl:hasValue` (line ~851): if `z_gel` is a literal but the property
     turns out to be an object property (the `|ope| ...` arm), `log::warn!`
     and fall back to `ClassExpression::ObjectHasValue` being skipped
     entirely for this restriction — return `ClassExpression::OwlThing` (the
     same "fall back to `owl:Thing`" convention `owl:hasSelf`'s malformed-bool
     case already established in #400) rather than inventing a new fallback
     shape.

## Fix

1. Delete `try_get_literal` entirely (dead code, zero call sites).
2. Change `try_get_individual`'s signature to
   `Result<Individual, TranslatorError>`. Reuse `TranslatorError`'s existing
   granularity — check `error.rs` for whether a generic "malformed axiom"
   variant already fits, or add a new `LiteralUsedAsIndividual(String)`
   variant matching the established per-panic-site convention
   (`CyclicDependency`, `MultipleOwlMembers`).
3. Update the 5 `axiom_parser.rs` call sites to use `?`.
4. Update the 2 `class_expression_parser.rs` call sites to the
   log::warn!-and-fallback pattern described above (NOT `?`, since the
   enclosing closures aren't `Result`-returning).
5. Update `error.rs`'s module doc comment: remove `try_get_individual` from
   the "not yet represented" follow-up list (it's now handled); keep
   `try_get_literal` off the list entirely since it's deleted, not deferred.

## Tests (TDD)

- Unit tests for `try_get_individual`: `Ok` on an IRI resource and on an
  anonymous blank node (regression); `Err` on a literal; `Err` on a triple
  term (matches the existing RDF 1.2 `#143` panic message content, just as
  an `Err` now).
- Integration tests (via `rdf2owl`, small inline Turtle fixtures):
  - `owl:NamedIndividual` declaration whose subject is (malformed) a literal
    → clean `Err`, not a panic.
  - `owl:AllDifferent`/`owl:sameAs` axiom with a literal in individual
    position → clean `Err`.
  - `owl:oneOf` list containing one literal among otherwise-valid IRI
    members → translation succeeds, `ObjectOneOf` contains only the valid
    members, a `log::warn!` fires for the skipped one (assert on the
    resulting axiom's individual count, not the log output, unless this
    crate already has a log-capturing test utility — check first).
  - `owl:hasValue` restriction on an object property whose `owl:hasValue`
    object is (malformed) a literal → translation succeeds, restriction
    falls back to `owl:Thing`, `log::warn!` fires (same assertion caveat as
    above).
  - Regression: an ordinary, well-formed `owl:oneOf`/`owl:hasValue` still
    produces the expected `ObjectOneOf`/`ObjectHasValue` exactly as before.

## Out of scope

Nothing further remains panicking in `rdf_owl_translator` after this PR, as
far as this cluster of #363 investigation found — but do a final
`grep -rn 'panic!\|unwrap()\|expect(' rdf_owl_translator/src/` sweep before
closing #363, since earlier clusters (#400, #418, #424) each found scope by
grepping fresh rather than trusting a prior enumeration.

**Update after implementation:** the final sweep did find three genuine
remaining panic sites (`topological_sort`'s cycle-detection panic in
`ingress.rs`, and the two "Multiple owl:members on
owl:AllDisjointClasses/Properties" panics in `axiom_parser.rs`). These
overlapped with [PR #424](https://github.com/daghovland/rdf-datalog/pull/424),
which targeted exactly these sites — so they were left alone here rather
than duplicated. #424 has since merged; this branch was rebased onto that
merge. See the [issue](https://github.com/daghovland/rdf-datalog/issues/363)
for current status.

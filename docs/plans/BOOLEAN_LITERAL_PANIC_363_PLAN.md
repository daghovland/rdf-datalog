# Plan: fix `try_get_bool_literal` panic on legal XSD boolean lexical forms

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (first cluster of several; epic [#218](https://github.com/daghovland/rdf-datalog/issues/218))

## Problem

Two duplicate functions — `dag_rdf::ingress::try_get_bool_literal` and
`rdf_owl_translator::ingress::try_get_bool_literal` — both `panic!` when an
`xsd:boolean`-typed literal's lexical form is anything other than the exact
strings `"true"`/`"false"`. XSD's boolean lexical space also legally permits
`"1"`/`"0"`, so even a spec-valid literal crashes the process. The
`rdf_owl_translator` copy is reachable from `owl:hasSelf` restriction parsing
(`class_expression_parser.rs:809`) when loading any Turtle-encoded OWL
ontology via `--ontology` / Jupyter kernel cell — a DoS on legitimate input.

## Fix

No signature change, no new error type. Both functions already return
`Option<bool>`, where `None` means "not usable as a boolean". The one real
call site (`class_expression_parser.rs:809`) already treats `None` as a
non-fatal case: it logs `log::warn!` and falls back to `owl:Thing`. So:

- Accept `"1"` → `Some(true)`, `"0"` → `Some(false)` (both legal XSD lexical
  forms, previously rejected).
- Replace the `panic!` arm with `_ => None` — reuses the existing graceful
  fallback path instead of introducing new error-plumbing across two crates
  for what the call site already treats as recoverable.

`dag_rdf`'s copy has no current callers in the workspace but is `pub`
(re-exported via `dag_rdf::ingress::*`), so it's part of the public API
surface and gets the identical fix for consistency.

## Tests (TDD — written first, ignored, then unignored as fixed)

`dag_rdf/src/ingress.rs` unit tests and `rdf_owl_translator/src/ingress.rs`
unit tests (one set per crate, since the functions are independent copies):

- `try_get_bool_literal_accepts_true_false` (already passed, keep as
  regression)
- `try_get_bool_literal_accepts_xsd_lexical_1_and_0` (new — currently panics
  without the fix)
- `try_get_bool_literal_returns_none_for_invalid_lexical_form` (new —
  currently panics on e.g. `"yes"`; must return `None`, not panic)
- `try_get_bool_literal_returns_none_for_non_boolean_literal` (already
  passes, keep as regression)

Integration-level: a test in `rdf_owl_translator` (or wherever
`class_expression_parser` tests live) loading an ontology with
`owl:hasSelf "1"^^xsd:boolean` and confirming it resolves to a self-restriction
(not the `owl:Thing` fallback), proving the `"1"`/`"0"` acceptance reaches the
real caller.

## Out of scope

The other clusters in #363 (`stratifier.rs` cycle panic, `datalog.rs` unsafe
rule panics, `eli2rl.rs` unimplemented-construct panics, remaining
`rdf_owl_translator`/`axiom_parser.rs` sites, `rml/src/translate.rs` dangling
parent panic) are separate PRs — #363 stays open for those as follow-ups.

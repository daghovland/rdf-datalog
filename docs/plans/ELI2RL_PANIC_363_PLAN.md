# Plan: fix `eli2rl.rs` panics on legal-but-unimplemented OWL 2 RL constructs

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (third cluster; epic [#218](https://github.com/daghovland/rdf-datalog/issues/218))

## Problem

`eli/src/eli2rl.rs` has three `panic!` sites for OWL 2 constructs that are
legal syntax but not yet implemented by the ELI translation:

- `get_obj_prop_pattern` (~line 103): `ObjectPropertyExpression::ObjectPropertyChain` inside an existential (`owl:someValuesFrom`) → `panic!("Property chain in existential not yet supported")`
- `get_obj_value_pattern` (~line 124): `ObjectPropertyExpression::InverseObjectProperty` inside `owl:hasValue` → `panic!("Inverse ObjectHasValue not yet supported")`
- `get_obj_value_pattern` (~line 127): `ObjectPropertyExpression::ObjectPropertyChain` inside `owl:hasValue` → `panic!("Property chain in ObjectHasValue not yet supported")`

Any legitimate ontology using these (uncommon but spec-legal) constructs
crashes the whole `--serve` process at load time, via
`apply_ontologies`/`compile_ontology_rules` (`src/lib.rs`).

## Fix

`eli::owl2datalog` (`eli/src/lib.rs:23`) already returns
`Option<Vec<Rule>>` for the existing "axiom is not ELI-expressible" case
(`eli_axiom_extractor` returning `None`), and its one caller,
`owl2rl2datalog::owl2datalog` (`owl2rl2datalog/src/lib.rs:280`), already
handles `None` gracefully via `.unwrap_or_default()` — treats it as "this
axiom contributes zero rules", not an error. Treat "legal syntax we haven't
implemented yet" the same way: skip just that axiom, log a warning, contribute
zero rules. **No changes needed outside the `eli` crate** — the skip signal
already propagates correctly once `eli::owl2datalog` can return `None`.

Thread `Option` through the call chain instead of `panic!`:

1. `get_obj_prop_pattern` → `Option<dag_rdf::QuadPattern>` (the
   `ObjectPropertyChain` arm becomes `log::warn!(...); return None;`; every
   other arm wraps its existing return in `Some(...)`).
2. `get_obj_value_pattern` → `Option<dag_rdf::QuadPattern>` (same treatment
   for its two panic arms; wrap other arms in `Some`).
3. `translate_eli` → `Option<Vec<dag_rdf::QuadPattern>>` (propagate `None`
   from any recursive call or from `get_obj_prop_pattern` via `?`/`and_then`;
   the `flat_map` combining sub-results needs to become a fallible collect —
   e.g. `.map(...).collect::<Option<Vec<_>>>()?` flattened, or an explicit
   loop that returns `None` early).
4. The normalized-rule builders that call the above
   (`get_universal_normalized_rule`, `get_at_most_one_normalized_rule`,
   `get_at_most_zero_normalized_rule`, `get_object_has_value_normalized_rule`,
   and anything calling `translate_eli`/`translate_simple_subclass`/
   `translate_empty_intersection`) → each returns `Option<Rule>` instead of
   `Rule`.
5. `generate_axiom_rl` → `Option<Vec<Rule>>` (if any one of the formula's
   generated rules is `None`, the whole axiom is skipped — simplest, most
   conservative interpretation; do not try to partially-apply an axiom).
6. `generate_tbox_rl` (`pub fn`, currently `Vec<Rule>`) → `Option<Vec<Rule>>`
   (skip if any formula in the input fails to translate).
7. `eli/src/lib.rs`'s `owl2datalog`: change
   `eli_axiom_extractor(axiom).map(|formulas| generate_tbox_rl(resources, formulas))`
   (currently `Option<Vec<Rule>>` via the outer `.map`, but `generate_tbox_rl`
   itself will now also return `Option`, so this needs `.and_then(...)`
   instead of `.map(...)` to flatten the nested `Option<Option<Vec<Rule>>>`.

Read the actual current code before editing — some of the described
functions may combine results in ways (e.g. `flat_map`) that need care to
convert to a fallible form without silently losing the "skip whole axiom on
any single unsupported sub-construct" semantics.

## Tests (TDD)

Unit tests near the relevant functions in `eli/src/eli2rl.rs` (check for an
existing `#[cfg(test)]` module first) or in `eli`'s own test files:

- construct an `ObjectPropertyExpression::ObjectPropertyChain` inside an
  existential concept and confirm `translate_eli`/the relevant normalized-rule
  builder returns `None` instead of panicking (currently panics — red first).
- same for `InverseObjectProperty` and `ObjectPropertyChain` inside
  `owl:hasValue`.
- regression: an ordinary (non-chain, non-inverse-in-hasValue) axiom still
  produces `Some(rules)` with the same rules as before.

Integration-level: a test going through `eli::owl2datalog` (or
`owl2rl2datalog::owl2datalog`, the crate's real public entry point) with an
axiom using one of these unsupported constructs, asserting it returns
cleanly (`None` / empty rules, not a panic) — proves the skip signal reaches
the real caller a `--ontology` load goes through.

## Out of scope

Remaining #363 clusters (`datalog.rs` unsafe-rule panics,
`rdf_owl_translator`/`axiom_parser.rs` sites, `rml/src/translate.rs` dangling
parent panic) stay as further follow-up PRs; #363 remains open. PR #400
(boolean-literal cluster) and the stratifier-cycle-panic PR (in progress) are
separate, already-in-flight work — do not touch those files/crates
(`dag_rdf`, `rdf_owl_translator/src/ingress.rs`, `datalog/src/stratifier.rs`,
`datalog/src/reasoner.rs`, `datalog/src/incremental.rs`).

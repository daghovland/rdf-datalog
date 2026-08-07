# Plan: fix `get_rdf_list_elements` panics on malformed `rdf:List`

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (sixth cluster; epic [#218](https://github.com/daghovland/rdf-datalog/issues/218))

## Problem

`rdf_owl_translator::ingress::get_rdf_list_elements` panics on three kinds
of malformed `rdf:List` structural encoding:

- a cyclic list (`rdf:rest` pointing back to an earlier node) — `panic!("Cyclic RDF list")`
- a node with != 1 `rdf:first` triple — `panic!("Invalid RDF list: wrong number of rdf:first triples")`
- a node with != 1 `rdf:rest` triple — `panic!("Invalid RDF list: wrong number of rdf:rest triples")`

`rdf:List` structural encoding is used throughout OWL 2's RDF mapping
(`owl:intersectionOf`, `owl:unionOf`, `owl:members` on
`AllDisjointClasses`/`AllDisjointProperties`/`AllDifferent`, property
chains, etc.) — any malformed list anywhere in a loaded Turtle-encoded OWL
ontology (via `--ontology` or a Jupyter kernel cell) crashes the whole
`--serve` process.

## Scope decision

This cluster is bigger than the four already-merged #363 fixes (#400,
stratifier-cycle, RML dangling-parent, eli2rl) — `get_rdf_list_elements` has
~10 call sites across `ingress.rs`, `class_expression_parser.rs`, and
`axiom_parser.rs`, all feeding into the crate's single top-level entry point
`rdf2owl(datastore: &mut Datastore) -> OntologyDocument` (`translator.rs`),
which itself isn't `Result`-returning yet and has 3 callers in the main
crate (`apply_ontologies`, `compile_ontology_rules`, plus a test).

This PR scopes to `get_rdf_list_elements` alone — the single highest-value,
most-reachable function (any malformed `rdf:List` anywhere triggers it).
**Not in scope for this PR**: `try_get_individual`'s literal-as-individual
panic, `try_get_literal`'s non-literal panic (`ingress.rs`), or
`axiom_parser.rs`'s two "multiple owl:members" panics — these stay as a
further follow-up under #363 (#363 remains open after this PR either way).

## Fix

1. Add a small error type for the `rdf_owl_translator` crate if one doesn't
   already exist (check `rdf_owl_translator/src/*.rs` for an existing
   `TranslatorError`/similar first — don't invent a second one). If none
   exists, a minimal `pub enum TranslatorError { MalformedRdfList(String), ... }`
   (or similarly named) in a sensible location (e.g. `lib.rs` or
   `translator.rs`) is enough for this PR's scope — it can grow variants in
   future follow-ups for the other panic sites.
2. Change `get_rdf_list_elements` to return
   `Result<Vec<GraphElementId>, TranslatorError>` instead of
   `Vec<GraphElementId>`; replace its three `panic!`s with `Err(...)`.
3. Thread `Result` through its ~10 callers in `ingress.rs`,
   `class_expression_parser.rs`, and `axiom_parser.rs` — each caller either
   propagates with `?` (if it's already/becomes fallible) or the fallibility
   bubbles further up the call chain. Read the actual current call sites
   before assuming a mechanical `?` insertion is sufficient — some of these
   functions build `Vec<...>`/`Option<...>` structures via iterator
   chains (`.map()`/`.flat_map()`) that will need converting to a fallible
   form (`.collect::<Result<Vec<_>, _>>()`), similar to the `eli2rl.rs`
   `Option`-threading done for issue #363's eli2rl cluster (already merged
   as PR #407) — that PR is a good style reference for this kind of
   "thread fallibility through a small recursive translation layer" change,
   even though this one uses `Result` instead of `Option`.
4. Change `rdf2owl(datastore: &mut Datastore) -> OntologyDocument` to
   `-> Result<OntologyDocument, TranslatorError>`.
5. Update `rdf2owl`'s 3 callers in the main crate (`src/lib.rs`:
   `apply_ontologies`, `compile_ontology_rules`, and a test) — check whether
   `apply_ontologies`/`compile_ontology_rules` already return `Result` (they
   likely do, given they're user-facing ontology-loading entry points) and
   propagate with `?`; the test call site gets `.unwrap()` if it's feeding
   known-good fixture data.
6. Check for any other `rdf2owl` callers in the workspace (`dagalog-kernel`,
   integration tests) via a full-workspace grep and update those too.

## Tests (TDD)

- Unit tests near `get_rdf_list_elements` (check for an existing
  `#[cfg(test)]` module in `ingress.rs` first) covering: a cyclic list
  returns `Err(...)` instead of panicking; a node with 2 `rdf:first`
  triples returns `Err(...)`; a node with 0 `rdf:rest` triples returns
  `Err(...)`; a well-formed list still returns `Ok(vec![...])` with the
  correct elements in order (regression).
- Integration-level: a test loading a small Turtle-encoded OWL ontology
  with a malformed `owl:intersectionOf` list (e.g. missing `rdf:rest`)
  through `rdf2owl` (or whichever higher-level entry point a `--ontology`
  load actually goes through — check `apply_ontologies`), asserting a clean
  `Err` instead of a crash.

## Out of scope

`try_get_individual`, `try_get_literal` (`ingress.rs`), and the two
"multiple owl:members" panics in `axiom_parser.rs` — separate follow-up
PR(s) under #363, which remains open after this PR merges.

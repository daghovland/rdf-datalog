# Plan: surface `assert_abox`'s skip signal ([#366](https://github.com/daghovland/rdf-datalog/issues/366))

## Problem

`owl2rl2datalog::abox::assert_abox` silently drops non-atomic
`ClassAssertion`/`ObjectPropertyAssertion` (and other non-materialisable) ABox
axioms, reporting only via `log::warn!`. Its return value (a materialised-quad
count, `usize`) is discarded at every call site:

- `src/lib.rs::load_file` (`.omn` branch)
- `src/lib.rs::compile_ontology_rules`
- `dagalog-kernel/src/cell/manchester.rs::execute_manchester_file` (this one
  *does* use the count, but has no access to the skip signal at all)

PR #370 (issue #177) already solved half of this problem for the newer,
*additive* `owl2rl2datalog::owl_to_rdf::owl2rdf` path: it returns a
`#[must_use] RdfTranslationReport { triples_added, skipped }`. That path isn't
wired into any call site yet and is out of scope here. `assert_abox` itself —
the function actually named in #366 — is untouched by #370.

Out of scope: actually encoding complex class/property expressions as RDF
(blank-node `owl:Restriction` structures, `rdf:List`s, …). That's
[#373](https://github.com/daghovland/rdf-datalog/issues/373). This issue is
only about not silently losing the *signal* that something was skipped.

## Approach

`atomic_assertion_triple` (in `owl_to_rdf.rs`) is already shared between
`owl2rdf` and `assert_abox`, and `RdfTranslationReport` is the exact shape
`assert_abox` needs (a triple count plus a list of skip descriptions) — no new
type needed.

1. **`owl2rl2datalog/src/abox.rs`**: change `assert_abox`'s return type from
   `usize` to `owl_to_rdf::RdfTranslationReport` (already `#[must_use]`).
   Every skip branch pushes its message into `report.skipped` in addition to
   the existing `log::warn!` (kept for anyone tailing logs).

2. **`src/lib.rs`**:
   - `OntologyCompilation` gains `pub abox_skipped: Vec<String>`, accumulated
     across every `.omn` path processed by `compile_ontology_rules`.
   - `ReasoningStats` (returned by `apply_ontologies`) gains
     `pub abox_skipped: Vec<String>`, copied from the `OntologyCompilation`.
   - `load_file`'s `.omn` branch: since `load_file`'s signature
     (`Result<(), String>`) is depended on by ~50 call sites across the repo
     and changing it is out of proportion to this issue, it instead
     `eprintln!`s a one-line summary warning directly to stderr when
     `report.skipped` is non-empty. This is unconditional (not gated behind a
     verbose flag) because it flags actual data loss, not routine progress
     info — and it's directly observable by any caller capturing stderr,
     unlike a `log::warn!` that needs `RUST_LOG` configured.

3. **`src/main.rs`**: when `apply_ontologies` returns a non-empty
   `stats.abox_skipped`, print a warning (count + first few descriptions) to
   stderr, unconditionally (same reasoning as above).

4. **`dagalog-kernel/src/cell/manchester.rs`**: `execute_manchester_file` uses
   `report.triples_added` (renamed from the old `usize`) and appends a
   " (N ABox assertion(s) skipped, see issue #366)"-style clause to its
   returned status string when `report.skipped` is non-empty. This is the
   kernel's existing return-string mechanism for cell output, so no new
   plumbing is needed there.

5. **`docs/user/reasoning.md`**: short callout documenting the limitation,
   linking to #373 for the actual fix and #366 for this visibility change.

## Tests (red phase, initially `#[ignore]`)

New fixture: `tests/testdata/animals_complex_abox.omn` — like `animals.omn`
but with one assertion that isn't a single ground triple:
`Individual: fido  Types: Dog or Cat` (`ObjectUnionOf`, no single-triple
encoding).

- `owl2rl2datalog/src/abox.rs` (unit tests, extend existing module):
  - `assert_abox_reports_skipped_non_atomic_class_assertion`: a `ClassAssertion`
    over an `ObjectUnionOf` yields `report.skipped.len() == 1` and
    `report.triples_added == 0`.
  - Existing `anonymous_individual_does_not_collide_with_rdf_blank_node` test
    updated to read `report.triples_added` instead of a bare `usize`.

- `tests/cli_integration.rs`:
  - `apply_ontologies_reports_skipped_abox_assertions`: load
    `animals_complex_abox.omn` through `apply_ontologies`, assert
    `stats.abox_skipped.len() >= 1`.

- `dagalog-kernel/src/cell/manchester.rs` (existing test module):
  - `test_manchester_file_reports_skipped_abox_assertion`: load the new
    fixture through `execute_manchester_file`, assert the returned message
    mentions "skipped".

## Order of implementation

1. `assert_abox` return type change + its own unit test (smallest, most
   isolated).
2. `OntologyCompilation`/`ReasoningStats` field + `compile_ontology_rules`/
   `apply_ontologies` wiring + `cli_integration.rs` test.
3. `load_file`'s eprintln (no dedicated automated test beyond manual
   inspection — stderr text-format isn't a stable contract worth asserting on
   byte-for-byte; the count is already covered via `apply_ontologies` which
   also drives `assert_abox` on `.omn` paths).
4. `dagalog-kernel` call site + its test.
5. `docs/user/reasoning.md` callout.

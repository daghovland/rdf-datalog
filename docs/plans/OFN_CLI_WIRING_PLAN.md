# OWL 2 Functional-Style Syntax: CLI / notebook-kernel wiring

Tracks [#633](https://github.com/daghovland/rdf-datalog/issues/633), split out of
[#180](https://github.com/daghovland/rdf-datalog/issues/180)/[#627](https://github.com/daghovland/rdf-datalog/issues/627)
(the `owl_functional_parser` crate itself), the same way Manchester Syntax's
CLI wiring ([#161](https://github.com/daghovland/rdf-datalog/issues/161)) was
split from its parser ([#139](https://github.com/daghovland/rdf-datalog/issues/139)).

## Goal

Wire `owl_functional_parser::parse` (`.ofn` → `owl_ontology::Ontology`) into
the same two call sites Manchester's `.omn` support (#161) uses, mirroring
its behavior exactly:

1. **Main CLI (`dagalog` crate, `src/lib.rs`)** — `load_file` and
   `apply_ontologies`/`compile_ontology_rules` recognize the `.ofn`
   extension and dispatch to `owl_functional_parser::parse`, exactly as they
   do today for `.omn` → `manchester_parser::parse`. Same split: `load_file`
   materialises ABox-only (via `owl2rl2datalog::assert_abox`), and
   `apply_ontologies`/`compile_ontology_rules` additionally compiles the
   TBox to Datalog rules via `owl2datalog`, because a Functional-Style TBox
   axiom has no RDF-triple representation either (mirrors #177's Manchester
   gap) and would otherwise be silently lost once `load_file` alone had
   been used.
2. **`dagalog-kernel` notebook kernel** — a new `%%functional <path>` cell
   magic (module `dagalog-kernel/src/cell/functional.rs`,
   `execute_functional_file`), registered in `CellType`/`detect_cell_type`
   (`dagalog-kernel/src/cell/mod.rs`) and dispatched in
   `dagalog-kernel/src/sockets.rs`'s `dispatch_cell`. Mirrors
   `cell/manchester.rs`'s `execute_manchester_file` line for line: ABox via
   `assert_abox`, TBox via `owl2datalog` + `datalog::evaluate_rules`, same
   status-message shape (axiom/rule/triple counts, skipped-ABox warning).

## Out of scope

- Graph Store Protocol content negotiation for Functional-Style Syntax —
  mirrors the Manchester GSP gap tracked in
  [#291](https://github.com/daghovland/rdf-datalog/issues/291), itself
  pending [#177](https://github.com/daghovland/rdf-datalog/issues/177)
  (no RDF triple representation for frame-based OWL TBox axioms yet). Not
  attempted here for the same reason it hasn't been for Manchester.
- The parser itself (#180/#627) and the pairing serializer (#181).

## Test plan (TDD)

Mirrors the existing Manchester test coverage as closely as the two syntaxes'
semantics allow (all initially `#[ignore]`d, unignored one at a time during
implementation):

- `dagalog-kernel/src/cell/functional.rs`:
  - `test_functional_file_materialises_abox_and_reasons` — small `.ofn`
    fixture (`SubClassOf`, `ClassAssertion`), asserts the inferred triple
    is present after the cell runs (mirrors
    `test_manchester_file_materialises_abox_and_reasons`).
  - `test_functional_file_missing_returns_error`.
- `dagalog-kernel/src/cell/mod.rs`:
  - `test_functional_magic` / `test_functional_magic_trailing_newline` —
    `%%functional <path>` parses to `CellType::Functional(PathBuf)`.
- `src/lib.rs` (main CLI):
  - a `load_file`/`apply_ontologies` test loading a `.ofn` fixture,
    confirming ABox-only vs. full-TBox-reasoning behavior matches the
    `.omn` tests already present for the same split.

## Non-goals for the skipped-ABox test

Manchester's `test_manchester_file_reports_skipped_abox_assertion` covers a
non-atomic `Types:` assertion (`Dog or Cat`, an `ObjectUnionOf`) skipped by
`assert_abox`. The equivalent `.ofn` ABox assertion
(`ClassAssertion(ObjectUnionOf(:Dog :Cat) :fido)`) is included only if
`owl_functional_parser` already round-trips such class expressions inside
`ClassAssertion` — checked during implementation rather than assumed here,
since the parser's ABox-assertion coverage is out of this issue's scope to
verify from scratch.

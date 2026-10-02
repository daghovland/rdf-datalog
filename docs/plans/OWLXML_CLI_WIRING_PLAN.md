# OWL 2 XML Serialization: CLI / notebook-kernel wiring

Tracks [#609](https://github.com/daghovland/rdf-datalog/issues/609), the final
sub-issue of epic [#564](https://github.com/daghovland/rdf-datalog/issues/564)
(OWL 2 XML Serialization parser). Mirrors `.ofn`'s CLI wiring
([#633](https://github.com/daghovland/rdf-datalog/issues/633), see
[`OFN_CLI_WIRING_PLAN.md`](OFN_CLI_WIRING_PLAN.md)), which is itself modeled on
`.omn`'s ([#161](https://github.com/daghovland/rdf-datalog/issues/161)).

## Goal

Wire `owl_xml_parser::parse` (`.owx`/`.owl` → `owl_ontology::Ontology`) into
the same two call sites `.omn`/`.ofn` use:

1. **Main CLI (`dagalog` crate, `src/lib.rs`)** — `load_file` and
   `apply_ontologies`/`compile_ontology_rules` recognize `.owx` and (when
   content-sniffing confirms it) `.owl`, dispatching to
   `owl_xml_parser::parse` exactly as they do for `.omn`/`.ofn`. Same split:
   `load_file` materialises ABox-only (`owl2rl2datalog::assert_abox`),
   `apply_ontologies`/`compile_ontology_rules` additionally compiles the TBox
   to Datalog rules via `owl2datalog` (a frame/XML-sourced TBox axiom has no
   RDF-triple representation — #177 — so it would be silently lost if only
   `load_file` ran).
2. **`dagalog-kernel` notebook kernel** — a new `%%owlxml <path>` cell magic
   (module `dagalog-kernel/src/cell/owl_xml.rs`, `execute_owl_xml_file`),
   registered in `CellType`/`detect_cell_type`
   (`dagalog-kernel/src/cell/mod.rs`) and dispatched in
   `dagalog-kernel/src/sockets.rs`. Mirrors `cell/functional.rs` line for
   line.

## The `.owl` extension ambiguity

Per the issue body: `.owl` is ambiguous. This repository's own test fixtures
(`tests/testdata/equality.owl`, `empty.owl`, `subclass_of_restriction.owl`)
are **Turtle**-serialized ontologies using the `.owl` extension, currently
loaded by `load_file`'s default (Turtle) branch. Naively routing every `.owl`
file to `owl_xml_parser` would silently break these.

Resolution: content-sniff rather than trust the extension for `.owl` alone.
`owl_xml_parser::looks_like_owl_xml(src: &str) -> bool` parses the document
with `roxmltree` and checks whether the **root element's tag name is
literally `Ontology`** (the OWL/XML Serialization spec's root element). This
is cheap (one XML parse) and distinguishes:

- OWL/XML Serialization (`<Ontology ...>` root) → route to `owl_xml_parser`.
- Turtle (`@prefix ...`, starts with a non-`<` token in nearly all cases, and
  even the pathological `<http://x> a ...` Turtle still fails to parse as
  XML — a bare IRI isn't a legal XML tag) → sniff returns `false`, falls
  through to the existing Turtle-loading path unchanged.
- RDF/XML (`<rdf:RDF ...>` root) → sniff returns `false` (root tag is `RDF`,
  not `Ontology`) and falls through to the Turtle loader, which will fail to
  parse it. This is a **pre-existing gap** (RDF/XML has never been supported
  by this codebase under any extension) — not a regression introduced here,
  and out of scope for #609.

`.owx` has no such ambiguity (a dedicated extension for this one format) and
is always routed to `owl_xml_parser`.

## ABox support is not yet landed (#608)

Unlike `.ofn` at the time of its CLI wiring, `owl_xml_parser` does not yet
parse ABox assertion axioms (`<ClassAssertion>`, `<ObjectPropertyAssertion>`,
...) — issue [#608](https://github.com/daghovland/rdf-datalog/issues/608) is
still open. `owl_xml_parser::parse` returns a hard `Err` if any such element
is encountered (see `owl_xml_parser/src/lib.rs`'s `other => Err(...)` arm),
rather than silently dropping it. Consequences for this wiring:

- TBox-only `.owx`/`.owl` documents load and reason correctly today.
- A document containing ABox assertions fails to parse (clear error,
  pointing at #608) until that issue merges — at which point this wiring
  gains full ABox support automatically, with no changes needed here per se
  (same as `.omn`/`.ofn`'s `assert_abox` call already being format-agnostic
  over `owl_ontology::Ontology`).
- No analogue of `.ofn`'s "skipped non-atomic ABox assertion" test is added
  here, since there is no ABox path to exercise yet.

## Out of scope

- #608 itself (ABox axioms) — not duplicated here.
- Graph Store Protocol content negotiation for OWL/XML — mirrors the
  Manchester/Functional-Style GSP gap (#291, pending #177). Not attempted
  here for the same reason.
- `scripts/download_test_ontologies.sh`'s Gene Ontology (`go.owl`) riot
  fallback: filed separately as a follow-up issue rather than attempted in
  this PR, since confirming `go.owl`'s actual root element (OWL/XML
  Serialization vs. RDF/XML — the existing script's own comments are
  inconsistent about which it is) requires a network fetch this task wasn't
  scoped to perform.

## Test plan (TDD)

All initially `#[ignore]`d, unignored one at a time during implementation:

- `owl_xml_parser/src/lib.rs`:
  - `looks_like_owl_xml` unit tests: `<Ontology ...>` root → `true`; Turtle
    text → `false`; `<rdf:RDF ...>` root → `false`; malformed XML → `false`.
- `src/lib.rs` (main CLI):
  - `load_file`/`apply_ontologies` tests loading a small `.owx` fixture
    (`SubClassOf` only, no ABox), confirming the same ABox-only vs.
    full-TBox-reasoning split as the `.omn`/`.ofn` tests.
  - A `.owl`-with-OWL/XML-content fixture, confirming it's routed the same
    way as `.owx`.
  - A regression test loading an existing Turtle-syntax `.owl` fixture
    (`tests/testdata/equality.owl`) through `load_file`, confirming the
    sniff correctly falls through and behavior is unchanged.
- `dagalog-kernel/src/cell/owl_xml.rs`:
  - `test_owl_xml_file_materialises_abox_and_reasons` (TBox-only fixture —
    "ABox" here is empty, so this really just confirms TBox reasoning
    applies and the status message's shape matches Manchester/Functional).
  - `test_owl_xml_file_missing_returns_error`.
- `dagalog-kernel/src/cell/mod.rs`:
  - `test_owlxml_magic` / `test_owlxml_magic_trailing_newline` — `%%owlxml
    <path>` parses to `CellType::OwlXml(PathBuf)`.

# Plan: adopt the W3C SHACL test suite (#268)

## Goal

Wire up the official W3C SHACL 1.0 conformance test suite as a maintained
Rust test file, `tests/w3c_shacl_suite.rs`, mirroring the pattern already
proven for SPARQL 1.1 (`tests/w3c_sparql11_suite.rs`) and RDF 1.1
(`tests/w3c_rdf_conformance.rs`): real Turtle parsing into a `Datastore`,
walked with real SPARQL queries via `sparql_parser`'s executor — no
hand-rolled manifest line-scanning (per #192).

## Source and vendoring

- Suite source: <https://github.com/w3c/data-shapes>, `data-shapes-test-suite/tests/`.
- Vendored under `tests/testdata/w3c_shacl/core/` (license:
  `tests/testdata/w3c_shacl/LICENSE`, same W3C Software and Document License
  terms as the other vendored suites).
- Only `core/` is vendored: `complex/`, `misc/`, `node/`, `path/`,
  `property/`, `targets/`, `validation-reports/` (121 test files total).
  `core/sparql/` (SHACL-SPARQL constraints, §5–6) is deliberately **not**
  vendored — already tracked as out of scope by #54; nothing to skip-list
  because it was never pulled in.
- `shacl12-test-suite/` (the newer SHACL 1.2 draft suite) is out of scope —
  this crate implements the SHACL 1.0 Recommendation.

## Manifest structure (not identical to the SPARQL suite)

Each test file (e.g. `core/node/and-001.ttl`) is **self-contained**: it holds
the shapes graph, the data graph (often the same graph, referenced via `<>`),
its own tiny `mf:Manifest`/`mf:entries` list, and one `sht:Validate` entry
whose `mf:result` is an **inline blank-node subgraph** (`sh:ValidationReport`
with `sh:result` entries) — not a separate result file, unlike SPARQL's
`.srx`. Discovery is two `mf:include` hops:
`core/manifest.ttl` → `core/<area>/manifest.ttl` → `core/<area>/<test>.ttl`.
Both hops are walked with a SPARQL query (`mf:include`), not by globbing the
directory, so that entry counts are meaningful (globbing would also match
`*-data.ttl`/`*-shapes.ttl` companion files that contribute zero entries).

Test entries use `rdfs:label` for the human-readable name (not `mf:name` as
in the SPARQL suite) and `sht:dataGraph`/`sht:shapesGraph` (not `qt:data`/
`qt:query`) in `mf:action`.

## Comparison strategy

The expected result is a full `sh:ValidationReport` graph, not a results
table, so an SRX-style comparison doesn't apply. A full RDF-graph-isomorphism
comparison (as used for Turtle/N-Triples eval tests) is also the wrong tool
here: `shacl::ValidationResult`'s fields (`source_shape`, `result_path`,
`focus_node`, `value`) are already-flattened `String`s (an IRI, a blank-node
label `_:bN`, or a literal display form) rather than RDF terms, and some
expected `sh:resultPath` values are RDF-list-valued (complex property paths)
which this crate's `sh:path` support doesn't represent at all yet — so
blank-node-for-blank-node isomorphism on both sides isn't achievable without
inventing structure the actual side doesn't have.

Instead the harness uses a tiered comparator, from hard requirement to
skipped-with-reason:

1. `sh:conforms` boolean must match exactly (hard requirement).
2. Result count must match exactly (hard requirement).
3. Each expected result must find an unmatched actual result agreeing on
   `sh:resultSeverity` and `sh:sourceConstraintComponent` (both always
   resolvable IRIs on both sides — hard requirement; this alone would have
   caught most of the #256/#258/#260/#262 regressions).
4. `sh:focusNode`/`sh:value`/`sh:sourceShape`/`sh:resultPath` are compared
   too, but only when the expected term is a plain IRI — a blank-node or
   RDF-list-valued expected term is skipped for that field only (blank-node
   labels aren't stable across the two independent Turtle parses involved:
   one for reading the manifest's embedded expected-report graph, one inside
   `shacl::validate` for the actual data/shapes graphs).
5. `sh:resultMessage` is **never** compared — message text is
   implementation-defined per spec (only normative when the shape declares
   its own `sh:message`, which the vendored suite's expected reports don't
   use).

This is a deliberate, explicit deviation from doing byte-for-byte
blank-node-isomorphic report comparison; noted here and in the PR description
for review.

## Expected skip-list (first pass)

- `core/path/*` — nearly all of it: this crate's `sh:path` parsing only
  supports a single predicate IRI, not sequence/inverse/alternative/
  `zeroOrMore`/`oneOrMore`/`zeroOrOne` path expressions. Tracked by a new
  follow-up issue (filed during implementation, linked from the skip-list
  comment).
- `core/complex/shacl-shacl.ttl` — validates SHACL's own shapes-of-shapes
  ontology; depends on SHACL-SPARQL-ish meta-shapes and is out of scope with
  §5–6 (#54).
- Any entry where `shacl::validate` returns `Err` (e.g. the #278 shape-cycle
  guard) is reported as a failure, never silently swallowed via `unwrap`.
- Anything else the first pass surfaces as a genuine, previously-unknown
  `shacl` crate gap gets skip-listed with a link to a newly filed (unlabeled)
  issue, per the parent issue's explicit "expect a substantial skip-list on
  the first PR" framing. No inline crate fixes in this PR.

## Test structure

One `#[test]` per vendored sub-directory (`core_node`, `core_property`,
`core_path`, `core_misc`, `core_targets`, `core_complex`,
`core_validation_reports`) — matching the loop-with-collected-failures idiom
`w3c_sparql11_suite.rs` uses for `run_syntax_tests`/`assert_no_failures`,
rather than 121 individual `#[test]` functions. Each test asserts a minimum
expected entry count for its directory (so a manifest-parsing regression that
silently yields zero entries fails loudly instead of reporting a vacuous
pass).

---
name: project-shacl
description: SHACL validation crate and test suite status — what exists, what's stub, what's planned next
metadata:
  type: project
---

SHACL validation test suite and stub crate created (2026-06-13). Plan updated 2026-06-14.

**Approach (updated):** No external processes. Two strategies:
1. **SHACL Core → Datalog** — translate each shape into `datalog::Rule` objects (same as `owl2rl2datalog`), then run `datalog::evaluate_rules`. Translation is derived from W3C spec §4 SPARQL "potential definitions".
2. **SHACL-SPARQL (§5-6)** → execute embedded SPARQL SELECT/ASK queries directly using `sparql-parser`.

**What exists:**
- `shacl/` crate: stub types (`ValidationReport`, `ValidationResult`, `Severity`) and stub `validate(_data, _shapes)` / `report_to_turtle(_report)` — both `todo!()`.
- `tests/shacl_suite.rs`: 30 `#[ignore]` tests + 1 non-ignored parse guard (`shacl_testdata_parses`).
- 60 `.ttl` test data files in `tests/testdata/shacl_s*.ttl` (data + shapes pairs), verbatim from W3C SHACL spec §1.4, §2.1.3.x, §4.1–4.8.
- README SHACL section with API example, §1.4 shape code, and full component table linked to test names.
- SHACL_PLAN.md: detailed SHACL→Datalog translation table per constraint component, Datalog built-in extension list, phased implementation plan (Phase 1-4).

**Planned crate structure:**
`shacl/src/` → vocab.rs, graph.rs, shapes.rs, targets.rs, translate.rs, evaluate.rs, report.rs

**How to apply:** When implementing `shacl::validate`, the test assertions in `shacl_suite.rs` define the expected violation counts. Each test cites the exact W3C spec section. Un-ignore tests as constraint components are implemented per the phases in SHACL_PLAN.md.

Phase 1 needs: vocab.rs + graph.rs helpers + shapes.rs parser + targets + translate for minCount/maxCount/hasValue/in/class/closed + logical sh:not/and/or.
Phase 2 needs: Datalog built-in predicates (isIRI, datatype, regex, strlen, comparisons).

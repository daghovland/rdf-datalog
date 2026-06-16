---
name: project-shacl
description: SHACL validation crate and test suite status — what exists, what's stub, what's planned next
metadata:
  type: project
---

SHACL validation full implementation complete through Phase 3 (2026-06-15).

**Approach:** No external processes. Two strategies:
1. **Phase 1 — SHACL Core → Datalog** — translate each shape's existence/structural constraints into `datalog::Rule` objects (same as `owl2rl2datalog`), then run `datalog::evaluate_rules`.
2. **Phase 2 — Direct Rust evaluation** — value-testing constraints (datatype, nodeKind, range, string, property pair, sh:node, sh:qualifiedValueShape, sh:xone) evaluated directly in `shacl/src/evaluate.rs` against the original data graph (mirrors `sh:closed` pattern — avoids Datalog built-in extensions).

**What exists:**
- `shacl/` crate: full `validate(_data, _shapes)` and `report_to_turtle(_report)` working.
- `tests/shacl_suite.rs`: all 31 tests pass (including parse guard + all §1–§4.8 spec examples).
- 60 `.ttl` test data files in `tests/testdata/shacl_s*.ttl`.
- README SHACL section.
- `sparql_endpoint/src/shacl_endpoint.rs`: `POST /{name}/shacl` handler.
- `sparql_endpoint/tests/shacl.rs`: 4 HTTP endpoint tests (conforms, violation, 404, 400).

**Crate structure:**
`shacl/src/` → evaluate.rs, graph.rs, lib.rs (validate + report_to_turtle), shapes.rs, translate.rs, vocab.rs

**Phase 2 note:** All Phase 2 constraints are implemented in `shacl/src/evaluate.rs` as direct Rust code, NOT as Datalog built-in predicates. This matches the `sh:closed` isolation pattern. SPARQL STRLEN and DATATYPE were added to `sparql_parser::eval_function_value` for Datalog filter tests.

**How to apply:** When implementing Phase 4 (SHACL-SPARQL), parse `sh:sparql` from the shapes graph and execute embedded SELECT/ASK queries using `sparql_parser::run_sparql_query` with `$this` pre-bound.

**Planned:**
- Phase 4: SHACL-SPARQL (§5–6 of SHACL-AF) — shapes with `sh:sparql [ sh:select "..." ]`

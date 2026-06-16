---
name: project-expression-plan
description: EXPRESSION_PLAN.md status — SPARQL expressions as Datalog FILTER guards; all phases E1-E5 complete
metadata:
  type: project
---

EXPRESSION_PLAN.md implementation complete as of 2026-06-15.

**Goal:** Make SPARQL `rExpression` usable as guard predicates in Datalog rule bodies.

**Dependency graph:**
```
datalog  →  sparql-parser  →  dag-rdf, ingress
datalog-parser  →  sparql-parser, datalog, dag-rdf
```

## What was done

| Phase | Status | Key files |
|---|---|---|
| E1: Expose SPARQL eval functions as pub | ✓ Done | `sparql_parser/src/execute.rs` — `eval_expr_as_filter` |
| E2: FilterAtom in Datalog engine | ✓ Done | `datalog/src/types.rs`, `datalog/src/datalog.rs` |
| E3: RDFox-style extensions | Not needed yet | — |
| E4: SHACL evaluate.rs refactor | Deferred | Only value-testing constraints could migrate; counting/set constraints (uniqueLang, xone, qualifiedValueShape) require aggregation not in engine |
| E5: FILTER in Datalog parser | ✓ Done | `datalog_parser/src/lib.rs`, `sparql_parser/src/lib.rs` |

## E5 implementation details

- `sparql_parser::parse_filter_expression(input, &ctx) -> Result<(usize, Expression), String>` — returns `(bytes_consumed, expr)` to avoid lifetime coupling to ctx
- `datalog_parser` now has `sparql-parser` dependency in `Cargo.toml`
- `ParsedRuleAtom::FilterAtom(Expression)` intermediate AST variant
- `keyword_filter()` recognizes `FILTER` keyword (case-insensitive, word boundary)
- `ParserContext::to_sparql_context()` converts prefix maps

## Tests

8 FilterAtom tests in `tests/datalog_integration.rs`:
- E2 engine tests: `filter_numeric_comparison`, `filter_strlen_guard`, `filter_is_iri_guard`, `filter_datatype_guard`, `filter_regex_guard` (all passing)
- E5 parser tests: `parse_filter_in_datalog_rule`, `parse_filter_strlen_in_datalog_rule`, `parsed_filter_rule_end_to_end` (all passing)

**Why:** SPARQL expressions as Datalog guards enable SHACL translation to emit FilterAtom rules (Phase E4, deferred) and allow rule authors to write value tests in Datalog syntax without hand-coding Rust.

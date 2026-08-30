# Shared Expression Layer Plan

## Goal

Make SPARQL [`rExpression`](https://www.w3.org/TR/sparql11-query/#rExpression) usable as
guard predicates in Datalog rule bodies, so that:

1. Datalog rules can use `FILTER(expr)` guards with identical syntax and semantics to SPARQL FILTER.
2. SHACL translation (`shacl/src/translate.rs`) can emit Datalog rules with expression guards,
   removing the hand-coded Rust evaluation currently in `shacl/src/evaluate.rs`.
3. The Datalog parser (`datalog_parser`) can accept FILTER guards in rule bodies.
4. Any capabilities beyond SPARQL 1.1 expressions use [RDFox built-in syntax](https://docs.oxfordsemantic.tech/builtins.html).

## Overview

This plan is tracked under epic [#59](https://github.com/daghovland/rdf-datalog/issues/59).
Phase E4 (SHACL refactor) is tracked under [#62](https://github.com/daghovland/rdf-datalog/issues/62).

## Architecture

### No new crate needed

`sparql-parser` does not depend on `datalog`, so `datalog` can safely depend on
`sparql-parser` without a cycle.  Both crates already share `dag-rdf`.

Dependency graph after this change:

```
datalog  →  sparql-parser  →  dag-rdf, ingress
         →  dag-rdf
```

### New `RuleAtom` variant

```rust
// datalog/src/types.rs
pub enum RuleAtom {
    PositivePattern(QuadPattern),
    NotPattern(QuadPattern),
    NotEqualsAtom(Term, Term),
    FilterAtom(sparql_parser::ast::Expression),   // ← NEW
}
```

A `FilterAtom` acts as a guard: the substitution passes iff the expression evaluates to `true`.
Variables in the expression are resolved through the current substitution, exactly as in SPARQL FILTER.

### Evaluation bridge

`sparql_parser::execute::eval_expression_bool` already has the correct signature:

```rust
fn eval_expression_bool(
    expr: &Expression,
    sub: &HashMap<String, GraphElementId>,   // same type as Datalog Substitution
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Option<bool>
```

The only change needed is to make it `pub` (or add a thin `pub` wrapper) and to pass the
`Datastore` into the datalog evaluator.  Currently `evaluate()` in `datalog.rs` only receives
a `&QuadTable`; we need to extend it to also receive `&dag_rdf::resources::GraphElementManager`
(or the full `&Datastore`) so literals can be resolved by ID.

---

## Implementation phases

### Phase E1 — Expose SPARQL expression evaluator

**Files:** `sparql_parser/src/execute.rs`

- Make `eval_expression_bool` and `eval_expression_value` `pub(crate)` → `pub`.
  (Or add a single `pub fn eval_filter(expr, sub, datastore) -> bool` wrapper.)
- Add `ActiveGraph` parameter default (use default graph) so callers outside sparql_parser
  don't need to construct one.

**Tests (ignored until E2):**
- None; this is a pure visibility change.

---

### Phase E2 — Add `FilterAtom` to Datalog

**Files:**
- `datalog/Cargo.toml` — add `sparql-parser` dependency
- `datalog/src/types.rs` — add `RuleAtom::FilterAtom(sparql_parser::ast::Expression)`
- `datalog/src/datalog.rs` — handle `FilterAtom` in `evaluate()`:
  - Extend signature to accept `&Datastore` (or `&GraphElementManager`) alongside `&QuadTable`
  - After positive atoms are matched, filter substitutions through `eval_expression_bool`
- `datalog/src/reasoner.rs` — forward new `&Datastore` arg through `evaluate_rules`

**Ignored integration test (create now, un-ignore after implementing):**

```rust
// tests/datalog_integration.rs
#[test]
#[ignore = "FilterAtom not yet implemented"]
fn datalog_filter_numeric_guard() {
    // Rule: violation(x) :- [x, ex:age, ?a], FILTER(?a < 18)
    // Data: ex:alice ex:age 25; ex:bob ex:age 15
    // Expected: violation(ex:bob) only
}
```

---

### Phase E3 — RDFox-style extensions beyond SPARQL 1.1

SPARQL 1.1 `rExpression` covers:
- Arithmetic: `+`, `-`, `*`, `/`
- Comparison: `=`, `!=`, `<`, `>`, `<=`, `>=`
- Boolean: `&&`, `||`, `!`
- String: `STRLEN`, `SUBSTR`, `UCASE`, `LCASE`, `STRSTARTS`, `STRENDS`, `CONTAINS`, `REGEX`
- Type tests: `isIRI`, `isLiteral`, `isBlankNode`, `DATATYPE`, `LANG`, `LANGMATCHES`
- Node construction: `IRI`, `STR`, `BNODE`
- Aggregate: `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`

For any capabilities beyond SPARQL 1.1, adopt [RDFox built-in syntax](https://docs.oxfordsemantic.tech/builtins.html):

| Extended built-in | RDFox-style syntax | Use case |
|---|---|---|
| `BIND(?x := expr)` | `BIND(expr AS ?x)` (already SPARQL) | value derivation in rule head |
| `SKOLEM(?x, ?y)` | `SKOLEM(?x, ?y)` | blank node generation |
| `rdfox:substr` | `SUBSTR(?s, ?start, ?len)` | (already SPARQL) |

In practice, SPARQL 1.1 expressions are sufficient for all SHACL Core constraints.
RDFox extensions are reserved for future Datalog rule authoring beyond SHACL.

---

### Phase E4 — Rewrite SHACL evaluate.rs using FilterAtom rules

**Status as of issue [#62](https://github.com/daghovland/rdf-datalog/issues/62):** the mapping
table below (written before E1/E2/E5 landed) is stale in two ways: the named Rust functions
(`eval_node_kind`, `eval_datatype`, …) no longer exist under those names — the logic they
described now lives in shared helpers (`matches_node_kind`, `has_datatype`, `range_violates`,
`lang_matches`, `sparql_compare`, `regex_with_flags`) called from **two** independent
consumers, not one:

1. `evaluate.rs::eval_prop_constraint` — the top-level per-shape/per-property violation
   producer (what this table originally described).
2. `evaluate.rs::constraint_conforms` — a yes/no conformance check used *recursively* by
   `shape_conforms_for_node`, which `sh:and`/`sh:or`/`sh:not`/`sh:xone`/`sh:node`/
   `sh:qualifiedValueShape` call to test whether a node conforms to an arbitrary referenced
   inner shape. This consumer didn't exist when the table was written (added across
   [#258](https://github.com/daghovland/rdf-datalog/issues/258)/[#276](https://github.com/daghovland/rdf-datalog/issues/276)/
   [#309](https://github.com/daghovland/rdf-datalog/issues/309)/[#311](https://github.com/daghovland/rdf-datalog/issues/311)).
   Porting it to Datalog would mean recursive rule generation over the shape-reference graph —
   a materially larger change than "port six functions," and out of scope for #62.

So #62's scope is narrower than the original table: only consumer (1) is touched. Consumer
(2) keeps using the shared Rust helpers, which is why those helpers are not deleted even
for the constraint kinds that do get a Datalog rule.

**Constraint-by-constraint verdict** (checked against `sparql_parser`'s actual expression
evaluator, `sparql_parser/src/execute/expressions.rs` + `functions.rs`, not just the
plan's original intent):

| Constraint | Ported to `FilterAtom`? | Why / why not |
|---|---|---|
| `sh:nodeKind` | **Yes** | `ISIRI`/`ISBLANK`/`ISLITERAL` match `GraphElement` variants directly — no lossy string conversion, no comparison-error edge cases. Clean 1:1 with `matches_node_kind`. |
| `sh:languageIn` | **Yes** | `LANG()` returns `""` for any non-`LangLiteral` value (literal or not) without erroring, and `LANGMATCHES("", tag)` is `false` for every concrete tag — so "not a language-tagged literal ⇒ violates" (the #303/#266 rule) falls out of the SPARQL semantics for free, no special-casing needed. |
| `sh:datatype` | No — deferred | `DATATYPE()` only compares the datatype IRI; it does not check lexical well-formedness (`"aldi"^^xsd:integer` per [#325](https://github.com/daghovland/rdf-datalog/issues/325)). Porting would silently drop that check. |
| `sh:minInclusive`/`sh:maxInclusive`/`sh:minExclusive`/`sh:maxExclusive` | No — deferred | The generic SPARQL `<`/`>`/`<=`/`>=` evaluator (`compare_graph_elements`) is *weaker* than SHACL's own `sparql_compare`: it falls back to naive string comparison for any two literals that aren't both numeric, so e.g. `"2020-01-01"^^xsd:date` vs `"2020-01-01"^^xsd:string` would compare equal instead of being flagged incomparable-hence-violation (the exact bug class fixed by [#303](https://github.com/daghovland/rdf-datalog/issues/303)/[#322](https://github.com/daghovland/rdf-datalog/issues/322)/[#325](https://github.com/daghovland/rdf-datalog/issues/325)). `FILTER(!(v >= min))` *would* correctly turn a comparison error into a violation (`eval_expr_as_filter` defaults errors to `false`, and `!false = true`), so the "incomparable ⇒ violation" direction is not the blocker — the type-check laxness of `compare_graph_elements` itself is. |
| `sh:pattern` | No — deferred | `REGEX(v, pat)` evaluates its text argument via `graph_element_to_string`, which only covers `LiteralString`/`LangLiteral`/`TypedLiteral`/IRI — not the natively-typed `IntegerLiteral`/`BooleanLiteral`/`DateLiteral`/etc. variants that `lexical_form` (used by the current Rust path) does cover. A numeric-typed value would always evaluate the filter to "no match" (text unresolvable) regardless of whether the pattern would actually match its lexical form — a real regression risk, not just a style difference. Blank-node "always violates" *does* work correctly (`graph_element_to_string` returns `None` for a blank node, so `!REGEX(...)` correctly becomes `true`). |
| `sh:lessThan`/`sh:lessThanOrEquals` | No — deferred | Same comparator gap as the range constraints, **plus** a cardinality problem: [#343](https://github.com/daghovland/rdf-datalog/issues/343) requires one violation *per failing `(value, otherValue)` pair* (not one per `value`), using per-pair discriminated predicates so they don't collapse in the dedup'd `QuadTable`. A Datalog rule head can only carry the bound variables that appear in it; encoding "one fact per failing pair" with a fresh synthetic predicate per pair isn't expressible as a fixed rule head the way `viol_discriminated` currently mints predicates at Rust runtime. |

Follow-up issue [#631](https://github.com/daghovland/rdf-datalog/issues/631) tracks porting the deferred constraints once (a) `sparql_parser`'s
comparator and string-coercion functions are brought up to full SPARQL 1.1 §17.1/§17.3
fidelity for XSD-typed literals (a `sparql_parser` fix, not a `shacl` one), and (b) a
per-pair-multiplicity rule-generation pattern exists for `sh:lessThan`-shaped constraints.

**Files:** `shacl/src/translate.rs` (adds `NodeKind`/`LanguageIn` rule generation),
`shacl/src/evaluate.rs` (removes the now-redundant `eval_prop_constraint` arms for those two
— `matches_node_kind`/`lang_matches` themselves stay, still used by `constraint_conforms`).

**Tests:** All existing SHACL tests (including the W3C SHACL suite, `tests/w3c_shacl_suite.rs`)
must continue passing after the refactor, with no change to any skip list.

---

### Phase E5 — Datalog parser: FILTER in rule bodies

`datalog_parser/src/lib.rs` now parses `FILTER(expr)` in rule bodies, emitting
`RuleAtom::FilterAtom`.  The expression parser is shared via `sparql_parser::parse_filter_expression`.

Implementation:
- `datalog_parser/Cargo.toml` adds `sparql-parser` dependency
- `ParsedRuleAtom::FilterAtom(Expression)` intermediate AST variant
- `keyword_filter()` recognises the `FILTER` keyword (case-insensitive)
- `ParserContext::to_sparql_context()` converts prefix maps for the SPARQL parser
- `parse_filter_expression(input, &sparql_ctx)` returns `(bytes_consumed, expr)` to avoid lifetime coupling
- `intern_rule_atom` passes `FilterAtom(expr)` through unchanged (no IRI interning needed)

Tests in `tests/datalog_integration.rs`:
- `parse_filter_in_datalog_rule` — structure test (PositivePattern + FilterAtom)
- `parse_filter_strlen_in_datalog_rule` — function call in FILTER
- `parsed_filter_rule_end_to_end` — parse + evaluate + SPARQL query

---

## Relationship to SHACL_PLAN.md

The SHACL Phase 2 constraints (nodeKind, datatype, range, string, property pairs) are fully
implemented and tested in `shacl/src/evaluate.rs` using hand-coded Rust.

- **Phase E2** provides the infrastructure that makes the SHACL Datalog-translation strategy
  viable for value-testing constraints (previously only possible in Rust).
- **Phase E4** is an optional refactor: migrates the hand-coded evaluate.rs logic into
  Datalog rules with FilterAtom guards.  Functionally equivalent; architecturally cleaner.
- **SHACL Phase 3** (HTTP endpoint + report_to_turtle) is independent and can proceed
  without Phase E2/E4.
- **SHACL Phase 4** (SHACL-SPARQL §5–6) already uses the SPARQL engine directly and
  does not need FilterAtom.

---

## File change summary

| File | Change |
|---|---|
| `sparql_parser/src/execute.rs` | `eval_expr_as_filter` made `pub` (wrapper for SPARQL filter evaluation) |
| `sparql_parser/src/lib.rs` | Added `pub fn parse_filter_expression(input, ctx)` for Datalog parser use |
| `datalog/Cargo.toml` | Added `sparql-parser = { path = "../sparql_parser" }` |
| `datalog/src/types.rs` | Added `RuleAtom::FilterAtom(sparql_parser::ast::Expression)` |
| `datalog/src/datalog.rs` | Handle `FilterAtom` in `evaluate()` via `sparql_parser::eval_expr_as_filter` |
| `datalog/src/reasoner.rs` | Pass `&Datastore` through to `evaluate()` |
| `datalog_parser/Cargo.toml` | Added `sparql-parser = { path = "../sparql_parser" }` |
| `datalog_parser/src/lib.rs` | Parses `FILTER(expr)` in rule bodies; emits `RuleAtom::FilterAtom` |
| `tests/datalog_integration.rs` | 8 FilterAtom tests: 5 engine tests + 3 parser+end-to-end tests |
| `shacl/src/translate.rs` | (Phase E4 deferred) |
| `shacl/src/evaluate.rs` | (Phase E4 deferred) |

---

## Progress tracking

Progress on completed and deferred phases is tracked via GitHub issues:
- Epic: [#59 Shared Expression Layer](https://github.com/daghovland/rdf-datalog/issues/59)
- E4 SHACL refactor: [#62 Expression layer E3: replace shacl hand-coded Rust constraint evals](https://github.com/daghovland/rdf-datalog/issues/62)

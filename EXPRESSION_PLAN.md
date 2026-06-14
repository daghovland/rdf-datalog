# Shared Expression Layer Plan

## Goal

Make SPARQL [`rExpression`](https://www.w3.org/TR/sparql11-query/#rExpression) usable as
guard predicates in Datalog rule bodies, so that:

1. Datalog rules can use `FILTER(expr)` guards with identical syntax and semantics to SPARQL FILTER.
2. SHACL translation (`shacl/src/translate.rs`) can emit Datalog rules with expression guards,
   removing the hand-coded Rust evaluation currently in `shacl/src/evaluate.rs`.
3. The Datalog parser (`datalog_parser`) can accept FILTER guards in rule bodies.
4. Any capabilities beyond SPARQL 1.1 expressions use [RDFox built-in syntax](https://docs.oxfordsemantic.tech/builtins.html).

## Current state

| Component | Status |
|---|---|
| `sparql_parser/src/ast.rs` — `Expression` enum | ✓ Full SPARQL 1.1 expression AST |
| `sparql_parser/src/execute.rs` — `eval_expression_bool`, `eval_expression_value` | ✓ Full SPARQL 1.1 evaluation (private) |
| `datalog/src/types.rs` — `RuleAtom` | Only `PositivePattern`, `NotPattern`, `NotEqualsAtom` |
| `datalog/src/datalog.rs` — `evaluate()` | Handles only `NotEqualsAtom` as a built-in |
| `shacl/src/evaluate.rs` | Hand-coded Rust for nodeKind, datatype, range, string, pairs |

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

Once Phase E2 is complete, the hand-coded Rust in `shacl/src/evaluate.rs` can be replaced
with Datalog rules that include `FilterAtom` guards.  This is a refactor — the existing
SHACL tests continue passing throughout.

**Current mapping (evaluate.rs → Datalog rule with FilterAtom):**

| Current Rust function | Datalog rule equivalent |
|---|---|
| `eval_node_kind()` | `sh_viol(n, v) :- target(n), value(n,v), FILTER(!isIRI(v))` |
| `eval_datatype()` | `sh_viol(n, v) :- target(n), value(n,v), FILTER(DATATYPE(v) != D)` |
| `eval_range()` | `sh_viol(n, v) :- target(n), value(n,v), FILTER(v < minVal)` |
| `eval_min_length()` | `sh_viol(n, v) :- target(n), value(n,v), FILTER(STRLEN(STR(v)) < N)` |
| `eval_regex_check()` | `sh_viol(n, v) :- target(n), value(n,v), FILTER(!REGEX(STR(v), pat, flags))` |
| `eval_language_in()` | `sh_viol(n, v) :- target(n), value(n,v), FILTER(!LANGMATCHES(LANG(v), tag))` |
| `eval_less_than()` | `sh_viol(n, v) :- target(n), [n,p1,v], [n,p2,w], FILTER(!(v < w))` |

**Files:** `shacl/src/translate.rs`, `shacl/src/evaluate.rs`

**Tests:** All 31 existing SHACL tests must continue passing after refactor.

---

### Phase E5 — Datalog parser: FILTER in rule bodies

Extend `datalog_parser/src/lib.rs` to parse `FILTER(expr)` in rule bodies, emitting
`RuleAtom::FilterAtom`.  The expression parser can be shared from or extracted from
`sparql_parser`.

**Ignored test (create now):**

```rust
// datalog_parser/tests/
#[test]
#[ignore = "FILTER in Datalog not yet implemented"]
fn parse_datalog_rule_with_filter() {
    let src = r#"violation(?x) :- [?x, ex:age, ?a], FILTER(?a < 18) ."#;
    let rules = parse_datalog(src).unwrap();
    assert_eq!(rules.len(), 1);
    assert!(rules[0].body.iter().any(|a| matches!(a, RuleAtom::FilterAtom(_))));
}
```

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
| `sparql_parser/src/execute.rs` | Make `eval_expression_bool` + `eval_expression_value` `pub` |
| `datalog/Cargo.toml` | Add `sparql-parser = { path = "../sparql_parser" }` |
| `datalog/src/types.rs` | Add `RuleAtom::FilterAtom(sparql_parser::ast::Expression)` |
| `datalog/src/datalog.rs` | Handle `FilterAtom` in `evaluate()`; extend to take `&Datastore` |
| `datalog/src/reasoner.rs` | Pass `&Datastore` through to `evaluate()` |
| `tests/datalog_integration.rs` | Add ignored `datalog_filter_*` tests |
| `datalog_parser/src/lib.rs` | (Phase E5) Parse `FILTER(expr)` in rule bodies |
| `shacl/src/translate.rs` | (Phase E4) Emit `FilterAtom` rules instead of calling evaluate.rs |
| `shacl/src/evaluate.rs` | (Phase E4) Remove hand-coded constraint evaluation |

---

## Status

| Phase | Status |
|---|---|
| E1: Expose SPARQL eval functions as pub | Planned |
| E2: FilterAtom in Datalog RuleAtom + evaluation | Planned |
| E3: RDFox-style extensions | Not needed yet |
| E4: SHACL evaluate.rs → FilterAtom rules (refactor) | Planned (after E2) |
| E5: FILTER in Datalog parser | Planned (after E2) |

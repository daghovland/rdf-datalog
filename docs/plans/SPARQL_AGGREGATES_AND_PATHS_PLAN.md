# SPARQL Aggregates and Extended Property Paths — Implementation Plan

**Status**: Complete. Both tracks implemented and all 16 new tests passing.

**Last updated**: 2026-06-18

---

## Overview

Two independent feature tracks, delivered in two separate implementation sessions:

| Track | What is missing |
|---|---|
| **A — Aggregates** | Executor support for `GROUP BY`, `HAVING`, `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `SAMPLE`, `GROUP_CONCAT` |
| **B — Extended property paths** | Parser + executor support for `*`, `+`, `?`, `\|`, `^`, `!` path operators |

The AST (`ast.rs`) and GROUP BY / HAVING parser already exist. Track A is a pure executor change. Track B requires new AST nodes, parser rules, and executor evaluation.

---

## Existing test coverage

| Location | What it covers | State |
|---|---|---|
| `tests/api_integration.rs::sparql_aggregate_sum_group_by` | `SUM` + `GROUP BY` | `#[ignore]` |
| `tests/w3c_sparql11_suite.rs::w3c_sparql11_aggregates` | W3C aggregate conformance suite | `#[ignore]` |
| `tests/w3c_sparql11_suite.rs::w3c_sparql11_grouping` | W3C grouping conformance suite | `#[ignore]` |
| `tests/w3c_sparql11_suite.rs::w3c_sparql11_property_path` | W3C property-path conformance suite | `#[ignore]` |
| `tests/sparql12_suite.rs::spec_s9_*` | `/` sequence paths | **green** |

New tests added in this session live in `tests/sparql12_suite.rs` (aggregate section §11, path section §9 extension), using the same helpers (`load`, `query_rows`, `query_values`).

---

## Track A — SPARQL Aggregates

### What already exists

- `ast::Aggregate` enum: `Count`, `Sum`, `Avg`, `Min`, `Max`, `Sample`, `GroupConcat`
- `ast::Expression::Aggregate(Aggregate)` variant
- `ast::ProjectionElement::Expression(Expression, String)` — alias for aggregate result
- `ast::Query::Select` has `group_by: Vec<Expression>` and `having: Vec<Expression>` fields
- Parser parses all of the above (GROUP BY / HAVING / aggregate functions)
- **Executor ignores `group_by` and `having`** (`..` in the `Select` arm of `execute`)

### What needs to be implemented

> **Correction**: The plan initially stated Track A was an executor-only change because the `Aggregate` AST enum and GROUP BY/HAVING parser already exist.  The actual first failing query confirms two parser gaps:
> (a) `SELECT (COUNT(*) AS ?n)` — projection alias `(?expr AS ?alias)` is not parsed;
> (b) `COUNT(*)` inside a function call is not parsed into `Expression::Aggregate`.
> Track A therefore spans **AST + parser + executor**.

#### Parser changes (`sparql_parser/src/lib.rs` and `ast.rs`)

1. **AST** — add `CountStar` variant to `Aggregate` enum (COUNT(*) has no sub-expression).
2. **Projection alias** — `parse_projection_element` currently only handles `?var`; extend it to also parse `(?expr AS ?alias)` → `ProjectionElement::Expression(expr, alias)`. Function needs `ctx` parameter for nested expression parsing.
3. **Aggregate functions** — modify `parse_function_call` to detect aggregate keywords (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `SAMPLE`, `GROUP_CONCAT`) and parse them into `Expression::Aggregate(...)` instead of `Expression::FunctionCall`. COUNT takes `*` or `DISTINCT? expr`; GROUP_CONCAT takes `DISTINCT? expr (; separator="sep")?`.

#### Executor changes (`sparql_parser/src/execute.rs`)

1. **Detect aggregate mode**: the query is in aggregate mode if `!group_by.is_empty()` OR any projected element contains `Expression::Aggregate(_)`.

2. **Group solutions**: partition the `Vec<PartialSub>` into groups keyed by the GROUP BY expressions evaluated over each solution. A query with no GROUP BY but an aggregate has one implicit group (all solutions).

3. **Compute aggregates per group**: for each group, evaluate each `Aggregate` variant:
   - `Count(expr, distinct)`: count non-null bindings of `expr`; `COUNT(*)` counts all rows
   - `Sum(expr, distinct)`: sum numeric values
   - `Avg(expr, distinct)`: arithmetic mean
   - `Min(expr, distinct)` / `Max(expr, distinct)`: minimum / maximum (numeric then lexicographic)
   - `Sample(expr, _)`: return any one value from the group
   - `GroupConcat(expr, sep, distinct)`: concatenate string values with separator (default `" "`)

4. **Project aggregate results**: each `ProjectionElement::Expression(Aggregate(...), alias)` produces one column in the output row, named `alias`.

5. **Apply HAVING**: filter the grouped rows using the HAVING expressions (evaluated in the aggregate context, where aggregate expressions resolve to the computed value).

6. **Order invariance**: ORDER BY is still silently ignored (separate task). Tests must assert on sets/counts, not on row order.

### New test data

Add `tests/testdata/sparql12_aggregates.ttl` — a small dataset of books and their prices across two organisations (extends the inline data already in `api_integration.rs` to a reusable file).

### New tests (all `#[ignore]` until implementation)

In `tests/sparql12_suite.rs`, new section `§11 Aggregates`:

| Test name | What it checks |
|---|---|
| `spec_s11_count_star` | `COUNT(*)` with no GROUP BY → 1 row, count = N |
| `spec_s11_count_var` | `COUNT(?x)` over a variable, skips unbound |
| `spec_s11_count_distinct` | `COUNT(DISTINCT ?creator)` deduplicated |
| `spec_s11_sum_group_by` | `SUM(?price) GROUP BY ?org` → 2 rows, correct sums |
| `spec_s11_avg` | `AVG(?price) GROUP BY ?org` |
| `spec_s11_min_max` | `MIN(?price)` and `MAX(?price)` |
| `spec_s11_having` | `GROUP BY ?org HAVING (SUM(?price) > 25)` filters groups |
| `spec_s11_group_concat` | `GROUP_CONCAT(?name ; separator=",")` |
| `spec_s11_implicit_group` | Aggregate in SELECT with no GROUP BY → one output row |

---

## Track B — Extended Property Paths

### What already exists

- Sequence `/` path: expanded at parse time into a chain of bridge-variable triple patterns (currently in `parse_property_path` + `expand_property_path_to_triples`)
- Three green tests: `spec_s9_sequence_path`, `spec_s9_three_hop_path`, `spec_s9_select_star_no_internal_path_vars`

### What needs to be implemented

#### B1 — AST (`sparql_parser/src/ast.rs`)

Add a `PropertyPath` enum (separate from `Term`):

```rust
pub enum PropertyPath {
    Iri(GraphElement),              // single IRI (leaf)
    Sequence(Vec<PropertyPath>),    // p1/p2/...
    Alternative(Box<PropertyPath>, Box<PropertyPath>),  // p1|p2
    Inverse(Box<PropertyPath>),     // ^p
    ZeroOrMore(Box<PropertyPath>),  // p*
    OneOrMore(Box<PropertyPath>),   // p+
    ZeroOrOne(Box<PropertyPath>),   // p?
    NegatedSet(Vec<GraphElement>),  // !(p1|p2|...)
}
```

Add a new `QueryComponent` variant:

```rust
QueryComponent::PathPattern(Term, PropertyPath, Term)
```

Keep `BGP` as-is — existing `/` handling migrates to `PropertyPath::Sequence` inside `PathPattern`.

#### B2 — Parser (`sparql_parser/src/lib.rs`)

Replace `parse_property_path` (returns `Vec<Term>`) with a full property path parser returning `PropertyPath`. The grammar is:

```
PathAlternative := PathSequence ( '|' PathSequence )*
PathSequence    := PathEltOrInverse ( '/' PathEltOrInverse )*
PathEltOrInverse:= PathElt | '^' PathElt
PathElt         := PathPrimary PathMod?
PathMod         := '*' | '+' | '?'
PathPrimary     := IRI | 'a' | '!' PathNegatedPropertySet | '(' PathAlternative ')'
```

`parse_triple_pattern_statement` routes to `QueryComponent::PathPattern` when the predicate is a `PropertyPath`; a plain IRI with no operators continues to go into a `TriplePattern` inside `BGP` (for zero friction with existing code paths, or fold into `PathPattern::Iri` — decide during implementation).

#### B3 — Executor (`sparql_parser/src/execute.rs`)

Add `eval_path_pattern(subject, path, object, solutions, datastore, active_graph)`:

| Operator | Strategy |
|---|---|
| `Iri(iri)` | Same as current triple-pattern eval |
| `Sequence(steps)` | Iterate: bridge variable for each step (mirrors current `__path_*` expansion, but at runtime) |
| `Alternative(l, r)` | Union of results from `l` and `r` |
| `Inverse(p)` | Swap subject/object, evaluate `p` |
| `ZeroOrOne(p)` | Union of: identity (subject = object) + one application of `p` |
| `OneOrMore(p)` | Transitive closure: BFS/DFS from subject following `p`, at least one hop |
| `ZeroOrMore(p)` | Transitive closure: like `+` but also includes zero hops (subject = object) |
| `NegatedSet(iris)` | Match any predicate NOT in the set |

**Important**: `*` and `+` require a transitive closure loop over the datastore. Use iterative BFS, not recursive evaluation of the path AST, to avoid stack overflow on large graphs.

**SELECT * variable hiding**: `PathPattern` components never expose the internal variables to `SELECT *`. The `collect_vars_from_components` function must handle `QueryComponent::PathPattern` by collecting only the `subject` and `object` terms' variables.

#### B4 — `/` migration decision

**Chosen approach**: migrate the parse-time `/` expansion into `PropertyPath::Sequence` inside `QueryComponent::PathPattern`. This removes the `__path_*` bridge variable leakage concern entirely (no bridge vars at parse time) and unifies all path handling. The three existing green `spec_s9_*` tests remain the acceptance gate — they must stay green after the migration.

### New test data

Extend `tests/testdata/sparql12_paths.ttl` (or add `sparql12_paths_extended.ttl`) with:
- A second predicate (e.g. `ex:likes`) for `|` alternative path tests
- The existing `foaf:knows` chain covers `*`, `+`, `?`, `^`

### New tests (all `#[ignore]` until implementation)

In `tests/sparql12_suite.rs`, extended section `§9 Property Paths`:

| Test name | Operator | What it checks |
|---|---|---|
| `spec_s9_alternative_path` | `\|` | `foaf:knows\|ex:likes` finds both kinds of connections |
| `spec_s9_inverse_path` | `^` | `?x ^foaf:knows ex:carol` → who carol is known by |
| `spec_s9_zero_or_more` | `*` | All reachable nodes from ex:alice via `foaf:knows*` |
| `spec_s9_one_or_more` | `+` | All nodes reachable via at least one `foaf:knows` hop |
| `spec_s9_zero_or_one` | `?` | Direct knows or self (zero-hop) |
| `spec_s9_negated_property_set` | `!` | Match triples whose predicate is NOT in a set |
| `spec_s9_inverse_sequence` | `^p1/p2` | Compose inverse with sequence |

---

## Files to create / modify

### New files
- `docs/plans/SPARQL_AGGREGATES_AND_PATHS_PLAN.md` (this file)
- `tests/testdata/sparql12_aggregates.ttl`

### Modified files

| File | Change |
|---|---|
| `tests/sparql12_suite.rs` | Add `§11 Aggregates` and `§9 extended paths` test sections (all `#[ignore]`) |
| `tests/testdata/sparql12_paths.ttl` | Add `ex:likes` triples for alternative path tests |
| `sparql_parser/src/ast.rs` | Add `PropertyPath` enum and `QueryComponent::PathPattern` (Track B session) |
| `sparql_parser/src/lib.rs` | Replace `parse_property_path` with full grammar (Track B session) |
| `sparql_parser/src/execute.rs` | Implement aggregation + path eval (Tracks A and B sessions) |
| `docs/plans/overview.md` | Update "SPARQL Aggregates" and "SPARQL Property Paths" entries |

---

## Implementation order (for later sessions)

### Track A session
1. Add `tests/testdata/sparql12_aggregates.ttl`
2. Implement aggregate grouping + evaluation in `execute.rs`
3. Un-ignore Track A tests one at a time (easiest first: `COUNT(*)` → `SUM`+`GROUP BY` → `HAVING`)
4. Un-ignore `w3c_sparql11_aggregates` and `w3c_sparql11_grouping` at the end

### Track B session
1. Add `PropertyPath` AST + `QueryComponent::PathPattern`
2. Replace parser; verify 3 existing `spec_s9_*` tests stay green
3. Implement path eval in executor (Iri, Sequence, Alternative, Inverse, ZeroOrOne first)
4. Implement transitive closure for `OneOrMore` and `ZeroOrMore`
5. Implement `NegatedSet`
6. Un-ignore Track B tests one at a time
7. Un-ignore `w3c_sparql11_property_path`

---

## Quality gates (run before each handoff)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

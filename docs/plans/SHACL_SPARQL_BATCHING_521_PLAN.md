# SHACL-SPARQL: batch `sh:sparql` SELECT-constraint evaluation (#521)

See [#521](https://github.com/daghovland/rdf-datalog/issues/521), follow-up from
[#54](https://github.com/daghovland/rdf-datalog/issues/54) /
[#518](https://github.com/daghovland/rdf-datalog/pull/518). Design rationale for
per-focus-node execution is documented in `docs/plans/SHACL_PLAN.md`'s
"SHACL-SPARQL (§5–6 of SHACL-AF)" section and in the module doc comment of
`shacl/src/sparql_constraints.rs`.

## Current behaviour

`shacl::sparql_constraints::eval_one_constraint` (called from `eval_all`, which
runs once per shape with `sh:sparql` constraints) loops over every focus node of
the shape and, **for each node**:

1. Re-parses the constraint's query text from scratch (`parse(&query_text)`,
   inside the loop).
2. Injects a single-row `VALUES ($this) { (<node>) }` at the front of the
   `WHERE` clause (`inject_this_value`).
3. Executes the query via `sparql_parser::execute::execute_with_base`.

For a SELECT constraint this means N query parses and N full BGP evaluations for
N focus nodes, even when the query has no `LIMIT`/`OFFSET`/aggregate and would
produce identical per-node results if evaluated once with all N focus nodes
`VALUES`-bound simultaneously and the resulting rows split by their `$this`
binding.

## Empirical measurement

Before implementing anything, the issue asks for a check that per-node
re-execution is actually a measurable cost, not just a theoretical one.

**Setup:** 1000 individuals (`ex:N0`..`ex:N999`), each `rdf:type ex:Thing` with
an `ex:score` integer literal (every 50th individual has a negative score, so
20 violate). One shape, `sh:targetClass ex:Thing`, with a single `sh:sparql`
`sh:select` constraint:

```sparql
SELECT $this ?value WHERE { $this <http://example.org/ns#score> ?value . FILTER (?value < 0) }
```

**Measurement A (current per-node path):** `shacl::validate(&data, &shapes)`
end-to-end (includes shape parsing, target collection, and the per-node
`sh:sparql` loop).

**Measurement B (simulated batched path):** the same query rewritten by hand
with a single 1000-row `VALUES (?this) { (ex:N0) (ex:N1) ... }` block, parsed
and executed exactly once via the public `dagalog::run_sparql_query`.

Ad hoc benchmark (temporary `#[ignore]`d test, run via
`cargo test --test shacl_suite bench_sparql_constraint_batching_521 -- --ignored --nocapture`,
not part of the final diff):

| Build     | N (focus nodes) | Per-node (A) | Batched (B) | Speedup |
|-----------|-----------------|---------------|--------------|---------|
| `debug`   | 1000            | 157.1 ms      | 11.3 ms      | **13.8x** |

(A `--release` run of the same benchmark was also kicked off, but the
throwaway `#[ignore]`d benchmark test was removed from `tests/shacl_suite.rs`
before that background build finished, so no separate release number was
captured — not needed to reach the conclusion below, since the debug-mode gap
already demonstrates the effect clearly and the underlying reason for it
holds independent of optimization level, see next paragraph.)

The per-node cost is dominated by re-parsing the same ~90-byte query string
1000 times (`nom` combinator parsing has real constant-factor overhead) plus
1000 independent BGP-evaluation setups against the datastore, each of which
re-walks the same execution machinery (dataset resolution, active-graph
selection, index lookups) for a single-row `VALUES` join. The batched path
pays that fixed setup cost exactly once. **Conclusion: for shapes with many
focus nodes and simple queries, the per-node overhead is real and worth
avoiding — the batched fast path is worth implementing.**

## Detecting the safe-to-batch case

Batching is equivalence-preserving (produces the exact same per-focus-node
result set as N separate per-node executions) **iff** none of the following
apply to the query's **top-level** `Select` clause (nested `Subquery`
components are independently scoped in SPARQL — they don't see outer `$this`
injection or interact with outer `LIMIT`/`OFFSET`/`GROUP BY`, so they are not
inspected):

1. **`LIMIT`** is present (`limit: Option<u64>` is `Some`) — a `LIMIT` applies
   to the whole result set once; batched, it would truncate across focus
   nodes instead of applying independently per node.
2. **`OFFSET`** is present (`offset: Option<u64>` is `Some`) — same reasoning.
3. **An aggregate is used without grouping by `$this`.** Precisely: let
   `has_aggregate` = true iff any `Expression::Aggregate(_)` appears (possibly
   nested inside `Binary`/`Unary`/`FunctionCall`) in any projection
   (`ProjectionElement::Expression`), any `HAVING` expression, or any `ORDER
   BY` expression. Let `grouped_by_this` = true iff `group_by` is non-empty
   and contains a `GroupCondition` whose `expr` is exactly
   `Expression::Variable("this")` (the internal, `?`-sigil name that `$this`
   is rewritten to — see `normalize_dollar_vars`). Then:
   - `group_by` empty and `has_aggregate` → **unsafe** (an aggregate with no
     `GROUP BY` is one implicit group over the *entire* result set — SPARQL
     1.1 §11.4 — so batching would aggregate across every focus node's rows
     together instead of one aggregate value per node).
   - `group_by` non-empty and **not** `grouped_by_this` → **unsafe**,
     regardless of whether an aggregate is present: grouping by anything other
     than (at least) `?this` can merge solution rows from *different* focus
     nodes into the same group, corrupting the per-node split even for a
     non-aggregating `GROUP BY` (used e.g. to deduplicate).
   - `group_by` non-empty and `grouped_by_this` → **safe**: every group
     contains solutions from exactly one focus node (possibly further split by
     additional grouping keys), so aggregates computed per group are
     equivalent to the per-node aggregate the old path would have computed.
   - `group_by` empty and `!has_aggregate` → **safe** (an ordinary
     non-aggregating query).

4. The query is an **ASK** constraint (`sh:ask`, `Query::Ask`) — out of scope
   for this issue by design (see the issue title: "batch `sh:sparql`
   SELECT-constraint evaluation"). An `ASK` query returns a single boolean
   with no `$this` binding in its result to split by, so there's no
   meaningful multi-row batching for it; it always stays per-node.

This is implemented as `fn is_batchable(query: &Query) -> bool` in
`shacl/src/sparql_constraints.rs`, pattern-matching only `Query::Select` (any
other variant — `Ask`/`Construct`/`Describe` — returns `false`).

## Batched execution design

For a batchable `Query::Select`:

1. Parse the constraint's query text **once** per constraint (not once per
   node — this alone removes the N-1 redundant parses even conceptually,
   though the real win is skipping N-1 redundant BGP-evaluation setups).
2. **Ensure `?this` is projected.** The original query may `SELECT ?value`
   without projecting `$this` at all (it doesn't need to when executed
   per-node, since the caller already knows which node it ran the query for).
   Batched execution needs `?this` in every output row to attribute it back
   to the right focus node, so: if the projection is not already `SELECT *`
   and does not already include a `this`-named column (bare `?this` or
   `(... AS ?this)`), append `ProjectionElement::Variable("this".to_string())`
   to the projection list. This is safe to do unconditionally:
   - For `SELECT DISTINCT`, adding `?this` to the *projected* columns changes
     what gets deduplicated — from "distinct across everything" to "distinct
     within `(other-columns, this)`" — but that is exactly the semantics we
     want: distinctness scoped per node, matching what N independent per-node
     `DISTINCT` queries would have produced (each dedups only its own single
     `$this`'s rows). Concatenating those N independent distinct row-sets is
     exactly equivalent to a batched `DISTINCT` over `(columns, this)`.
   - For aggregate mode with `GROUP BY ?this[, ...]`, `?this` is already
     bound as a non-aggregated variable in the group's representative
     solution, so adding it to the projection just surfaces it in the output
     row (`project_aggregate_row`'s `ProjectionElement::Variable` arm reads it
     from `rep`).
3. Build a multi-row `VALUES (?this) { (<node1>) (<node2>) ... }` block from
   *all* the shape's focus nodes and insert it as `where_clause[0]` (same
   position `inject_this_value` uses for the single-row case).
4. Execute once via `run_select`.
5. Re-split: build a `HashMap<GraphElement, GraphElementId>` from the shape's
   focus node list (`data.resources.get_graph_element(id).clone() -> id`), then
   for each returned `SolutionRow`, look up `row["this"]` in that map to
   recover the originating `GraphElementId` and build a `ValidationResult`
   exactly as `eval_one_constraint`'s existing per-node loop body does (same
   `value`/`path` extraction from the row).

Non-batchable queries (`LIMIT`/`OFFSET`/ungrouped-aggregate SELECTs, and all
`ASK` constraints) keep running the existing per-node loop unchanged — that
code path is not deleted, only reached conditionally.

**Result ordering:** the batched path's row order (and hence
`ValidationResult` order for a shape) is not guaranteed to match the
old per-node path's node-major order — the join/group evaluation order inside
`sparql_parser` is an implementation detail, not part of `$this` batching's
correctness contract. Tests compare the batched path's results as a set
(sorted by `(focus_node, value)`) rather than asserting positional order. SHACL
validation reports are conformance sets, not ordered sequences, so this is not
a spec regression.

## Test plan (TDD — written first, `#[ignore]`d)

New fixtures under `tests/testdata/`, new tests in `tests/shacl_suite.rs`:

1. `spec_s6_batched_many_focus_nodes` — a shape targeting ~200 individuals via
   `sh:targetClass`, a simple `sh:sparql sh:select` constraint (no
   `LIMIT`/`OFFSET`/aggregate) that flags every 10th individual. Asserts the
   violation set (by focus node) is exactly the expected 10%-subset —
   correctness of the new batched path with many focus nodes.
2. `spec_s6_batched_limit_falls_back` — same shape/data shape as (1) but the
   embedded query has a `LIMIT` clause. Asserts the result is identical to
   what the *unbatched* per-node evaluation would produce (every violating
   node still reported — `LIMIT` inside a per-node query bounds *that node's*
   row count, not the total across nodes) — the critical regression test that
   a wrongly-applied fast path would corrupt.
3. `spec_s6_batched_offset_falls_back` — same, with `OFFSET`.
4. `spec_s6_batched_ungrouped_aggregate_falls_back` — a `COUNT`/`SUM`
   aggregate with no `GROUP BY` at all. Falls back to per-node (each node gets
   its own aggregate value).
5. `spec_s6_batched_group_by_this_uses_batched_path` — `GROUP BY $this` with
   an aggregate (e.g. `HAVING (COUNT(?v) > 1)`), confirmed via the same
   correctness assertion as (1) — this case IS safe to batch per the design
   above, and the test exists to pin that the detection logic doesn't
   over-conservatively reject it too.

Each test loads its own small Turtle fixture pair (data + shapes), matching
the existing `spec_s6_1_sparql_*` naming/style precedent.

## Non-goals

- `sh:ask` constraint batching — not attempted (see "Detecting the safe case"
  point 4).
- `sh:target SPARQLTarget` (§5) batching — a different code path
  (`eval_sparql_target`), not touched by this issue.
- Batching across *shapes* (only within one shape's own focus-node set) — out
  of scope.

# BGP join reordering and sideways information passing — plan

## Evidence this is worth doing

From the partial DBLP benchmark run (see `docs/plans/DBLP_BENCHMARK_PLAN.md`,
`project_dblp_benchmark` memory): most of the 49 completed queries ran in
single-digit milliseconds, but a handful spiked hard —
`join-2-large-large` (20.1 s), `join-2-largest-result` (24.9 s),
`join-3-star-largest-sum-of-join-sizes` (19.1 s), `union-constraint-large-join`
(42.4 s) — all on a 14.7M-triple sample. Reading `sparql_parser/src/execute.rs`
confirms why:

- `dag_rdf`'s `QuadTable` already has real indexes (by predicate,
  subject+predicate, object+predicate, graph, exact-quad) — confirmed via
  code reading, not a guess. Point lookups for a single triple pattern are
  not the bottleneck.
- `eval_bgp` (`sparql_parser/src/execute.rs`) joins triple patterns in
  **exactly the order they appear in the query text**:

  ```rust
  fn eval_bgp(patterns: &[TriplePattern], solutions: Vec<PartialSub>, ...) -> Vec<PartialSub> {
      let mut current = solutions;
      for pattern in patterns {
          current = current.into_iter().flat_map(|sub| eval_triple_pattern(pattern, &sub, ...)).collect();
          if current.is_empty() { break; }
      }
      current
  }
  ```

  There is no cost model, no statistics, no reordering. If the first pattern
  in a query happens to be a low-selectivity one (e.g. `?x a dblp:Person`),
  every subsequent pattern is evaluated once per match — and the benchmark's
  slow queries are exactly the ones where this happens.
- `PartialSub = HashMap<String, GraphElement>` stores full `GraphElement`
  values (cloned) per binding, not interned `GraphElementId`s. Every accepted
  match in `eval_triple_pattern` does `sub.clone()` plus value clones into
  the new map. This is a constant-factor cost on top of the join-order
  problem, but it multiplies with every extra intermediate row the bad join
  order produces.

## Terminology, so the plan stays accurate

Sideways information passing (SIP) is sometimes presented as a separate
technique from join reordering; in dagalog's case the two are entangled:

- **What dagalog already does**: `ast_term_to_dag_term` resolves
  already-bound variables from `sub` before building the index lookup for
  the *next* pattern. That already is sideways information passing in the
  classic Datalog/magic-sets sense (bindings from one body atom restrict the
  lookup for the next) — it's how an indexed nested-loop join always works.
  So SIP, narrowly defined, is not missing.
- **What's actually missing** is *which order* patterns are joined in, so
  that the bound-variable filtering above kicks in as early and as
  restrictively as possible. This is the classical query-optimization
  problem (Selinger-style join ordering / SPARQL triple-pattern reordering
  by cardinality), not a new propagation mechanism.
- **A further form of SIP worth adding separately**: semi-join reduction /
  Bloom-filter pushdown across *non-adjacent* branches — e.g. `UNION` arms or
  `OPTIONAL` branches that share a variable with the outer pattern but
  aren't directly chained. Today each branch is evaluated independently and
  joined afterward; pushing a filter on the shared variable's bound values
  into the branch *before* it runs would shrink exactly the kind of
  `union-constraint-*` queries that were the single worst performer (42 s).
- **Worst-case-optimal join (WCOJ)**, e.g. Leapfrog Triejoin, is a different
  axis again: for genuinely cyclic join graphs (e.g. 3+ patterns forming a
  triangle on shared variables — common in star-shaped BGPs), *no* left-to-
  right sequence of binary joins is asymptotically optimal, regardless of
  order. This is the technique RDFox and QLever use for their best
  star-query numbers, and is the only item here that requires a genuinely
  different evaluation algorithm rather than ordering the existing one.
- **Multi-threading** (the RdfOx-patent angle the user asked about) is
  orthogonal to all of the above: it buys a constant-factor (≈ core count)
  speedup on whatever work the algorithm decides to do. Applying it before
  fixing join order would mean parallelizing a combinatorial blowup —
  real, but the wrong place to spend effort first.

Sources consulted: Sideways Information Passing for Push-Style Query
Processing (cis.upenn.edu/~zives/research/push.pdf); Magic Sets for
Disjunctive Datalog Programs (arxiv.org/pdf/1204.6346); Soufflé's
magic-set documentation (souffle-lang.github.io/magicset); SPARQL Query
Optimization Using Selectivity Estimation and OptARQ (ResearchGate); Distance-
Based Triple Reordering for SPARQL Query Optimization (IEEE); Leapfrog
Triejoin: A Simple, Worst-Case Optimal Join Algorithm (arxiv.org/abs/1210.0481);
A Worst-Case Optimal Join Algorithm for SPARQL (aidanhogan.com/docs/SPARQL_worst_case_optimal.pdf).

## Proposed phases

Ordered by ROI / implementation risk, not by dependency — each phase stands
on its own and can be merged independently.

### Phase A — Selectivity-based BGP reordering (highest ROI, lowest risk)

Reorder `patterns: &[TriplePattern]` once, before evaluation, instead of
changing how each pattern is evaluated.

**Correction from the first draft of this plan:** exact index-derived
cardinality is only available for terms that are `Term::Constant` *at plan
time*. A term that's `Term::Variable` and bound by an earlier pattern *in
the same ordering pass* doesn't have a known value yet — different rows in
`solutions` may bind it to different things — so there is no `.len()` to
look up for it. The cost model below only uses real index counts for
constant terms, and treats "bound by an earlier pattern" as a structural
signal (it *will* restrict the lookup at runtime, we just can't size that
restriction in advance).

- Cost model, per pattern, given the set of variables already bound by
  patterns scheduled so far:
  1. `bound_count` = number of {subject, predicate, object} terms that are
     `Term::Variable` and already in the bound set. **Constants do not
     count here** — a constant's selectivity is already measured exactly
     by the cardinality tie-break below, so counting it again in the
     primary key would double-count it and can only make the ordering
     worse, never better (see worked counterexample below — found during
     the green phase, not anticipated in the original draft). Higher
     `bound_count` means more of this pattern's runtime restriction comes
     from joins already performed, which `known_cardinality` cannot see
     since it's computed before any binding happens. This is the primary
     sort key.
  2. Tie-break with a real cardinality computed *only* from the pattern's
     constant terms, via direct `.len()` lookups on `QuadTable`'s public
     index fields (no allocation, no `.collect()`):
     `predicate_index[p].len()` (predicate only),
     `subject_predicate_index[s][p].len()` (subject+predicate),
     `object_predicate_index[o][p].len()` (object+predicate),
     `min` of the subject-predicate and object-predicate counts when all
     three are constant, sum-over-predicates-for-a-subject/object when only
     subject or only object is constant, `quad_count` when nothing is
     constant. A constant missing from `resource_map` gives cardinality 0
     (pattern can never match — scheduled first so `eval_bgp` short-
     circuits immediately, matching what `eval_triple_pattern` already does
     today). Graph (`triple_id_index`) is deliberately not folded into the
     cost model — there's no combined graph+predicate index, so it
     wouldn't be O(1), and the default-graph case (the overwhelming
     majority of queries) makes it unnecessary.

  **Worked counterexample for why constants must not count toward
  `bound_count`:** pattern X = `(const, const, var)` with
  `subject_predicate_index[s][p].len()` = 1000, vs. pattern Y =
  `(var, const, var)` with `predicate_index[q].len()` = 1. Counting
  constants would give X a `bound_count` of 2 vs. Y's 1, scheduling X
  first — even though X produces 1000x more intermediate rows than Y. The
  exact cardinality already available for both makes the constant-count
  signal not just redundant but actively wrong whenever it disagrees with
  cardinality.
- Ordering algorithm: greedy, not exact DP. Seed the "bound" set with
  `already_bound`. At each step, pick the not-yet-scheduled pattern with
  the best `(bound_count desc, tie-break cardinality asc)` key, then add
  its variables to "bound" before the next step. There is no separate
  connectedness filter to implement — it falls out of the corrected cost
  model for free: a pattern sharing no bound variable with anything
  scheduled so far has `bound_count == 0` (constants no longer
  contribute), while a pattern sharing at least one bound variable has
  `bound_count >= 1`. Ranking by `bound_count` descending therefore always
  prefers a connected candidate over a disconnected one whenever both
  exist, with no extra bookkeeping. This is the standard "selectivity +
  connectedness" heuristic from the SPARQL/Datalog reordering literature
  (OptARQ, distance-based triple reordering). Exact DP was the original
  idea but doesn't fit cleanly once cardinalities for bound-but-unknown-
  value variables are admitted to be unknown rather than exact — revisit
  DP only if the greedy heuristic measurably underperforms on the
  benchmark.
- Where this plugs in: a new `sparql_parser::join_ordering` module, with
  `fn order_patterns(patterns: &[TriplePattern], already_bound: &HashSet<String>, datastore: &Datastore) -> Vec<usize>`
  (returns a permutation of pattern indices), called from `eval_bgp` before
  its join loop. `already_bound` is derived from the incoming `solutions`
  (the variables already present in a representative row), since the same
  BGP can be invoked with different bound prefixes when nested inside
  OPTIONAL/UNION — reordering happens per `eval_bgp` call, not statically
  per query.

### Phase B — Shrink `PartialSub` (memory + constant-factor speed)

- Replace `HashMap<String, GraphElement>` with `HashMap<String, GraphElementId>`
  (or, more aggressively, resolve variable names to small integer slot
  indices once per query at parse time and use `Vec<Option<GraphElementId>>`
  for the hot path — avoids per-binding string hashing entirely). This
  removes the repeated `GraphElement` clone/allocation per accepted match
  and shrinks every intermediate solution set proportionally.
- `GraphElementId` is already `u32` (confirmed in `dag_rdf`), so this is a
  pure win with no semantic change — resolution back to `GraphElement` only
  needs to happen once, when producing final query results.
- This phase is independent of Phase A and can land first or in parallel;
  it reduces the constant factor on every query, including the ones Phase A
  doesn't change the order of.

### Phase C — Semi-join pushdown across UNION/OPTIONAL branches

- For `UNION` arms and `OPTIONAL` branches that share a variable with the
  current solutions but are evaluated independently today, compute the set
  of bound values for the shared variable from the outer solutions first,
  and filter the branch's own pattern evaluation against that set before
  the branches are combined — a semi-join reduction, the same class of
  technique as Bloom-join pushdown in the SIP literature.
- Most directly targets `union-constraint-large-join` (42.4 s, the single
  worst query observed) and the `optional-join-*`/`exists-join-*` family.
- Higher implementation risk than A/B: needs care with NULL/unbound
  semantics in OPTIONAL (a semi-join filter must not accidentally drop rows
  that SPARQL's OPTIONAL semantics require to be kept unbound).

### Phase D — Worst-case-optimal join for cyclic star BGPs (exploratory)

- Detect when a BGP's join graph is cyclic (e.g. 3+ patterns mutually
  joined on shared variables, the classic star/triangle shape) and route
  those through a Leapfrog-Triejoin-style multi-way merge instead of a
  sequence of binary joins. Binary joins, in any order, are provably
  suboptimal on cyclic queries (the source of the "worst-case optimal join"
  literature); Phase A's DP search picks the *best available* binary order
  but cannot escape that ceiling.
- Significant new code: needs sorted/seekable iterators over each index
  (dagalog's `Vec<QuadListIndex>` per index entry would need to support
  ordered seeking, not just iteration) and a merge-join driver.
- Treat as a follow-up investigation after A–C land and are measured against
  the benchmark, not a near-term commitment — worthwhile only if star-shaped
  queries remain the dominant cost after the cheaper fixes.

### Phase E — Multi-threading (last, not first)

- Once join order and representation are fixed, the remaining work per
  query is "evaluate N independent index probes / filter N independent
  rows" — both embarrassingly parallel. A `rayon`-based parallel
  `flat_map`/`filter` over the solutions vector in `eval_bgp` and
  `eval_components` would be a small, low-risk change at that point.
  Note: dag_rdf's indexes are read-only during query execution (the
  `Arc<RwLock<Datastore>>` read lock in `sparql_endpoint` is held for the
  whole query), so no new concurrent-write design is needed for
  read-side parallelism — this is much simpler than RDFox's patented
  concurrent *materialization* (parallel rule firing with concurrent writes
  to the store), which only matters for the `datalog` reasoner crate, not
  the SPARQL executor.
- Recommendation: do not pursue multi-threading before A and B are merged
  and re-measured against the benchmark. A 4-8x constant-factor speedup on
  a query that's currently doing 1000x too much work is far less valuable
  than fixing the 1000x.

## Suggested order for upcoming TDD sessions

1. **Phase B** first — smallest, purely internal change (no observable
   behavior change, just representation), easy to verify via existing
   `sparql12_suite.rs`/`api_integration.rs` tests passing unchanged, and it
   shrinks every later measurement's constant factor so Phase A's wins are
   easier to see clearly.
2. **Phase A** — the main fix. Needs new unit tests for the ordering
   function itself (given a BGP and known index cardinalities, does it pick
   the cheapest order?) plus a regression check that previously-slow DBLP
   benchmark queries (`join-2-large-large`, `union-constraint-large-join`,
   etc.) get measurably faster on the existing `tests/dblp_benchmark.rs`
   diagnostic.
3. **Phase C** — once A is in and measured, revisit whether
   `union-constraint-*`/`optional-join-*` are still outliers.
4. **Phase D** and **Phase E** — re-evaluate based on what the benchmark
   shows after 1–3; not committed yet.

## Testing approach (per project TDD rules)

Each phase gets its own red→green cycle:

- Phase B: unit tests on `PartialSub` construction/lookup behavior (can
  reuse existing SPARQL suite tests as a regression net — no new test
  *behavior* to add, since this is a refactor; a couple of targeted tests
  asserting variable bindings still resolve correctly after the
  representation change are still worth adding explicitly).
- Phase A: new tests in a `join_ordering` module — given synthetic index
  statistics, assert the chosen order matches the expected cheapest-first
  sequence; plus an end-to-end test with a query written in deliberately
  bad order (most-selective pattern last) that should still execute fast
  and return correct bindings.
- Phase C/D: deferred — design their tests when those phases are actually
  scheduled, since the exact semantics (especially OPTIONAL-safety for
  Phase C) need to be nailed down first.

# VQS Productive-Extension Index — Implementation Plan

The `vqs_index` crate implements the navigation graph,
basic counts, configuration queries/index, reference configurations, cost/precision
estimators, search methods, and query-log transformation. `sparql_endpoint` exposes
`GET /vqs/productive-values?class=<IRI>&property=<IRI>`. See the per-phase notes below
for where the implementation diverged from this plan. **The frontend query builder does
not call this endpoint yet** — see `docs/plans/QUERY_BUILDER_PLAN.md`.

Based on:
> Klungre, V.N., Soylu, A., Giese, M. — "Avoiding unproductive SPARQL queries through
> optimized indices", *World Wide Web* 29:32 (2026).
> https://doi.org/10.1007/s11280-026-01419-6

The paper solves the dead-end problem in the existing OptiqueVQS-style query builder
(see `docs/plans/QUERY_BUILDER_PLAN.md`): when a user adds a filter while building a
SPARQL query, the system should immediately know which filter values would make the
query return nothing (unproductive), without having to fire an expensive live SPARQL
query after each keystroke.

The solution is a precomputed index defined by a **configuration set** W — a finite
collection of tree-shaped filterless queries. The index stores, for each configuration
query Z, the compressed answer table `ans^E(Z, D)`. During a query session, the system
intersects index lookups to narrow down suggested productive values in O(index-scan)
time instead of executing costly joins over the full dataset.

---

## Key definitions (from the paper)

| Symbol | Meaning |
|---|---|
| N | Navigation graph: finite directed labelled graph of classes, datatypes, properties |
| D | Dataset (RDF triples) |
| Z | Configuration query: simple, filterless, rooted, tree-shaped query over N |
| W | Configuration set: a set of configuration queries |
| ans^E(Z,D) | Minimal set of compressed answer functions (instances → χ, data values kept) |
| cost(W) | Σ_{Z∈W} (|V(Z)| − 1) · |ans^E(Z,D)| |
| prec(W) | Weighted-average precision over a query log L |
| S_a^W(Q, (p,t)) | Intersection of index lookups; the set of suggested productive values |

---

## Crate layout

Add one new crate at the workspace root:

```
vqs_index/          — navigation graph, config queries, index, search methods
├── src/
│   ├── lib.rs
│   ├── navigation_graph.rs   — NavGraph, NavNode, NavEdge
│   ├── config_query.rs       — ConfigQuery, ConfigSet
│   ├── index.rs              — IndexTable, build from SPARQL with nested OPTIONALs
│   ├── basic_counts.rs       — BasicCounts, Histogram (collected from Datastore)
│   ├── estimators.rs         — ans̃, ãns^P, ãns^O, ãns^E
│   ├── reference_configs.rs  — W_d, W_r, W_rd, W_l, W_ld, W_m
│   ├── search.rs             — GreedyWeight, GreedyPrecision, Exploratory, Random
│   └── query_log.rs          — QueryLog, weight aggregation, log transformation
└── tests/
    ├── estimator_tests.rs
    ├── index_tests.rs
    └── search_tests.rs
```

Dependencies: `dag_rdf`, `sparql_parser`, `turtle`, `ingress`.

---

## Phase 0 — Data structures (compile-only stubs)

**Goal:** all types exist and compile; no logic.

### 0.1 `NavGraph`

```rust
// navigation_graph.rs
pub struct NavNode { pub name: String, pub is_class: bool /* vs datatype */ }
pub struct NavEdge { pub label: String, pub src: NavNodeId, pub tgt: NavNodeId }
pub struct NavGraph { /* nodes, edges, adjacency */ }
impl NavGraph {
    pub fn from_datastore(ds: &Datastore) -> Self { todo!() }
    pub fn from_manual(...) -> Self { todo!() }
    pub fn inverse_edge(&self, e: NavEdgeId) -> Option<NavEdgeId> { todo!() }
}
```

`NavGraph::from_datastore` inspects rdf:type / rdfs:domain / rdfs:range triples to
auto-construct N; manual construction supports curated subsets like the WD navigation
graph from the paper.

### 0.2 `ConfigQuery` and `ConfigSet`

```rust
// config_query.rs
pub struct ConfigQuery {
    pub root_class: NavNodeId,
    pub tree: /* tree of NavEdgeIds */ ...,
}
impl ConfigQuery {
    pub fn root_only(class: NavNodeId) -> Self { todo!() }
    pub fn extend(&self, edge: NavEdgeId) -> Self { todo!() }
    pub fn subtrees(&self) -> Vec<ConfigQuery> { todo!() }   // all subqueries
    pub fn to_sparql_optional(&self) -> String { todo!() }   // OPTIONAL-wrapped SPARQL
}
pub type ConfigSet = Vec<ConfigQuery>;
```

### 0.3 `IndexTable`

```rust
// index.rs
pub struct IndexRow { /* one entry per non-root variable: data value or χ or ω */ }
pub struct IndexTable { pub config: ConfigQuery, pub rows: Vec<IndexRow> }
impl IndexTable {
    pub fn build(config: &ConfigQuery, ds: &Datastore) -> Self { todo!() }
    pub fn lookup(&self, filters: &QueryFilters) -> HashSet<DataValue> { todo!() }
}
```

### 0.4 `BasicCounts` and `Histogram`

```rust
pub struct BasicCounts {
    pub class_count:       HashMap<NavNodeId, u64>,   // C_c(t)
    pub edge_count:        HashMap<NavEdgeId, u64>,   // C_e(e)
    pub edge_src_count:    HashMap<NavEdgeId, u64>,   // C_es(e)
    pub edge_tgt_count:    HashMap<NavEdgeId, u64>,   // C_et(e)
}
pub type Histogram = HashMap<DataValue, f64>;   // H_e: value → frequency
pub struct NavStats { pub counts: BasicCounts, pub histograms: HashMap<NavEdgeId, Histogram> }
impl NavStats {
    pub fn compute(nav: &NavGraph, ds: &Datastore) -> Self { todo!() }
}
```

All tests in this phase are `#[ignore]` stubs that assert `todo!()` doesn't panic when
structures are constructed from test data.

---

## Phase 1 — Basic counts and histograms

**Goal:** `NavStats::compute` runs over a real Datastore and produces correct counts.

Implementation: scan the quad table once, classify each triple against N, accumulate
`C_c`, `C_e`, `C_es`, `C_et`.  For histograms, scan each data-edge type and build the
frequency map H_e.

Tests (unit, small synthetic dataset):
- Verify C_c(Person) on Figure 3 dataset from paper = 6
- Verify C_e(age) = 6, C_es(age) = 6, C_et(age) = 6
- Verify H_age sums to 1.0

---

## Phase 2 — Index construction

**Goal:** `IndexTable::build` executes the OPTIONAL-wrapped SPARQL and produces the
compressed table `ans^E(Z, D)`.

Steps:
1. `ConfigQuery::to_sparql_optional` emits SPARQL with every branch wrapped in
   `OPTIONAL { }`.  This corresponds to `ans^O(Z,D)`.
2. Execute via the existing `sparql_parser::execute`.
3. Replace every instance (non-literal) value with χ; retain data values.
4. Deduplicate (subfunction removal): a row φ is removed if there exists another row φ′
   such that φ(v) = φ′(v) or φ(v) = ω for every column v (i.e., φ is a subfunction of φ′).
5. Drop the root column (always χ).
6. Cost = (|V(Z)| − 1) × |rows|.

Tests:
- Z2 example from §2.12: expect 7 rows and cost = 21 (paper Table 3).
- Empty dataset → 0 rows.
- Single-variable config → 0 cost (no non-root columns).

---

## Phase 3 — Reference configurations

**Goal:** implement all six reference configurations and compute exact cost/precision.

```rust
pub fn w_empty() -> ConfigSet { vec![] }
pub fn w_max(nav: &NavGraph, log: &QueryLog) -> ConfigSet { /* Qt for every class */ }
pub fn w_property(nav: &NavGraph) -> ConfigSet { /* one Ze per edge in N */ }
pub fn w_property_data_only(nav: &NavGraph) -> ConfigSet { /* only data edges */ }
pub fn w_local(nav: &NavGraph) -> ConfigSet { /* star-shaped per class */ }
pub fn w_local_data_only(nav: &NavGraph) -> ConfigSet { /* star-shaped, data only */ }
```

Also implement `SaW` (the value function):
```rust
pub fn productive_values(
    W: &[IndexTable],
    query: &PartialQuery,
    extension: NavEdgeId,
) -> HashSet<DataValue>
```

And exact cost/precision:
```rust
pub fn exact_cost(W: &[IndexTable]) -> u64
pub fn exact_precision(W: &[IndexTable], log: &QueryLog, ds: &Datastore) -> f64
```

Tests:
- Paper §5.2.1: W_r and W_rd have precision 0.14, W_l = 0.72, W_m = 0.89 (Lα).
- Reproduce Table 12 on paper's Wikidata setup (see §Test suites below).

---

## Phase 4 — Cost and precision estimators

**Goal:** implement the estimation algorithms from §4.4, needed by the search methods.

### 4.1 `est_ans_cardinality(Q, stats) -> f64`

Equation from §4.4.3:
```
ãns(Q,D) = C_c(TQ(root)) × Π_{e∈E(Q)} bf(TQ(e))
```
where `bf(e) = C_e(e) / C_c(src(e))`.

With filters (§4.4.4): multiply filterless estimate by Σ_{u∈FQ(v)} H_e(u) per data var.

### 4.2 `est_ans_p_cardinality(Q, v, stats) -> f64`

§4.4.5: estimate number of distinct values assigned to data variable v using equation (9):
```
ãns^P = Σ_{u∈Γv} [1 − (1 − H_e(u))^k]    where k = ãns(Q,D)
```

### 4.3 `est_ans_o_cardinality(Z, stats) -> f64`

§4.4.6: recursive expansion factor m_v over the config query tree (equation 10).

### 4.4 `est_ans_e_cardinality(Z, stats) -> f64`

§4.4.7: n = d_vr − 1, then `ñ = n − n(1 − 1/n)^k` where k = ãns^O.

### 4.5 `est_cost(W, stats) -> f64` and `est_precision(W, log, stats) -> f64`

Use the above estimates in equations (8) and (5).

Tests:
- On the paper's Figure 3 / Figure 8 example, estimated cost and precision should be
  within 2× of exact (consistent with paper §5.4: "estimated cost off by factor of 2,
  precision estimate < 7% off").

---

## Phase 5 — Search methods

**Goal:** implement all four heuristics from §4.3.

### 5.1 `get_successors(W, nav, log) -> Vec<ConfigSet>`

Returns all config sets reachable by adding exactly one variable to any query in W,
limited to what appears in the query log.

### 5.2 `GreedyQueryWeight`

Algorithm 1:
- Start with one single-variable config per class.
- Sort successors by property frequency in L; add in that order.
- Never actually evaluates cost or precision; uses log frequencies only.
- Runs in seconds.

### 5.3 `Random`

Algorithm 2: like GreedyQueryWeight but random order. Baseline only.

### 5.4 `GreedyPrecision`

Algorithm 3:
- Start with W = ∅.
- At each step, compute `est_precision` for all successors; pick the best.
- Stop when precision hasn't improved for N iterations (or at cost threshold).

### 5.5 `Exploratory`

Algorithm 4 (MCTS-inspired):
- At each step, for each direct successor W′, sample 10 random further successors,
  compute `est_precision` for each, and score W′ by the maximum found.
- Pick the W′ with the highest score.
- Much slower but finds configurations with better long-term precision.

### 5.6 Hybrid

Run all four; take the Pareto-optimal frontier from the union of results.

All search methods accept an optional `max_cost: Option<f64>` threshold.

Tests:
- GreedyQueryWeight on small synthetic nav graph reaches expected final precision
  faster than Random.
- GreedyPrecision finds higher precision than GreedyQueryWeight on Lα.
- Exploratory (limited to 20 iterations) beats GreedyPrecision on Lα.

---

## Phase 6 — Query log processing

**Goal:** transform a raw SPARQL query log into the typed, tree-shaped form the
algorithms need.

Steps (from §5.1.3):
1. Parse SPARQL queries.
2. Filter: remove triples not conforming to N.
3. Remove LIMIT, OFFSET, ORDER BY, GROUP BY, DISTINCT, REDUCE (keep weight).
4. Merge equal queries; sum weights.
5. Split UNION queries; distribute weight.
6. Remove empty, disconnected, or cyclic queries.
7. Weight each query by frequency; normalise.

Output: `QueryLog { queries: Vec<(f64, TypedQuery)> }` where `TypedQuery` is a
tree-shaped pattern conforming to N.

Tests:
- A synthetic log of 10 queries, including one UNION and one cyclic query: verify
  correct counts after transformation.

**Implemented as:** `vqs_index/src/query_log.rs`, `transform_query_log(raw, nav) -> QueryLog`.
Reuses `search.rs`'s existing `QueryLog = Vec<(f64, Vec<NavEdgeId>)>` (root→leaf edge paths)
rather than introducing a separate `TypedQuery` tree type, since nothing downstream consumes
a richer structure. One extension case is emitted per data-edge leaf in each query's
variable tree. Known simplifications: `FILTER`/`BIND`/`VALUES`/`MINUS`/`GRAPH`/`SERVICE`/
subqueries contribute no triples (only `BGP`/`OPTIONAL` bodies are scanned); ambiguous
predicate→edge matches (same property IRI on multiple domain classes) resolve to the first
match in `NavGraph::edges()` order, with no type-aware disambiguation. 9 tests, all green.

---

## Phase 7 — Integration with the query builder

**Goal:** the SPARQL endpoint uses a precomputed index at startup to power the
"productive values" endpoint used by the query builder UI.

New REST endpoint:
```
POST /vqs/productive-values
Body: { "query": <partial SPARQL>, "extension_property": "wdt:P31", "extension_type": "wd:Q5" }
Response: { "values": ["Q5", "Q6", ...] }
```

The endpoint:
1. Loads the precomputed `ConfigSet` and `IndexTable`s (built offline and persisted).
2. Prunes the incoming partial query to the largest covered subtree of each config query.
3. Intersects the index lookups (equation 7).
4. Returns the productive values in < 100 ms.

Index persistence: serialize `IndexTable`s to a binary file (e.g. using `bincode` or
`postcard`) so they don't need to be rebuilt on each server restart.

**Implemented as:** `GET /vqs/productive-values?class=<IRI>&property=<IRI>` in
`sparql_endpoint/src/vqs_routes.rs` — a `GET` with query params instead of the `POST`
with a partial-query body sketched above, since the current frontend has no caller yet
and a simple class/property lookup against the **Wld** reference configuration
(`ConfigSet::w_local_data_only`) covers today's use case. No offline persistence: the
`NavGraph` + `ConfigSet` are rebuilt in-memory and cached per dataset, keyed on the
`Datastore` generation counter (already used for HTTP ETags), invalidating automatically
on writes. Revisit persistence/`bincode` if rebuild cost becomes a problem at larger
dataset sizes.

---

## Test suites from the paper — can they be used?

### What the paper provides (all public)

| Asset | Location | Usable? |
|---|---|---|
| Reference implementation (Java, Jena) | https://gitlab.com/vidarkl/optiquevqs-index-generator | Study only (Java) |
| Navigation graph library | https://gitlab.com/vidarkl/optiquevqs-graph | Study only (Java) |
| WD navigation graph (15 classes, 107 props) | https://gitlab.com/vidarkl/wikidata-assets | **Yes** — load as nav graph fixture |
| WD SPARQL query log (2017–2018, 3.5M queries) | https://iccl.inf.tu-dresden.de/web/Wikidata_SPARQL_Logs/en | **Yes** — use as Lα/Lβ |
| WD dataset | Wikidata dump (2015 version is no longer the default) | Partially — see below |

### Using the WD navigation graph

The navigation graph (15 classes, 5 datatypes, 107 properties) is the key fixture.
Check it out from `gitlab.com/vidarkl/wikidata-assets` and load it as a reference
`NavGraph` instance in integration tests.  This gives us concrete expected values
from Tables 6–13 in the paper to verify our implementations.

### Using the WD query log

Download the 2017-2018 query log (~3.5 M queries, compressed) from TU Dresden.
Filter it using the WD navigation graph → produce Lα and Lβ as in §5.1.3.
This gives us realistic query-log fixtures for testing all four search methods.

Note: the paper used Wikidata 2015 data.  Our Wikidata sample (`wikidata-sample.nt`,
1M lines from the current truthy dump) does not match the 2015 version exactly.  For
tests that only verify algorithmic correctness (not reproduce paper numbers exactly),
the current dump is fine.  To reproduce paper numbers (Tables 12–13), either:
- Obtain the 2015 Wikidata dump (available via Wikimedia archives), or
- Accept that our precision/cost numbers will differ and test only structural properties
  (e.g. GreedyPrecision ≥ GreedyQueryWeight on large queries).

### Recommendation: two test tiers

1. **Unit / synthetic tests** — small hand-crafted datasets (Figures 3, 5 from paper),
   all assertions exact.  No download required.  Run in `cargo test`.

2. **Performance / realism tests** — use the downloaded `wikidata-sample.nt` +
   WD navigation graph + WD query log.  Marked `#[ignore]`, run manually.
   Add to `download_test_ontologies.sh` with new download steps for the nav graph
   and a representative sample of the query log.

---

## Work order

1. **Phase 0** — stubs + compilation (tests `#[ignore]`)
2. **Phase 1** — basic counts (fast, unit-testable)
3. **Phase 2** — index construction (validates against paper §2.12)
4. **Phase 3** — reference configurations
5. **Phase 4** — estimators (validates against §5.4 ±2× rule)
6. **Phase 5** — search methods (easiest first: Random → GreedyWeight → GreedyPrecision → Exploratory)
7. **Phase 6** — query log processing
8. **Phase 7** — endpoint integration

Phases 0–4 are prerequisites for Phases 5–7 and should be completed before any search
work starts.

---

## Open questions

- **NavGraph derivation**: do we derive N automatically from OWL axioms
  (domain/range) or require manual curation?  Auto-derivation is possible using the
  existing `rdf_owl_translator`, but curated subsets (like the paper's 15-class WD graph)
  give better precision in practice.

- **Persistence format**: `bincode` is idiomatic Rust; `postcard` is no-std friendly.
  Decision deferred to Phase 7.

- **Index update strategy**: the paper treats index construction as an offline batch job.
  Incremental maintenance is future work in the paper and out of scope here too.

- **Non-simple query handling**: the paper notes non-simple queries (repeated outgoing
  properties from same variable) can reduce precision.  Handle by pruning duplicate
  branches during query-log transformation.

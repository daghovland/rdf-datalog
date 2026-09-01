# SHACL: cache compound `sh:path` extension per validation run (#558)

See [#558](https://github.com/daghovland/rdf-datalog/issues/558), found during
the same performance-gap sweep that produced
[#533](https://github.com/daghovland/rdf-datalog/issues/533)/[#534](https://github.com/daghovland/rdf-datalog/issues/534)/[#535](https://github.com/daghovland/rdf-datalog/issues/535).
Design rationale for the two-consumer split (`translate.rs` vs `evaluate.rs`)
of `shacl/src/path.rs` is documented in that file's module doc and in
[`docs/plans/SHACL_COMPLEX_PATHS_PLAN.md`](SHACL_COMPLEX_PATHS_PLAN.md).

## Scope check: does this matter for real shapes?

`shacl/src/path.rs::values_from` has two branches:

- `ShPath::Predicate(iri)` — indexed lookup via
  `Datastore::get_triples_with_subject_predicate`. Already O(matches), fine.
- Every compound variant (`Sequence`, `Alternative`, `Inverse`, `ZeroOrOne`,
  `OneOrMore`, `ZeroOrMore`) — falls through to `pairs(data, path)`, which
  recomputes the path's **entire** `(subject, object)` extension over `data`
  from scratch, then filters down to the one focus node's pairs.

`shacl/src/translate.rs` (Phase 1, Datalog rule generation) has its own,
separate route: `resolve_one_path` (called once per property shape, not once
per focus node — `shacl/src/translate.rs:81`) materializes a compound path's
extension into `work` as ground triples under a synthetic predicate, so every
rule-generation code path downstream only ever sees a single indexed
predicate. **This does not help `evaluate.rs`.** `evaluate.rs`'s Phase 2
constraint evaluation (datatype/nodeKind/range/string/`sh:node`/
`sh:qualifiedValueShape`/etc.) deliberately runs against the **original**
`data` store, before Datalog materialization (see `evaluate.rs`'s module
doc — this is so synthetic helper predicates in `work` never leak into
constraint checks). `data` never gets the synthetic-predicate treatment;
`values_for` (`evaluate.rs:1406`) always calls `path::values_from` against
`data` directly, at 8 call sites, all inside per-focus-node loops (verified:
`eval_prop_constraint`, `eval_prop_combinators`×4, `eval_node_shape`,
`eval_qualified_value`, `prop_combinators_conform`, `constraint_conforms`).

So: **the finding is real, not just theoretical.** Confirmed by grep — 13
W3C SHACL test suite fixtures under `tests/testdata/w3c_shacl` use
`sh:alternativePath`/`sh:zeroOrMorePath`/`sh:oneOrMorePath`/
`sh:zeroOrOnePath`/`sh:inversePath`, and any of those combined with more than
one focus node already re-walks the whole graph once per node in Phase 2.

## Why a per-validation-run cache is safe here

`shacl::validate` (`shacl/src/lib.rs:213`) is the single entry point;
`pre_compute_violations` → `evaluate::eval_all` is the only call chain that
reaches `values_for`. The relevant safety argument is not "nothing runs
concurrently" (thread-local reasoning), it's specifically about the two
stores in play:

- `data: &Datastore` is `&`-borrowed (never `&mut`) for the entire
  `eval_all` call and everything under it. Nothing in this call graph
  mutates `data`, so a cache keyed off `data`'s content, live only for the
  duration of one such call, cannot see it change mid-run.
- `work: &mut Datastore` **is** mutated throughout (`add_viol` adds
  violation triples), but no `values_for`/`path::values_from` call site ever
  reads a path's extension from `work` — every one reads from `data`. So the
  one store that does change during a run is never a caching input.

Because of this, a cache object created once at the top of `validate`
(scoped to that one call, dropped at the end, never a `static`/
`thread_local!`) and passed down by reference is sound: it can never observe
a stale-vs-fresh mismatch, and — unlike a `thread_local!` — it can never
leak a cache entry keyed by one `Datastore`'s `GraphElementId` space into a
call against a *different* `Datastore` (this codebase interns per-store, so
a `GraphElementId` is only meaningful relative to the store it came from;
that hazard is real for a thread-local since `cargo test` runs many
`validate()` calls, each with its own `Datastore`, on the same worker
threads).

## Cache key correctness: is `ShPath`'s `Eq` safe to key on?

`ShPath` already derives `Hash`/`Eq` (`path.rs:56`). Its `Eq` is fully
structural (`Predicate(String)`, and nesting of the same over `Box`/`Vec`) —
no blank-node/shapes-graph identity is embedded anywhere in the type, and
`parse_path_body` builds it purely from the shape (IRI strings and list/
wrapper structure), never from `GraphElementId`s of the shapes store. So two
occurrences of, textually, `( ex:a ex:b )` parsed from *different* blank
nodes in the shapes graph (e.g. one written directly on a property shape,
another reached through an `sh:not`/`sh:and`/`sh:or` inner shape reference
that gets re-parsed via `shapes::parse_one_shape` on every
`shape_conforms_for_node` call) produce `Eq`-equal `ShPath` values and can
safely share one cache entry — which is exactly the "reused across different
property shapes that happen to reference the same compound path" case the
issue asks for.

## Design

Add `PathCache` to `shacl/src/path.rs` (it already owns `pairs`, the
function being cached):

```rust
pub struct PathCache {
    compound: RefCell<HashMap<ShPath, Rc<HashMap<GraphElementId, Vec<GraphElementId>>>>>,
    hits: Cell<usize>,
    misses: Cell<usize>,
}
```

- Keyed on the full `ShPath` (only compound paths ever get inserted — a
  `Predicate` never reaches this cache, it stays on the already-indexed
  fast path in `values_from`, unchanged).
- Value is a **per-subject index** (`HashMap<subject, Vec<object>>`), not
  the flat `HashSet<(subject, object)>` `pairs` returns — caching the flat
  pair set would still leave a `.filter(|(s,_)| *s == node)` linear scan per
  focus node (O(extension) per node instead of O(1) amortized dict lookup),
  which only halves the problem instead of fixing it.
- Value is wrapped in `Rc` so a lookup can clone the `Rc` and drop the
  `RefCell` borrow immediately, before the caller iterates — needed because
  shape conformance checking recurses (`shape_conforms_for_node` calls
  itself, and can be reached from inside another `values_for` loop's
  per-value processing via `sh:node`/`sh:qualifiedValueShape`/inner
  `sh:not`/`sh:and`/`sh:or`), so a long-lived `Ref`/`RefMut` borrow across a
  nested lookup would panic at runtime (`RefCell`'s dynamic borrow check).
- `hits`/`misses` counters (`Cell<usize>`, plain `pub fn` accessors — not
  `#[cfg(test)]`-gated, since the reuse-proof test lives in the top-level
  `tests/` integration-test crate, which builds `shacl` as a normal
  dependency and would not see `cfg(test)` items from it) exist purely to
  let a test prove reuse without instrumenting `pairs` itself, mirroring how
  `datalog::IncrementalReasoner` tests assert `fallback_count`.

`path::values_from` gains a `cache: &PathCache` parameter; the compound
branch calls `cache.index_for(data, path)` instead of `pairs(data,
path).into_iter().filter(...)`.

`evaluate.rs`'s `values_for` and the eight functions on the call path from
`eval_all` down to its 8 call sites gain the same `cache: &path::PathCache`
parameter: `eval_prop_constraint`, `eval_prop_combinators`, `eval_xone`
(does not call `values_for` itself but is a sibling in the same call
group — checked; left unchanged, no path calls in it),
`eval_node_shape`, `eval_qualified_value`, `shape_conforms_for_node`,
`prop_combinators_conform`, `constraint_conforms`. The other ~20
`data: &Datastore`-taking helpers in `evaluate.rs` (`has_datatype`,
`lexical_form`, `matches_node_kind`, `lit_comparable`, `sparql_compare`,
…) are leaf value-testing primitives that never see a `ShPath` and are left
untouched — no wrapper-context refactor, to keep the diff readable as "a
cache was threaded through the path-evaluation call chain," not "evaluate.rs
was restructured."

`shacl::validate` creates one `PathCache::new()` right before
`pre_compute_violations` and passes it through to `evaluate::eval_all`.

## Testing

- Reuse-proof test (`tests/shacl_suite.rs` or a small dedicated test): a
  shape with a compound path (e.g. `sh:alternativePath` or a two-step
  sequence) and several focus nodes; call `shacl::validate` (or, if finer
  control is needed, exercise `path::values_from`/`PathCache` more directly)
  and assert `misses` stays at 1 for that path while `hits` grows with the
  additional focus nodes.
- Correctness: `cargo test --test shacl_suite` and
  `cargo test --test w3c_shacl_suite` passing unchanged is the strong
  signal — the 13 compound-path W3C fixtures already exercise every
  compound `ShPath` variant end-to-end through `shacl::validate`, so a
  caching bug (stale reuse, key collision) would show up as a regression
  there without needing a bespoke differential harness.
- Both tests start `#[ignore]`d and are un-ignored once `PathCache` exists
  and is wired through, per this repo's TDD workflow.

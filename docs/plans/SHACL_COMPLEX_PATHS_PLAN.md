# SHACL complex `sh:path` property paths

Tracking issue: [#307](https://github.com/daghovland/rdf-datalog/issues/307)
(sub-issue of the W3C SHACL suite adoption epic, [#268](https://github.com/daghovland/rdf-datalog/issues/268)).

## Problem

`shacl/src/shapes.rs`'s `sh:path` parsing only accepts a plain single
predicate IRI. SHACL's property-path expressions
(<https://www.w3.org/TR/shacl/#property-paths>) are not supported at all:
`sh:inversePath`, sequence paths (RDF list of paths), `sh:alternativePath`,
`sh:zeroOrMorePath`, `sh:oneOrMorePath`, `sh:zeroOrOnePath`, and combinations
of these ("complex"/"strange" paths in the W3C test suite). A property shape
whose `sh:path` isn't a plain IRI is silently dropped by
`parse_property_shapes` today (`graph::iri_string(shapes, path_id)?` returns
`None`).

## AST

New module `shacl/src/path.rs`, `pub enum ShPath`:

```rust
enum ShPath {
    Predicate(String),          // ex:foo
    Inverse(Box<ShPath>),       // [ sh:inversePath ex:foo ]
    Sequence(Vec<ShPath>),      // ( ex:foo ex:bar )  — RDF list
    Alternative(Vec<ShPath>),   // [ sh:alternativePath ( ex:foo ex:bar ) ]
    ZeroOrMore(Box<ShPath>),    // [ sh:zeroOrMorePath ex:foo ]
    OneOrMore(Box<ShPath>),     // [ sh:oneOrMorePath ex:foo ]
    ZeroOrOne(Box<ShPath>),     // [ sh:zeroOrOnePath ex:foo ]
}
```

This mirrors `sparql_parser::ast::PropertyPath` in shape (this codebase
already implements exactly this class of expression for SPARQL property
paths) but is a separate type: SHACL paths are parsed out of an RDF
structure (`sh:path`'s object — an IRI, or a blank node carrying one of the
above predicates, or an RDF list), not SPARQL query syntax, so the parsing
side has nothing in common with `sparql_parser`. `ShPath` has no `Repeat`/
`NegatedSet` variants — SHACL's property-path grammar doesn't have them.

`ShPath::as_simple_iri(&self) -> Option<&str>` — `Some` only for
`Predicate`. Used by `sh:closed` (only simple paths contribute to a shape's
allowed-predicates set per spec) and by reporting (see below).

## Parsing (`shapes.rs`)

`ParsedPropShape::path` changes from `String` to `ShPath`.
`path::parse_path(shapes, path_id)` recurses:
1. `path_id` is an IRI → `Predicate`.
2. Else check `sh:inversePath`/`sh:zeroOrMorePath`/`sh:oneOrMorePath`/
   `sh:zeroOrOnePath` objects → wrap the recursively-parsed inner path.
3. Else check `sh:alternativePath` (its object is itself an RDF list of
   paths) → `Alternative`.
4. Else treat `path_id` as the head of an RDF list (`graph::rdf_list`) →
   `Sequence` (a single-element list collapses to that element directly).
5. Else `None` (malformed path; property shape is skipped, matching existing
   behaviour for any other malformed shape).

A `seen: HashSet<GraphElementId>` guards recursion against a cyclic path
blank-node structure (not sanctioned by SHACL, but a shapes graph is
untrusted input) — a repeated node aborts that branch with `None` rather
than looping.

Both call sites that build a `ParsedPropShape` (`sh:property` blocks and a
`sh:PropertyShape`'s direct `sh:path`) switch from `graph::iri_string` to
`path::parse_path`.

`parse_closed`'s allowed-predicates set changes from
`props.iter().map(|p| p.path.clone())` to
`props.iter().filter_map(|p| p.path.as_simple_iri().map(str::to_string))` —
only simple-predicate property shapes contribute to `sh:closed`'s allowed
set (a property shape whose path is a compound expression isn't naming one
predicate to allow).

## Evaluation strategy: shared path-extension logic, two consumers

`translate.rs` (Datalog rule generation) and `evaluate.rs` (direct
constraint evaluation) both used to assume a path *is* a single predicate
`GraphElementId`. Rather than threading path-expression evaluation through
both independently (duplicating traversal logic and doubling the surface
for bugs), one function — `path::pairs(data, path) -> HashSet<(subject,
object)>` — computes a compound path's full extension, and each consumer
uses it the way that fits its own scope and mutability constraints:

- **`evaluate.rs`** (Phase 2, direct evaluation, read-only against the
  original data graph) calls `path::values_from(data, node, path)`, which
  re-evaluates `pairs` per focus node and filters to that node — no stored
  predicate, no mutation. This also transparently covers `sh:not`/`sh:and`/
  `sh:or` inner shapes, which `shape_conforms_for_node` re-parses ad hoc via
  `shapes::parse_one_shape` on every call (not part of the top-level
  `Vec<ParsedShape>`) — there is no stable place to cache a resolved
  predicate id against such a shape, so a design requiring pre-resolution
  breaks for them (this was tried and reverted after 38 existing tests
  started panicking — inner shapes reached `values_for` with an
  unresolved path).
- **`translate.rs`** (Phase 1, Datalog rule generation) calls
  `path::resolve_one_path(work, path, shape_idx, prop_idx)`, which
  interns a simple `Predicate(iri)` path directly (zero overhead beyond
  pre-#307 behaviour) or, for a compound path, materializes `pairs`' result
  as ground triples in `work` under a fresh synthetic predicate IRI
  (`urn:dagalog:shacl:pathext:{shape_idx}:{prop_idx}`) — every existing
  Datalog rule-generation code path then treats it exactly like a simple
  predicate. This only ever runs against `work` (Datalog's own working
  store, already how this crate encodes derived facts — see `translate.rs`'s
  module doc) for the finitely-many top-level property shapes in the parsed
  `Vec<ParsedShape>`, so there's no cross-store id-space concern and no
  need for `sh:closed` (which scans the *original* data graph, never
  `work`) to filter anything out.

Each `ParsedPropShape` still gets a `path_display: String` computed
directly at parse time (no data access needed): the IRI itself for a simple
path, or `_:path{shape_idx}_{prop_idx}` for a compound one — used wherever
a `ViolMeta`/report needs a `sh:resultPath` string, in both `translate.rs`
and `evaluate.rs`.

### Path extension (`path::pairs`)

`pairs(data: &Datastore, path: &ShPath) -> HashSet<(GraphElementId,
GraphElementId)>`, recursive:
- `Predicate`: all `(s, o)` for that predicate in the default graph.
- `Inverse`: swap `pairs(inner)`.
- `Sequence`: relational join, left to right, over each step's pairs.
- `Alternative`: union of each branch's pairs.
- `ZeroOrOne`: `pairs(inner)` plus `(n, n)` for every node `n` appearing
  anywhere in the data graph.
- `OneOrMore`/`ZeroOrMore`: BFS-per-start-node reachability closure over
  `pairs(inner)`'s adjacency, `ZeroOrMore` additionally adding `(n, n)` for
  every node.

This mirrors `sparql_parser::execute`'s `eval_path_pattern`/
`transitive_closure` semantics (same reachability definition — arbitrary
path length, not bounded repetition) rather than reinventing traversal
rules, adapted from "extend one partial solution" to "compute the full pair
set once", since SHACL path evaluation has no notion of a SPARQL solution
binding to extend.

Test-suite-sized graphs only (W3C fixtures, hand-written unit tests) — this
is not written for web-scale graphs; a future largeish-graph performance
pass is out of scope here.

## `sh:closed`

`closed_violations` (`lib.rs`) scans `data.get_triples_with_subject(node_id)`
for every predicate on a focus node. Because `path::resolve_one_path`'s
synthetic-predicate materialization only ever touches `work` (never the
original `data` that `closed_violations` scans), a compound path's
bookkeeping predicate can never show up there as a spurious "extra
property" violation — no filtering needed. `parse_closed`'s allowed-set
computation (`ShPath::as_simple_iri`, above) is the only other `sh:closed`
change.

## `sh:resultPath` reporting

`ViolMeta::path` keeps its existing `Option<String>` display-string
signature — no change to `lib.rs`'s report serialization
(`report_to_turtle`'s `turtle_term(path)` already renders anything starting
with `_:` as a blank-node term, and anything IRI-shaped as `<iri>`).
Compound paths get `prop.path_display` (`_:path{si}_{pi}`) instead of a
proper `sh:alternativePath`/RDF-list blank-node *structure* — genuinely
spec-compliant `sh:resultPath` serialization of a compound path (a nested
blank-node graph matching the shape's own `sh:path` object shape) is
deferred; `tests/w3c_shacl_suite.rs`'s comparator already special-cases this
(`result_path_is_blank` skips the exact-value comparison whenever the
*expected* report's `sh:resultPath` is itself a blank node — true for every
compound-path fixture in the W3C suite), so this is sufficient to pass the
suite without over-building. A follow-up issue is filed for full structural
`sh:resultPath` serialization if a future consumer needs it (not filed yet
as of this plan — filed once the fixture pass confirms nothing else needs
it; see PR for the actual issue number if one was opened).

## Test-by-test order (simplest to hardest, per CLAUDE.md)

1. `sh:inversePath` — pure swap, no traversal.
2. Sequence — relational join, no fixpoint.
3. `sh:alternativePath` — union, no fixpoint.
4. `sh:zeroOrOnePath` — union with per-node identity, no fixpoint.
5. `sh:oneOrMorePath` / `sh:zeroOrMorePath` — need the BFS reachability
   closure.
6. "complex"/"strange" fixtures (nested combinations, e.g.
   `rdf:type/rdfs:subClassOf*`) last, since they exercise several of the
   above together.

Hand-written unit tests live in `tests/shacl_suite.rs` (new fixture pairs
under `tests/testdata/shacl_spath_*.ttl`), added `#[ignore]`d in the first
commit and un-ignored one at a time as each path kind goes green. The 12
listed W3C fixtures are unskipped in `tests/w3c_shacl_suite.rs`'s
`w3c_shacl_core_path` test one at a time in the same order, as each is
confirmed passing.

## Outcome

All 12 previously-skipped W3C `core/path` fixtures pass; the skip list in
`w3c_shacl_core_path` is now empty. Two subtleties surfaced only once real
fixtures were un-skipped (not anticipated by the initial design above):

- **`parse_path`'s cycle guard was too aggressive.** A `seen` set that
  permanently marks every visited shapes-graph node (rather than only the
  current recursion stack) rejects a path that legitimately *reuses* one
  blank node in two positions — e.g. `sh:path ( _:pinv _:pinv )`, a
  two-step sequence repeating one `[ sh:inversePath ex:p ]` node (see
  `path-complex-002.ttl`, "Test of complex path validation results"). Fixed
  by pushing/popping `path_id` around each recursive call (mirroring
  `shapes.rs`'s existing `dfs_find_cycle` DFS-stack pattern) instead of a
  monotonic "ever seen" set — this still catches a genuine cycle (a node
  that is its own ancestor) while allowing sibling reuse.
- **Ambiguous path-node precedence.** The W3C suite's "strange path"
  fixtures deliberately attach an `sh:inversePath` triple to a blank node
  that is *also* a well-formed `rdf:first`/`rdf:rest` list — testing that a
  conformant reader picks one interpretation consistently. `parse_path`
  checks "is this an RDF list" before the special-predicate
  (`sh:inversePath`/`sh:*OrPath`) cases, matching the expected reports in
  `path-strange-{001,002}.ttl`.

No fixtures needed to be deferred; no follow-up issue was filed.

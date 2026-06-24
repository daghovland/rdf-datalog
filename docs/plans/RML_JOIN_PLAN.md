# RML Join Plan: `rml:JoinCondition` (cross-source joins)

> **Status: RED PHASE** — plan + ignored stub tests are now both in place (no
> execution logic implemented yet). `ast.rs` has the new `JoinConditionRef`
> struct and `ObjectMap.join_conditions` field; `plan.rs`'s `LogicalJoin` now
> takes `conditions: Vec<JoinCondition>` (pluralized per "Why the existing
> `plan.rs` types need one change" below); `loader.rs` parses
> `rml:joinCondition`/`rml:child`/`rml:parent`. 12 new `#[ignore]`d tests exist
> and compile clean: 6 in `rml/tests/join_end_to_end.rs` (against the
> `rmltc0009a_join`/`rmltc0009b_join` fixtures), 3 in `rml/tests/plan_tests.rs`,
> 3 in `rml/tests/loader_tests.rs`. `cargo test -p rml`, `cargo fmt --all --
> --check`, and `cargo clippy -p rml --all-targets -- -D warnings` are all
> clean. `translate.rs` and `engine.rs` are unchanged — no `LogicalPlan::Join`
> is constructed or executed yet. See "TDD phases" below for the green-phase
> order.

## Goal

Implement `rml:JoinCondition` (R2RML-inherited cross-source join) so that an
`ObjectMap` can pull its term from a *different* `TriplesMap`'s logical
source ("parent"), joined to the current row ("child") on matching field
values. Today `rml/src/loader.rs` reads `rml:parentTriplesMap` into
`ObjectMap.parent_triples_map` but the value is never consumed downstream —
`translate.rs` ignores it and `engine.rs` silently produces no triple for
that predicate-object pair. This is the most concrete, well-scoped gap
identified in a documentation audit of `docs/user/rml-mapping.md` (which
lists join conditions as "not yet implemented" alongside SQL/JDBC sources
and FunctionMap/FNML).

## Spec references

- RML 1.0 §Join — <https://www.w3.org/TR/rml/>
- R2RML §10 Join (RML inherits join semantics from R2RML's `rr:RefObjectMap`,
  `rr:joinCondition`, `rr:child`, `rr:parent`) — <https://www.w3.org/TR/r2rml/#joins>
- W3C RML test cases — <https://github.com/kg-construct/rml-test-cases>

### Provenance note on the W3C test fixtures used here

The upstream repo's dedicated join test cases are **`RMLTC0009a-CSV`** and
**`RMLTC0009b-CSV`** (confirmed by fetching `test-cases/RMLTC0009a-CSV/` and
`test-cases/RMLTC0009b-CSV/` from `kg-construct/rml-test-cases@master` and
grepping all 324 `mapping.ttl` files in that repo for `joinCondition` —
`RMLTC0008b-*` also uses `rr:RefObjectMap`/`rr:parentTriplesMap` but with an
*implicit* same-source join and no explicit `joinCondition`, so it's not used
here). Despite the "0004" pattern suggested as a starting guess in the task
description, the real join cases are numbered 0008b/0009a/0009b upstream;
0004a/0004b turned out (on inspection) to be unrelated CSV/literal-termType
cases.

**Two renaming facts to record, because they are easy to get wrong in green
phase:**

1. **Namespace translation required.** Upstream fixtures are written against
   the legacy Dimou-lab namespace (`@prefix rr: <http://www.w3.org/ns/r2rml#>`,
   `@prefix rml: <http://semweb.mmlab.be/ns/rml#>`, `@prefix ql: <...ns/ql#>`)
   using `rr:subjectMap`, `rr:predicateObjectMap`, `rr:RefObjectMap`,
   `rr:parentTriplesMap`, `rr:joinCondition`, `rr:child`, `rr:parent`. This
   crate's loader only recognises the **W3C `rml:` namespace**
   (`http://w3id.org/rml/`) for everything except `referenceFormulation`
   (where `ql:JSONPath`/`ql:XPath` are accepted as aliases — `ql:CSV` is not
   even checked, `rml:CSV` is the only literal compared). There is no `rr:`
   compatibility shim (see `RML_PLAN.md`'s "Old namespace compatibility"
   section — deferred). **All new fixtures below are therefore re-authored
   in the W3C `rml:` namespace**, translating `rr:subjectMap` →
   `rml:subjectMap`, `rr:predicateObjectMap` → `rml:predicateObjectMap`,
   `rr:predicate` → `rml:predicate`, `rr:objectMap`/`rr:template`/
   `rr:class` → `rml:objectMap`/`rml:template`/`rml:class`,
   `rr:RefObjectMap`/`rr:parentTriplesMap` → (typed implicitly, just)
   `rml:parentTriplesMap`, and `rr:joinCondition`/`rr:child`/`rr:parent` →
   `rml:joinCondition`/`rml:child`/`rml:parent`. Same translation already
   applied (for non-join features) when porting other `RMLTCxxxx` upstream
   fixtures into `rml/tests/fixtures/rmltc...` in earlier phases.

2. **Fixture directory name collision.** `rml/tests/fixtures/rmltc0009a/`
   and `rmltc0009b/` **already exist** in this repo (named-graph / JSON
   named-graph end-to-end tests from `RML_PLAN.md`/`RML_JSON_PLAN.md`,
   unrelated to upstream's numbering — this repo assigns CSV/JSON/XML
   variants of the *same* logical test number `a`/`b`/`c` suffixes, which
   happens to collide with upstream's distinct `0009a`/`0009b` join cases).
   New join fixtures use the suffixed directory names
   **`rmltc0009a_join`** and **`rmltc0009b_join`** to avoid clobbering the
   existing named-graph fixtures.

## Scope

**In scope (this plan, eventual green phase):**
- `rml:joinCondition` / `rml:child` / `rml:parent` loader parsing into the AST
- Single-column joins (one `joinCondition` block)
- Multi-column joins (repeated `rml:joinCondition` blocks, AND semantics —
  all conditions must hold for a parent row to match)
- Object term = parent TriplesMap's evaluated subject IRI (never a literal —
  R2RML/RML joins always produce object IRIs from the parent's subject map)
- Multiple matching parent rows → multiple triples (one per match)
- No match on a given child row → no triple for that predicate-object pair
  (other predicate-object pairs on the same child row are unaffected)
- `LogicalPlan::Join` construction in `translate.rs` and a hash-join
  execution path in `engine.rs`
- End-to-end fixture tests against `RMLTC0009a-CSV`/`RMLTC0009b-CSV`
  (re-authored in the W3C namespace, see provenance note above)

**Out of scope (deferred beyond this plan):**
- SQL/JDBC sources (separate gap, see `rml-mapping.md`)
- FunctionMap / FNML (separate gap)
- Joins where parent and child use *different* reference formulations
  (e.g. CSV child joined to JSON parent) — the existing `LogicalJoin` design
  supports this in principle (each side is an independent `LogicalScan`/
  `LogicalProjection`) but no fixture exercises it yet; tracked as a later
  fixture once same-source-type joins are green
- Joins on JSON/XML sources (JSONPath/XPath child or parent keys) — same
  mechanism, just untested; CSV-only for the initial green phase
- Nested-loop join fallback (`JoinAlgorithm::NestedLoop` is not even modeled
  yet — only `HashJoin` exists; nested-loop would matter for non-equality
  joins, which RML's `joinCondition` does not support, so it may never be
  needed)
- Nullable/optional joins (R2RML joins are always inner-join-like: no match
  ⇒ no triple, never a triple with a null/blank object) — this is already
  the correct default behaviour and needs no special-casing, just confirming
  via the Demi Moore fixture row (see Test plan)
- Partitioning/parallel execution across join inputs

## Why the existing `plan.rs` types need one change

`rml/src/plan.rs` already declares:

```rust
pub struct LogicalJoin {
    pub left: Box<LogicalPlan>,
    pub right: Box<LogicalPlan>,
    pub condition: JoinCondition,
    pub algorithm: JoinAlgorithm,
}

pub struct JoinCondition {
    pub left_column: String,
    pub right_column: String,
}

pub enum JoinAlgorithm {
    HashJoin,
}
```

This is **inadequate as-is**: `condition: JoinCondition` (singular) cannot
express a multi-column join, but both the RML spec and this plan's in-scope
list require it (`rml:joinCondition` may repeat, all conditions ANDed). The
fix is minimal — pluralize the field, keep the struct name:

```rust
pub struct LogicalJoin {
    pub left: Box<LogicalPlan>,
    pub right: Box<LogicalPlan>,
    pub conditions: Vec<JoinCondition>,   // was: condition: JoinCondition
    pub algorithm: JoinAlgorithm,
}
```

`JoinCondition { left_column, right_column }` itself is adequate: `left` is
the child side (the side that drives iteration — same convention as
`rr:child`), `right` is the parent side (`rr:parent`/`rml:parentTriplesMap`
target). This mapping (child → `left_column`, parent → `right_column`) must
be kept consistent through `translate.rs`: the child `TriplesMap`'s scan
becomes `LogicalJoin.left`, the parent `TriplesMap`'s scan becomes
`LogicalJoin.right`.

`JoinAlgorithm::HashJoin` is adequate for the in-scope equality-only joins —
RML's `joinCondition` is always an equi-join, so hash join is sufual and no
`NestedLoop` variant needs to be added in this phase.

This pluralization is the **only** signature change to existing code in the
red phase (everything else is additive). It is a breaking change to
`LogicalJoin`'s only current use site — there is none yet (nothing
constructs a `LogicalJoin` today), so it is safe.

## AST changes (`rml/src/ast.rs`)

Add a new struct for one `rml:joinCondition` block, and a `Vec` of them on
`ObjectMap` (replacing the implicit single-parent-map-only behaviour):

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct JoinConditionRef {
    pub child: String,
    pub parent: String,
}
```

```rust
pub struct ObjectMap {
    pub term_map: TermMap,
    pub term_type: TermType,
    pub language: Option<String>,
    pub datatype: Option<IriReference>,
    pub parent_triples_map: Option<IriReference>,
    pub join_conditions: Vec<JoinConditionRef>,   // new field
}
```

`join_conditions` is `pub` so it compiles and is inspectable from tests even
before the loader populates it (red phase: loader always sets it to
`vec![]`; parsing `rml:joinCondition`/`rml:child`/`rml:parent` quads is
green-phase work, see "TDD phases" below).

Every existing `ObjectMap { ... }` literal needs the new field added:
- `rml/src/loader.rs` — the two construction sites (`rml:object` shorthand
  and `rml:objectMap` full form, both in `extract_predicate_object_maps`)
- `rml/tests/plan_tests.rs` — the `simple_triples_map` helper and any inline
  `ObjectMap { ... }` literals in individual test bodies

## `plan.rs` changes

- Pluralize `LogicalJoin.condition` → `LogicalJoin.conditions: Vec<JoinCondition>`
  as described above.
- No other changes — `LogicalJoin`, `JoinCondition`, `JoinAlgorithm` stay
  otherwise as already sketched.

## `translate.rs` changes (deferred to green phase — not touched in red phase)

Sketch for the green phase, recorded here so the red-phase test plan targets
the right shape:

1. Build a `parent_by_id: HashMap<&IriReference, &TriplesMap>` index over
   `mapping.triples_maps` before translating any individual map (parent maps
   can be defined anywhere in the document, including after the child).
2. In `translate_triples_map`, when an `ObjectMap` has
   `parent_triples_map: Some(parent_id)`:
   - Look up the parent `TriplesMap` via `parent_by_id`.
   - Build `LogicalJoin { left: make_scan(child_tm), right: make_scan(parent_tm),
     conditions: obj_map.join_conditions.iter().map(|jc| JoinCondition {
     left_column: jc.child.clone(), right_column: jc.parent.clone() }).collect(),
     algorithm: JoinAlgorithm::HashJoin }`.
   - The projection's `Object` attribute generation logic is the **parent's
     subject map** logic (`term_map_to_logic(&parent_tm.subject_map.term_map,
     parent_tm.subject_map.term_type, None)`), not the child object map's own
     `term_map` (which for a join `ObjectMap` carries no useful
     template/reference — RML join object maps are identified solely by
     `parent_triples_map` + `join_conditions`).
   - Subject/Predicate/Graph attributes still come from the child side, as
     in the non-join case.
   - Wrap in `LogicalPlan::Projection { input: Box::new(LogicalPlan::Join(...)), attrs }`.
3. If `parent_triples_map` is absent (the common case today), behaviour is
   unchanged — `make_scan(tm)` directly, no `Join` node.

## `engine.rs` changes (deferred to green phase — not touched in red phase)

Sketch, recorded for the same reason:

1. `execute_plan` needs a new branch: `LogicalPlan::Projection(proj)` where
   `proj.input` is `LogicalPlan::Join(join)` rather than `LogicalPlan::Scan`.
2. **Critical correctness point** (the thing most likely to be implemented
   wrong if this isn't called out): the joined row is **not** a single
   merged `HashMap`/`SourceRow`. The RMLTC0009a fixture has both CSVs share
   column names (`ID`, `Name` appear in *both* `student.csv` and
   `sport.csv`). A naive merge would let the child's `ID=10` clobber the
   parent's `ID=100` before the parent's subject template `{ID}` is
   evaluated, producing the wrong IRI (`sport_10` instead of `sport_100`).
   The execution model must keep child and parent rows **separate** and
   resolve each projection attribute against the correct side explicitly:
   - `Subject`, `Predicate`, `Graph` (and the non-join `Object` case) →
     evaluate against the **child** row only, as today.
   - The join `Object` attribute (parent subject map logic) → evaluate
     against the **matched parent row** only.
   - A reasonable representation: a hash join algorithm that
     1. Materialises all parent rows into a `Vec<Box<dyn SourceRow>>` plus a
        `HashMap<Vec<String>, Vec<usize>>` keyed by the parent-side join
        column values (`right_column`s, in order), mapping to parent row
        indices.
     2. Streams child rows; for each, builds the child-side key (the
        `left_column`s, in order) and looks up matching parent row indices.
     3. For each match, evaluates the join `Object` logic against
        `parent_rows[idx]` (not the child row), and the rest of the
        projection attrs against the child row, emitting one triple per
        match.
     4. No match → no triple for that predicate-object pair (other
        predicate-object pairs for the same child TriplesMap, evaluated by
        separate `LogicalPlan::Projection` entries per the existing
        one-plan-per-predicate-object-pair design, are unaffected).
3. `RmlError` likely needs no new variant — a missing parent `TriplesMap` id
   would be a structural mapping error; whether to surface it as
   `MissingProperty` or a new `RmlError::UnknownParentTriplesMap` variant is
   an open question for green phase, not decided here.

## Test plan (fixtures + stub tests, red phase)

### Fixture: `rml/tests/fixtures/rmltc0009a_join/`

Re-authored from upstream `RMLTC0009a-CSV` in the W3C `rml:` namespace.

`student.csv`:
```csv
ID,Sport,Name
10,100,Venus Williams
20,,Demi Moore
```

`sport.csv`:
```csv
ID,Name
100,Tennis
```

`mapping.ttl` (W3C namespace; child = student map, parent = sport map):
```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix ex: <http://example.com/ontology/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [ rml:source "student.csv" ; rml:referenceFormulation rml:CSV ] ;
    rml:subjectMap [ rml:template "http://example.com/resource/student_{ID}" ] ;
    rml:predicateObjectMap [
        rml:predicate foaf:name ;
        rml:objectMap [ rml:reference "Name" ]
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:practises ;
        rml:objectMap [
            rml:parentTriplesMap <http://example.com/SportMap> ;
            rml:joinCondition [
                rml:child "Sport" ;
                rml:parent "ID"
            ]
        ]
    ] .

<http://example.com/SportMap>
    a rml:TriplesMap ;
    rml:logicalSource [ rml:source "sport.csv" ; rml:referenceFormulation rml:CSV ] ;
    rml:subjectMap [ rml:template "http://example.com/resource/sport_{ID}" ] ;
    rml:predicateObjectMap [
        rml:predicate rdfs:label ;
        rml:objectMap [ rml:reference "Name" ]
    ] .
```

Expected triples (matches upstream `output.nq` semantics, IRIs unchanged):
```turtle
<http://example.com/resource/student_10> <http://xmlns.com/foaf/0.1/name> "Venus Williams" .
<http://example.com/resource/student_20> <http://xmlns.com/foaf/0.1/name> "Demi Moore" .
<http://example.com/resource/sport_100> <http://www.w3.org/2000/01/rdf-schema#label> "Tennis" .
<http://example.com/resource/student_10> <http://example.com/ontology/practises> <http://example.com/resource/sport_100> .
```

Note the absence of a `practises` triple for `student_20` (Demi Moore has an
empty `Sport` cell — no match, no triple) while her `foaf:name` triple is
still produced. This is the key discriminating case for join correctness.

### Fixture: `rml/tests/fixtures/rmltc0009b_join/`

Re-authored from upstream `RMLTC0009b-CSV` — same join, plus named graphs on
every triple (`rml:class` and `rml:graph` shorthands added to both
TriplesMaps) to confirm join output composes correctly with the existing
named-graph feature. Same `student.csv`/`sport.csv` as above.

### Stub tests: `rml/tests/join_end_to_end.rs` (new file)

All `#[ignore]`d. Mirrors the structure of `rml/tests/xml_end_to_end.rs`
(fixture-directory helper, `intern!` macro, `apply_rml_mapping` + `contains_triple`/
`contains_quad` assertions).

```rust
#[test]
#[ignore]
fn rmltc0009a_join_matched_row_produces_object_iri_from_parent_subject() { ... }
// student_10 → practises → sport_100, where sport_100 is SportMap's
// evaluated subject (not a literal "100" or "Tennis").

#[test]
#[ignore]
fn rmltc0009a_join_unmatched_row_produces_no_join_triple() { ... }
// student_20 (empty Sport) has zero ex:practises triples.

#[test]
#[ignore]
fn rmltc0009a_join_unmatched_row_still_gets_non_join_triples() { ... }
// student_20 still gets its foaf:name "Demi Moore" triple — join failure on
// one predicate-object pair must not suppress unrelated pairs.

#[test]
#[ignore]
fn rmltc0009a_join_parent_triples_map_also_produces_its_own_triples() { ... }
// sport_100 rdfs:label "Tennis" triple exists independently of the join
// (SportMap is also a standalone TriplesMap with its own predicateObjectMap).

#[test]
#[ignore]
fn rmltc0009a_join_produces_exactly_one_join_triple_for_matched_row() { ... }
// student_10 has exactly one ex:practises triple (one matching parent row).

#[test]
#[ignore]
fn rmltc0009b_join_with_named_graphs_matched_row() { ... }
// Same join as 0009a, asserted via contains_quad against the named graph.
```

### Stub tests: `rml/tests/plan_tests.rs` (new cases, appended, `#[ignore]`d)

```rust
#[test]
#[ignore]
fn translate_object_map_with_parent_triples_map_yields_join_plan() { ... }
// Build a MappingDocument with two TriplesMaps, child ObjectMap has
// parent_triples_map + one join_conditions entry. translate() output
// contains a Projection whose input is LogicalPlan::Join, not Scan.

#[test]
#[ignore]
fn translate_join_condition_maps_child_to_left_parent_to_right() { ... }
// JoinCondition.left_column == "Sport" (child), right_column == "ID" (parent).

#[test]
#[ignore]
fn translate_multi_column_join_condition_preserves_all_conditions() { ... }
// Two join_conditions entries on the ObjectMap → LogicalJoin.conditions has
// length 2, in input order.
```

### Stub tests: `rml/tests/loader_tests.rs` (new cases, appended, `#[ignore]`d)

```rust
#[test]
#[ignore]
fn loader_parses_join_condition_child_and_parent() { ... }
// rml:joinCondition [ rml:child "Sport" ; rml:parent "ID" ] →
// ObjectMap.join_conditions == vec![JoinConditionRef { child: "Sport", parent: "ID" }]

#[test]
#[ignore]
fn loader_parses_multiple_join_conditions_as_and_semantics() { ... }
// Two rml:joinCondition blocks → join_conditions.len() == 2

#[test]
#[ignore]
fn loader_object_map_without_join_condition_has_empty_vec() { ... }
// Existing non-join ObjectMap fixtures: join_conditions == vec![] (default,
// not None — confirms the red-phase stub doesn't regress existing mappings)
```

## TDD phases (green phase — not executed now)

### Phase 1 — AST + plan type changes (red, this plan)
`JoinConditionRef`, `ObjectMap.join_conditions`, `LogicalJoin.conditions`
pluralization. All call sites updated to compile. No loader/translate/engine
behaviour changes.

### Phase 2 — Loader parses `rml:joinCondition` (green)
Unignore `loader_parses_join_condition_child_and_parent`,
`loader_parses_multiple_join_conditions_as_and_semantics`,
`loader_object_map_without_join_condition_has_empty_vec` in that order.
Implement `extract_join_conditions` in `loader.rs`, call it from
`extract_predicate_object_maps`'s `rml:objectMap` branch.

### Phase 3 — Translate builds `LogicalPlan::Join` (green)
Unignore the three `plan_tests.rs` join cases. Implement the
`parent_by_id` index + join-plan construction sketched above in
`translate.rs`.

### Phase 4 — Engine executes hash join (green)
Unignore `rmltc0009a_join_*` tests one at a time, easiest first:
1. `rmltc0009a_join_parent_triples_map_also_produces_its_own_triples` (no
   join logic needed — parent TriplesMap's own projection already works)
2. `rmltc0009a_join_unmatched_row_still_gets_non_join_triples` (confirms
   non-join predicate-object pairs on a join-bearing TriplesMap are
   unaffected by join plan construction)
3. `rmltc0009a_join_matched_row_produces_object_iri_from_parent_subject`
4. `rmltc0009a_join_produces_exactly_one_join_triple_for_matched_row`
5. `rmltc0009a_join_unmatched_row_produces_no_join_triple`

Implement the hash-join physical operator in `engine.rs` as sketched above
— parent rows materialised + indexed by join key, child rows streamed,
object logic evaluated against the matched parent row specifically (the
keep-rows-separate correctness point above).

### Phase 5 — Named graphs + multi-column joins (green)
Unignore `rmltc0009b_join_with_named_graphs_matched_row` and
`translate_multi_column_join_condition_preserves_all_conditions`. Confirm
`conditions: Vec<JoinCondition>` AND-semantics (a parent row must match on
every condition to be considered a match) and that join output respects
`rml:graph` shorthands already wired for non-join projections.

### Phase 6 — Root-crate integration test (green, optional)
`tests/rml_join_integration.rs` mirroring `tests/rml_xml_integration.rs`:
SPARQL query over join-produced triples, combined with a Turtle ontology,
OWL-RL reasoning over join-derived `rdf:type` assertions if a fixture
exercises `rml:class` on the parent map. Not required by this plan's red
phase; tracked here so the eventual green-phase session knows it's expected
before declaring the feature complete, matching the precedent set by
`tests/rml_json_integration.rs` and `tests/rml_xml_integration.rs`.

## Dependencies

None — no new crates needed. Joins are pure in-memory hash-join logic over
the existing `SourceRow`/`CsvSource` infrastructure.

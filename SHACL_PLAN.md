# SHACL Validation Plan

## Goal

Add SHACL (Shapes Constraint Language) validation to dagalog so `DagalogRecordBackend`
can satisfy `IRecordBackend.ValidateContentWithShacl` and `IRecordBackend.ValidateShacl`.

The records library uses SHACL to validate content graphs against domain shapes before
accepting or publishing a record (e.g., IMF equipment templates, P&ID schemas).

## Spec references

- SHACL Core — <https://www.w3.org/TR/shacl/>
- SHACL Advanced Features (SHACL-AF) — <https://www.w3.org/TR/shacl-af/>
- SHACL Core is sufficient for records validation; SHACL-AF §6 (SPARQL-based constraints)
  is a secondary target handled differently (see below).

---

## How the records library uses SHACL

`FusekiRecordBackend` calls:

```
POST /{dataset}/shacl?graph=<content-graph-iri>
Content-Type: text/turtle
[SHACL shapes graph in body]
→ 200 text/turtle  (SHACL validation report)
```

The response is a SHACL validation report graph. `ShaclValidationOutcome` in the records
library parses `sh:conforms`, `sh:result`, `sh:focusNode`, and `sh:resultMessage`.

---

## Implementation approach

No external processes or external Rust crates. Two complementary strategies:

### Strategy 1 — SHACL Core → Datalog (primary)

SHACL Core constraints are translated to stratified Datalog rules at validation time.
The existing `datalog` engine (stratified negation, forward-chaining materialisation)
evaluates them over the data graph. The result — which derived facts about violations are
true — is read back to build the `ValidationReport`.

This mirrors the existing `owl2rl2datalog` pipeline and makes SHACL shapes first-class
citizens of the Datalog reasoning layer.

### Strategy 2 — SHACL-SPARQL → native SPARQL (§5 / §6 of SHACL-AF)

SHACL-AF §5–6 allow shapes to embed SPARQL SELECT/ASK queries. These run directly
against the data graph using the existing `sparql-parser` engine. Each solution row of
a SELECT query becomes one `ValidationResult`. No Datalog translation is needed.

---

## Specification basis for the SHACL→Datalog translation

There is no single W3C specification for SHACL→Datalog. The translation is derived from
two sources:

1. **W3C SHACL spec §4 "potential definitions in SPARQL"** — non-normative SPARQL ASK/SELECT
   patterns that define the conformance semantics of each constraint component. These SPARQL
   patterns translate directly to Datalog rules (SPARQL body atoms → Datalog body literals;
   `FILTER` expressions → Datalog built-in predicates).

   Example — `sh:nodeKind sh:IRI` potential definition:
   ```sparql
   ASK { FILTER (isIRI($value) && $nodeKind IN (sh:IRI, sh:BlankNodeOrIRI, sh:IRIOrLiteral)) }
   ```
   Datalog translation:
   ```datalog
   sh_violation_nodeKind(focusNode, value) :-
       sh_target(focusNode),
       sh_value(focusNode, path, value),
       NOT isIRI(value).   % when required kind is sh:IRI
   ```

2. **Academic semantics** — Corman, Reutter, Savković "Semantics and Validation of
   Recursive SHACL" (ISWC 2018) provides a formal fixpoint semantics for SHACL that
   maps naturally to stratified Datalog with negation. For SHACL Core (non-recursive
   shapes), the translation is fully compositional and always produces a stratified program.

---

## Datalog translation rules for SHACL Core

### Target declarations → Datalog facts / rules

Each shape `S` with a target declaration generates rules for a `sh_target(node, S)` predicate:

| SHACL declaration | Datalog |
|---|---|
| `sh:targetNode n` | `sh_target(n, S).` (fact) |
| `sh:targetClass C` | `sh_target(?n, S) :- [?n, rdf:type, C].` |
| `sh:targetSubjectsOf p` | `sh_target(?n, S) :- [?n, p, ?_].` |
| `sh:targetObjectsOf p` | `sh_target(?n, S) :- [?_, p, ?n].` |
| Implicit (`S` is `rdfs:Class` + `sh:NodeShape`) | `sh_target(?n, S) :- [?n, rdf:type, S].` |

### Value collection → Datalog rules

For a shape `S` with `sh:property [sh:path p; ...]` a helper predicate collects path values:

```datalog
sh_value(node, S, value) :- sh_target(node, S), [node, p, value].
```

(Only simple IRI paths in Phase 1; property path sequences and inverses in Phase 2.)

### Constraint component translations

The violation predicate is `sh_violation(focusNode, shapeId, constraintName, value)`.

#### §4.1 Value Type Constraint Components

| Constraint | Datalog (schematic) |
|---|---|
| `sh:class C` on path `p` | `sh_violation(?n, S, "class", ?v) :- sh_value(?n, S, ?v), NOT [?v, rdf:type, C].` |
| `sh:datatype D` on path `p` | `sh_violation(?n, S, "datatype", ?v) :- sh_value(?n, S, ?v), NOT sh_has_datatype(?v, D).` |
| `sh:nodeKind sh:IRI` | `sh_violation(?n, S, "nodeKind", ?v) :- sh_value(?n, S, ?v), NOT isIRI(?v).` |
| `sh:nodeKind sh:Literal` | `sh_violation(?n, S, "nodeKind", ?v) :- sh_value(?n, S, ?v), NOT isLiteral(?v).` |
| `sh:nodeKind sh:BlankNode` | `sh_violation(?n, S, "nodeKind", ?v) :- sh_value(?n, S, ?v), NOT isBlankNode(?v).` |
| (mixed BlankNodeOrIRI etc.) | `sh_violation(?n, S, "nodeKind", ?v) :- sh_value(?n, S, ?v), NOT isBlankNodeOrIRI(?v).` |

`sh_has_datatype` and `isIRI` etc. are **built-in predicates** added to the Datalog engine
(see §Built-in predicate extensions below).

#### §4.2 Cardinality Constraint Components

| Constraint | Approach |
|---|---|
| `sh:minCount 1` | `sh_violation(?n, S, "minCount", sh:nil) :- sh_target(?n, S), NOT sh_has_value(?n, S).` |
| `sh:minCount N` (N > 1) | generate N-ary distinctness rules using `?v1 != ?v2 != ...` (built-in inequality), or use aggregation built-in |
| `sh:maxCount 0` | `sh_violation(?n, S, "maxCount", ?v) :- sh_value(?n, S, ?v).` |
| `sh:maxCount N` (N ≥ 1) | generate (N+1)-ary co-occurrence rules with pair-inequality |

Note: Cardinality > 1 and maxCount ≥ 1 require either a counting built-in
(`COUNT(?v) > N`) or the N-ary rules pattern. The N-ary pattern is simple for small N
(the common case); a counting built-in is added to the engine for arbitrary N.

#### §4.3 Value Range Constraint Components

All four range constraints use comparison built-ins:

| Constraint | Datalog |
|---|---|
| `sh:minInclusive L` | `sh_violation(?n, S, "minInclusive", ?v) :- sh_value(?n, S, ?v), isNumeric(?v), sh_lt(?v, L).` |
| `sh:maxInclusive H` | `sh_violation(?n, S, "maxInclusive", ?v) :- sh_value(?n, S, ?v), isNumeric(?v), sh_gt(?v, H).` |
| `sh:minExclusive L` | analogous with `sh_le(?v, L)` |
| `sh:maxExclusive H` | analogous with `sh_ge(?v, H)` |

Only comparable-typed values participate; non-numeric literals are skipped (no violation).

#### §4.4 String-based Constraint Components

| Constraint | Datalog built-in used |
|---|---|
| `sh:minLength N` | `sh_strlen(?v) < N` |
| `sh:maxLength N` | `sh_strlen(?v) > N` |
| `sh:pattern "re" flags "f"` | `NOT sh_regex(?v, "re", "f")` |
| `sh:languageIn ("en" "de")` | `NOT sh_lang_in(?v, list)` |
| `sh:uniqueLang` | counting / pair-distinctness over language tags |

#### §4.5 Property Pair Constraint Components

Property pair constraints compare the value sets of two properties on the same focus node.
They require a second `sh_value` lookup for the comparison property.

| Constraint | Semantics |
|---|---|
| `sh:equals Q` | `{values of P} = {values of Q}` — each value of P must be in Q and vice versa |
| `sh:disjoint Q` | `{values of P} ∩ {values of Q} = ∅` — no value appears in both |
| `sh:lessThan Q` | every `(p_val, q_val)` pair: `p_val < q_val` |
| `sh:lessThanOrEquals Q` | every `(p_val, q_val)` pair: `p_val ≤ q_val` |

#### §4.6 Logical Constraint Components

| Constraint | Datalog |
|---|---|
| `sh:not S2` | `sh_violation(?n, S, "not", sh:nil) :- sh_target(?n, S), NOT sh_violation_any(?n, S2).` — uses stratified negation |
| `sh:and (S1 S2 …)` | union of rules from each shape; violation if any Si violated |
| `sh:or (S1 S2 …)` | `sh_violation(?n, S, "or", sh:nil) :- sh_target(?n, S), sh_violation_any(?n, S1), sh_violation_any(?n, S2) …` — stratified conjunction of violations (violated iff *all* disjuncts fail) |
| `sh:xone (S1 S2 …)` | count of conforming shapes ≠ 1; requires counting |

#### §4.7 Shape-based Constraint Components

| Constraint | Datalog |
|---|---|
| `sh:node S2` on path `p` | treat each value `v` as a new focus node for `S2`; propagate violations |
| `sh:qualifiedValueShape S2 sh:qualifiedMinCount N` | count qualifying values; violation if count < N |

#### §4.8 Other Constraint Components

| Constraint | Datalog |
|---|---|
| `sh:closed true; sh:ignoredProperties (…)` | enumerate declared paths; `sh_violation(?n, S, "closed", ?v) :- sh_target(?n, S), [?n, ?p, ?v], NOT sh_allowed_predicate(?p, S).` |
| `sh:hasValue V` | `sh_violation(?n, S, "hasValue", sh:nil) :- sh_target(?n, S), NOT [?n, p, V].` |
| `sh:in (V1 V2 …)` | `sh_violation(?n, S, "in", ?v) :- sh_value(?n, S, ?v), NOT sh_in_list(?v, list).` |

---

## Built-in predicate extensions required in the Datalog engine

The following built-in predicates must be added to `datalog/src/datalog.rs` (or a built-ins module):

| Built-in | Meaning | Used by |
|---|---|---|
| `isIRI(x)` | `x` is an IRI resource | `sh:nodeKind sh:IRI` |
| `isLiteral(x)` | `x` is a literal | `sh:nodeKind sh:Literal` |
| `isBlankNode(x)` | `x` is a blank node | `sh:nodeKind sh:BlankNode` |
| `sh_has_datatype(x, D)` | `x` has RDF datatype `D` | `sh:datatype` |
| `sh_lang(x)` → tag | language tag of `x` | `sh:languageIn`, `sh:uniqueLang` |
| `sh_strlen(x)` → n | UTF-8 codepoint length of `x`'s lexical form | `sh:minLength`, `sh:maxLength` |
| `sh_regex(x, pattern, flags)` | regex match | `sh:pattern` |
| `sh_lt(x, y)`, `sh_le(x, y)` | numeric/date comparison | range constraints |
| `sh_gt(x, y)`, `sh_ge(x, y)` | numeric/date comparison | range constraints |
| `x != y` | inequality (already in SPARQL FILTER) | `sh:maxCount`, `sh:uniqueLang` |
| `sh_count(group, var) >= N` | aggregation | `sh:minCount N > 1`, `sh:maxCount` |

These are evaluated natively in Rust during Datalog rule evaluation; they do not add new derived facts, they act as filter guards.

---

## SHACL-SPARQL (§5–6 of SHACL-AF) via native SPARQL

For shapes that use `sh:sparql` with `sh:select` or `sh:ask`:

1. Extract the SPARQL query string from the shapes graph.
2. Pre-bind `$this` to each focus node.
3. Execute using `sparql-parser::run_sparql_query` against the data `Datastore`.
4. Each solution row of a SELECT becomes one `ValidationResult`; a false ASK becomes one result.

This requires no Datalog translation and can be implemented independently after Phase 1.

---

## New crate: `shacl`

The `shacl` crate (already created as a stub) will contain:

```
shacl/src/
├── lib.rs          — public API: validate(), report_to_turtle(), types
├── vocab.rs        — SHACL namespace constants (SH_NODE_SHAPE, SH_TARGET_CLASS, …)
├── graph.rs        — Datastore query helpers: lookup_iri(), get_objects(), rdf_list()
├── shapes.rs       — parse shapes graph into Shape structs (targets + constraint list)
├── targets.rs      — collect target nodes for each shape from the data graph
├── translate.rs    — translate Shape structs into Datalog Rule Vec<Rule>
├── evaluate.rs     — run rules, collect sh_violation facts → Vec<ValidationResult>
└── report.rs       — report_to_turtle() serialisation
```

The `translate.rs` module is the core: it walks the parsed shapes and emits `datalog::Rule`
objects using the same `Rule`, `RuleAtom`, `RuleHead` types that `owl2rl2datalog` produces.
The rules are then run through the existing `datalog::evaluate_rules` function.

---

## SHACL validation report format

The HTTP endpoint returns a SHACL report graph in Turtle. Minimum conforming output:

```turtle
@prefix sh: <http://www.w3.org/ns/shacl#> .

[] a sh:ValidationReport ;
   sh:conforms true .
```

When violations are present:

```turtle
[] a sh:ValidationReport ;
   sh:conforms false ;
   sh:result [
       a sh:ValidationResult ;
       sh:focusNode <http://example.org/node> ;
       sh:resultSeverity sh:Violation ;
       sh:resultMessage "Value does not have datatype xsd:integer" ;
       sh:sourceConstraintComponent sh:DatatypeConstraintComponent ;
       sh:sourceShape <http://example.org/shapes/MyShape> ;
       sh:resultPath ex:age ;
       sh:value "abc" ;
   ] .
```

The records library reads `sh:conforms`, `sh:result`, `sh:focusNode`, and `sh:resultMessage`.

---

## HTTP endpoint

Add `POST /{name}/shacl` matching Fuseki's interface:

```
POST /{name}/shacl?graph=<content-graph-iri>
Content-Type: text/turtle
[SHACL shapes in request body]

→ 200 text/turtle
[SHACL validation report]
```

Route in `sparql_endpoint/src/server.rs`:

```rust
.route("/{name}/shacl", post(crate::shacl::shacl_post))
```

---

## Testing strategy

Integration tests in [`tests/shacl_suite.rs`](tests/shacl_suite.rs) cover every SHACL
Core constraint component from the W3C SHACL specification §1–§4.8.
Each test has:

- A data graph in `tests/testdata/shacl_s*_data.ttl`
- A shapes graph in `tests/testdata/shacl_s*_shapes.ttl`
- An assertion on `report.conforms` and `report.results.len()`
- A doc-comment citing the exact spec section and URL

All SHACL tests are `#[ignore]` until the relevant translation slice is complete.
`shacl_testdata_parses` (never ignored) guards the TTL files in CI.

### Constraint components with tests

| Spec | Component | Test | File pair |
|---|---|---|---|
| §1.4 | Intro (multi-constraint) | `spec_s1_4_intro_person_shape_violations` | `shacl_s1_intro_{data,shapes}.ttl` |
| §2.1.3.1 | `sh:targetNode` | `spec_s2_1_1_target_node` | `shacl_s2_target_node_*.ttl` |
| §2.1.3.2 | `sh:targetClass` | `spec_s2_1_2_target_class` | `shacl_s2_target_class_*.ttl` |
| §2.1.3.3 | Implicit class target | `spec_s2_1_3_target_implicit_class` | `shacl_s2_target_implicit_*.ttl` |
| §2.1.3.4 | `sh:targetSubjectsOf` | `spec_s2_1_4_target_subjects_of` | `shacl_s2_target_subjects_*.ttl` |
| §2.1.3.5 | `sh:targetObjectsOf` | `spec_s2_1_5_target_objects_of` | `shacl_s2_target_objects_*.ttl` |
| §4.1.1 | `sh:class` | `spec_s4_1_1_class` | `shacl_s4_class_*.ttl` |
| §4.1.2 | `sh:datatype` | `spec_s4_1_2_datatype` | `shacl_s4_datatype_*.ttl` |
| §4.1.3 | `sh:nodeKind` | `spec_s4_1_3_nodekind` | `shacl_s4_nodekind_*.ttl` |
| §4.2.1 | `sh:minCount` | `spec_s4_2_1_mincount` | `shacl_s4_mincount_*.ttl` |
| §4.2.2 | `sh:maxCount` | `spec_s4_2_2_maxcount` | `shacl_s4_maxcount_*.ttl` |
| §4.3 | `sh:minInclusive`, `sh:maxInclusive` | `spec_s4_3_value_range` | `shacl_s4_range_*.ttl` |
| §4.4.1 | `sh:minLength` | `spec_s4_4_1_minlength` | `shacl_s4_minlength_*.ttl` |
| §4.4.2 | `sh:maxLength` | `spec_s4_4_2_maxlength` | `shacl_s4_maxlength_*.ttl` |
| §4.4.3 | `sh:pattern` | `spec_s4_4_3_pattern` | `shacl_s4_pattern_*.ttl` |
| §4.4.4 | `sh:languageIn` | `spec_s4_4_4_languagein` | `shacl_s4_languagein_*.ttl` |
| §4.4.5 | `sh:uniqueLang` | `spec_s4_4_5_uniquelang` | `shacl_s4_uniquelang_*.ttl` |
| §4.5.1 | `sh:equals` | `spec_s4_5_1_equals` | `shacl_s4_equals_*.ttl` |
| §4.5.2 | `sh:disjoint` | `spec_s4_5_2_disjoint` | `shacl_s4_disjoint_*.ttl` |
| §4.5.3 | `sh:lessThan` | `spec_s4_5_3_lessthan` | `shacl_s4_lessthan_*.ttl` |
| §4.5.4 | `sh:lessThanOrEquals` | `spec_s4_5_4_lessthanorequals` | `shacl_s4_lessthanorequals_*.ttl` |
| §4.6.1 | `sh:not` | `spec_s4_6_1_not` | `shacl_s4_not_*.ttl` |
| §4.6.2 | `sh:and` | `spec_s4_6_2_and` | `shacl_s4_and_*.ttl` |
| §4.6.3 | `sh:or` | `spec_s4_6_3_or` | `shacl_s4_or_*.ttl` |
| §4.6.4 | `sh:xone` | `spec_s4_6_4_xone` | `shacl_s4_xone_*.ttl` |
| §4.7.1 | `sh:node` | `spec_s4_7_1_node` | `shacl_s4_node_*.ttl` |
| §4.7.3 | `sh:qualifiedValueShape` | `spec_s4_7_3_qualified_value_shape` | `shacl_s4_qualified_*.ttl` |
| §4.8.1 | `sh:closed` | `spec_s4_8_1_closed` | `shacl_s4_closed_*.ttl` |
| §4.8.2 | `sh:hasValue` | `spec_s4_8_2_has_value` | `shacl_s4_hasvalue_*.ttl` |
| §4.8.3 | `sh:in` | `spec_s4_8_3_in` | `shacl_s4_in_*.ttl` |

### Not yet covered by tests

- §4.3.1 `sh:minExclusive`, §4.3.3 `sh:maxExclusive`
- §4.7.2 `sh:property` as a standalone property-shape reference
- §4.7.3 `sh:qualifiedMaxCount`
- §5–6 SHACL-AF SPARQL-based constraints / targets

Additional W3C conformance test suite: <https://github.com/w3c/data-shapes/tree/gh-pages/shacl/tests>

---

## Implementation phases

### Phase 1 — Spine: targets + existence/class/nodeKind + structural constraints

Implements enough to pass the majority of tests using only stratified Datalog (no new
built-ins beyond those the Datalog engine already handles):

| Step | Change | Tests unignored |
|---|---|---|
| 1a | `shacl/src/vocab.rs` — SHACL constants | — |
| 1b | `shacl/src/graph.rs` — `lookup_iri`, `get_objects`, `rdf_list` helpers | — |
| 1c | `shacl/src/shapes.rs` — parse shapes graph into `Shape` + `Constraint` structs | — |
| 1d | `shacl/src/targets.rs` — collect target nodes from data graph | §2 target tests |
| 1e | `shacl/src/translate.rs` — emit Datalog rules for `sh:minCount 1`, `sh:maxCount 0` | minCount, maxCount |
| 1f | Translate `sh:hasValue`, `sh:in`, `sh:class`, `sh:closed` | §4.1.1, §4.8.x |
| 1g | Translate `sh:not`, `sh:and`, `sh:or` (stratified negation) | §4.6.1-3 |
| 1h | Intro example end-to-end (combines above with `sh:pattern` stub) | — |

### Phase 2 — Built-in predicates: value testing and counting

Extends the Datalog engine with built-in guards for value-level checks:

| Step | Change | Tests unignored |
|---|---|---|
| 2a | `isIRI`, `isLiteral`, `isBlankNode` built-ins | `sh:nodeKind` §4.1.3 |
| 2b | `sh_has_datatype(x, D)` built-in | `sh:datatype` §4.1.2 |
| 2c | Comparison built-ins (`sh_lt`, `sh_le`, `sh_gt`, `sh_ge`) | range §4.3 |
| 2d | `sh_strlen`, `sh_regex`, `sh_lang` built-ins | string §4.4 |
| 2e | Cardinality for N > 1: pair-inequality or counting built-in | `sh:minCount N`, `sh:maxCount N` |
| 2f | Property pair built-ins (`sh:equals`, `sh:disjoint`, `sh:lessThan`) | §4.5 |
| 2g | `sh:xone` (counting conforming shapes) | §4.6.4 |
| 2h | `sh:qualifiedValueShape` + `sh:qualifiedMinCount` | §4.7.3 |

### Phase 3 — HTTP endpoint + report serialisation

| Step | Change |
|---|---|
| 3a | `shacl/src/report.rs` — `report_to_turtle()` |
| 3b | `sparql_endpoint/src/shacl.rs` — `shacl_post` handler |
| 3c | Register route `POST /{name}/shacl` in `server.rs` |
| 3d | README HTTP endpoint table update |

### Phase 4 — SHACL-SPARQL (§5–6 of SHACL-AF)

| Step | Change |
|---|---|
| 4a | Parse `sh:sparql [ sh:select "..." ]` / `sh:sparql [ sh:ask "..." ]` from shapes graph |
| 4b | Pre-bind `$this` and execute against data using `sparql-parser::run_sparql_query` |
| 4c | Map SELECT solution rows → `ValidationResult`; false ASK → `ValidationResult` |

---

## Status

| Step | Status |
|---|---|
| `shacl` crate stub + types | ✓ Done |
| Integration tests for all §1–§4.8 constraints | ✓ Done |
| README SHACL section | ✓ Done |
| SHACL→Datalog plan | ✓ Done (this document) |
| Phase 1 implementation | Planned |
| Phase 2 built-ins | Planned |
| Phase 3 HTTP endpoint | Planned |
| Phase 4 SHACL-SPARQL | Planned |

# Plan: `owl2rdf` — complex class-expression blank-node structural encoding ([#509](https://github.com/daghovland/rdf-datalog/issues/509))

Split out from [#373](https://github.com/daghovland/rdf-datalog/issues/373) item 1 (originally
deferred by [#177](https://github.com/daghovland/rdf-datalog/issues/177)/PR #370), part of the
general OWL 2 → RDF structural mapping. Implemented in `owl2rl2datalog/src/owl_to_rdf.rs`.

## Spec citation

<https://www.w3.org/TR/owl2-mapping-to-rdf/> §2.1 ("Translation of Expressions", Table 1 and
Table 12/13 depending on REC edition, "Translation of Class Expressions"). Fetched and verified
against the live page.

### Class compositions (over `rdf:List`, `T(SEQ ...)`)

* **`ObjectIntersectionOf(CE1 ... CEn)`** →
  ```
  _:x rdf:type owl:Class .
  _:x owl:intersectionOf T(SEQ CE1 ... CEn) .
  ```
* **`ObjectUnionOf(CE1 ... CEn)`** →
  ```
  _:x rdf:type owl:Class .
  _:x owl:unionOf T(SEQ CE1 ... CEn) .
  ```
* **`ObjectComplementOf(CE)`** →
  ```
  _:x rdf:type owl:Class .
  _:x owl:complementOf T(CE) .
  ```
* **`ObjectOneOf(a1 ... an)`** →
  ```
  _:x rdf:type owl:Class .
  _:x owl:oneOf T(SEQ a1 ... an) .
  ```
  (`T(SEQ ...)` here is the `rdf_list` helper over the *individuals'* resolved ids, via
  `intern_individual`, not `named_class`/general class-expression translation.)

### `owl:Restriction` blank nodes

All restrictions: `_:x rdf:type owl:Restriction`, plus `owl:onProperty T(OPE)` (or
`owl:onProperties T(SEQ DPE1 ... DPEn)` for `DataSomeValuesFrom`/`DataAllValuesFrom` with 2+ data
properties — `n == 1` still uses `owl:onProperty`), plus one of:

| Expression | Extra triples |
|---|---|
| `ObjectSomeValuesFrom(OPE CE)` | `owl:someValuesFrom T(CE)` |
| `ObjectAllValuesFrom(OPE CE)` | `owl:allValuesFrom T(CE)` |
| `ObjectHasValue(OPE a)` | `owl:hasValue T(a)` |
| `ObjectHasSelf(OPE)` | `owl:hasSelf "true"^^xsd:boolean` |
| `ObjectMinCardinality(n OPE)` | `owl:minCardinality "n"^^xsd:nonNegativeInteger` |
| `ObjectMinQualifiedCardinality(n OPE CE)` | `owl:minQualifiedCardinality "n"^^xsd:nonNegativeInteger`, `owl:onClass T(CE)` |
| `ObjectMaxCardinality(n OPE)` | `owl:maxCardinality "n"^^xsd:nonNegativeInteger` |
| `ObjectMaxQualifiedCardinality(n OPE CE)` | `owl:maxQualifiedCardinality "n"^^xsd:nonNegativeInteger`, `owl:onClass T(CE)` |
| `ObjectExactCardinality(n OPE)` | `owl:cardinality "n"^^xsd:nonNegativeInteger` |
| `ObjectExactQualifiedCardinality(n OPE CE)` | `owl:qualifiedCardinality "n"^^xsd:nonNegativeInteger`, `owl:onClass T(CE)` |
| `DataSomeValuesFrom(DPEs DR)` | `owl:someValuesFrom T(DR)` |
| `DataAllValuesFrom(DPEs DR)` | `owl:allValuesFrom T(DR)` |
| `DataHasValue(DPE lt)` | `owl:hasValue T(lt)` |
| `DataMinCardinality(n DPE)` | `owl:minCardinality "n"^^xsd:nonNegativeInteger` |
| `DataMinQualifiedCardinality(n DPE DR)` | `owl:minQualifiedCardinality "n"^^xsd:nonNegativeInteger`, `owl:onDataRange T(DR)` |
| `DataMaxCardinality(n DPE)` | `owl:maxCardinality "n"^^xsd:nonNegativeInteger` |
| `DataMaxQualifiedCardinality(n DPE DR)` | `owl:maxQualifiedCardinality "n"^^xsd:nonNegativeInteger`, `owl:onDataRange T(DR)` |
| `DataExactCardinality(n DPE)` | `owl:cardinality "n"^^xsd:nonNegativeInteger` |
| `DataExactQualifiedCardinality(n DPE DR)` | `owl:qualifiedCardinality "n"^^xsd:nonNegativeInteger`, `owl:onDataRange T(DR)` |

Cardinality literals are `xsd:nonNegativeInteger`-typed, not the codebase's default
`xsd:integer` mapping for `RdfLiteral::IntegerLiteral` — built directly as
`RdfLiteral::TypedLiteral { type_iri: XSD_NON_NEGATIVE_INTEGER, literal: n.to_string() }` from the
axiom's `BigInt`.

## Scope boundary: `DataRange`

`T(DR)` for a **named** datatype (`DataRange::NamedDataRange`) is just the datatype IRI — already
handled by the existing atomic path. A **complex** `DataRange` (`DataUnionOf`,
`DataIntersectionOf`, `DataComplementOf`, `DataOneOf`, `DatatypeRestriction`) has no RDF encoding
yet; that structural mapping is [#512](https://github.com/daghovland/rdf-datalog/issues/512)'s
scope, not this issue's. `DataSomeValuesFrom`/`DataAllValuesFrom`/cardinality-with-`DataRange`
restrictions therefore only translate when their `DataRange` is `NamedDataRange`; a complex
`DataRange` inside one of them is reported skipped (referencing #512), same as today.

## Blank-node minting

Reuse `Translator::rdf_list` (already generic over `&[GraphElementId]`, used by
`DisjointUnion`/`HasKey`) for every `T(SEQ ...)` list, and
`self.datastore.new_anonymous_blank_node()` (already used by `emit_axiom_annotations`) for each
fresh `owl:Class`/`owl:Restriction` blank node — no new minting primitive needed.

## Recursion: a general `class_expression` method

Today `Translator::named_class` returns `Option<GraphElementId>` (`Some` for
`ClassName`/`AnonymousClass`, `None` — meaning "caller must skip" — for anything complex). This PR
adds a sibling, infallible `Translator::class_expression(&ClassExpression) -> GraphElementId` that:

* delegates to `named_class` for the two atomic cases,
* recurses into itself for nested `ClassExpression` operands (union/intersection members,
  complement's operand, restriction's filler class), so `SubClassOf(A, ObjectUnionOf(B,
  ObjectIntersectionOf(C, D)))` "just works" without special-casing depth,
* returns a fresh blank node per complex case per the tables above.

`ClassAxiom::SubClassOf`/`EquivalentClasses`/`DisjointClasses`/`DisjointUnion`,
`ObjectPropertyAxiom::ObjectPropertyDomain`/`Range`, `DataPropertyAxiom::DataPropertyDomain`,
`AxiomHasKey`'s class operand, and `Assertion::ClassAssertion` switch from `named_class`
(`Option`, skip-on-`None`) to `class_expression` (infallible) wherever the *class-expression*
argument is itself the thing being resolved for RDF, **except** where the surrounding axiom's
*other* operand still has its own gap (e.g. `ObjectPropertyAssertion` on a non-atomic property
expression is unaffected by this issue — that's `ObjectPropertyExpression`, not
`ClassExpression`, and inverse/property-chain properties are #510's scope).

A separate `object_property_or_restriction`-style resolver is **not** introduced for
`ObjectPropertyExpression`/`DataRange` in this PR — those stay `named_*`/`Option`-returning as
today, `None` still means "skip, unimplemented" (inverse object properties are #510, complex
`DataRange` is #512). Restriction filler classes go through the new `class_expression`, but a
restriction's own `OPE`/`DPE`/`DR` operand does not.

## Reuse note for future data-range work (#512)

`rdf_list` and the blank-node-minting pattern used here for `ObjectUnionOf`/`ObjectIntersectionOf`
generalize directly to `DataUnionOf`/`DataIntersectionOf` (`owl:unionOf`/`owl:intersectionOf` over
an `rdf:List` of resolved `DataRange` ids) and `DatatypeRestriction` (an `owl:Restriction`-shaped
node with `owl:onDatatype`/`owl:withRestrictions`) — a future `data_range(&DataRange) ->
GraphElementId` method on `Translator` would mirror `class_expression`'s shape. No implementation
of that is included here.

## Tests (initially `#[ignore]`d, one per expression kind + one nested)

Added to `owl2rl2datalog/src/owl_to_rdf.rs`'s `tests` module, following the existing
`read_rdf_list`/`object_of`/`has_triple` helpers:

1. `object_union_of_becomes_owl_class_with_union_of_list`
2. `object_intersection_of_becomes_owl_class_with_intersection_of_list`
3. `object_complement_of_becomes_owl_class_with_complement_of`
4. `object_one_of_becomes_owl_class_with_one_of_list`
5. `object_some_values_from_becomes_owl_restriction`
6. `object_all_values_from_becomes_owl_restriction`
7. `object_has_value_becomes_owl_restriction`
8. `object_has_self_becomes_owl_restriction`
9. `object_min_max_exact_cardinality_becomes_owl_restriction` (unqualified, one test covering all
   three predicates)
10. `object_qualified_cardinality_becomes_owl_restriction_with_on_class` (qualified, one test)
11. `data_some_values_from_single_property_uses_on_property`
12. `data_some_values_from_multiple_properties_uses_on_properties_list`
13. `data_cardinality_restrictions_become_owl_restriction`
14. `data_range_restriction_with_complex_data_range_is_reported_not_silently_dropped` (documents the
    #512 boundary)
15. `nested_union_of_union_and_intersection_recurses` — `SubClassOf(A, ObjectUnionOf(B,
    ObjectIntersectionOf(C, D)))`, asserting the outer union's list has 2 elements, the second of
    which is itself an `owl:Class` blank node with its own `owl:intersectionOf` list `[C, D]`.
16. `disjoint_union_with_complex_member_is_now_translated` — flips the existing
    `disjoint_union_with_complex_member_is_reported_not_silently_dropped` test (from the earlier
    PR) into a positive assertion now that #509 is implemented; the old "must be skipped" test is
    replaced, not duplicated.
17. `has_key_on_complex_class_expression_is_now_translated` — same flip for `has_key`'s
    class-expression operand.
18. `subclassof_with_complex_class_expression_is_now_translated` — same flip for the
    `complex_expressions_are_reported_not_silently_dropped` test's `SubClassOf` case (its
    `ClassAssertion` half also flips, covered by test 19).
19. `class_assertion_on_complex_class_expression_is_now_translated` — the `ClassAssertion` half of
    the same old test.

## Implementation order

Union → intersection → complement → oneOf (compositions share `rdf_list`/blank-node shape) →
`ObjectSomeValuesFrom`/`AllValuesFrom` (simplest restrictions) → `HasValue`/`HasSelf` → unqualified
cardinalities → qualified cardinalities (`owl:onClass`) → data-property analogues
(`onProperty`/`onProperties` fork) → nested-recursion test → flip the four now-stale
"skip" tests from earlier PRs to positive assertions.

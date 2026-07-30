# Issue #266 — SHACL suspicions plan

Issue: <https://github.com/daghovland/rdf-datalog/issues/266>

## 1. `sh:languageIn` on a non-literal value node

**Spec citation** (W3C SHACL, §4.4.4, normative textual definition, and the
constraint-component summary table):

> `sh:LanguageInConstraintComponent`: For each value node that is either not a
> literal or that does not have a language tag matching any of the basic
> language ranges that are the members of `$languageIn` following the
> filtering schema defined by the SPARQL `langMatches` function, there is a
> validation result with the value node as `sh:value`.

("is either not a literal **or** ...") — a non-literal value node
unconditionally produces a validation result. There is no "out of scope"
carve-out for non-literals in this component (contrast with e.g.
`sh:minLength`/`sh:pattern`, whose SPARQL potential-definition explicitly
guards only against blank nodes, or `sh:datatype`, which is about literal
typing specifically — languageIn's own normative text is explicit: non-literal
⇒ violate).

**Conclusion: CONFIRMED BUG.** Current code (`shacl/src/evaluate.rs`,
`LanguageIn` arm in `eval_prop_constraint`, and the mirror arm in the
inner-shape `conforms` boolean checker) matches non-literals with `_ => false`
(no violation) / `_ => true` (conforms), i.e. treats them as out of scope.
Both call sites must instead treat a non-literal value node as violating.

**Fix**: change `_ => false` → `_ => true` in `eval_prop_constraint`'s
`LanguageIn` arm (violates), and `_ => true` → `_ => false` in the `conforms`
boolean checker's `LanguageIn` arm (does not conform).

## 2. `sh:lessThan` / `sh:lessThanOrEquals` on incomparable literal pairs

**Spec citation** (§4.5.3 / §4.5.4 normative text, and the summary table):

> `sh:LessThanConstraintComponent`: For each pair of value nodes and the
> values of the property `$lessThan` at the given focus node where the first
> value is not less than the second value (based on SPARQL's `<` operator)
> **or where the two values cannot be compared**, there is a validation
> result with the value node as `sh:value`.

(`sh:LessThanOrEqualsConstraintComponent` has the identical "or where the two
values cannot be compared" clause with `<=`.)

**Conclusion: CONFIRMED BUG.** The spec explicitly requires a violation for
incomparable pairs — this is not an implementation choice. Current code
(`lit_comparable` returning `None`) causes the comparison to be silently
skipped via `if let Some(pvc) = ... { ... }` / `.is_none_or(...)` — a `None`
in either position currently means "conforms", the opposite of what the spec
requires.

Additionally, the `Comparable` `Ord` impl (`shacl/src/evaluate.rs`) falls
through mismatched variants (e.g. `Numeric` vs `Date`) to
`Ordering::Equal`:
```rust
_ => std::cmp::Ordering::Equal,
```
Per SPARQL/XPath comparison semantics, comparing a number to a date is a type
error, not equality — this is a **second, independent bug**: mismatched
`Comparable` variants must also be treated as "cannot be compared" (i.e. a
violation), not as equal. It happens to produce a correct-looking result for
`sh:lessThan` (`>=` matches `Ordering::Equal`, so cross-type currently
violates by accident) but produces the **wrong** result for
`sh:lessThanOrEquals` (`>` does not match `Ordering::Equal`, so cross-type
currently silently conforms — wrong per spec, which says incomparable ⇒
violate unconditionally).

**Fix**: replace the `Option<Comparable>`/`Ord`-based comparison with an
explicit `fn comparable_lt`/`comparable_cmp`-style helper that:
- returns "incomparable" (⇒ violation) when either literal isn't a
  recognized comparable type at all, and
- returns "incomparable" (⇒ violation) when both are recognized but of
  different `Comparable` variants (Numeric vs Date vs DateTime),
- otherwise compares normally within the same variant.

Both `eval_prop_constraint`'s `LessThan`/`LessThanOrEquals` arms and the
`conforms` boolean checker's arms need the same fix, applied consistently.

**Explicitly out of scope**: `sh:minInclusive`/`sh:maxInclusive`/
`sh:minExclusive`/`sh:maxExclusive` (value range) have an analogous
`lit_comparable`-based skip-on-`None` pattern, but issue #266 only asks about
`sh:lessThan`/`sh:lessThanOrEquals`, and the value-range components' spec
wording differs (a bound of unknown/different type is a distinct question —
not investigated here). If this is a real gap it should be filed as its own
follow-up issue (unlabeled, needs `ready`) rather than folded into this PR's
scope.

## 3. `sh:equals` report-detail (not a conformance bug)

Current: one violation triple per **focus node** whose value sets differ
(`add_viol(work, *node, viol, nil)` — using a synthetic `rdf:nil` as the
value, no matter how many terms actually differ).

For consistency with `sh:disjoint` (which reports one violation per
overlapping *value*, via `path_vals.intersection(&other_vals)`), `sh:equals`
should report one violation per differing value — i.e. per member of the
**symmetric difference** of `path_vals` and `other_vals` — instead of a
single synthetic-nil result per focus node.

**Decision: fix it.** This aligns `sh:equals`'s report granularity with
`sh:disjoint`'s existing pattern, and PR #300 already added the
`source_shape`/`source_constraint`/`result_path` plumbing this can reuse
without duplicating it — each per-value violation triple naturally gets that
metadata for free via the existing `ViolMeta`/predicate-metadata plumbing
(the `(viol, constraint.component_iri())` return value used elsewhere).

## 4. New tests (`tests/shacl_suite.rs`)

A pass over the existing suite shows several of the issue's listed gaps are
**already covered** by follow-up work done for #256/#258/#260/#261/#263/#265:
- or/not with datatype/pattern/in/hasValue/maxCount: covered
  (`regression_issue_258_or_*`, `regression_issue_258_not_*`)
- node/xone/qualifiedValueShape with datatype/pattern/in/hasValue/maxCount:
  covered (`regression_issue_258_node_*`, `_xone_*`, `_qualified_*`)
- node-level (pathless) constraints beyond nodeKind: covered
  (`regression_issue_260_node_level_*`: datatype/in/class/hasValue)
- minCount/maxCount at N ≥ 2: covered (`regression_issue_256_*_n`)
- pattern/minLength/maxLength vs IRI/blank node: covered
  (`regression_issue_261_*`)
- sh:datatype vs language-tagged literal: covered (`regression_259_*`)
- sh:class across rdfs:subClassOf: covered (`regression_issue_265_*`)

Genuine remaining gaps to add:
1. `sh:and` tested with `sh:pattern`/`sh:in`/`sh:hasValue`/`sh:maxCount` inner
   shapes (only `sh:datatype` currently covered for `sh:and` specifically,
   via `spec_s4_6_2_and_with_datatype_constraint`).
2. A report-detail test asserting `sh:resultPath`/
   `sh:sourceConstraintComponent`/severity/`sh:deactivated` actually appear
   correctly together in a report (Turtle serialization) — no existing test
   inspects `result_path`/`source_constraint`/`source_shape` on
   `ValidationResult` or their Turtle rendering directly.
3. Regression tests locking in the two confirmed fixes above
   (`sh:languageIn` non-literal, `sh:lessThan`/`sh:lessThanOrEquals`
   incomparable pairs including cross-type numeric/date), and one for the
   `sh:equals` per-differing-value report-detail change.

Tests for confirmed bugs are added `#[ignore]`d first (red), then unignored
once the fix lands (green). Tests locking in already-correct behavior
(e.g. `sh:and` combinations, which already work via the shared
`node_conforms_to_inner` checker from #258) are added without the ignore
dance, since no bug is being fixed there.

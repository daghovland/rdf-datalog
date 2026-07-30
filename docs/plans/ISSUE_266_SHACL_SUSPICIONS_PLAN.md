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

Genuine remaining gap:
1. `sh:and` tested with `sh:pattern`/`sh:in`/`sh:hasValue`/`sh:maxCount` inner
   shapes (only `sh:datatype` currently covered for `sh:and` specifically,
   via `spec_s4_6_2_and_with_datatype_constraint`). Added:
   `regression_issue_266_and_{pattern,in,hasvalue,maxcount}`.

Already closed by other work (checked after rebasing this branch onto
`origin/main`, which pulled in PR #300/#264 — "resultPath/sourceConstraintComponent/severity/deactivated
in a report" is fully covered by `regression_264_mincount_result_path_and_component`,
`regression_264_class_result_path_and_component`,
`regression_264_pattern_result_path_and_component`, and
`regression_264_full_detail_in_turtle_report` (Turtle serialization), plus the
pre-existing `regression_issue_262_*` (deactivated) and `regression_issue_263_*`
(severity) tests. No new test added for this item — would duplicate #300's own
coverage.

Regression tests added locking in the two confirmed fixes:
- `sh:languageIn` non-literal value node:
  `regression_issue_266_languagein_non_literal_violates`.
- `sh:lessThan`/`sh:lessThanOrEquals` incomparable pairs, including the
  specific cross-type (numeric vs. date) `Ordering::Equal` fallthrough bug,
  a non-literal (blank node) pair, the vacuous (no other-path values) case
  that must still conform, and the "string/boolean pairs ARE SPARQL-comparable"
  guard the advisor flagged (these must NOT become false positives):
  `regression_issue_266_lessthan_cross_type_violates`,
  `regression_issue_266_lessthanorequals_cross_type_violates`,
  `regression_issue_266_lessthan_non_literal_violates`,
  `regression_issue_266_lessthan_vacuous_no_other_values_conforms`,
  `regression_issue_266_lessthan_string_pair_compared_normally`,
  `regression_issue_266_lessthan_boolean_pair_compared_normally`.
- `sh:equals` per-differing-value report detail:
  `regression_issue_266_equals_reports_per_differing_value`, plus the
  pre-existing `spec_s4_5_1_equals` was updated from asserting 1 result to 2
  (one per differing value in the symmetric difference).

## Correction after advisor review

The initial plan draft proposed flipping `lit_comparable() == None` to
"violation" wholesale for `sh:lessThan`/`sh:lessThanOrEquals`. This would have
been a **new bug**: SPARQL's `<` operator (SPARQL 1.1 §17.3 operator mapping)
IS defined for xsd:string/simple-literal pairs (via `fn:compare`) and
xsd:boolean pairs (`op:boolean-less-than`), which `lit_comparable` does not
recognize at all (it only covers numeric/date/dateTime). The fix instead adds
a separate `sparql_compare`/`SparqlCmpValue` classification (not reusing or
modifying the existing `Comparable`/`Ord` impl, which remains exactly as
before and is still used unmodified by the value-range constraints) that
recognizes numeric, string, boolean, date, and dateTime pairs, comparing
within a matching kind and treating any other combination (including
mismatched kinds, e.g. numeric vs. date) as incomparable ⇒ violation.

Tests for confirmed bugs are added `#[ignore]`d first (red), then unignored
once the fix lands (green). Tests locking in already-correct behavior
(e.g. `sh:and` combinations, which already work via the shared
`node_conforms_to_inner` checker from #258) are added without the ignore
dance, since no bug is being fixed there.

# Plan: default-reject malformed OWL axioms, opt-in lenient approximation (#426)

Issue: [#426](https://github.com/daghovland/rdf-datalog/issues/426), part of the malformed-OWL-input panic-removal effort tracked under [#363](https://github.com/daghovland/rdf-datalog/issues/363). Related precedent: [#400](https://github.com/daghovland/rdf-datalog/pull/400) (`owl:hasSelf`), [#425](https://github.com/daghovland/rdf-datalog/pull/425) (`owl:oneOf`, `owl:hasValue`).

Working branch: `feat/426-owl-axiom-rejection`. **This document is plan-only — no implementation.** Per [Dag's revision comment on #426](https://github.com/daghovland/rdf-datalog/issues/426#issuecomment-5236253979): the default behavior for a malformed anonymous class expression changes from "warn + fall back to `owl:Thing`" to "reject the axiom loudly" (`Err`), with the old warn-and-approximate behavior kept available as an explicit opt-in.

## 0. Scope boundary (read this before the rest)

`OntologyDeclarations::class_expression()` (`rdf_owl_translator/src/class_expression_parser.rs:86-119`) has **three** distinct fallback-to-`owl:Thing` branches, only one family of which #426 is actually about:

1. A **data range** id used where a class is expected (line 94-100) — different malformation, not discussed in #426.
2. A **blank node with no matching entry at all** in `self.class_expressions` (line 103-110) — this fires for any anonymous node that was never parsed into a `ClassExpression` in the first place (an `owl:Restriction`/`owl:Class` blank node this crate doesn't recognise at all, or one silently dropped by an existing `Ok(None)`-returning skip elsewhere in `class_expression_parser.rs`, e.g. "Anonymous owl:Class without defining expression", "owl:Restriction... no recognized restriction predicate"). Out of scope for #426 — those are "this construct isn't supported," a different failure mode from "this construct's declared shape is structurally invalid," and changing them is a bigger blast radius (every currently-unsupported OWL construct becomes a hard `Err` instead of a skip) that deserves its own issue if wanted.
3. **The two sites #426's title names**: `owl:oneOf` with no valid individual members left after skipping malformed ones (`class_expression_parser.rs:584-596`), and `owl:hasValue` whose value is not a valid individual (`class_expression_parser.rs:915-924`). **This plan is scoped to these two.**

This scoping also resolves the position-tracking difficulty the original issue worried about — see §2.

**Important precondition on reaching branch 3 at all**, checked by reading the collection predicates rather than assumed: `collect_anon_class_exprs` (`class_expression_parser.rs:502-508`) only collects a blank node as a candidate `owl:oneOf`/`owl:intersectionOf`/`owl:unionOf`/`owl:complementOf` expression if it has an explicit `rdf:type owl:Class` triple; `collect_anon_restriction_exprs` (line 659-660) likewise requires `rdf:type owl:Restriction` for the `owl:hasValue` case. **#426's own one-line reproduction in the issue body — `[ owl:oneOf ( "not an individual" ) ] rdfs:subClassOf :B .` — has neither triple.** Such a blank node is never collected, never reaches a builder, and instead falls straight through to branch 2 above (the out-of-scope "blank node with no matching entry" fallback at line 103-110), regardless of this plan's fix.

This is not a flaw in the reproduction, it's a fact about the OWL 2 RDF mapping: real OWL tooling emitting an `owl:oneOf`-based enumeration or an `owl:Restriction` *always* emits the `rdf:type` triple (it's required by Table 13/14 of the [OWL 2 Mapping to RDF Graphs](https://www.w3.org/TR/owl2-mapping-to-rdf/) spec this crate mirrors) — the issue's one-liner is a minimal illustration of the *semantic* problem, not a literal fixture. §5.2's reproduction fixtures below include the `a owl:Class`/`a owl:Restriction` triple so they exercise branch 3 (this plan's actual fix), matching how the two existing #425 fixtures (`oneOfAllMembersMalformed.ttl`, `hasValueMalformedObjectProperty.ttl`) are already written — both already carry the type triple, confirmed by reading them.

The untyped case (no `rdf:type` triple at all) still resolves via branch 2's unconditional `owl:Thing`, uncovered by this plan, and shares branch 2's soundness profile (also position-blind, also capable of the same subclass-position corruption `owl:oneOf`/`owl:hasValue` had). Fast-following that into scope — e.g. by making branch 2 default-reject as well — is a natural next issue but is **not** done here, to keep this plan's blast radius matching #426's literal title.

## 1. The `ClassExprBuilder` signature problem — investigated, and a simpler fix than the issue assumed

Current signature (`class_expression_parser.rs:472-473`):

```rust
type ClassExprBuilder =
    Box<dyn Fn(&OntologyDeclarations, &dag_rdf::GraphElementManager) -> ClassExpression>;
```

Every `AnonExpr.builder` (11 construction sites across `collect_anon_class_exprs` and `build_restriction`/`build_restriction_rest`) is this type. They're invoked in exactly one place, `parse_anonymous_exprs` (`class_expression_parser.rs:701-732`):

```rust
for id in sorted {
    if let Some(&idx) = builder_map.get(&id) {
        let expr = (all[idx].builder)(decls, &datastore.resources);   // line 726
        decls.class_expressions.insert(id, expr);
    }
}
```

`parse_anonymous_exprs` **already** returns `Result<(), TranslatorError>` and is **already** called with `?` from `OntologyDeclarations::build()` (line 72), which is itself called with `?` from `rdf2owl()` (`translator.rs:34`), which returns `Result<OntologyDocument, TranslatorError>` and is the crate's public entry point. So the `Result` plumbing from "malformed builder" up to "public API caller sees `Err`" **already exists end-to-end** — nothing between `parse_anonymous_exprs` and `rdf2owl`'s callers needs to change.

The only gap is the leaf: `ClassExprBuilder` itself isn't `Result`-returning, so the two malformed-input branches inside it (`oneOf`, `hasValue`) have no way to signal failure except the existing `log::warn!` + `owl:Thing` substitution.

**Fix**: change the type alias to

```rust
type ClassExprBuilder = Box<
    dyn Fn(&OntologyDeclarations, &dag_rdf::GraphElementManager) -> Result<ClassExpression, TranslatorError>,
>;
```

and thread `Result` through:
- `parse_anonymous_exprs` line 726: `let expr = (all[idx].builder)(decls, &datastore.resources)?;`
- All 11 builder closures: wrap their final expression in `Ok(...)`; the ones that recursively call `decls.class_expression(dep, res)` internally (complementOf, intersectionOf, unionOf, someValuesFrom, allValuesFrom, onClass) need that inner call updated too — **but `OntologyDeclarations::class_expression()` (the public method used both here and from `axiom_parser.rs`) does NOT need to become `Result`-returning**, because of the ordering argument in §2 below. Only the two builder closures for `oneOf`/`hasValue` actually change their *return value* from `Ok(ClassExpression::ClassName(owl:Thing))` to `Err(TranslatorError::MalformedClassExpression(...))`; the other 9 closures just get a mechanical `Ok(...)` wrapper (and `?` on any nested calls) with no behavior change.

This mechanical wrapping (9 closures: no-op besides `Ok`; 2 closures: real behavior change) is a small, low-risk diff — confirmed by reading all 11 call sites (`class_expression_parser.rs:540-1066`), not assumed.

## 2. Why position-threading through `axiom_parser.rs` turns out to be unnecessary for default-reject

The original issue worried the fix would need to reach `axiom_parser.rs`'s `rdfs:subClassOf` handler (`axiom_parser.rs:278-284`, calling `decls.class_expression(triple.subject, res)` for the subclass side) to know whether it's resolving a subject or object. That's true for a *sound approximation* (§3), but **not for outright rejection**:

`OntologyDeclarations::build()` (`class_expression_parser.rs:50-75`) runs `parse_anonymous_exprs` — which pre-resolves *every* anonymous class expression in the whole document into the `class_expressions: HashMap<GraphElementId, ClassExpression>` map — **before** `rdf2owl` calls `extract_axioms_indexed`/`extract_axiom` (`translator.rs:36`, `axiom_parser.rs:82`), which is where subject/object (subclass/superclass) position is actually known. By the time any `axiom_parser.rs` call site runs `decls.class_expression(id, res)`, the map is either fully and successfully populated, or `OntologyDeclarations::build()` already returned `Err` and `rdf2owl` never got to axiom extraction at all.

So: reject-by-default requires **zero changes to `axiom_parser.rs`**. The `Err` from a malformed `owl:oneOf`/`owl:hasValue` propagates out of `OntologyDeclarations::build()` before position is even relevant, and the whole `rdf2owl(datastore)` call fails — this is design (a) from the task brief, and it's not a new choice we're inventing, it's the shape the existing `?`-chain already forces once the leaf closures return `Result`.

## 3. Default-reject design

- **New `TranslatorError` variant** in `rdf_owl_translator/src/error.rs`:
  ```rust
  /// An anonymous class expression's asserted shape is structurally
  /// invalid — an `owl:oneOf` list with no valid individual members, or
  /// an `owl:hasValue` restriction whose value is not a valid individual
  /// on an object property. See
  /// <https://github.com/daghovland/rdf-datalog/issues/426>.
  MalformedClassExpression(String),
  ```
  with a `Display` arm (`"malformed class expression: {msg}"`) alongside the other four variants. This is additive — `error.rs`'s own `match` is the only exhaustive match over `TranslatorError` in the repo (confirmed via `grep -rn "TranslatorError::"` across the workspace; all other references are non-exhaustive `if let`/single-arm matches in tests), so adding a variant doesn't break other crates.
- **Whole-call failure, not skip-and-continue** (task brief's option (a)): confirmed by §2 — `rdf2owl` already fails the *entire* translation on the first `TranslatorError` anywhere (malformed `rdf:List`, cyclic dependency, multiple `owl:members`, invalid individual all already work this way; there is no existing "collect all errors" convention anywhere in this crate to break from). Matches Dag's "axioms are like source code, best to crash" framing directly — one malformed axiom in a document invalidates translating that document, full stop.
- **Caller impact**: `src/lib.rs`'s `compile_ontology_rules` (line 270) and `run_owlrl_reasoning` (line 294) already do `rdf2owl(datastore).map_err(|e| e.to_string())?` — this is the CLI's only path to `rdf2owl` (confirmed via `grep -rln rdf2owl` across the workspace: hits are `src/lib.rs`, `rdf_owl_translator/src/{translator,axiom_parser,lib}.rs`, plus test/bench files; `sparql_endpoint` has **no** call site — the HTTP endpoint does not currently invoke `rdf2owl` at all, so this change has no endpoint-side surface to update). No signature or call-site change needed in `src/lib.rs`/`main.rs` for the default path — a malformed axiom already surfaces as a CLI error message (`error: malformed class expression: ...`) via the existing `map_err`. This is a **behavior change** worth flagging explicitly in the PR body: a document that previously loaded successfully (silently, with a warning, approximating to `owl:Thing`) will now make the whole load fail — that's the point of #426, but it means any existing test fixture relying on the old warn-and-approximate behavior for these two shapes needs updating (see §5; `translate_one_of_falls_back_to_owl_thing_when_all_members_malformed` and `translate_has_value_falls_back_to_owl_thing_on_malformed_object` in `rdf_owl_translator/tests/translation_tests.rs` are exactly those two tests and must change to assert `Err` under the new default).

## 4. Opt-in lenient-mode design

### 4.1 Flag shape — follows the `--read-only` pattern exactly

`--read-only` (`src/main.rs:112-113`, `#[arg(long = "read-only", env = "DAGALOG_READ_ONLY")] read_only: bool`) is the existing precedent for a CLI-configurable behavior toggle threaded as a plain `bool` field on the `Cli` struct with an `env` fallback. Follow it:

```rust
#[arg(long = "lenient-owl-parsing", env = "DAGALOG_LENIENT_OWL_PARSING")]
lenient_owl_parsing: bool,
```

Threading: `rdf2owl` gains an options parameter rather than a second function, to avoid duplicating `extract_axioms_indexed`/`OntologyDeclarations::build`:

```rust
pub struct TranslationOptions {
    /// When true, a malformed owl:oneOf/owl:hasValue falls back to a
    /// polarity-aware placeholder class instead of returning Err.
    /// Default: false (reject). See #426.
    pub lenient_owl_parsing: bool,
}
impl Default for TranslationOptions { fn default() -> Self { Self { lenient_owl_parsing: false } } }

pub fn rdf2owl(datastore: &mut Datastore) -> Result<OntologyDocument, TranslatorError> {
    rdf2owl_with_options(datastore, &TranslationOptions::default())
}
pub fn rdf2owl_with_options(
    datastore: &mut Datastore,
    options: &TranslationOptions,
) -> Result<OntologyDocument, TranslatorError> { ... }
```

Keeping the existing zero-arg `rdf2owl` as a thin default-options wrapper avoids touching any of the ~10 existing call sites (tests, benches, `dagalog-kernel`) that call `rdf2owl(&mut datastore)` today; only `src/lib.rs`'s `compile_ontology_rules`/`run_owlrl_reasoning` (and, if desired, their own callers up through `main.rs`) need to thread the new CLI flag down to `rdf2owl_with_options`.

`options` needs to reach `OntologyDeclarations::build` (where `parse_anonymous_exprs` and hence the two malformed-fallback closures live), so `build` also grows an `options: &TranslationOptions` parameter, threaded down to `collect_anon_class_exprs`.

### 4.2 Position-aware fallback — genuinely harder, and *does* need the position threading the original issue anticipated

Unlike default-reject (§2), a **sound approximation** cannot be computed at `parse_anonymous_exprs` time, because position (subclass/negative vs. superclass-or-equivalent/positive) is a property of *where a class-expression id is referenced from* (`axiom_parser.rs`'s per-axiom subject/object slots), not of the id itself — and in principle the same blank node id could be referenced from more than one axiom in more than one position (unusual for a single-use `owl:Restriction`/`owl:oneOf` blank node in practice, but not structurally forbidden). So lenient mode cannot bake a single Thing-or-Nothing choice into the `class_expressions` map up front the way default-reject's `Err` (which doesn't need a *value*, just needs to abort) can.

Two options, to be decided during implementation review rather than settled here:

**(A) Two-pass / lazy resolution.** Don't insert a `ClassExpression` for a malformed id into `decls.class_expressions` at all when lenient mode is on and a builder hits the malformed branch — instead record it in a new `decls.malformed_lenient: HashSet<GraphElementId>`. Give `OntologyDeclarations::class_expression()` a `position: Position` parameter (`enum Position { Positive, Negative }`, reusing the terminology already established as precedent in `eli/src/extractor.rs`'s `concept_positive_occurrence_normalization`/`concept_negative_occurrence_normalization`, even though — confirmed while reading `eli/src/extractor.rs:324` (`sub_class_axiom_normalization`) — no code is shared between the two crates; this is naming precedent only, not a dependency). When `id` is in `malformed_lenient`, return `Positive => owl:Thing`, `Negative => owl:Nothing`, instead of the current always-`owl:Thing` unconditional fallback at line 103-110. This requires updating all 18 `class_expression(` call sites (`class_expression_parser.rs`: 6 internal recursive calls from the 9 unaffected builders — position threads recursively rather than being fixed at the two `axiom_parser.rs` leaf calls, since e.g. an `ObjectIntersectionOf`/`ObjectUnionOf` member keeps the polarity of its containing expression, but `ObjectComplementOf`'s single operand *flips* it (`¬P` is positive iff `P` is negative) — the one non-identity case among the 6, worth a one-line comment at that call site so it isn't "fixed" back to identity by a future reader; `axiom_parser.rs`: 12 direct calls) to pass a `Position`, derived per Table 16/17 the same way `eli::extractor::sub_class_axiom_normalization` derives it: `rdfs:subClassOf` subject = Negative, object = Positive; `owl:equivalentClass`/`ClassAssertion`/`owl:disjointWith`/`owl:disjointUnionOf` = Positive on both sides (conservative — equivalence and disjointness don't have the SubClassOf's inherent asymmetry, so both sides get the "safe over-approximation" treatment rather than trying to derive a tighter per-side polarity, which is out of scope for this issue).

**(B) Skip the axiom instead of substituting a value.** Simpler: when lenient mode is on and a builder hits a malformed branch, still return `Err` internally, but catch it **at the point a `class_expression` reference resolves to a malformed id from within `axiom_parser.rs`** and drop just that one axiom (matching the `owl:onProperties without allValuesFrom` style of "warn and return `Ok(None)`" already used elsewhere in this file for other non-`rdf:List` malformations), rather than computing a substitute class at all. This avoids the Thing/Nothing polarity question entirely but is a smaller feature than what #426's original body asked for (it doesn't produce the "sound conservative bound" Dag's comment says is possible for a single axiom in isolation — it just silently drops the axiom, which has its own soundness profile: dropping `C ⊑ D` where `D` is malformed is not obviously safer than asserting `C ⊑ ⊤`, since both are "no information" in different ways, but dropping never causes *spurious* materialisation the way `owl:Thing` in subclass position did in the original bug report).

**Recommendation for the implementation phase**: start with (A), since it's the one that actually matches #426's original technical proposal and the polarity terminology Dag explicitly pointed at (`eli::extractor`'s existing positive/negative machinery) is precedent for exactly this shape existing elsewhere in the codebase already. (B) is a fallback if (A)'s call-site fan-out (18 sites, recursive position propagation through 6 of the 9 unaffected builders) proves too invasive for the value delivered, given lenient mode is explicitly the *secondary*, opt-in path.

### 4.3 What lenient mode does NOT need to change

- The default-reject `Err` path (§2, §3) is unaffected — `TranslationOptions::default()` still rejects.
- `axiom_parser.rs`'s non-class-expression call sites (`object_property_expression`, `data_property_expression`, `data_range`, `object_or_data_property`) are untouched — #426 is scoped to class expressions only.
- `eli::extractor.rs` is untouched — confirmed no shared code path (different crate, different translation stage: RDF→OWL vs. OWL→datalog).

## 5. Test plan (TDD — tests written first, `#[ignore]`d, per this repo's CLAUDE.md workflow)

All new/changed tests live in `rdf_owl_translator/tests/translation_tests.rs`, alongside the existing `#[test] fn translate_one_of_*`/`translate_has_value_*` tests it will partially replace. New Turtle fixtures go in `rdf_owl_translator/tests/data/`, mirroring existing naming (`oneOfAllMembersMalformed.ttl`, `hasValueMalformedObjectProperty.ttl` already exist and are reused below — no new fixture needed for the two "all malformed" cases, since they're the same fixtures, just asserting a different outcome).

### 5.1 Default rejection (superclass/equivalent position — behavior CHANGE to two existing tests)

1. **Modify** `translate_one_of_falls_back_to_owl_thing_when_all_members_malformed` → rename `translate_one_of_all_members_malformed_rejects_by_default`. Same fixture (`oneOfAllMembersMalformed.ttl`). Asserts `rdf2owl(&mut datastore)` (not the `parse_and_translate` helper, which currently `.expect()`s success — needs a second helper or an inline call) returns `Err(TranslatorError::MalformedClassExpression(_))`, not `Ok`.
2. **Modify** `translate_has_value_falls_back_to_owl_thing_on_malformed_object` → rename `translate_has_value_malformed_object_rejects_by_default`. Same fixture (`hasValueMalformedObjectProperty.ttl`). Same `Err` assertion.

### 5.2 Default rejection (subclass position — the actual reproduction from #426's original bug report; NEW)

3. **New fixture** `rdf_owl_translator/tests/data/oneOfMalformedInSubclassPosition.ttl`. **Must** include `a owl:Class` on the anonymous node — see §0's note — or `collect_anon_class_exprs` never collects it and the test would exercise the out-of-scope branch 2 fallback instead of this plan's fix:
   ```turtle
   @prefix owl: <http://www.w3.org/2002/07/owl#> .
   @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
   @prefix ex: <http://example.org/> .
   ex:B a owl:Class .
   [ a owl:Class ; owl:oneOf ( "not an individual" ) ] rdfs:subClassOf ex:B .
   ```
4. **New test** `translate_one_of_malformed_subclass_position_rejects_not_universally_true`: asserts `rdf2owl(&mut datastore)` is `Err`. This is the test that actually proves the soundness bug from #426's original body is fixed — under the *old* behavior this fixture would translate successfully to `owl:Thing rdfs:subClassOf ex:B`, and a follow-up assertion (materialise OWL-RL rules over a datastore containing some unrelated individual `ex:i` and confirm `ex:i rdf:type ex:B` is **not** derived) would have caught the original bug; under the new default, translation fails before any rule materialisation happens, so this single `Err` assertion is sufficient — no materialisation-level assertion needed since there's no `Ok(OntologyDocument)` to materialise from. Mirrors the existing style of `test_malformed_rdf_list_returns_err_not_panic` in `tests/owl_integration.rs`.
5. **New fixture + test**, mirrored, for `owl:hasValue` in subclass position (`hasValueMalformedObjectPropertySubclassPosition.ttl` / `translate_has_value_malformed_subclass_position_rejects`) — same shape as #4 but with a malformed `owl:hasValue` restriction as the subclass. Belt-and-suspenders: proves the fix isn't accidentally `oneOf`-specific.

### 5.3 Well-formed regression (unaffected — confirm no false positives)

6. `translate_one_of_well_formed` and the existing well-formed `owl:hasValue` test stay green unmodified — confirms the `Ok(...)` wrapping added to the 9 unaffected builder closures (§1) is genuinely a no-op for the non-malformed path.

### 5.4 Opt-in lenient mode (deferred to implementation phase, sketched here for scope; exact shape depends on §4.2's (A) vs (B) decision)

7. `translate_one_of_all_members_malformed_lenient_mode_falls_back_to_owl_thing` — same fixture as #1, but called through `rdf2owl_with_options(&mut ds, &TranslationOptions { lenient_owl_parsing: true })`; asserts `Ok` and the old owl:Thing-substitution behavior (this is effectively today's `translate_one_of_falls_back_to_owl_thing_when_all_members_malformed`, moved under the opt-in flag rather than deleted — preserves regression coverage for the pre-#426 behavior, now gated).
8. `translate_one_of_malformed_subclass_position_lenient_mode_uses_owl_nothing` — same fixture as #4, lenient mode on. Asserts the resulting `SubClassOf` axiom's subclass side is `owl:Nothing`, not `owl:Thing` — this is the actual polarity-correctness assertion #426's original body wanted, and the one that would have caught the original soundness bug if it existed under the old code. Requires implementing §4.2 first (whichever of (A)/(B) is chosen; if (B) is chosen this test instead asserts the axiom is *absent* from the translated ontology rather than asserting an `owl:Nothing` subclass — the exact assertion shape is one of the open decisions from §4.2, noted here so implementation doesn't silently pick one without the test reflecting which).
9. `translate_has_value_lenient_mode_superclass_position_uses_owl_thing` — a malformed `owl:hasValue` used in *superclass*/equivalent position under lenient mode still gets `owl:Thing` (proving the polarity split goes both ways, not just "always Nothing now").

### 5.5 CLI wiring (only if §3/§4.1's flag threading is implemented in this same follow-up PR)

10. `tests/cli_integration.rs` gains a test asserting `--lenient-owl-parsing` flips a malformed-fixture load from failing to succeeding (black-box, via `assert_cmd` or equivalent — follow whatever harness `cli_integration.rs` already uses, not sketched further here since it's outside `rdf_owl_translator`'s own test suite).

## 6. Suggested implementation order (not binding — for whoever picks this up)

1. §3 (default-reject): `TranslatorError::MalformedClassExpression`, `ClassExprBuilder` → `Result`-returning, mechanical `Ok(...)` wrap on 9 closures, real `Err` on 2. Tests 1–6. This alone closes the soundness hole #426 is about — everything from here is the opt-in nice-to-have.
2. §4.1 (flag plumbing): `TranslationOptions`, `rdf2owl_with_options`, CLI flag. Test 10.
3. §4.2 (polarity-aware lenient fallback): pick (A) or (B), implement, tests 7–9.

Steps 2–3 could reasonably be split into a separate follow-up issue/PR from step 1, since step 1 alone is a complete, mergeable fix for #426's core soundness concern and Dag's comment treats the opt-in mode as explicitly secondary ("maybe this could be planned as an optional feature").

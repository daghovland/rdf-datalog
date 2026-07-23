# RML FunctionMap (FNML) Plan

Tracks [issue #27](https://github.com/daghovland/rdf-datalog/issues/27), part of
[epic #25](https://github.com/daghovland/rdf-datalog/issues/25). See also
[`RML_PLAN.md`](RML_PLAN.md) (core RML) and
[`PIPELINE_BACKLOG.md`](PIPELINE_BACKLOG.md) §1 for how this fits the wider
pipeline backlog.

Working branch: `feature/27-rml-fnml`.

## Goal

Let a `SubjectMap`/`ObjectMap` generate its term by invoking a named function
over other term maps' values, instead of only `rr:template` / `rr:reference` /
`rr:constant`. This is FNML — RML's integration with
[FnO (the Function Ontology)](https://fno.io/rml/).

## Spec grounding

FNML has no single stable, published vocabulary — there are two competing
generations:

1. **The legacy/widely-implemented shape** (`fnml:` @
   `http://semweb.mmlab.be/ns/fnml#`, `fno:` @
   `https://w3id.org/function/ontology#`), used by `rmlmapper-java` (the
   reference implementation) and every FNML mapping fixture actually found in
   the wild, e.g. the RML-FNO test suite's
   [`RMLFNOTC0005-CSV/mapping.ttl`](https://github.com/RMLio/rmlmapper-java/blob/master/src/test/resources/rml-fno-test-cases/RMLFNOTC0005-CSV/mapping.ttl)
   (quoted verbatim):

   ```turtle
   @prefix rr: <http://www.w3.org/ns/r2rml#> .
   @prefix rml: <http://semweb.mmlab.be/ns/rml#> .
   @prefix fnml: <http://semweb.mmlab.be/ns/fnml#> .
   @prefix fno: <https://w3id.org/function/ontology#> .
   @prefix idlab-fn: <https://w3id.org/imec/idlab/function#> .

   <TriplesMap1>
     a rr:TriplesMap;
     rml:logicalSource [ rml:source "./student.csv"; rml:referenceFormulation ql:CSV ];
     rr:subjectMap [
       fnml:functionValue [
         rr:predicateObjectMap [
           rr:predicate fno:executes ;
           rr:objectMap [ rr:constant idlab-fn:toUpperCaseURL ]
         ] ;
         rr:predicateObjectMap [
           rr:predicate idlab-fn:str ;
           rr:objectMap [ rml:reference "url" ]
         ]
       ] ;
       rr:termType rr:IRI
     ] ;
     rr:predicateObjectMap [ rr:predicate foaf:name; rr:objectMap [ rml:reference "Name"] ] .
   ```

2. **A newer, still-drafting "rml-core" rewrite** (`rml:FunctionExecution`,
   `rml:Input`, `rml:ParameterMap`, `rml:inputValueMap`, everything under
   `http://w3id.org/rml/`) at
   [kg-construct.github.io/rml-fnml](https://kg-construct.github.io/rml-fnml/spec/docs/).
   This spec is an unreleased community-group draft; two separate fetches of
   its own docs page returned inconsistent property names for the same
   concept (`functionValue` vs. `functionExecution`, `rml:function` vs.
   `rml:constant`), i.e. it is not a stable target to implement against yet.

**Decision: implement shape (1), the legacy/widely-implemented one.** It is
what issue #27 itself quotes, what the reference implementation and every
real-world fixture use, and it's stable. If the rml-core draft stabilizes
later, upgrading is a follow-up issue.

### Dialect split (important, so the loader's namespace choices aren't a
### surprise later)

This crate's existing `loader.rs` already made its own dialect decision for
core RML: it collapses R2RML (`rr:`) and RML (`rml:`) into a single unified
`rml:` = `http://w3id.org/rml/` namespace for every *structural* term
(`rml:predicateObjectMap`, `rml:predicate`, `rml:objectMap`, `rml:constant`,
`rml:reference`, `rml:template`, ...) — see the `rml()` helper and every
fixture under `rml/tests/fixtures/`. FNML introduces genuinely distinct,
independently-real vocabularies for the function-invocation part, and those
IRIs are **not invented** — they come straight from the fixture quoted above
and from FnO/GREL's own published vocabulary files:

| Concept | Namespace | Example IRI |
|---|---|---|
| FNML function-map trigger | `fnml:` = `http://semweb.mmlab.be/ns/fnml#` | `fnml:functionValue` |
| Function invocation / FnO | `fno:` = `https://w3id.org/function/ontology#` | `fno:executes` |
| Built-in GREL functions | `grel:` = `https://users.ugent.be/~bjdmeest/function/grel.ttl#` | `grel:toUpperCase`, `grel:valueParam` |
| Everything *inside* the function-map node that is plain RML structure (predicateObjectMap/predicate/objectMap/constant/reference) | this crate's existing unified `rml:` = `http://w3id.org/rml/` | `rml:predicateObjectMap`, `rml:predicate`, `rml:objectMap`, `rml:reference`, `rml:constant` |

So a mapping fixture in this crate's own dialect looks like:

```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix fnml: <http://semweb.mmlab.be/ns/fnml#> .
@prefix fno: <https://w3id.org/function/ontology#> .
@prefix grel: <https://users.ugent.be/~bjdmeest/function/grel.ttl#> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [ rml:source "data.csv" ; rml:referenceFormulation rml:CSV ] ;
    rml:subjectMap [ rml:template "http://example.com/Person/{id}" ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [
            fnml:functionValue [
                rml:predicateObjectMap [
                    rml:predicate fno:executes ;
                    rml:objectMap [ rml:constant grel:toUpperCase ]
                ] ;
                rml:predicateObjectMap [
                    rml:predicate grel:valueParam ;
                    rml:objectMap [ rml:reference "name" ]
                ]
            ]
        ]
    ] .
```

Both trigger property (`fnml:functionValue`) and the function-selector
property (`fno:executes`) plus the function/parameter IRIs (`grel:*`) use
their real, externally-defined IRIs; only the surrounding structural glue
follows this crate's pre-existing `rml:` convention. This is a deliberate,
documented dialect choice, not a shortcut — the same pattern the crate
already applies to core RML.

## Built-in function registry (scope decision)

A fully generic FnO dispatcher — resolving arbitrary `fno:Function` IRIs,
fetching remote function descriptions, invoking arbitrary implementations —
is out of scope for this pass. Real-world FNML mappings overwhelmingly use a
small, fixed set of GREL string functions. We ship a **closed built-in
registry** for exactly three unary GREL functions, verified against the
published [`grel.ttl`](https://users.ugent.be/~bjdmeest/function/grel.ttl)
vocabulary:

| Function IRI | Expected param IRI | Behavior |
|---|---|---|
| `grel:toUpperCase` | `grel:valueParam` | uppercase the string |
| `grel:toLowerCase` | `grel:valueParam` | lowercase the string |
| `grel:string_trim` | `grel:valueParam` | trim leading/trailing whitespace |

`grel:array_join` (string concatenation with separator) was considered and
**dropped**: its real signature takes an *array* parameter
(`grel:param_a`), which doesn't fit this crate's per-row scalar term-map
model without inventing an array-typed parameter kind. Adding a
multi-parameter/array-aware built-in is a natural, isolated follow-up once
there's a concrete use case — the AST and plan representation below are
already N-ary (a function call carries a `Vec` of parameters keyed by param
IRI), so this is not a redesign later, just a registry addition.

Extension point: `rml::functions::resolve_builtin(iri: &IriReference) ->
Option<BuiltinFunction>`, a plain match/lookup. Adding a function means
adding an enum variant + a `fn(&str) -> String` (or `fn(&[String]) ->
String` for multi-arg) body + a registry-lookup arm — no architecture
change.

**Parameter value maps in this pass are restricted to `rml:template` /
`rml:reference` / `rml:constant`** (i.e. ordinary `TermMap`, not a nested
`fnml:functionValue`). Function composition (a function's parameter being the
output of another function) is deferred; nothing here blocks adding it later
since parameters are already modeled as a `Vec<(param_iri, TermMap)>` and
`TermMap` already has room for a `FunctionCall` variant to nest into.

## AST additions (`rml/src/ast.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TermMap {
    Template(String),
    Constant(GraphElement),
    Reference(String),
    FunctionCall(FunctionCall),   // NEW
}

/// `fnml:functionValue [ ... ]`: invoke a named function against parameter
/// values sourced from ordinary term maps.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCall {
    /// The `fno:executes` object — the function IRI, e.g. `grel:toUpperCase`.
    pub function_iri: IriReference,
    /// One entry per non-`fno:executes` `rml:predicateObjectMap` inside the
    /// function-map node: (parameter IRI, value-producing term map).
    pub parameters: Vec<FunctionParameter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionParameter {
    pub param_iri: IriReference,
    /// Restricted to Template/Reference/Constant in this pass (see plan).
    pub value_map: TermMap,
}
```

## Loader additions (`rml/src/loader.rs`)

`extract_term_map(ds, node)` gains a new first check: if `node` has an
`fnml:functionValue` object (a blank node `fm_id`), extract it as a
`TermMap::FunctionCall` instead of falling through to
template/reference/constant:

- Iterate `fm_id`'s `rml:predicateObjectMap`s (reusing the exact same
  `all_objs`/`first_obj` helpers already used for ordinary predicate-object
  maps — no new graph-walking logic).
- The one whose `rml:predicate` is `fno:executes` supplies `function_iri` via
  its `rml:objectMap`'s `rml:constant` (must resolve to an IRI; if absent, an
  empty-string `IriReference` is stored — this is intentionally *not* an
  error at load time, since function-IRI validity is a semantic property
  resolved later, in `translate()`, matching how the rest of this loader
  defers semantic validation — see `extract_logical_source`'s handling of
  missing `rml:source` as a contrast: *structural* absence errors in the
  loader, *semantic* mismatches (unknown function) error in `translate()`).
- Every other `rml:predicateObjectMap` becomes one `FunctionParameter`:
  `param_iri` from `rml:predicate`, `value_map` from
  `extract_term_map(ds, objectMap_node)`.

This is additive — no change to how Template/Reference/Constant are
extracted, and `extract_term_type` (reading `rml:termType` off the same
node) composes for free on both `SubjectMap` and `ObjectMap`, since both
call the same `extract_term_map`.

## Plan/translate additions (`rml/src/plan.rs`, `rml/src/translate.rs`)

`translate()` currently returns `Vec<LogicalPlan>` unconditionally. It
becomes fallible:

```rust
pub fn translate(mapping: &MappingDocument) -> Result<Vec<LogicalPlan>, RmlError>;
```

because resolving `TermMap::FunctionCall` requires looking up
`function_iri` in the built-in registry — an **unknown function IRI is a
hard error at translate time**, not a per-row skip (unlike a missing CSV
column, which is legitimately data-shaped and already handled by returning
`None` from `eval_format_function`). New error variant:

```rust
RmlError::UnknownFunction(String), // the unresolved function IRI
```

`plan::GenerationLogic` gains a new variant:

```rust
pub enum GenerationLogic {
    Constant(GraphElement),
    Dynamic(FormatFunction),
    Function(FunctionCallLogic),      // NEW
}

pub struct FunctionCallLogic {
    pub function: crate::functions::BuiltinFunction,
    pub params: Vec<(IriReference, ParamSource)>,
    pub term_type: TermType,
    pub language: Option<String>,
    pub datatype: Option<IriReference>,
}

/// A parameter's value-producing side, mirroring TermPattern but also
/// covering rml:constant (TermPattern only covers Template/Reference today).
pub enum ParamSource {
    Template(String),
    Reference(String),
    Constant(GraphElement),
}
```

`translate_triples_map`'s `term_map_to_logic` becomes fallible
(`-> Result<GenerationLogic, RmlError>`) and gains a `TermMap::FunctionCall`
arm that calls `crate::functions::resolve_builtin(&fc.function_iri)`,
`.ok_or_else(|| RmlError::UnknownFunction(fc.function_iri.0.clone()))?`, and
maps each `FunctionParameter` to a `(param_iri, ParamSource)` pair (erroring
the same way if a parameter's own value map were ever a nested
`FunctionCall`, which the loader doesn't currently produce, but the match
must be exhaustive).

## Engine additions (`rml/src/engine.rs`, new `rml/src/functions.rs`)

`rml/src/functions.rs` (new file) holds the closed registry:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFunction { ToUpperCase, ToLowerCase, Trim }

pub fn resolve_builtin(iri: &IriReference) -> Option<BuiltinFunction> { ... }

/// Applies the function given its resolved single string parameter value.
pub fn apply(f: BuiltinFunction, input: &str) -> String {
    match f {
        BuiltinFunction::ToUpperCase => input.to_uppercase(),
        BuiltinFunction::ToLowerCase => input.to_lowercase(),
        BuiltinFunction::Trim => input.trim().to_string(),
    }
}
```

`engine::eval_logic` gains a `GenerationLogic::Function(fc)` arm: evaluate
each `(param_iri, ParamSource)` against the current row exactly like
`eval_format_function` does today for `TermPattern` (template expansion /
reference lookup / constant-to-string), look up the single value expected by
`fc.function` (v1 functions are all unary, so this is "the first — and
only — parameter's value", not yet a param-IRI-keyed dispatch since there's
nothing to disambiguate with one param), run `functions::apply`, then build
the final term the same way `eval_format_function` already does (respecting
`term_type`/`language`/`datatype`). No change to `eval_attr` or the
row/triple emission path.

## Tests (`rml/tests/fnml_tests.rs`, new file, `#[ignore]`d until implemented)

Mirrors the file-per-concern convention (`loader_tests.rs` for parsing,
`plan_tests.rs` for translate, `end_to_end.rs`-style for full mapping →
triples). New fixture: `rml/tests/fixtures/fnml_basic/` (mapping.ttl +
person.csv).

1. `loader_parses_function_value_object_map` — parses an object map with
   `fnml:functionValue`; asserts `TermMap::FunctionCall` with the right
   `function_iri` (`grel:toUpperCase`) and one parameter
   (`grel:valueParam` → `TermMap::Reference("name")`).
2. `translate_unknown_function_iri_is_an_error` — a mapping whose
   `fno:executes` object is some made-up IRI; `translate()` returns
   `Err(RmlError::UnknownFunction(_))`.
3. `end_to_end_to_upper_case_transforms_object_value` — full mapping → CSV →
   `apply_rml_mapping`; asserts the resulting literal is the upper-cased CSV
   value.
4. `end_to_end_to_lower_case_transforms_object_value` — same shape with
   `grel:toLowerCase`, second row/column, to cover a second built-in
   end-to-end per the issue's requirement.
5. `end_to_end_trim_transforms_object_value` — `grel:string_trim` on a value
   with leading/trailing whitespace in the CSV.

## What's explicitly deferred (follow-ups)

- Generic `fno:Function` dispatch / remote function description resolution.
- `grel:array_join` / any array-typed or multi-row-aggregating GREL function.
- Nested function composition (a parameter whose value is itself a
  `fnml:functionValue`).
- Adopting the rml-core `rml:FunctionExecution`/`rml:Input` shape if/when
  that community-group draft stabilizes and publishes fixed IRIs.

File a follow-up issue under [epic #25](https://github.com/daghovland/rdf-datalog/issues/25)
for broader FnO dispatch once this lands, if warranted.

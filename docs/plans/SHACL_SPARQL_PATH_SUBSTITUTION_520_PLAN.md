# SHACL `sh:sparql` on property shapes + `$PATH` substitution (#520)

Follow-up from [#54](https://github.com/daghovland/rdf-datalog/issues/54) (SHACL
Phase 4: SHACL-SPARQL constraints §5-6, `sparql_constraints.rs`), tracked as
[#520](https://github.com/daghovland/rdf-datalog/issues/520).

Spec: <https://www.w3.org/TR/shacl-af/#SPARQLConstraintComponent>

## Scope

`sh:sparql [ a sh:SPARQLConstraint ; ... ]` is currently only parsed/evaluated
when declared directly on a node shape (`ParsedShape::sparql_constraints`).
Per SHACL-AF §6.1, it may also be declared on a **property shape** (one with
`sh:path`, i.e. inside a `sh:property [ ... ]` block or a shape node that
carries `sh:path` directly). When declared on a property shape:

- `$this` is still always bound to the *focus node* (not a path-traversed
  value — same posture #54 already established for node shapes; a property
  shape's `sh:sparql` does not iterate the path's value nodes the way
  `sh:propertyValidator` does for custom components).
- The query additionally gets access to `$PATH`/`?PATH`, which must be
  substituted with the property shape's `sh:path`, rendered as actual SPARQL
  1.1 property-path surface syntax (not just a bound IRI term — a path may be
  a sequence/alternative/inverse/repeated expression with no single IRI to
  bind), before the query is parsed.

## Reuse, not reinvention

[#519](https://github.com/daghovland/rdf-datalog/issues/519) (PR #672,
`custom_components.rs`) already implements exactly this substitution for
`sh:propertyValidator` queries: `path::to_sparql_path` renders an `ShPath` to
SPARQL property-path syntax, and a `PATH_TOKEN` regex (`[$?]PATH\b`) replaces
every `$PATH`/`?PATH` occurrence with that rendered syntax, textually, before
the usual `$name` -> `?name` dollar-normalization and `PREFIX` header
assembly (`custom_components::build_validator_query_text`).

This issue moves that substitution step into `sparql_constraints.rs` itself
(as `pub(crate) fn build_query_text(sq, path_sub: Option<&ShPath>)`, replacing
the current no-path `build_query_text(sq)`) so `custom_components.rs` can
call it instead of keeping its own duplicate `PATH_TOKEN` regex/substitution
— a plain dedup, not a behavior change for `custom_components.rs`'s existing
callers (they already always pass the path-rendered substitution; `None`
there is just the node-validator case already passing `None`).

## Changes

- `shapes.rs`: add `sparql_constraints: Vec<SparqlConstraint>` to
  `ParsedPropShape`, populated by the existing `parse_sparql_constraints`
  helper (already shape-node-id-generic — it already takes any
  `GraphElementId`, not just a node shape's) from both `parse_property_shapes`
  (the `sh:property [...]` block case) and the direct-`sh:path`-on-shape-node
  case in `parse_one_shape`.
- `sparql_constraints.rs`:
  - `build_query_text` gains a `path_sub: Option<&ShPath>` parameter; `Some`
    textually substitutes `$PATH`/`?PATH` via the moved-in `PATH_TOKEN` regex
    before dollar-normalization, exactly mirroring
    `custom_components::build_validator_query_text`'s existing order of
    operations.
  - `check_query_syntax`/`eval_one_constraint`/`eval_batched_select`/
    `make_result` gain a `path_scope: Option<&ShPath>` parameter (property
    shapes pass `Some(&prop.path)`; node shapes keep passing `None`), used
    both for the `$PATH` substitution above and as the fallback
    `result_path` on a `ValidationResult` when the query's own row doesn't
    bind a `?path` column (mirrors `custom_components::eval_property_scope`'s
    `result_path.or_else(|| Some(prop.path.clone()))`).
  - `eval_all` additionally loops over every non-deactivated property shape's
    `sparql_constraints`, same pre-flight-checked/batched-where-possible
    evaluation as the node-shape loop.
- `lib.rs`: the §5/§6 pre-flight syntax-check loop in `validate()` also
  checks every property shape's `sparql_constraints` (with
  `Some(&prop.path)`), not just node-shape-level ones.
- `custom_components.rs`: drop its own `PATH_TOKEN` static/substitution logic
  from `build_validator_query_text`, delegating to
  `sparql_constraints::build_query_text` instead.

## Non-goals

- `sh:sparql` on a property shape does not gain a `$value`-per-path-value
  iteration mode — per spec and the precedent above, `$this` is always the
  focus node. A query that wants per-value behavior should traverse
  `?this <path-syntax> ?value` itself inside the query body, which is exactly
  what `$PATH` substitution is for.
- No change to `sh:target`'s `sh:SPARQLTarget` (§5) — that mechanism has no
  notion of a property shape at all (targets are declared on node shapes).

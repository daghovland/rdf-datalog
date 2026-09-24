# SHACL-SPARQL: custom constraint components (`sh:ConstraintComponent`, `sh:parameter`, `sh:validator`) (#519)

See [#519](https://github.com/daghovland/rdf-datalog/issues/519), follow-up from
[#54](https://github.com/daghovland/rdf-datalog/issues/54) (which implemented the
simpler embedded `sh:sparql`/`sh:SPARQLConstraint` mechanism, W3C SHACL spec §5).
This issue covers spec §6, "SPARQL-based Constraint Components" — reusable,
declaratively-parameterised constraint components.

**Correction to the issue title:** the issue text says `sh:SPARQLConstraintComponent`,
but per the actual normative spec text (verified against
<https://www.w3.org/TR/shacl/#constraints-sparql>, §6.2: *"A SPARQL-based
constraint component is an IRI that has SHACL type `sh:ConstraintComponent` in the
shapes graph"*), the RDF type to detect is the generic `sh:ConstraintComponent`,
not a `SPARQL`-prefixed subtype — the mechanism is generic (also usable by
non-SPARQL extension languages per the spec's own note in §6.2), and
SPARQL-based-ness is inferred from the presence of `sh:validator`/
`sh:nodeValidator`/`sh:propertyValidator` pointing at `sh:SPARQLAskValidator`/
`sh:SPARQLSelectValidator` nodes (in practice, at nodes carrying `sh:ask`/
`sh:select`, mirroring how `shapes.rs` already detects a `sh:SPARQLConstraint`
by the presence of `sh:select`/`sh:ask` rather than requiring an explicit
`rdf:type` triple). `crate::vocab::CC_SPARQL` (the existing
`sh:SPARQLConstraintComponent` constant) is unrelated — that's the fixed
`sh:sourceConstraintComponent` value for §5's embedded `sh:sparql` mechanism
(#54), not the type used to declare a custom component.

## Spec semantics (verified against the normative W3C SHACL Recommendation, not
## SHACL-AF as the issue text suggested — SPARQLConstraintComponents live in the
## Core spec's §6, not in SHACL-AF)

Fetched and read verbatim (§6.1–6.3, plus §5.3.1/Appendix A for pre-binding):

- **§6.2.1 Parameter Declarations (`sh:parameter`)**: each parameter has exactly
  one `sh:path` (an IRI); the parameter name is the local name of that IRI, used
  as the SPARQL variable name. Names `this`/`shapesGraph`/`currentShape`/`path`/
  `PATH`/`value` are reserved (not enforced by this implementation — malformed
  shapes graphs are out of scope, matching this crate's existing SHACL parsing
  posture). `sh:optional true` marks a parameter optional; every component has
  at least one non-optional parameter.
- **§6.2.3 Validators**: validator selection order — node shapes use
  `sh:nodeValidator` if present, property shapes use `sh:propertyValidator` if
  present, otherwise both fall back to `sh:validator`. If none apply, the
  constraint is ignored (not an error). `sh:nodeValidator`/`sh:propertyValidator`
  are always SELECT-based; `sh:validator` is always ASK-based.
- **§6.2.3.1 SELECT-based validators**: project `?this`, and are expected to
  produce solutions with `?value`, `?message`, optionally `?path` — identical
  result-row convention to the existing §5.1 `sh:sparql sh:select` mechanism
  (`sparql_constraints.rs`'s `row_value_and_path`). For a **property shape**,
  the token `$PATH`/`?PATH` (wherever it appears in a triple pattern's predicate
  position) is textually substituted, prior to execution, with a valid SPARQL
  surface-syntax property path built from the property shape's `sh:path` — a
  one-time textual macro expansion, not a pre-bound variable value.
- **§6.2.3.2 ASK-based validators**: evaluated once per **value node** `v`
  (for a property shape: each path-traversed value; for a node shape: the focus
  node itself, per SHACL §3.7's "the value nodes for a node shape are the focus
  node itself"), with `$value` pre-bound to `v` and `$this` to the focus node.
  `false` ⇒ one violation with `sh:value = v`.
- **§6.3 / §5.3.1**: every validator execution pre-binds `$this` (and, for
  every parameter of the constraint, `$paramName` to the shape's value for that
  parameter's `sh:path`). `$shapesGraph`/`$currentShape` (shapes-graph
  self-reflection) are optional pre-bound variables this implementation does
  **not** support — no existing consumer needs them; noted as a non-goal below.
- Message templates (`sh:message` on a validator node) may contain
  `{$paramName}`/`{?paramName}` placeholders, replaced with the parameter's
  bound value at report time (§6.2.2's `sh:labelTemplate` describes the same
  substitution syntax for a sibling but analogous property).

## Scope for this PR

**In scope:**
- Parsing `sh:ConstraintComponent` declarations anywhere in the shapes graph:
  `sh:parameter` (path + `sh:optional`), `sh:validator`/`sh:nodeValidator`/
  `sh:propertyValidator` (each resolved to a `sh:select` or `sh:ask` query,
  `sh:prefixes` handling reused verbatim from `parse_sparql_prefixes`).
- Invocation detection: a shape (node shape or property shape) "uses" a
  component when it has a value for every one of the component's non-optional
  parameters (§6.1's informative algorithm, simplified: since `sh:path` on a
  parameter is required, a shape either has a value there or doesn't).
  **Single-value simplification**: a parameter is read via `get_object`
  (first/only value) rather than the full cartesian product across
  multi-valued parameters — see Non-goals.
- Validator selection per §6.2.3's exact order (node/property specific, falling
  back to generic).
- SELECT validators (with `$PATH` substitution for property shapes) and ASK
  validators (per-value-node, both node- and property-shape scoped).
- `sh:message` template substitution (`{$name}`/`{?name}`).
- `sh:sourceConstraintComponent` in the resulting `ValidationResult` is the
  component's own IRI (not a fixed constant, unlike §5.1's `CC_SPARQL`).
- Severity/message override precedence mirrors the rest of the crate: a
  property shape's own `sh:severity`/`sh:message` overrides the parent node
  shape's, which is the final fallback under a validator's own message
  template.
- Pre-flight parse-checking of every declared component's validator query
  (mirrors #54's pre-flight check for `sh:sparql`/`sh:target`), and hard `Err`
  on an execution-time failure — the discipline #522 established for
  `sh:target` SPARQLTarget execution errors extends here too (no
  warn-and-skip).

**Non-goals / deferred (filed as follow-ups where they represent real,
plausibly-wanted future work — see below):**
- **Nested/inner-shape scope**: custom components are evaluated only for
  top-level shapes (both as node shapes and their direct `sh:property`
  blocks) — *not* inside `sh:not`/`sh:and`/`sh:or`/`sh:node`/`sh:xone`/
  `sh:qualifiedValueShape` inner-shape references. This mirrors the *existing*
  scope limit already in place for §5.1 `sh:sparql` constraints (never
  evaluated inside `evaluate::shape_conforms_for_node`'s inner-shape
  conformance check either) — not a new gap this PR introduces.
- **Multi-valued parameters** (a shape with more than one value for a
  component's parameter path): only the first value found is bound; no
  cartesian product across combinations. Every worked example in the spec
  (§6.2.3.1/§6.2.3.2, and the informative §6.1 example) uses single-valued
  parameters, so this is not expected to matter in practice, but it is a
  genuine spec deviation for the corner case — **filed as follow-up
  [#TBD](https://github.com/daghovland/rdf-datalog/issues) at Status `Todo`**
  (see below for the actual issue number once filed).
- **Blank-node-valued parameters**: a parameter's value is bound into the
  validator query as a raw `GraphElement` cloned from the *shapes* store; for
  an IRI or literal value (every spec example) this is safe since identity is
  self-describing. A blank-node-valued parameter would carry the *shapes*
  store's internal blank-node counter value into a query executed against the
  *data* store, which is not a case this PR handles correctly (could
  spuriously collide with an unrelated blank node in `data`). Not filed as a
  follow-up: parameters are descriptive metadata (patterns, languages,
  lengths, thresholds), never blank nodes, in every real-world use this
  crate's own W3C SHACL suite or spec examples exercise.
- **`$shapesGraph`/`$currentShape`** pre-bound variables (shapes-graph
  self-reflection from within a validator query) — no test or example in
  scope needs them, and no other part of this crate currently exposes the
  shapes graph to a running SPARQL query's variable bindings at all.
- **Batching** (§521's multi-focus-node `VALUES` fast path for the existing
  `sh:sparql` mechanism) is not extended to constraint-component validators —
  correctness first; a per-focus-node (and, for ASK, per-value-node) loop is
  used throughout. Given this mirrors `sparql_constraints.rs`'s pre-#521
  baseline exactly, no follow-up filed (#521's own approach can be extended
  here later if profiling ever shows it matters).

## Implementation

- `shacl/src/vocab.rs`: new constants `SH_CONSTRAINT_COMPONENT`,
  `SH_PARAMETER`, `SH_OPTIONAL`, `SH_VALIDATOR`, `SH_NODE_VALIDATOR`,
  `SH_PROPERTY_VALIDATOR`.
- `shacl/src/shapes.rs`: `ComponentParameter { path, var_name, optional }`,
  `ValidatorDef { query: SparqlQuery, is_ask: bool, message: Option<String> }`,
  `ConstraintComponentDef { component_id, parameters, validator, node_validator,
  property_validator }`, `ComponentInvocation { component_id, bindings:
  Vec<(String, GraphElement)>, validator, node_validator, property_validator }`
  (the invocation clones the selected component's validator definitions
  directly, so evaluation never needs a second lookup by id).
  `parse_constraint_components(shapes) -> Vec<ConstraintComponentDef>` scans
  `?c a sh:ConstraintComponent`. `parse_shapes` calls it once and attaches
  invocations to every top-level `ParsedShape`/`ParsedPropShape` via a new
  `attach_component_invocations` pass — no signature change to
  `parse_one_shape` itself (keeping `evaluate::shape_conforms_for_node`'s ad hoc
  inner-shape re-parse untouched, consistent with the nested-scope non-goal
  above). New fields: `ParsedShape::node_component_invocations`,
  `ParsedPropShape::component_invocations`.
- `shacl/src/path.rs`: `to_sparql_path(&ShPath) -> String`, a SPARQL 1.1
  property-path surface-syntax serializer (distinct from the existing
  `to_turtle`, which emits the SHACL-Turtle RDF-list encoding, not SPARQL path
  syntax) — used for `$PATH` substitution. Always parenthesizes a compound
  child of a compound expression; a bare top-level compound path needs no
  wrapping parens in predicate position.
- `shacl/src/custom_components.rs` (new): `eval_all`, mirroring
  `sparql_constraints.rs`'s structure (parse-check up front, direct evaluation
  against the un-materialised `data` graph, hard `Err` on execution failure).
  Reuses `sparql_constraints::{parse, run_select, run_ask, ge_display,
  normalize_dollar_vars}` (made `pub(crate)`) rather than duplicating the
  SPARQL-execution plumbing.
- `shacl/src/lib.rs`: pre-flight-check every declared component's validator
  query in `validate()` (alongside the existing `sh:sparql`/`sh:target`
  checks), call `custom_components::eval_all` alongside
  `sparql_constraints::eval_all`.

## Tests

Added to `tests/shacl_suite.rs`, section `#519`, following the file's existing
`spec_s{section}_*`/`regression_*` naming:
1. `spec_s6_2_ask_validator_node_shape` — `sh:validator` (ASK) invoked from a
   node shape's own parameter properties (mirrors the spec's §6.2.3.2
   `ex:hasLang` example almost verbatim).
2. `spec_s6_2_ask_validator_property_shape` — the same component invoked from
   a `sh:property` block; violation's `sh:value` is the individual
   non-conforming path value, `sh:resultPath` is populated.
3. `spec_s6_2_select_validator_property_shape_path_substitution` — a
   `sh:propertyValidator` SELECT query using `$this $PATH ?value` (mirrors the
   spec's §6.2.3.1 `ex:LanguageConstraintComponentUsingSELECT` example),
   confirming `$PATH` textual substitution actually happens.
4. `regression_519_optional_parameter_absent` — a component with one optional
   parameter; a shape invoking it without a value for the optional parameter
   still validates (using `bound($flags)`-style conditional logic in the ASK
   query, mirroring the spec's §6.1 `sh:PatternConstraintComponent` example).
5. `regression_519_missing_required_parameter_not_invoked` — a shape with
   *no* value for a component's mandatory parameter never triggers that
   component at all (not an error — just not invoked, per §6.1).
6. `regression_519_no_suitable_validator_ignored` — a component declaring only
   `sh:propertyValidator` invoked from a node-shape context (no matching
   validator) is silently ignored, per §6.2.3's explicit "SHACL-SPARQL
   processor ignores the constraint" rule.
7. `regression_519_message_template_substitution` — validator's `sh:message
   "... {$lang} ..."` renders with the actual bound parameter value in
   `sh:resultMessage`.

Test data: `tests/testdata/shacl_s519_*.ttl` pairs, added to
`shacl_testdata_parses`'s file list.

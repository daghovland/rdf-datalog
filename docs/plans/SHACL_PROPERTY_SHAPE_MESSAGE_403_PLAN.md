# Plan: surface a property shape's own `sh:message` in `sh:resultMessage`

Issue: [#403](https://github.com/daghovland/rdf-datalog/issues/403)

## Problem

A `sh:message` declared on a property shape (the object of `sh:property`,
often an inline blank node) is silently dropped from the validation report
— no `sh:resultMessage` appears on the resulting `sh:ValidationResult`, even
though the shape declares one. Repro from the issue:

```turtle
[] a sh:NodeShape ;
   sh:targetNode <https://ssi.example.com/subject/1> ;
   sh:property [
       sh:path <https://ssi.example.com/predicate/missing> ;
       sh:minCount 1 ;
       sh:message "Missing predicate" ;
   ] .
```

## Root cause

`shacl::ViolMeta::new_with_severity_override` (`shacl/src/lib.rs`) always
sets `message: shape.message.clone()` — `shape` is the **enclosing node
shape** (`ParsedShape`), which has no `sh:message` in this repro (the
message is declared on the inline property shape instead). There's an
exact precedent for this exact problem already solved for `sh:severity`
(issue [#312](https://github.com/daghovland/rdf-datalog/issues/312)):
`ParsedPropShape` has its own `severity: Option<Severity>` field, parsed
from the property shape's own node, and `new_with_severity_override` takes
a `severity_override` parameter that takes precedence over the parent
shape's severity when present. **No equivalent field/override exists for
`message`.** `ParsedPropShape` (`shacl/src/shapes.rs`) has no `message`
field at all, and `parse_property_shapes` never reads `SH_MESSAGE` off the
property-shape node.

## Fix

Mirror the `severity`/`severity_override` pattern exactly, for `message`:

1. Add `message: Option<String>` to `ParsedPropShape` (`shacl/src/shapes.rs`), doc comment analogous to the existing `severity` field's.
2. In `parse_property_shapes`, parse it: `graph::get_object(shapes, prop_node, SH_MESSAGE).and_then(|id| literal_string(shapes, id))` — same pattern already used for the node-shape-level `message` field on `ParsedShape` (see the existing `SH_MESSAGE` lookup in the node-shape parsing function, same file).
3. Add a `message_override: Option<String>` parameter to `ViolMeta::new_with_severity_override` (rename it if that reads better once it takes two overrides — e.g. `new_with_overrides` — your call, but keep `new`'s existing zero-override call sites working via a thin wrapper or default-`None` args so unrelated call sites don't need touching). When `Some`, it takes precedence over `shape.message.clone()`, exactly like `severity_override` already does for severity.
4. Update the two call sites in `shacl/src/evaluate.rs` (property-constraint and property-combinator violation collection, both already pass `prop.severity.clone()` as the severity override) and the equivalent in `shacl/src/translate.rs` to also pass `prop.message.clone()` as the new message override.
5. Leave the plain `ViolMeta::new(...)` call sites (node-level, pathless constraints — no property shape involved) untouched.

## Tests (TDD)

- Unit/integration test reproducing the issue's exact shape+data (probably
  fits best alongside existing severity-override tests for #312 — search
  for those, likely in `shacl/tests/` or `shacl/src/lib.rs`'s own test
  module) — validate, assert the resulting `ValidationReport`'s single
  `ValidationResult` has `message == Some("Missing predicate".to_string())`,
  and that the serialized RDF report (`report_to_datastore` /
  `serialize_report`, whichever `shacl` crate function produces the
  `sh:resultMessage` triple) actually contains
  `sh:resultMessage "Missing predicate"`.
- Regression: a node-shape-level `sh:message` (declared directly on the
  `sh:NodeShape`, no `sh:property` involved) still surfaces correctly —
  should already pass, confirms the fix didn't regress the existing path.
- Regression: property-shape `sh:severity` override (the #312 test) still
  passes unchanged.
- If a node shape declares `sh:message` AND its property shape also
  declares its own `sh:message`, the property shape's own message should
  win for that property shape's violations (same precedence semantics as
  the existing severity override) — add a test for this precedence case
  explicitly, don't leave it implicit.

## Out of scope

Nothing else in this issue — it's a single, self-contained bug, closed by
this PR.

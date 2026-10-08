# OWL meta-annotations (annotations on annotations) — #695

Parent epic: [#564](https://github.com/daghovland/rdf-datalog/issues/564). Filed
while working [#608](https://github.com/daghovland/rdf-datalog/issues/608)
(OWL/XML ABox axioms, in flight in a separate worktree).

## Problem

The OWL 2 structural spec's `Annotation` production is recursive:

```
Annotation := annotationAnnotations AnnotationProperty AnnotationValue
annotationAnnotations := { Annotation }
```

i.e. an annotation can itself carry a list of annotations on itself (a
"meta-annotation"), e.g.
`Annotation(Annotation(ex:source ex:Textbook) ex:comment "...")`.
`owl_ontology::Annotation` (`owl_ontology/src/axioms.rs`) is currently a flat
type alias with no slot for this:

```rust
pub type Annotation = (AnnotationProperty, AnnotationValue);
```

## New type shape

A type alias can't be recursive (`type Annotation = (Vec<Annotation>, ...)`
is a compile error — infinite size). `Annotation` becomes a struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Annotation {
    pub property: AnnotationProperty,
    pub value: AnnotationValue,
    /// Meta-annotations: annotations on this annotation itself. Empty for
    /// every concrete syntax that doesn't yet populate it (see table below).
    pub annotations: Vec<Annotation>,
}

impl Annotation {
    /// A plain (non-meta-annotated) annotation — the common case, used by
    /// every call site that doesn't thread meta-annotations through yet.
    pub fn new(property: AnnotationProperty, value: AnnotationValue) -> Self {
        Self { property, value, annotations: Vec::new() }
    }
}
```

Field order `property, value, annotations` keeps the derived order
consistent with the old `(property, value)` tuple's own comparison order
(not currently relied on for `Ord`, since neither `Annotation` nor
`AnnotationValue` derives it today, but kept consistent on principle).

## Per-crate scope: who gets real meta-annotation support, who gets an always-empty field

| Crate | Grammar supports nested `Annotation`? | This PR's scope |
|---|---|---|
| `owl_functional_parser` | Yes — `Annotation ::= 'Annotation' '(' annotationAnnotations AnnotationProperty AnnotationValue ')'`. Already parses its own `annotationAnnotations` recursively and discards the payload (`annotation.rs`'s own module doc says so). | **Real support.** Thread the already-parsed `_meta` into the new `annotations` field. `serialize.rs::fmt_annotation` gains the matching `Annotation(<meta> prop value)` nesting via the existing `fmt_axiom_annotations` helper. |
| `manchester_parser` | Yes — `annotation ::= annotationPropertyIRI annotationTarget` is used via the generic `annotatedList(p) ::= [annotations] p {',' [annotations] p}` pattern wherever a list of annotations appears (`annotationAnnotatedList ::= annotatedList(annotation)`), i.e. each list item may have its own leading `Annotations:` block. | **Real support.** `annotation()` in `annotation.rs` gains an optional leading `opt_leading_annotations(ctx)` call before the property+target, populating the new field. Mutually recursive with `opt_leading_annotations`/`annotations_section`, which is fine for `nom` combinator-factory functions (recursion happens at runtime through ordinary function calls, not through the opaque `impl Trait` return type, so there's no "recursive opaque type" compile error). `serialize.rs::fmt_annotation` gains the matching nested `Annotations: ... ` prefix via the existing `ann_prefix` helper. |
| `owl_xml_parser` | Yes — OWL/XML's `<Annotation>` element can contain a nested `<Annotation>` child per the spec, same as Functional-Style Syntax. | **Always-empty only.** This crate's ABox/axiom-annotation work (#608) is in flight in a separate worktree touching these exact files; this PR only makes `owl_xml_parser` compile against the new struct (`Annotation::new(...)`), it does not add nested-`<Annotation>` parsing. A follow-up issue is filed for that (see below) rather than doing it here, to avoid colliding with #608's in-progress edits. |

## RDF translation (`owl2rl2datalog::owl_to_rdf`, `rdf_owl_translator::axiom_parser`)

- **OWL → RDF** (`owl_to_rdf.rs::emit_axiom_annotations`): reifies an axiom's
  own `Vec<Annotation>` via a single `owl:Axiom` blank node and one triple
  per annotation, per the W3C mapping spec's
  [Translation of Annotations](https://www.w3.org/TR/owl2-mapping-to-rdf/#Translation_of_Annotations).
  That mapping does not itself define how to further reify annotations *on*
  an annotation triple (recursively re-reifying the per-annotation triple
  `(_:x, AP, T(av))` would need its own `owl:Axiom` node with
  `annotatedSource = _:x`). This PR updates the loop to read the new
  struct fields (`ap = &a.property`, `av = &a.value`) but does **not** emit
  reification for `a.annotations` — that's a separate, genuinely spec-level
  design decision, filed as a follow-up issue rather than guessed at here.
- **RDF → OWL** (`axiom_parser.rs`/`class_expression_parser.rs::build_annotations`):
  reads plain `(subject, annotation-property, value)` triples into
  `Vec<Annotation>`, with no awareness of further `owl:Axiom` reification
  nesting on the annotation's own triple. Updated to construct
  `Annotation::new(...)` (always-empty `annotations`); recovering nested
  meta-annotations from RDF is the reverse of the gap above and is covered
  by the same follow-up issue.

## Mechanical-only call sites (always-empty, no grammar/feature change)

- `owl_xml_parser/src/annotation.rs` (+ its one test) — see table above.
- `tests/manchester_roundtrip.rs::ModelRenamer::annotation` — a test helper
  that renames anonymous-individual ids consistently across a
  parse→serialize→parse round trip; updated to use the struct and recurse
  into `a.annotations` (so a meta-annotation's own anonymous individuals, if
  any, get renamed consistently too — cheap to do correctly while touching
  this function regardless).

## Follow-up issues filed (Status `Todo`, awaiting review)

1. [#710](https://github.com/daghovland/rdf-datalog/issues/710):
   `owl_xml_parser`: parse nested `<Annotation>` into the new field, once
   #608 merges and its own stopgap error for nested `<Annotation>` can be
   replaced with real support.
2. [#711](https://github.com/daghovland/rdf-datalog/issues/711):
   `owl2rl2datalog`/`rdf_owl_translator`: decide and implement (or
   explicitly reject) an RDF encoding for meta-annotations (annotations on
   a reified annotation triple), in both directions.

Both are sub-issues of parent epic #564.

## TDD sequence

1. Change the type (struct + `Annotation::new`), fix every call site
   mechanically (always-empty) so the workspace compiles — this is the
   "stub so tests compile" phase.
2. Add ignored tests:
   - `owl_functional_parser`: parsing + serializing a nested
     `Annotation(Annotation(...) ...)`.
   - `manchester_parser`: parsing + serializing a nested
     `Annotations: Annotations: ... , ...` block.
3. Unignore and implement one at a time.
4. Full workspace `cargo fmt`/`clippy`/`test` pass.

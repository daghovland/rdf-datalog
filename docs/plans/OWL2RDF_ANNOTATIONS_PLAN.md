# Plan: `owl2rdf` — annotation axioms and axiom annotations ([#514](https://github.com/daghovland/rdf-datalog/issues/514))

Split out from [#373](https://github.com/daghovland/rdf-datalog/issues/373) item 7, part of the
general OWL 2 → RDF structural mapping ([#177](https://github.com/daghovland/rdf-datalog/issues/177)).
Implemented in `owl2rl2datalog/src/owl_to_rdf.rs`.

## Spec citation

<https://www.w3.org/TR/owl2-mapping-to-rdf/#Translation_of_Annotations> (§2.3.2 in the current
REC), together with the axiom-annotation reification rule in §2.3.1 ("Axioms") and the
`AnnotationAssertion`/`SubAnnotationPropertyOf`/`AnnotationPropertyDomain`/`AnnotationPropertyRange`
rows of Table 1 (§2.1).

Fetched section content (paraphrased from the spec, verified against the live page):

* **`AnnotationAssertion(AP as av)`** → `T(as) T(AP) T(av) .` — a plain ground triple, subject
  `as`/value `av` each an IRI, literal, or anonymous individual (`GraphElement` in this codebase's
  types — see `AnnotationValue`).
* **`SubAnnotationPropertyOf(AP1 AP2)`** → `T(AP1) rdfs:subPropertyOf T(AP2) .`
* **`AnnotationPropertyDomain(AP U)`** → `T(AP) rdfs:domain T(U) .`
* **`AnnotationPropertyRange(AP U)`** → `T(AP) rdfs:range T(U) .`
* **Axiom annotations** (§2.3.1): when an axiom `ax` that translates to one ground triple `s p o`
  carries a non-empty annotation list `Annotation(AP1 av1) ... Annotation(APn avn)`, the RDF
  encoding emits the base triple `s p o` **and, in addition**, a reification:
  ```
  s p o .
  _:x rdf:type owl:Axiom .
  _:x owl:annotatedSource s .
  _:x owl:annotatedProperty p .
  _:x owl:annotatedTarget o .
  _:x AP1 T(av1) .
  ...
  _:x APn T(avn) .
  ```
  (the `TANN` helper macro's recursive "annotations on annotations" case — nested `Annotation(...)`
  values that are themselves annotated with `owl:Annotation` blank nodes instead of `owl:Axiom` — has
  no representation in `owl_ontology::Annotation = (AnnotationProperty, AnnotationValue)`, which is a
  flat pair with no further nesting, so only the one-level `owl:Axiom` case applies here).
* **n-ary axioms** (`EquivalentClasses`/`SameIndividual`/… with 3+ operands, encoded in RDF as a
  chain of binary triples): §2.3.2 states the axiom's annotations are **repeated on every triple**
  produced by the chain, each with its own `owl:Axiom` reification blank node.
* **List-object axioms** (`HasKey`, `DisjointUnion`, whose RDF encoding is `subject predicate
  rdf:list-head` plus separate `rdf:first`/`rdf:rest` list-cell triples): only the **main** triple
  (`subject predicate list-head`) is reified; the list-cell triples are emitted unchanged, without
  their own reification.

## The cross-cutting hook

Every `Translator` axiom-emitting method's first field is a `Vec<Annotation>` that is currently
discarded (bound as `_`). Rather than duplicating the reification logic at each of the ~20 call
sites, thread it through two small shared helpers on `Translator`:

```rust
/// Resolve an `AnnotationValue` to a `GraphElementId` (IRI, literal, or
/// anonymous-individual node).
fn annotation_value(&mut self, value: &AnnotationValue) -> GraphElementId { .. }

/// If `annotations` is non-empty, emit the `owl:Axiom` reification of the
/// ground triple `(subject, predicate, obj)`: a fresh blank node typed
/// `owl:Axiom`, `owl:annotatedSource`/`Property`/`Target` triples pointing
/// back at the three components, and one triple per annotation
/// `(_:x, AP, T(av))`. No-op when `annotations` is empty — callers must
/// never emit spurious reification triples for an unannotated axiom.
fn emit_axiom_annotations(
    &mut self,
    subject: GraphElementId,
    predicate: GraphElementId,
    obj: GraphElementId,
    annotations: &[Annotation],
) { .. }
```

Every axiom method that currently does `self.triple_p(s, PRED, o)` for its single ground triple
switches to also calling `self.emit_axiom_annotations(s, pred_id, o, annotations)` right after —
`pred_id` is already resolved as part of `triple_p`, so a convenience wrapper
`triple_p_annotated(subject, predicate_iri, obj, annotations)` folds both calls into one and is used
at each of those single-triple call sites (`SubClassOf`, `ObjectPropertyDomain`/`Range`'s
sibling forms on data properties and annotation properties, `SubObjectPropertyOf`,
`InverseObjectProperties`, `DisjointClasses`/`DisjointObjectProperties`/`DisjointDataProperties`
pairs, property characteristics via `type_triple`, `SubDataPropertyOf`, `DataPropertyDomain`,
`DataPropertyRange`, `FunctionalDataProperty`, the three atomic `Assertion` triple kinds,
`DifferentIndividuals` pair, declarations, and the four `AnnotationAxiom` forms).

**Exception found during implementation:** `ObjectPropertyAxiom::ObjectPropertyDomain` and
`::ObjectPropertyRange` are the only two axiom variants in the whole `owl_ontology` type hierarchy
that do *not* carry a `Vec<Annotation>` field (every sibling variant does). There is nothing for
`Translator::object_property_axiom` to thread through for these two, so they keep using the
unannotated `triple_p` — filed as
[#588](https://github.com/daghovland/rdf-datalog/issues/588) to add the missing field and wire it
up once `owl_ontology` itself is fixed.

For **chain-based** n-ary axioms (`EquivalentClasses`, `EquivalentObjectProperties`,
`EquivalentDataProperties`, `SameIndividual`), `chain()` grows an `annotations: &[Annotation]`
parameter and calls `emit_axiom_annotations` once per pairwise triple it emits (repeating
annotations on every triple, per §2.3.2).

For **list-object** axioms (`HasKey`, `DisjointUnion`), only the call that emits the main
`class predicate list-head` triple is switched to `triple_p_annotated`; the `rdf_list()` helper that
builds the `rdf:first`/`rdf:rest` cells is untouched (no annotations attached there, matching the
spec's "list construction triples... output separately without annotation attachment").

## `AnnotationAxiom` dispatch (new)

`Axiom::AxiomAnnotationAxiom` is currently not dispatched in `Translator::axiom` at all (falls into
the catch-all `other => self.skip(...)`). Add a new `annotation_axiom(&mut self, axiom:
&AnnotationAxiom)` method dispatched from `Translator::axiom`, covering:

* `AnnotationAssertion(annotations, AP, subject, value)` → `T(subject) T(AP) T(value)`, subject and
  value resolved via `annotation_value`-equivalent logic (subject/value here are already
  `GraphElement`, interned via `datastore.add_resource`), then `emit_axiom_annotations` for the
  axiom's own (outer) annotation list.
* `SubAnnotationPropertyOf(annotations, AP1, AP2)` → `T(AP1) rdfs:subPropertyOf T(AP2)`.
* `AnnotationPropertyDomain(annotations, AP, U)` → `T(AP) rdfs:domain T(U)`.
* `AnnotationPropertyRange(annotations, AP, U)` → `T(AP) rdfs:range T(U)`.

All four are single-ground-triple forms, so `triple_p_annotated` covers them directly.

## Tests (in `owl2rl2datalog/src/owl_to_rdf.rs`'s existing `#[cfg(test)] mod tests`, following the
`HasKey` PR's (#556) conventions: `translate(vec![axiom])`, `has_triple`/`object_of` helpers)

Initially `#[ignore]`d, unignored one at a time during implementation:

1. `annotation_assertion_on_named_individual_becomes_ground_triple` — plain `AnnotationAssertion`
   with no outer annotations, asserts the one ground triple and `report.skipped.is_empty()`, and
   that **no** `owl:Axiom` reification triples exist (nothing typed `owl:Axiom` in the store).
2. `sub_annotation_property_of_becomes_rdfs_sub_property_of`.
3. `annotation_property_domain_and_range_become_rdfs_domain_and_range`.
4. `subclassof_with_annotation_is_reified_via_owl_axiom` — `SubClassOf` with one non-empty
   `Annotation` in its `Vec<Annotation>`: asserts the base `rdfs:subClassOf` triple still exists,
   *and* a blank node exists with `rdf:type owl:Axiom`, `owl:annotatedSource`/`Property`/`Target`
   pointing at the `SubClassOf`'s subject/`rdfs:subClassOf`/object, and the annotation triple itself
   on that blank node.
5. `subclassof_with_empty_annotations_emits_no_reification` — explicit regression test for the
   off-by-one: an axiom with `vec![]` annotations must produce *zero* `owl:Axiom`-typed nodes and
   the same triple count as the pre-#514 behavior.
6. `equivalent_classes_annotations_repeat_on_every_chain_triple` — a ternary `EquivalentClasses`
   with a non-empty annotation list: asserts two separate `owl:Axiom` reification nodes exist (one
   per chain triple), not one.
7. `has_key_with_annotation_reifies_only_the_main_triple` — `HasKey` with a non-empty annotation
   list: asserts exactly one `owl:Axiom` node (for the `owl:hasKey` triple) and that the list-cell
   `rdf:first`/`rdf:rest` triples are *not* reified.

## Out of scope / follow-ups

* [#588](https://github.com/daghovland/rdf-datalog/issues/588): `owl_ontology`'s
  `ObjectPropertyDomain`/`ObjectPropertyRange` lack a `Vec<Annotation>` field, unlike every other
  axiom variant (see the "Exception found during implementation" note above).
* `Axiom::AxiomDatatypeDefinition` annotations — `DatatypeDefinition` itself has no RDF triple
  encoding yet ([#512](https://github.com/daghovland/rdf-datalog/issues/512)), so its annotation
  handling waits on that.
* The general blank-node structural encoding for complex class expressions
  ([#509](https://github.com/daghovland/rdf-datalog/issues/509)) and n-ary
  disjoint/different constructs ([#513](https://github.com/daghovland/rdf-datalog/issues/513)) are
  unaffected by this change; once implemented they'll need the same `emit_axiom_annotations` hook
  threaded through, noted here for whoever picks those up.
* Nested "annotations on annotations" (`owl:Annotation` reification, as opposed to `owl:Axiom`) has
  no representation in `owl_ontology::Annotation` today and is not addressed by this plan; filed
  separately if/when the type gains that nesting.
